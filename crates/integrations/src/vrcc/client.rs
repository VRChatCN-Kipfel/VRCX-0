//! 星遇同频（vrcc.cc）API 客户端。
//!
//! 这是本集成**唯一的网络出口**。合规约束照官方《使用要求》与参考实现
//! （`xingshi-app` 的 `VrccClient`）逐条落地：
//!
//! - 每个请求都带 `X-Client`，标识客户端身份；
//! - 只允许 `/api/v1/` 下的**相对路径**，绝对地址在这里直接拒绝，
//!   保证令牌不可能被发到第三方地址；
//! - 写操作（打分、连线、私聊）只由玩家点击触发，本层不做队列、不重试、不批处理；
//! - 令牌只发给 `vrcc.cc`。
//!
//! 限流照开放接口 1.14.0：
//! - 所有 `429` 都带 `Retry-After`（1.9.0 起），按它等；旧服务器没给时退回指数退避；
//! - 带每天次数的接口回答 `RateLimit-*`（1.11.0 起），用它代替本地猜的用量；
//! - 某一份额度当天用完，只停那一份，不让别的接口跟着退避。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::HeaderMap;
use serde::de::DeserializeOwned;
use serde_json::json;
use vrcx_0_contracts::vrcc::resolve_client_id;
use vrcx_0_contracts::vrcc::{
    VrccAnnouncements, VrccBoard, VrccBoardSort, VrccDeviceCode, VrccErrorBody, VrccMapDetail,
    VrccMutuals, VrccMyMap, VrccNotes, VrccNotifications, VrccRecentNotifications, VrccStatus,
    VrccToday, VrccTokenResponse, VrccUnread, VRCC_API_BASE, VRCC_SITE_ORIGIN,
};
use vrcx_0_core::proxy::with_remote_dns;

use super::throttle::{backoff_for, RateLimiter, MAX_BACKOFF, NOTIFICATIONS_MIN_GAP};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(25);

/// 一次请求最多愿意为限流等待多久。
///
/// 超过就返回 `TooSoon`，交给调用方决定（通常是拿缓存顶上），而不是让
/// 一次用户操作僵在那里。典型场景：`boards` 的 `min_gap` 是 60 秒，
/// 用户点一下刷新不能干等一分钟——官方自己说开放数据最多 5 分钟才更新一次，
/// 那份旧数据完全够用。
///
/// 取 10 秒是因为其余接口的 `min_gap` 都在 5–30 秒之间，正常页面加载
/// （间隔远大于 30 秒）不会碰到；只有连点两次之类的场景才会命中。
const MAX_ACQUIRE_WAIT: Duration = Duration::from_secs(10);

/// 客户端错误。定义在契约层（应用层要按它分类，不能反过来依赖网络层），
/// 这里只做转出，保持既有调用点不变。
pub use vrcx_0_contracts::vrcc::VrccError;

/// 官方限制 POST 正文最大 2 KB。
const MAX_BODY_BYTES: usize = vrcx_0_contracts::vrcc::VRCC_MAX_BODY_BYTES;

/// `/device/code` 的本地限流键。
///
/// 单独拎成常量是因为 `429` 的文案要按它分流：这个额度的维度是**网络出口
/// 地址**（每天 30 个，同一路由下的所有人共用），和个人授权维度的额度不是
/// 一回事，说错了会让玩家以为是自己账号的问题。
const DEVICE_CODE_KEY: &str = "device_code";

/// 发送前拦下超限的正文。
///
/// 抽成独立函数是为了**能真正测到**：`request()` 要先过 `acquire()` 再
/// `send()`，在单测里跑它要么真发网络、要么卡在限流窗口上。这里是拦截的
/// 全部逻辑，单独测它就等于测到了那条路径。
///
/// 官方限制 POST 正文 2 KB。正文超限属于我们自己构造错了请求，
/// 不该发出去让它变成 400。
fn check_body_size(len: usize) -> Result<(), VrccError> {
    if len > MAX_BODY_BYTES {
        return Err(VrccError::BodyTooLarge);
    }
    Ok(())
}

/// 带每天次数的接口各自算在哪一份额度里（官方《频率限制》）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuotaBucket {
    /// 第一份读：最近的提醒、星轨相交、互相连上、所在的地图、我打过分的地图。
    Reads,
    /// 第二份读：今天的星、通知中心等，和第一份分开算。
    MoreReads,
    /// `me/unread` 自己的一份。
    Unread,
}

/// 服务器在 `RateLimit-*` 里说的一份额度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerQuota {
    /// `RateLimit-Limit`：每天上限。
    pub limit: i64,
    /// `RateLimit-Remaining`：今天还剩几次（算上这一次之后）。
    pub remaining: i64,
    /// `RateLimit-Reset`：几秒后重置（北京时间零点）；没给时为 0。
    pub reset_secs: u64,
}

impl ServerQuota {
    pub fn used(&self) -> i64 {
        (self.limit - self.remaining).max(0)
    }
}

/// 一次回答里和限流有关的响应头。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RateHeaders {
    /// `Retry-After`（秒）。1.9.0 起所有 `429` 都带。
    pub retry_after_secs: Option<u64>,
    /// `RateLimit-Limit` / `-Remaining` / `-Reset`。1.11.0 起带每天次数的接口给。
    pub quota: Option<ServerQuota>,
}

impl RateHeaders {
    /// 从响应头里读。缺的、写坏的一律当没有，不让一个头把请求弄失败。
    ///
    /// `Retry-After` 按 RFC 9110 也可以是日期，但官方只给秒数，这里只认秒数。
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let number = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<i64>().ok())
        };
        let retry_after_secs = number("retry-after")
            .filter(|secs| *secs >= 0)
            .map(|secs| secs as u64);
        let quota = match (number("ratelimit-limit"), number("ratelimit-remaining")) {
            (Some(limit), Some(remaining)) if limit > 0 => Some(ServerQuota {
                limit,
                remaining: remaining.clamp(0, limit),
                reset_secs: number("ratelimit-reset")
                    .filter(|secs| *secs > 0)
                    .map(|secs| secs as u64)
                    .unwrap_or(0),
            }),
            _ => None,
        };
        Self {
            retry_after_secs,
            quota,
        }
    }
}

/// 服务器最近一次说的某份额度，带作废时间。
#[derive(Clone, Copy, Debug)]
struct SeenQuota {
    limit: i64,
    remaining: i64,
    /// 到这个时刻（服务器的重置时间）这份数就作废了。
    until: Instant,
}

/// 客户端共享状态：限流器与服务器下发的配额。
#[derive(Debug, Default)]
pub struct VrccClientState {
    limiter: RateLimiter,
    /// 429 退避截止时间。
    backoff_until: Option<Instant>,
    /// 连续 429 次数，用于指数退避。
    backoff_attempt: u32,
    /// 服务器在 `RateLimit-*` 里说的各份额度。
    ///
    /// **纯内存**：重启即丢，这是有意的。官方没有「查询额度」的接口，这组头
    /// 只在你发请求的那一刻回一次；把它落盘只会把一个必然过期的快照当成事实，
    /// 而官方的时间基准是北京时间零点，本地推算的日期跟它对不上。
    quotas: HashMap<QuotaBucket, SeenQuota>,
}

impl VrccClientState {
    pub fn window_used(&self, now: Instant) -> usize {
        self.limiter.window_used(now)
    }

    pub fn backoff_remaining(&self, now: Instant) -> Duration {
        self.backoff_until
            .map(|until| until.saturating_duration_since(now))
            .unwrap_or_default()
    }

    /// 还没到重置时间的那份服务器额度；没拿到过或已经过了零点时为 `None`。
    fn live_quota(&self, bucket: QuotaBucket, now: Instant) -> Option<SeenQuota> {
        self.quotas
            .get(&bucket)
            .copied()
            .filter(|quota| quota.until > now)
    }
}

/// 客户端用量快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VrccLocalUsage {
    /// 第一份读今天已用多少、上限多少。**只来自服务端的 `RateLimit-*`**；
    /// 还没拿到过时为 `None`——界面应显示「未知」，不要填一个自己编的数。
    pub reads: Option<(i64, i64)>,
    /// `me/unread` 的（已用，上限），只有服务器给过响应头时才有。
    pub unread: Option<(i64, i64)>,
    pub window_used: usize,
    pub backoff_remaining: Duration,
    /// 服务端下一次重置还有多少秒；没拿到过响应头时为 `None`。
    pub reset_secs: Option<i64>,
}

/// 一次请求的全部参数。
struct Call<'a> {
    method: &'static str,
    path: &'a str,
    body: Option<serde_json::Value>,
    /// 是否带 `Bearer`。
    authed: bool,
    /// 本地限流用的接口标识。
    key: &'a str,
    /// 同一个接口两次之间至少隔多久。
    min_gap: Duration,
    /// 算在哪一份每天的额度里；`None` 是没有每天上限的接口。
    bucket: Option<QuotaBucket>,
    /// 优先级请求：**不受退避和最小间隔限制，立刻发**。
    ///
    /// 只给 `disconnect` 用。理由：吊销是玩家明确要求的动作，而退避是全局的
    /// （别的接口吃到一次 429 就会进入退避）。如果吊销被退避挡住，本地令牌
    /// 已经被清掉，这场授权就**再也没有机会**在服务器上吊销了——服务器上
    /// 那份授权会一直有效，玩家在网页「第三方客户端」里也看不到它该消失。
    ///
    /// 这类请求都是玩家手动触发的一次性动作，不可能刷屏，所以不设限没有风险。
    bypass_gate: bool,
}

/// 一个可克隆的客户端句柄。
#[derive(Clone)]
pub struct VrccClient {
    http: reqwest::Client,
    base: String,
    client_id: String,
    token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    state: Arc<std::sync::Mutex<VrccClientState>>,
}

impl VrccClient {
    /// 新建客户端。`token` 是个取令牌的回调，这样令牌续期不用重建客户端。
    ///
    /// `proxy_url` 走宿主应用统一的代理设置（`WebClient::proxy_url`）。
    /// **必须传**：本机实测 `vrcc.cc` 直连会 DNS 超时，走代理才通。原来这里
    /// 自己 `build()` 一个不带代理的客户端，结果请求发不出去，界面上表现为
    /// 「接口暂时不可用」——看起来像服务挂了，其实是本地网络。
    ///
    /// 代理的构造方式照 `llm.rs` 的既有做法：`Proxy::all(with_remote_dns(url))`，
    /// `socks5://` 会被改写成 `socks5h://` 让 DNS 也走代理。
    pub fn new(
        client_id: impl Into<String>,
        token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        proxy_url: Option<&str>,
    ) -> Self {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT);
        match proxy_url.map(str::trim).filter(|url| !url.is_empty()) {
            Some(proxy_url) => match reqwest::Proxy::all(with_remote_dns(proxy_url).as_ref()) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                }
                Err(error) => {
                    // 代理串写错不该让整个客户端起不来：退回直连，
                    // 并把原因记下来，否则症状会变成"接口不可用"。
                    tracing::warn!(%error, proxy_url, "vrcc: invalid proxy url, using direct");
                }
            },
            None => {
                tracing::debug!("vrcc: no proxy configured, connecting directly");
            }
        }
        let http = builder.build().unwrap_or_default();
        Self {
            http,
            base: VRCC_API_BASE.to_string(),
            client_id: client_id.into(),
            token,
            state: Arc::new(std::sync::Mutex::new(VrccClientState::default())),
        }
    }

    /// 用构建期确定的标识新建。
    pub fn with_default_client(
        token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        proxy_url: Option<&str>,
    ) -> Self {
        Self::new(resolve_client_id(), token, proxy_url)
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// 读用量。
    ///
    /// 全部取自服务端的 `RateLimit-*`，不落盘、不推算：官方没有查询额度的
    /// 接口，这组头只在你发请求的那一刻回一次。还没拿到过时返回 `None`，
    /// 让界面显示「未知」，而不是编一个本机日期算出来的数。
    pub fn local_usage(&self) -> VrccLocalUsage {
        let now = Instant::now();
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let used = |quota: SeenQuota| ((quota.limit - quota.remaining).max(0), quota.limit);
        let reads = state.live_quota(QuotaBucket::Reads, now);
        let unread = state.live_quota(QuotaBucket::Unread, now);
        VrccLocalUsage {
            reads: reads.map(used),
            unread: unread.map(used),
            window_used: state.window_used(now),
            backoff_remaining: state.backoff_remaining(now),
            reset_secs: reads.map(|quota| {
                quota
                    .until
                    .saturating_duration_since(now)
                    .as_secs()
                    .try_into()
                    .unwrap_or(i64::MAX)
            }),
        }
    }

    /// 只允许 `/api/v1/` 下的相对路径。这是硬约束，不提供绕过开关。
    fn validate_path(path: &str) -> Result<(), VrccError> {
        let lowered = path.to_ascii_lowercase();
        if lowered.starts_with("http") || path.starts_with("//") {
            return Err(VrccError::AbsolutePath(path.to_string()));
        }
        Ok(())
    }

    /// 拿到一次发送许可：检查退避、限流，必要时等待。
    ///
    /// 三个要点：
    /// 1. **预占而不是先算后记**。原来先 `check` 拿 `wait`、放锁睡、再加锁
    ///    `record`，并发调用会拿到同一个 `wait`，一起醒来一起发出去。
    /// 2. **等待有上限**。`boards` 的 gap 是 60 秒，用户点刷新时不能让他干等：
    ///    超过 [`MAX_ACQUIRE_WAIT`] 就返回 `TooSoon`，让调用方改用缓存。
    /// 3. **醒来后再看一次退避**。等待期间可能有别的请求吃到了 429。
    async fn acquire(&self, key: &str, min_gap: Duration) -> Result<(), VrccError> {
        let now = Instant::now();
        let at = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let backoff = state.backoff_remaining(now);
            if !backoff.is_zero() {
                return Err(VrccError::RateLimited(format!(
                    "退避中，还需 {} 秒",
                    backoff.as_secs().max(1)
                )));
            }
            match state.limiter.reserve(key, min_gap, now, MAX_ACQUIRE_WAIT) {
                Ok(at) => at,
                Err(wait) => {
                    // 没占槽：调用方可以拿缓存顶上，而不是干等。
                    return Err(VrccError::TooSoon {
                        retry_after_secs: wait.as_secs().max(1),
                    });
                }
            }
        };
        let wait = at.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        // 睡醒再看一眼：等待期间可能已经有别的请求吃到 429，全局退避开始了。
        let backoff = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.backoff_remaining(Instant::now())
        };
        if !backoff.is_zero() {
            return Err(VrccError::RateLimited(format!(
                "退避中，还需 {} 秒",
                backoff.as_secs().max(1)
            )));
        }
        Ok(())
    }

    /// 旧服务器的 429 没有 `Retry-After`：按指数退避猜。
    fn note_rate_limited(&self) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let wait = backoff_for(state.backoff_attempt);
        state.backoff_attempt = state.backoff_attempt.saturating_add(1);
        state.backoff_until = Some(Instant::now() + wait);
        wait
    }

    /// 服务器给了 `Retry-After`：照它等，不自己加码。
    fn note_retry_after(&self, wait: Duration) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.backoff_attempt = state.backoff_attempt.saturating_add(1);
        state.backoff_until = Some(Instant::now() + wait);
        wait
    }

    fn note_success(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.backoff_attempt = 0;
        state.backoff_until = None;
    }

    /// 记下服务器在 `RateLimit-*` 里说的一份额度。
    ///
    /// 只存内存。这份数就是界面能看到的全部依据——不再拿它去覆盖什么本地计数，
    /// 因为本地计数已经没有了。
    fn note_quota(&self, bucket: QuotaBucket, quota: ServerQuota) {
        let now = Instant::now();
        let reset = if quota.reset_secs > 0 {
            quota.reset_secs
        } else {
            secs_until_beijing_midnight()
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.quotas.insert(
            bucket,
            SeenQuota {
                limit: quota.limit,
                remaining: quota.remaining,
                until: now + Duration::from_secs(reset),
            },
        );
    }

    /// 某一份额度今天用完了：在重置之前，这一份的请求不再发出去。
    fn mark_exhausted(&self, bucket: QuotaBucket, wait: Duration) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let limit = state.quotas.get(&bucket).map_or(0, |quota| quota.limit);
        state.quotas.insert(
            bucket,
            SeenQuota {
                limit,
                remaining: 0,
                until: Instant::now() + wait,
            },
        );
    }

    /// 发请求前过一下额度闸门。
    ///
    /// **只在拿到过服务端 `RateLimit-*` 时才拦。** 没拿到过就放行——早先会在
    /// 本地按自然日预扣到 1000 就自己停下，但官方的时间基准是北京时间零点，
    /// 本地推算的日期对非 UTC+8 的玩家必然错开，结果是「服务端明明还有余量，
    /// 客户端自己先拒绝」。宁可让服务端回 429（我们原样展示），也不要自己编。
    fn spend(&self, bucket: QuotaBucket) -> Result<(), VrccError> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(live) = state.live_quota(bucket, now) else {
            return Ok(());
        };
        if live.remaining <= 0 {
            return Err(VrccError::QuotaExhausted(format!(
                "今天的次数用完了，约 {} 后恢复",
                human_wait(live.until.saturating_duration_since(now).as_secs())
            )));
        }
        // 先扣着，免得两次响应之间连发越界；下一个响应头会覆盖它。
        if let Some(seen) = state.quotas.get_mut(&bucket) {
            seen.remaining -= 1;
        }
        Ok(())
    }

    /// 把 `spend` 预扣的那一次退回去。**只用于请求没发出去的情况。**
    ///
    /// 服务器根本没收到这个请求，本地就不该占着这次额度。
    fn refund(&self, bucket: QuotaBucket) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(seen) = state.quotas.get_mut(&bucket) {
            seen.remaining = seen.remaining.saturating_add(1);
        }
    }

    /// 先于通用 429 分流处理的几个 `(状态码, error)` 组合。
    ///
    /// 官方错误码表里有两个 `error` 串**一个串两种含义**，只能靠状态码区分
    /// （README「网页上的动作和打分的错误码」一节原话：「同一个码有两种状态的
    /// （`busy`、`xd_world`），按状态码区分」）。`busy` 是最容易踩的那一个：
    ///
    /// | 状态 | error | 含义 |
    /// |---|---|---|
    /// | `409` | `busy` | 这一轮的星卡还在生成，等一下就好 |
    /// | `429` | `busy` | 全站今天输错的连接码太多，连接功能暂停到次日 |
    ///
    /// 两者都和本机额度无关，所以都**不能**交给 [`Self::on_too_many`]——
    /// 那会把它们算成 `QuotaExhausted` 并说成「今天的次数用完了」。
    ///
    /// 单独抽成纯函数是为了能测到：`request()` 要先过限流再真的发网络请求，
    /// 在单测里跑不到这条分支。
    fn classify_status_error(code: u16, error: &str) -> Option<VrccError> {
        match (code, error) {
            (409, "busy") => Some(VrccError::NotReady),
            (429, "busy") => Some(VrccError::ConnectPaused),
            _ => None,
        }
    }

    /// 把一个 `429` 变成错误，顺带决定要不要退避、退避多久。
    ///
    /// 官方的 `429` 分几种，处理完全不同：
    /// - `too_soon`：两次轮询间隔不到 25 秒。等 `Retry-After` 秒（旧服务器
    ///   没给就用 `every`）再问就行，**不该**触发长时间退避。
    /// - 每天的次数用完（`RateLimit-Remaining` 是 0，或 `Retry-After` 长到
    ///   只能是「到北京时间零点」）：只停这一份额度，不让别的接口跟着停。
    /// - 其余（每分钟的限制）：照 `Retry-After` 全局退避；旧服务器没给头时
    ///   按指数退避猜。
    ///
    /// `429 busy` 不会走到这里，见 [`Self::classify_status_error`]。
    fn on_too_many(
        &self,
        error: &str,
        body: &VrccErrorBody,
        rate: RateHeaders,
        bucket: Option<QuotaBucket>,
        call_key: Option<&str>,
    ) -> VrccError {
        if error == "too_soon" {
            let gap = rate
                .retry_after_secs
                .filter(|secs| *secs > 0)
                .or_else(|| {
                    body.every
                        .filter(|every| *every > 0)
                        .map(|every| every as u64)
                })
                .unwrap_or(NOTIFICATIONS_MIN_GAP.as_secs());
            return VrccError::TooSoon {
                retry_after_secs: gap,
            };
        }
        let daily = rate.quota.is_some_and(|quota| quota.remaining <= 0)
            || rate
                .retry_after_secs
                .is_some_and(|secs| Duration::from_secs(secs) > MAX_BACKOFF);
        if daily {
            let wait = rate
                .retry_after_secs
                .or(rate.quota.map(|quota| quota.reset_secs))
                .filter(|secs| *secs > 0)
                .unwrap_or_else(secs_until_beijing_midnight);
            if let Some(bucket) = bucket {
                self.mark_exhausted(bucket, Duration::from_secs(wait));
            }
            // 设备码的 30 次/天是按**网络出口地址**算的，不是按本客户端算的：
            // 同一路由下的所有人共用一个额度。所以这里的措辞必须点明这一点，
            // 否则玩家会以为是自己的账号出了问题、或者怪这个软件。
            if call_key == Some(DEVICE_CODE_KEY) {
                return VrccError::QuotaExhausted(format!(
                    "申请连接码失败（{error}）：该额度按网络出口 IP 计算，每天 30 个，\
                     可能已被同一网络下的其他设备或玩家用完。约 {} 后恢复；\
                     换一个网络（如手机热点）可以立刻再用。",
                    human_wait(wait)
                ));
            }
            return VrccError::QuotaExhausted(format!(
                "{error}，今天的次数用完了，约 {} 后恢复",
                human_wait(wait)
            ));
        }
        let wait = match rate.retry_after_secs {
            Some(secs) => self.note_retry_after(Duration::from_secs(secs.max(1))),
            None => self.note_rate_limited(),
        };
        VrccError::RateLimited(format!("{error}，退避 {} 秒", wait.as_secs()))
    }

    /// 把请求真正发出去之前的所有步骤。
    ///
    /// 单独抽一层，是为了让「没发出去就要退配额」只写一次：这一层返回的
    /// `Err` 一律表示**服务器没有收到这次请求**，调用方据此退还预扣。
    ///
    /// 注意 `send()` 失败也算没发出去——连接都建不起来，服务端不可能计数。
    async fn send_request(&self, call: &Call<'_>) -> Result<reqwest::Response, VrccError> {
        // 优先级请求（吊销）不排队：被退避挡住就永远吊销不了了。
        if !call.bypass_gate {
            self.acquire(call.key, call.min_gap).await?;
        }

        let mut builder = self
            .http
            .request(
                call.method.parse().map_err(|_| {
                    VrccError::Transport(format!("不支持的请求方法 {}", call.method))
                })?,
                format!("{}{}", self.base, call.path),
            )
            .header("X-Client", &self.client_id)
            .header("Accept", "application/json");

        if call.authed {
            let token = (self.token)().ok_or(VrccError::TokenMissing)?;
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }

        if let Some(payload) = &call.body {
            let text = payload.to_string();
            check_body_size(text.len())?;
            builder = builder
                .header("Content-Type", "application/json; charset=utf-8")
                .body(text);
        }

        builder
            .send()
            .await
            .map_err(|error| VrccError::Transport(error.to_string()))
    }

    /// 核心请求。
    async fn request<T: DeserializeOwned>(&self, call: Call<'_>) -> Result<T, VrccError> {
        Self::validate_path(call.path)?;
        let bucket = call.bucket;
        if let Some(bucket) = bucket {
            self.spend(bucket)?;
        }
        // `spend` 是**预扣**：先记上一次，免得两次响应头之间连发越界。但预扣
        // 只有「请求真的发出去」才成立——限流没过、令牌没有、连接就失败，
        // 服务器一次都没数，这地方必须退回去。
        //
        // 不退的后果很实在：断网时后台轮询每分钟扣一次，本地计数很快跑满，
        // 网络恢复后会被自己拦到第二天，而服务端一次都没记过。
        let response = match self.send_request(&call).await {
            Ok(response) => response,
            Err(error) => {
                if let Some(bucket) = bucket {
                    self.refund(bucket);
                }
                return Err(error);
            }
        };
        let status = response.status();
        let rate = RateHeaders::from_headers(response.headers());
        if let Some(sunset) = response
            .headers()
            .get("sunset")
            .and_then(|value| value.to_str().ok())
        {
            // 1.14.0 起，带着弃用字段的回答有 `Deprecation` / `Sunset` 头。
            // 契约层已经不读那些字段，这里只留个线索，方便以后核对。
            tracing::debug!(
                key = call.key,
                sunset,
                "vrcc: response carries deprecated fields"
            );
        }
        let text = response
            .text()
            .await
            .map_err(|error| VrccError::Transport(error.to_string()))?;

        if let (Some(bucket), Some(quota)) = (call.bucket, rate.quota) {
            self.note_quota(bucket, quota);
        }

        if status.is_success() {
            self.note_success();
            serde_json::from_str(&text).map_err(|error| {
                // 不把正文塞进错误里：里面可能有玩家数据。
                VrccError::Decode(error.to_string())
            })
        } else {
            let code = status.as_u16();
            let parsed: VrccErrorBody = serde_json::from_str(&text).unwrap_or_default();
            let error = parsed
                .error
                .clone()
                .unwrap_or_else(|| format!("http_{code}"));
            if let Some(error) = Self::classify_status_error(code, &error) {
                return Err(error);
            }
            if code == 429 {
                return Err(self.on_too_many(&error, &parsed, rate, call.bucket, Some(call.key)));
            }
            if code == 503 && error == "stop" {
                return Err(VrccError::Paused {
                    say: parsed.say,
                    until: parsed.until,
                });
            }
            Err(VrccError::Api {
                status: code,
                error,
                need: parsed.need,
                why: parsed.why,
            })
        }
    }

    // -- 开放数据（不需要令牌） --------------------------------------------

    /// 地图榜。官方说明服务器最多 5 分钟更新一次，问得再勤也是同一份。
    ///
    /// `sort`（1.13.0 起）：总榜、本周榜、新上榜。旧服务器不认 `sort`、照旧
    /// 回总榜，这时回答里没有 `sort` 字段，调用方据此判断「还不支持」。
    pub async fn boards(&self, limit: u32, sort: VrccBoardSort) -> Result<VrccBoard, VrccError> {
        let limit = limit.clamp(1, 100);
        self.request(Call {
            method: "GET",
            path: &boards_path(limit, sort),
            body: None,
            authed: false,
            // 每种排序各算各的间隔：切到别的排序不用等上一种的 60 秒。
            key: match sort {
                VrccBoardSort::Top => "boards",
                VrccBoardSort::Week => "boards_week",
                VrccBoardSort::New => "boards_new",
            },
            min_gap: Duration::from_secs(60),
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    /// 服务探活。不需要 client 标识，也不计入个人额度。
    pub async fn status(&self) -> Result<VrccStatus, VrccError> {
        self.request(Call {
            method: "GET",
            path: "status",
            body: None,
            authed: false,
            key: "status",
            min_gap: Duration::from_secs(30),
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    /// 取站点上的一个静态资源（例如星遇企划图标）。
    ///
    /// 走 `VRCC_SITE_ORIGIN` 而不是 API 基址：站点资源不在 `/api/v1/` 下。
    /// 仍然复用同一个 http 客户端，所以**代理设置一致**——否则会出现
    /// 「接口通、图标取不到」这种看起来像图标坏了、实际是没走代理的情况。
    ///
    /// 同样硬拦绝对地址：这个客户端的承诺是「只和 vrcc.cc 说话」，
    /// 不能因为多了一个取图方法就开个口子把令牌或请求发到别处去。
    ///
    /// 不计入个人接口的日配额：那不是带令牌的 API 调用，服务端也不按令牌算。
    pub async fn fetch_site_asset(&self, path: &str) -> Result<Vec<u8>, VrccError> {
        Self::validate_path(path)?;
        let url = format!("{VRCC_SITE_ORIGIN}{path}");
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|error| VrccError::Transport(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(VrccError::Api {
                status: status.as_u16(),
                error: "asset".to_string(),
                need: None,
                why: None,
            });
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| VrccError::Transport(error.to_string()))
    }

    /// 单张地图详情。未上榜或未过审返回 `404 not_found`。
    pub async fn map(&self, world: &str) -> Result<VrccMapDetail, VrccError> {
        self.request(Call {
            method: "GET",
            path: &format!("maps/{world}"),
            body: None,
            authed: false,
            key: "map",
            min_gap: Duration::from_secs(30),
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    /// 公告和停站（1.12.0 起）。不用令牌，停站时也能问，服务器缓存 1 分钟。
    ///
    /// 旧服务器没有这个接口，回答 `404 not_found`。
    pub async fn announcements(&self) -> Result<VrccAnnouncements, VrccError> {
        self.request(Call {
            method: "GET",
            path: "announcements",
            body: None,
            authed: false,
            key: "announcements",
            min_gap: Duration::from_secs(30),
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    // -- 设备授权 ----------------------------------------------------------

    /// 申请连接码。
    ///
    /// 官方定义（`POST /api/v1/device/code`）：正文可以不带；要指定权限就带
    /// `scope`。不写 `scope` 时按客户端登记状态取默认值（`dev-` 是
    /// `profile notifications maps`，`profile` 总会加上）。
    ///
    /// 所以这里传 `None` 表示「用官方默认」，而不是「不要权限」。
    ///
    /// **`scope` 走查询串，不走 JSON 正文。** 这一点是实测出来的，和官方
    /// `openapi.yaml` 不一致：
    ///
    /// | 写法 | 实测结果 |
    /// | --- | --- |
    /// | 不带正文 | `200` |
    /// | JSON 正文 `{"scope": …}` | `400 bad_request` |
    /// | 查询串 `?scope=…` | `200` |
    /// | 表单 `scope=…` | `200` |
    ///
    /// 文档说 400 只表示 `invalid_scope`，但要了 JSON 正文时现网回的是
    /// `bad_request`——也就是说服务器根本没解析 JSON 里的 scope。参考实现
    /// （`xingshi-app`）里「遇 400 退回查询串」的兼容逻辑是对的，这里直接
    /// 从一开始就发查询串，省掉一次注定失败的往返。
    pub async fn device_code(&self, scope: Option<&str>) -> Result<VrccDeviceCode, VrccError> {
        let path = match scope.map(str::trim).filter(|scope| !scope.is_empty()) {
            Some(scope) => format!("device/code?scope={}", encode_query_value(scope)),
            None => "device/code".to_string(),
        };
        self.request(Call {
            method: "POST",
            path: &path,
            body: None,
            authed: false,
            key: DEVICE_CODE_KEY,
            min_gap: Duration::from_secs(5),
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    /// 用设备码换令牌。玩家还没点同意时返回 `authorization_pending`，
    /// 调用方应按 `interval` 秒后再问；`access_denied` / `expired_token` 要停下。
    pub async fn device_token(&self, device_code: &str) -> Result<VrccTokenResponse, VrccError> {
        self.request(Call {
            method: "POST",
            path: "device/token",
            body: Some(json!({ "device_code": device_code })),
            authed: false,
            key: "device_token",
            min_gap: Duration::from_secs(5),
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    /// 断开：让服务器吊销当前令牌。
    ///
    /// **不受退避限制**（`bypass_gate`）：吊销是玩家明确要求的动作，而退避是
    /// 全局的——别的接口吃到一次 429 就会进入退避。被它挡住的话，本地令牌
    /// 已经清掉，这场授权就再也没有机会在服务器上吊销了。
    pub async fn disconnect(&self) -> Result<(), VrccError> {
        let _: serde_json::Value = self
            .request(Call {
                method: "POST",
                path: "disconnect",
                body: None,
                authed: true,
                key: "disconnect",
                min_gap: Duration::from_secs(5),
                bucket: None,
                bypass_gate: true,
            })
            .await?;
        Ok(())
    }

    // -- 个人接口 ----------------------------------------------------------

    /// 增量轮询提醒。首次不传 `since`，之后回传上次的 `last`。
    ///
    /// 官方硬性要求两次之间至少 25 秒，这里在限流层强制。它**没有每天上限**
    /// （官方《频率限制》），所以不扣任何一份额度——后台轮询就靠它。
    pub async fn notifications(&self, since: Option<i64>) -> Result<VrccNotifications, VrccError> {
        let path = match since {
            Some(cursor) => format!("me/notifications?since={cursor}"),
            None => "me/notifications".to_string(),
        };
        self.request(Call {
            method: "GET",
            path: &path,
            body: None,
            authed: true,
            key: "notifications",
            min_gap: NOTIFICATIONS_MIN_GAP,
            bucket: None,
            bypass_gate: false,
        })
        .await
    }

    /// 最近一天的最后几条提醒（第一份读）。
    pub async fn recent_notifications(
        &self,
        mark: bool,
    ) -> Result<VrccRecentNotifications, VrccError> {
        self.request(Call {
            method: "GET",
            path: recent_path(mark),
            body: None,
            authed: true,
            key: "recent",
            min_gap: Duration::from_secs(10),
            bucket: Some(QuotaBucket::Reads),
            bypass_gate: false,
        })
        .await
    }

    /// 几处未读数（1.13.0 起，`notifications` 权限）。读了不标已读。
    ///
    /// 每个授权每天 1500 次，单独计数，官方建议 1–2 分钟问一次。旧服务器没有
    /// 这个接口，回答 `404 not_found`。
    pub async fn unread(&self) -> Result<VrccUnread, VrccError> {
        self.request(Call {
            method: "GET",
            path: "me/unread",
            body: None,
            authed: true,
            key: "unread",
            min_gap: Duration::from_secs(25),
            bucket: Some(QuotaBucket::Unread),
            bypass_gate: false,
        })
        .await
    }

    /// 通知中心（1.13.0 起，`notifications` 权限，第二份读）。读了不标已读。
    pub async fn notes(&self) -> Result<VrccNotes, VrccError> {
        self.request(Call {
            method: "GET",
            path: "me/notes",
            body: None,
            authed: true,
            key: "notes",
            min_gap: Duration::from_secs(10),
            bucket: Some(QuotaBucket::MoreReads),
            bypass_gate: false,
        })
        .await
    }

    /// 当前所在地图在地图榜上的状态与我自己的评分（第一份读）。
    ///
    /// 需要 `maps`（普通权限，`dev-` 就能用）。这是官方插件「所在地图」那块
    /// 的数据来源：能看分、看人数、看自己打了几分。
    ///
    /// `world` 只能是 `wrld_…`，不要带房间实例号。
    pub async fn my_map(&self, world: &str) -> Result<VrccMyMap, VrccError> {
        self.request(Call {
            method: "GET",
            path: &format!("me/maps/{world}"),
            body: None,
            authed: true,
            key: "my_map",
            min_gap: Duration::from_secs(10),
            bucket: Some(QuotaBucket::Reads),
            bypass_gate: false,
        })
        .await
    }

    /// 今天这一轮路过的星 + 轮次进度。需要 `today` 权限（敏感，要审核）。
    ///
    /// 它是「网页上的动作」，算第二份读；停站时回答 `503 stop`。
    pub async fn today(&self) -> Result<VrccToday, VrccError> {
        self.request(Call {
            method: "GET",
            path: "me/today",
            body: None,
            authed: true,
            key: "today",
            min_gap: Duration::from_secs(30),
            bucket: Some(QuotaBucket::MoreReads),
            bypass_gate: false,
        })
        .await
    }

    /// 「互相连上」的星友列表（第一份读）。需要 `mutuals` 权限（敏感，要审核）。
    ///
    /// 条目里的 `vrcUser` 是对方的 VRChat 用户 ID，只在对方验证过且公开
    /// 「已验证」标志时才有。客户端拿它只能打开自己的玩家详情页，
    /// **不得代玩家发好友请求**——这条是官方的硬性合规要求。
    pub async fn mutuals(&self) -> Result<VrccMutuals, VrccError> {
        self.request(Call {
            method: "GET",
            path: "me/mutuals",
            body: None,
            authed: true,
            key: "mutuals",
            min_gap: Duration::from_secs(30),
            bucket: Some(QuotaBucket::Reads),
            bypass_gate: false,
        })
        .await
    }
}

/// 地图榜的路径。总榜不带 `sort`，和旧服务器上的写法一模一样。
fn boards_path(limit: u32, sort: VrccBoardSort) -> String {
    match sort {
        VrccBoardSort::Top => format!("boards/maps?limit={limit}"),
        other => format!("boards/maps?limit={limit}&sort={}", other.as_query()),
    }
}

/// 「最近的提醒」的路径。
///
/// 1.9.0 起 `?mark` 按字面算：只有 `1` / `true` 才标已读，`?mark=0` 不标。
/// 这里本来就只在要标的时候带 `?mark=1`，不标的时候整个参数都不带，新旧
/// 服务器上都对。
fn recent_path(mark: bool) -> &'static str {
    if mark {
        "me/notifications/recent?mark=1"
    } else {
        "me/notifications/recent"
    }
}

/// 离北京时间下一个零点还有几秒。服务器每天的次数都在这时重置。
fn secs_until_beijing_midnight() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    secs_until_beijing_midnight_from(now)
}

fn secs_until_beijing_midnight_from(unix_secs: u64) -> u64 {
    const DAY: u64 = 86_400;
    let beijing = unix_secs + 8 * 3600;
    (DAY - beijing % DAY).max(1)
}

/// 给人看的等待时长：`3 小时 20 分钟`、`5 分钟`、`40 秒`。
fn human_wait(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs} 秒");
    }
    let total_minutes = secs.div_ceil(60);
    match (total_minutes / 60, total_minutes % 60) {
        (0, minutes) => format!("{minutes} 分钟"),
        (hours, 0) => format!("{hours} 小时"),
        (hours, minutes) => format!("{hours} 小时 {minutes} 分钟"),
    }
}

/// 把 scope 编成查询串可用形式。
///
/// scope 是空格分隔的权限名（profile notifications maps），查询串里空格要
/// 编成 %20（官方示例用的是 +，两者都收，%20 更不容易被别的层误解）。
fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrcx_0_contracts::vrcc::{is_dev_client, VRCC_BUILD_CLIENT, VRCC_DEV_CLIENT};

    /// 构建期没注入时，解析结果必须就是开发态标识。
    ///
    /// 这条是「克隆下来直接能跑」的保证：不能解析出空串。
    fn expected_client() -> String {
        if VRCC_BUILD_CLIENT.trim().is_empty() {
            VRCC_DEV_CLIENT.to_string()
        } else {
            VRCC_BUILD_CLIENT.trim().to_string()
        }
    }

    fn client() -> VrccClient {
        VrccClient::new("dev-test", Arc::new(|| Some("tok".to_string())), None)
    }

    /// 标识的来源只有两层：构建期注入 > 源码默认，**没有运行时覆盖**。
    ///
    /// 这条钉住的是「同一个二进制永远用同一个标识」。留运行时覆盖就等于允许
    /// 用配置文件冒充已审核客户端，所以这里连参数都不该有。
    #[test]
    fn client_id_is_locked_at_build_time() {
        // 解析结果就是构建期那一层，不接受任何外部输入。
        assert_eq!(resolve_client_id(), expected_client());
        // 没注入时必须落到开发态，不能是空串——空 `X-Client` 每个请求都 403。
        if VRCC_BUILD_CLIENT.trim().is_empty() {
            assert_eq!(resolve_client_id(), VRCC_DEV_CLIENT);
        }
    }

    /// 无论注入成什么样，**发出去的那个值**必须是干净的非空串。
    ///
    /// 这是唯一真正重要的不变式：`option_env!` 拿到的是构建环境的原始串，
    /// CI 拼接时带空白或换行很常见，而带空白的 `X-Client` 会让**每一个**
    /// 接口都 `403 client_unknown`，报错里又看不出是空格的问题。
    ///
    /// 注意断言的是解析结果、不是常量本身：注入了纯空白时常量确实还是
    /// `"   "`，但 `resolve_client_id` 会回落到开发态——这种「常量脏、结果
    /// 干净」的状态是合法的，不该让构建失败。
    #[test]
    fn resolved_client_id_is_always_clean() {
        let resolved = resolve_client_id();
        assert!(!resolved.is_empty(), "X-Client must never be empty");
        assert_eq!(
            resolved,
            resolved.trim(),
            "resolved X-Client must not carry surrounding whitespace"
        );
    }

    /// 开发态判定只看 `dev-` 前缀，这是官方的判定方式。
    #[test]
    fn dev_client_is_detected_by_prefix() {
        assert!(is_dev_client(VRCC_DEV_CLIENT));
        assert!(is_dev_client("  dev-anything  "));
        assert!(!is_dev_client("app_0123456789abcdef"));
        assert!(!is_dev_client(""));
    }

    /// 不写 `scope` 时自助登记的 `app_…` 只有 `profile`，所以两种标识都要
    /// 显式地要；`dev-` 要了敏感权限会连码都拿不到。
    #[test]
    fn device_auth_scope_depends_on_the_client_kind() {
        use vrcx_0_contracts::vrcc::{
            device_auth_scope, VRCC_DEVICE_AUTH_SCOPE, VRCC_DEV_DEVICE_AUTH_SCOPE,
        };
        assert_eq!(
            device_auth_scope(VRCC_DEV_CLIENT),
            VRCC_DEV_DEVICE_AUTH_SCOPE
        );
        assert_eq!(
            device_auth_scope("app_0123456789abcdef"),
            VRCC_DEVICE_AUTH_SCOPE
        );
        assert!(!VRCC_DEV_DEVICE_AUTH_SCOPE.contains("mutuals"));
        for scope in [VRCC_DEVICE_AUTH_SCOPE, VRCC_DEV_DEVICE_AUTH_SCOPE] {
            assert!(scope.starts_with("profile notifications maps"));
        }
    }

    #[test]
    fn scope_grants_reads_the_token_scope() {
        use vrcx_0_contracts::vrcc::scope_grants;
        assert!(!scope_grants(Some("profile"), "notifications"));
        assert!(scope_grants(
            Some("profile notifications maps"),
            "notifications"
        ));
        assert!(scope_grants(Some("profile,notifications"), "notifications"));
        assert!(scope_grants(Some("profile+notifications"), "notifications"));
        // 只认整项，不认前缀：`maps.rate` 不等于 `maps`。
        assert!(!scope_grants(Some("profile maps.rate"), "maps"));
        // 旧存档没记权限：交给服务器判断。
        assert!(scope_grants(None, "notifications"));
    }

    /// 开发态常量本身必须符合官方的 `dev-` 前缀要求——
    /// 否则每个请求都会吃 `403 client_unknown`。
    #[test]
    fn dev_client_constant_is_a_valid_dev_id() {
        assert!(
            VRCC_DEV_CLIENT.starts_with("dev-"),
            "dev client must start with `dev-`, got {VRCC_DEV_CLIENT:?}"
        );
        assert!(
            VRCC_DEV_CLIENT.len() > 4,
            "dev client must have a name after the prefix"
        );
    }

    #[test]
    fn absolute_paths_are_rejected() {
        assert!(matches!(
            VrccClient::validate_path("https://evil.example/x"),
            Err(VrccError::AbsolutePath(_))
        ));
        assert!(matches!(
            VrccClient::validate_path("//evil.example/x"),
            Err(VrccError::AbsolutePath(_))
        ));
        assert!(VrccClient::validate_path("boards/maps?limit=5").is_ok());
        assert!(VrccClient::validate_path("me/notifications?since=1").is_ok());
    }

    #[test]
    fn gone_error_is_detected() {
        let error = VrccError::Api {
            status: 404,
            error: "gone".to_string(),
            need: None,
            why: None,
        };
        assert!(error.is_gone());
        assert!(error.needs_reauth());
    }

    #[test]
    fn star_gone_and_not_found_do_not_need_reauth() {
        // 官方错误码表明确区分：对方不在了 / 地图不在榜上，令牌都没事。
        for code in ["star_gone", "not_found"] {
            let error = VrccError::Api {
                status: 404,
                error: code.to_string(),
                need: None,
                why: None,
            };
            assert!(!error.is_gone(), "{code} 不该被当成令牌失效");
            assert!(!error.needs_reauth(), "{code} 不该触发重新授权");
        }
    }

    #[test]
    fn reauth_matches_official_error_table() {
        // 官方表里这些都要玩家重新授权。
        for (status, code) in [
            (403u16, "client_mismatch"),
            (403, "client_unknown"),
            (403, "client_off"),
            (403, "client_missing"),
            (403, "scope"),
            (403, "scope_unreviewed"),
            (401, "token"),
        ] {
            let error = VrccError::Api {
                status,
                error: code.to_string(),
                need: Some("card".to_string()),
                why: None,
            };
            assert!(error.needs_reauth(), "{status} {code} 应要求重新授权");
        }
        // 这些不需要。
        assert!(!VrccError::RateLimited("too_many".into()).needs_reauth());
        let map_not_went = VrccError::Api {
            status: 403,
            error: "map_not_went".to_string(),
            need: Some("verify".to_string()),
            why: None,
        };
        assert!(!map_not_went.needs_reauth(), "打分前置条件不要求重新授权");
    }

    #[test]
    fn device_flow_errors_are_classified() {
        let pending = VrccError::Api {
            status: 400,
            error: "authorization_pending".to_string(),
            need: None,
            why: None,
        };
        assert!(pending.is_authorization_pending());
        assert!(!pending.is_device_auth_terminal());

        for code in ["access_denied", "expired_token"] {
            let error = VrccError::Api {
                status: 400,
                error: code.to_string(),
                need: None,
                why: None,
            };
            assert!(error.is_device_auth_terminal(), "{code}");
            assert!(!error.is_authorization_pending(), "{code}");
        }
    }

    /// 钉住 `scope` 的编码方式。
    ///
    /// 相关内容：`device/code` 的 `scope` 必须走查询串——实测 JSON 正文里带
    /// `scope` 会被回 `400 bad_request`，查询串与表单都回 `200`。
    ///
    /// scope 是空格分隔的权限名，查询串里空格必须编码。编码函数写成净函数，
    /// 这样不依赖网络也能钉住行为。
    #[test]
    fn scope_is_encoded_for_the_query_string() {
        assert_eq!(
            encode_query_value("profile notifications maps"),
            "profile%20notifications%20maps"
        );
        // 权限名本身只含安全字符，不该被过度编码。
        assert_eq!(encode_query_value("maps.rate"), "maps.rate");
        // 其它字符一律编码，避免拼进 URL 时被误解。
        assert_eq!(encode_query_value("a&b=c"), "a%26b%3Dc");
    }

    /// `device/code` 的 400 分类：不是终态、也不是「等待中」。
    ///
    /// 现网对「JSON 正文带 scope」回的是 `bad_request`，对不认识的权限名回
    /// `invalid_scope`。两者都不是必须放弃授权流程的错误——玩家可以改了
    /// scope 再试，和 `access_denied` / `expired_token` 那种必须停下的不同。
    #[test]
    fn device_code_bad_request_is_recoverable() {
        for code in ["invalid_scope", "bad_request"] {
            let error = VrccError::Api {
                status: 400,
                error: code.to_string(),
                need: None,
                why: None,
            };
            assert!(!error.is_device_auth_terminal(), "{code}");
            assert!(!error.is_authorization_pending(), "{code}");
            assert!(!error.needs_reauth(), "{code}");
        }
        // 缺的权限要能带出去，界面才能告诉玩家是哪个名字写错了。
        let invalid_scope = VrccError::Api {
            status: 400,
            error: "invalid_scope".to_string(),
            need: Some("maps.rate".to_string()),
            why: None,
        };
        assert_eq!(invalid_scope.needed_scope(), Some("maps.rate"));
    }

    #[test]
    fn too_soon_is_not_a_quota_problem() {
        // 官方把 429 分成两种：too_soon 是问勤了，too_many 是额度用完。
        // 前者不该被当成需要长时间退避的配额问题。
        let too_soon = VrccError::TooSoon {
            retry_after_secs: 25,
        };
        assert!(!too_soon.needs_reauth());
        assert!(too_soon.to_string().contains("25"));

        let too_many = VrccError::RateLimited("too_many".into());
        assert!(!too_many.needs_reauth());
    }

    #[test]
    fn recent_notification_urls_are_relative_and_get_prefixed() {
        use vrcx_0_contracts::vrcc::VrccRecentNotificationItem;
        // 官方 recent 的 url 是相对路径（以 /match/ 开头）。
        let item = VrccRecentNotificationItem {
            url: Some("/match/?tab=inbox".to_string()),
            ..Default::default()
        };
        assert_eq!(item.absolute_url(), "https://vrcc.cc/match/?tab=inbox");

        // 已经是绝对地址就原样用。
        let absolute = VrccRecentNotificationItem {
            url: Some("https://vrcc.cc/match/?x=1".to_string()),
            ..Default::default()
        };
        assert_eq!(absolute.absolute_url(), "https://vrcc.cc/match/?x=1");

        // 没有 url 回到站点首页。
        let none = VrccRecentNotificationItem::default();
        assert_eq!(none.absolute_url(), "https://vrcc.cc/");
    }

    /// 没拿到过服务端配额时，`spend` 一律放行。
    ///
    /// 早先这里会按本地自然日预扣到 1000 就自己停下。那是错的：官方按北京时间
    /// 零点重置，本地推算的日期对非 UTC+8 的玩家必然错开，结果就是「服务端
    /// 明明还有余量，客户端自己先拒绝」。现在宁可让服务端回 429。
    #[tokio::test]
    async fn spend_passes_when_no_server_quota_is_known() {
        let client = client();
        assert!(client.spend(QuotaBucket::Reads).is_ok());
        assert!(client.local_usage().reads.is_none());
    }

    /// 服务端说过余量是 0 时，不再把请求发出去。
    #[tokio::test]
    async fn spend_stops_once_the_server_says_the_day_is_used_up() {
        let client = client();
        client.note_quota(
            QuotaBucket::Reads,
            ServerQuota {
                limit: 1000,
                remaining: 0,
                reset_secs: 3600,
            },
        );
        let error = client.spend(QuotaBucket::Reads).unwrap_err();
        assert!(matches!(error, VrccError::QuotaExhausted(_)), "{error:?}");
    }

    /// `spend` 与 `refund` 必须对称：请求没发出去时不能白占一次额度。
    #[test]
    fn refunding_a_spend_restores_the_server_count() {
        let client = client();
        client.note_quota(
            QuotaBucket::Reads,
            ServerQuota {
                limit: 1000,
                remaining: 10,
                reset_secs: 3600,
            },
        );
        let before = client.local_usage().reads.expect("server quota").0;
        client.spend(QuotaBucket::Reads).expect("spend");
        let after = client.local_usage().reads.expect("server quota").0;
        assert_eq!(after, before + 1);
        client.refund(QuotaBucket::Reads);
        assert_eq!(client.local_usage().reads.expect("server quota").0, before);
    }

    /// 没预扣就退还，不该把余量撑到超过上限。
    #[test]
    fn refunding_never_exceeds_the_server_limit() {
        let client = client();
        client.note_quota(
            QuotaBucket::Reads,
            ServerQuota {
                limit: 1000,
                remaining: 1000,
                reset_secs: 3600,
            },
        );
        client.refund(QuotaBucket::Reads);
        let (used, limit) = client.local_usage().reads.expect("server quota");
        assert_eq!(used, 0);
        assert_eq!(limit, 1000);
    }

    /// `/device/code` 不再有本地计数，也不再本地拦截。
    ///
    /// 那个额度是**按网络出口 IP** 算的（每天 30 个，同一路由下所有人共用），
    /// 本客户端无从得知、也不该假装知道。玩家换网络本来能继续用，本地拦着
    /// 就成了凭空拒绝。
    #[test]
    fn device_code_is_not_counted_locally() {
        let client = client();
        let usage = client.local_usage();
        // 快照里已经没有这一项了——不存在「本客户端今日申请了几个码」这个数。
        assert_eq!(usage.reads, None);
        assert_eq!(usage.unread, None);
    }

    #[test]
    fn body_over_2kb_is_rejected() {
        // 官方限制 POST 正文 2 KB，客户端在发送前就拦。
        //
        // 这条测试以前是假的：它只断言 `"x".repeat(MAX+1).len() > MAX`，
        // 那是恒真的算术，根本没碰到被测代码。现在直接调拦截函数。
        assert!(
            matches!(
                check_body_size(MAX_BODY_BYTES + 1),
                Err(VrccError::BodyTooLarge)
            ),
            "超 1 字节就该拦"
        );
        // 正好等于上限要通过：官方说的是「最大 2 KB」，边界不算超。
        assert!(check_body_size(MAX_BODY_BYTES).is_ok());
        assert!(check_body_size(0).is_ok());
        // 断言上限本身没被手滑改小。
        assert_eq!(MAX_BODY_BYTES, 2048);
    }

    // -- 1.9.0–1.14.0：限流响应头、错误码、按字面算的 mark ----------------

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    fn api_error(status: u16, code: &str) -> VrccError {
        VrccError::Api {
            status,
            error: code.to_string(),
            need: None,
            why: None,
        }
    }

    #[test]
    fn rate_headers_are_read_when_present() {
        let rate = RateHeaders::from_headers(&headers(&[
            ("Retry-After", "42"),
            ("RateLimit-Limit", "1000"),
            ("RateLimit-Remaining", "588"),
            ("RateLimit-Reset", "3600"),
        ]));
        assert_eq!(rate.retry_after_secs, Some(42));
        assert_eq!(
            rate.quota,
            Some(ServerQuota {
                limit: 1000,
                remaining: 588,
                reset_secs: 3600
            })
        );
        assert_eq!(rate.quota.unwrap().used(), 412);
    }

    #[test]
    fn missing_or_broken_rate_headers_mean_nothing() {
        // 旧服务器（1.8.x）什么都不给。
        assert_eq!(
            RateHeaders::from_headers(&HeaderMap::new()),
            RateHeaders::default()
        );
        // 写坏的头不该让请求失败，也不该被当成「用完了」。
        let rate = RateHeaders::from_headers(&headers(&[
            ("Retry-After", "Wed, 21 Oct 2026 07:28:00 GMT"),
            ("RateLimit-Limit", "lots"),
            ("RateLimit-Remaining", "1"),
        ]));
        assert_eq!(rate, RateHeaders::default());
        // 只有上限、没有余量，等于没说。
        let half = RateHeaders::from_headers(&headers(&[("RateLimit-Limit", "1000")]));
        assert_eq!(half.quota, None);
    }

    /// 同一个 `busy` 串在 409 和 429 下含义相反，必须走不同的变体。
    ///
    /// 钉住这条是因为它极容易写错：只要在 `request()` 里先按 429 分流、
    /// 或者用 `error == "busy"` 一处判断，就会把两种语义混成一种——
    /// 前者让 `409 busy` 掉回 `Api { status: 409 }` 被前端显示成「操作失败」，
    /// 后者把 `429 busy` 说成「今天的次数用完了」，可玩家一次都没用。
    #[test]
    fn busy_means_different_things_by_status() {
        assert!(
            matches!(
                VrccClient::classify_status_error(409, "busy"),
                Some(VrccError::NotReady)
            ),
            "409 busy 是「星卡还在生成」，应归 NotReady"
        );
        assert!(
            matches!(
                VrccClient::classify_status_error(429, "busy"),
                Some(VrccError::ConnectPaused)
            ),
            "429 busy 是「连接功能暂停到次日」，应归 ConnectPaused"
        );
        // 别的组合不许被这条短路抢走，仍要走原来的路径。
        for (code, error) in [
            (429, "too_many"),
            (429, "too_soon"),
            (429, "quota"),
            (409, "yq_wait"),
            (409, "msg_wait"),
            (503, "stop"),
            (403, "scope"),
        ] {
            assert!(
                VrccClient::classify_status_error(code, error).is_none(),
                "{code} {error} 不该被 busy 规则接走"
            );
        }
    }

    #[test]
    fn too_soon_waits_for_retry_after_over_every() {
        let client = client();
        let body = VrccErrorBody {
            every: Some(60),
            ..Default::default()
        };
        let rate = RateHeaders {
            retry_after_secs: Some(7),
            quota: None,
        };
        // 1.9.0 起 `Retry-After` 是 25 秒间隔还剩的秒数，比 `every` 准。
        match client.on_too_many("too_soon", &body, rate, None, None) {
            VrccError::TooSoon { retry_after_secs } => assert_eq!(retry_after_secs, 7),
            other => panic!("expected TooSoon, got {other:?}"),
        }
        // 旧服务器没有头：退回 `every`。
        match client.on_too_many("too_soon", &body, RateHeaders::default(), None, None) {
            VrccError::TooSoon { retry_after_secs } => assert_eq!(retry_after_secs, 60),
            other => panic!("expected TooSoon, got {other:?}"),
        }
        // too_soon 不触发全局退避。
        assert!(client.local_usage().backoff_remaining.is_zero());
    }

    #[test]
    fn minute_limit_backs_off_exactly_as_told() {
        let client = client();
        let rate = RateHeaders {
            retry_after_secs: Some(60),
            quota: None,
        };
        let error = client.on_too_many("too_many", &VrccErrorBody::default(), rate, None, None);
        assert!(matches!(error, VrccError::RateLimited(_)), "{error:?}");
        let backoff = client.local_usage().backoff_remaining;
        assert!(backoff > Duration::from_secs(55) && backoff <= Duration::from_secs(60));
        // 第二次照样按服务器说的等，不再自己翻倍。
        client.on_too_many("too_many", &VrccErrorBody::default(), rate, None, None);
        assert!(client.local_usage().backoff_remaining <= Duration::from_secs(60));
    }

    #[test]
    fn old_server_429_without_retry_after_still_backs_off() {
        let client = client();
        let error = client.on_too_many(
            "too_many",
            &VrccErrorBody::default(),
            RateHeaders::default(),
            None,
            None,
        );
        assert!(matches!(error, VrccError::RateLimited(_)));
        assert!(client.local_usage().backoff_remaining > Duration::from_secs(55));
    }

    #[test]
    fn a_used_up_day_stops_only_that_quota() {
        let client = client();
        // 第二份读今天用完了：服务器回 429，余量 0，`Retry-After` 到北京时间零点。
        let rate = RateHeaders {
            retry_after_secs: Some(5 * 3600),
            quota: Some(ServerQuota {
                limit: 1000,
                remaining: 0,
                reset_secs: 5 * 3600,
            }),
        };
        let error = client.on_too_many(
            "too_many",
            &VrccErrorBody::default(),
            rate,
            Some(QuotaBucket::MoreReads),
            None,
        );
        assert!(matches!(error, VrccError::QuotaExhausted(_)), "{error:?}");
        assert!(error.to_string().contains("5 小时"), "{error}");
        // 不让所有接口一起退避五个小时。
        assert!(client.local_usage().backoff_remaining.is_zero());
        // 这一份在零点前不再发请求……
        assert!(matches!(
            client.spend(QuotaBucket::MoreReads),
            Err(VrccError::QuotaExhausted(_))
        ));
        // ……别的几份照常。
        assert!(client.spend(QuotaBucket::Reads).is_ok());
        assert!(client.spend(QuotaBucket::Unread).is_ok());
    }

    #[test]
    fn long_retry_after_without_quota_headers_is_a_daily_limit() {
        // `device/code` 每个网络地址每天 30 个：429 只带 `Retry-After`（到零点）。
        let client = client();
        let rate = RateHeaders {
            retry_after_secs: Some(8 * 3600),
            quota: None,
        };
        let error = client.on_too_many("too_many", &VrccErrorBody::default(), rate, None, None);
        assert!(matches!(error, VrccError::QuotaExhausted(_)), "{error:?}");
        assert!(client.local_usage().backoff_remaining.is_zero());
    }

    /// 服务端给的数直接进快照，不再覆盖什么本地计数。
    #[test]
    fn server_quota_shows_up_in_the_snapshot() {
        let client = client();
        // 还没拿到过头：什么都不显示，也绝不编一个数出来。
        let before = client.local_usage();
        assert_eq!(before.reads, None);
        assert_eq!(before.reset_secs, None);

        // 服务器说这个授权今天已经用了 412 次。
        client.note_quota(
            QuotaBucket::Reads,
            ServerQuota {
                limit: 1000,
                remaining: 588,
                reset_secs: 3600,
            },
        );
        let after = client.local_usage();
        assert_eq!(after.reads, Some((412, 1000)));
        // 重置时间来自服务端（到北京时间零点的秒数），不是本地推算的。
        assert!(after.reset_secs.is_some_and(|secs| secs <= 3600));
        // 未读数那一份还没拿到过头，不显示。
        assert_eq!(after.unread, None);

        client.note_quota(
            QuotaBucket::Unread,
            ServerQuota {
                limit: 1500,
                remaining: 1400,
                reset_secs: 3600,
            },
        );
        assert_eq!(client.local_usage().unread, Some((100, 1500)));
    }

    /// 服务器哪天把上限调高了，本地不该有任何理由拦住。
    ///
    /// 现在本地根本没有上限这个概念了，这条钉住「没有本地上限」。
    #[test]
    fn a_raised_server_limit_is_never_capped_locally() {
        let client = client();
        client.note_quota(
            QuotaBucket::Reads,
            ServerQuota {
                limit: 2000,
                remaining: 1000,
                reset_secs: 3600,
            },
        );
        assert!(client.spend(QuotaBucket::Reads).is_ok());
        assert!(client.spend(QuotaBucket::Reads).is_ok());
    }

    #[test]
    fn server_saying_none_left_stops_before_the_request() {
        let client = client();
        client.note_quota(
            QuotaBucket::Unread,
            ServerQuota {
                limit: 1500,
                remaining: 0,
                reset_secs: 600,
            },
        );
        assert!(matches!(
            client.spend(QuotaBucket::Unread),
            Err(VrccError::QuotaExhausted(_))
        ));
    }

    #[test]
    fn expired_method_and_star_gone_are_not_token_loss() {
        // 1.9.0：签名图片失效改成 `404 expired`，`gone` 只表示令牌失效。
        // 1.11.0：方法不对是 `405 method`（原来是 `404 not_found`）。
        for (status, code) in [
            (404u16, "expired"),
            (405, "method"),
            (404, "star_gone"),
            (404, "map_gone"),
            (404, "yq_gone"),
            (404, "not_found"),
        ] {
            let error = api_error(status, code);
            assert!(!error.is_gone(), "{status} {code}");
            assert!(!error.needs_reauth(), "{status} {code}");
        }
        assert!(api_error(404, "not_found").is_not_found());
        assert!(!api_error(404, "gone").is_not_found());
        assert!(!api_error(405, "method").is_not_found());
    }

    #[test]
    fn stop_reads_as_maintenance_not_as_reauth() {
        let paused = VrccError::Paused {
            say: Some("升级数据库，约半小时".to_string()),
            until: Some(1_793_000_000_000),
        };
        assert!(!paused.needs_reauth());
        assert_eq!(
            paused.to_string(),
            "星遇同频停站维护中：升级数据库，约半小时"
        );
        let silent = VrccError::Paused {
            say: None,
            until: None,
        };
        assert_eq!(silent.to_string(), "星遇同频停站维护中");
    }

    #[test]
    fn mark_is_only_sent_when_marking() {
        // 1.9.0 起 `?mark=0` 不再标已读；这里不标时干脆不带这个参数。
        assert_eq!(recent_path(true), "me/notifications/recent?mark=1");
        assert_eq!(recent_path(false), "me/notifications/recent");
        assert!(!recent_path(false).contains("mark"));
    }

    #[test]
    fn board_paths_keep_the_old_form_for_the_top_board() {
        assert_eq!(boards_path(20, VrccBoardSort::Top), "boards/maps?limit=20");
        assert_eq!(
            boards_path(20, VrccBoardSort::Week),
            "boards/maps?limit=20&sort=week"
        );
        assert_eq!(
            boards_path(5, VrccBoardSort::New),
            "boards/maps?limit=5&sort=new"
        );
    }

    #[test]
    fn beijing_midnight_and_waits_read_right() {
        // 2026-10-27 00:00 北京 = 2026-10-26 16:00 UTC。
        let midnight_utc = 1_793_030_400;
        assert_eq!(secs_until_beijing_midnight_from(midnight_utc - 60), 60);
        assert_eq!(secs_until_beijing_midnight_from(midnight_utc), 86_400);
        assert_eq!(human_wait(40), "40 秒");
        assert_eq!(human_wait(300), "5 分钟");
        assert_eq!(human_wait(3599), "1 小时");
        assert_eq!(human_wait(5 * 3600 + 20 * 60), "5 小时 20 分钟");
    }
}
