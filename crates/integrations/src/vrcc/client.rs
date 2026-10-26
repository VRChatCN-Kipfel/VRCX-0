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

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::json;
use vrcx_0_contracts::vrcc::{
    VrccBoard, VrccDeviceCode, VrccErrorBody, VrccMapDetail, VrccMutuals, VrccMyMap,
    VrccNotifications, VrccRecentNotifications, VrccStatus, VrccToday, VrccTokenResponse,
    VRCC_API_BASE, VRCC_DEFAULT_CLIENT,
};
use vrcx_0_core::proxy::with_remote_dns;

use super::throttle::{
    backoff_for, DailyUsage, RateLimiter, NOTIFICATIONS_MIN_GAP, PERSONAL_DAILY_LIMIT,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(25);
/// 官方限制 POST 正文最大 2 KB。
const MAX_BODY_BYTES: usize = 2048;

/// 客户端错误。`status` 保留 HTTP 状态码，前端要靠它区分可重试与不可重试。
#[derive(Debug, thiserror::Error)]
pub enum VrccError {
    #[error("星遇同频 token 缺失，请先连接")]
    TokenMissing,
    #[error("路径必须相对 /api/v1/：{0}")]
    AbsolutePath(String),
    #[error("请求正文超过 {MAX_BODY_BYTES} 字节上限")]
    BodyTooLarge,
    #[error("星遇同频限流：{0}")]
    RateLimited(String),
    /// 429 `too_soon`：两次轮询间隔太短。等 `retry_after_secs` 再问，
    /// 不触发长时间退避——这不是配额问题，只是问勤了。
    #[error("星遇同频轮询太快，{retry_after_secs} 秒后再问")]
    TooSoon { retry_after_secs: u64 },
    #[error("星遇同频额度已用尽：{0}")]
    QuotaExhausted(String),
    #[error("星遇同频接口返回 HTTP {status}（{error}）")]
    Api {
        status: u16,
        error: String,
        need: Option<String>,
        why: Option<String>,
    },
    #[error("星遇同频接口暂时不可用：{0}")]
    Transport(String),
    #[error("星遇同频返回无法解析：{0}")]
    Decode(String),
}

impl VrccError {
    /// 官方错误码表里的 `404 gone`：**令牌失效**（玩家断开了、同一个客户端又
    /// 授权了一次、星被删除）。要停下轮询并让玩家重新授权。
    ///
    /// 注意区分 `star_gone`（连线/私聊的对方不在了，令牌没事）和
    /// `not_found`（查的地图不在榜上），这两个**不该**触发重新授权。
    pub fn is_gone(&self) -> bool {
        matches!(
            self,
            VrccError::Api {
                status: 404,
                error,
                ..
            } if error == "gone"
        )
    }

    /// 需要重新授权的情况。照官方错误码表逐条来：
    /// - `404 gone`：令牌失效
    /// - `403 client_mismatch`：令牌不是发给这个 `X-Client` 的
    /// - `403 client_unknown` / `client_off` / `client_missing`：标识本身有问题
    /// - `401 token`：没带令牌或格式不对
    /// - `403 scope`：缺权限，要玩家重新授权时勾上 `need` 那一项
    pub fn needs_reauth(&self) -> bool {
        if self.is_gone() {
            return true;
        }
        matches!(self, VrccError::Api { status: 401, .. })
            || matches!(
                self,
                VrccError::Api {
                    status: 403,
                    error,
                    ..
                } if matches!(
                    error.as_str(),
                    "client_mismatch"
                        | "client_unknown"
                        | "client_off"
                        | "client_missing"
                        | "scope"
                        | "scope_unreviewed"
                )
            )
            || matches!(self, VrccError::TokenMissing)
    }

    /// 缺哪个权限。前端据此提示「需要重新授权并勾上 X」。
    pub fn needed_scope(&self) -> Option<&str> {
        match self {
            VrccError::Api { need, .. } => need.as_deref(),
            _ => None,
        }
    }

    /// 设备授权期间「玩家还没点同意」，调用方应等 `interval` 秒再来问。
    pub fn is_authorization_pending(&self) -> bool {
        matches!(
            self,
            VrccError::Api {
                status: 400,
                error,
                ..
            } if error == "authorization_pending"
        )
    }

    /// 设备授权走到终态：玩家取消，或码过期/已用过。
    pub fn is_device_auth_terminal(&self) -> bool {
        matches!(
            self,
            VrccError::Api {
                status: 400,
                error,
                ..
            } if error == "access_denied" || error == "expired_token"
        )
    }
}

/// 客户端共享状态：限流器与用量计数。
#[derive(Debug, Default)]
pub struct VrccClientState {
    limiter: RateLimiter,
    usage: DailyUsage,
    /// 429 退避截止时间。
    backoff_until: Option<Instant>,
    /// 连续 429 次数，用于指数退避。
    backoff_attempt: u32,
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

    pub fn personal_used_today(&self) -> i64 {
        self.usage.personal_used()
    }

    pub fn device_code_used_today(&self) -> i64 {
        self.usage.device_code_used()
    }
}

/// 客户端本地用量快照。
///
/// 口径是「**已用**多少」，不是「还剩」多少。官方没有查询剩余额度的接口，
/// 服务器按令牌计数，我们只看得到自己发过多少次。命名里刻意不带 remaining。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VrccLocalUsage {
    pub personal_used_today: i64,
    pub device_code_used_today: i64,
    pub window_used: usize,
    pub backoff_remaining: Duration,
    /// 用量计数的日期（本地时区 `YYYY-MM-DD`）。
    pub date: String,
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

    /// 用默认标识新建。
    pub fn with_default_client(
        token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        proxy_url: Option<&str>,
    ) -> Self {
        Self::new(VRCC_DEFAULT_CLIENT, token, proxy_url)
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// 读本地用量计数。
    ///
    /// 会顺带把跨天的计数清零——调用方通常是界面，正好一天看一次。
    pub fn local_usage(&self) -> VrccLocalUsage {
        let today = super::throttle::local_date_today();
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.usage.roll_to(&today);
        VrccLocalUsage {
            personal_used_today: state.personal_used_today(),
            device_code_used_today: state.device_code_used_today(),
            window_used: state.window_used(now),
            backoff_remaining: state.backoff_remaining(now),
            date: today,
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

    /// 检查退避与限流，必要时等待。
    async fn acquire(&self, key: &str, min_gap: Duration) -> Result<(), VrccError> {
        let now = Instant::now();
        let (backoff, wait) = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let backoff = state.backoff_remaining(now);
            let wait = state
                .limiter
                .check(key, min_gap, now + backoff)
                .err()
                .unwrap_or_default();
            (backoff, wait)
        };
        if !backoff.is_zero() {
            return Err(VrccError::RateLimited(format!(
                "退避中，还需 {} 秒",
                backoff.as_secs().max(1)
            )));
        }
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.limiter.record(key, now);
        Ok(())
    }

    fn note_rate_limited(&self) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let wait = backoff_for(state.backoff_attempt);
        state.backoff_attempt = state.backoff_attempt.saturating_add(1);
        state.backoff_until = Some(Instant::now() + wait);
        wait
    }

    fn note_success(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.backoff_attempt = 0;
        state.backoff_until = None;
    }

    /// 扣一次个人接口额度；额度不足直接返回 `QuotaExhausted`，不发请求。
    fn spend_personal(&self) -> Result<(), VrccError> {
        let today = super::throttle::local_date_today();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.usage.roll_to(&today);
        if !state.usage.spend_personal(1) {
            return Err(VrccError::QuotaExhausted(format!(
                "本客户端今日个人接口已用满 {PERSONAL_DAILY_LIMIT} 次"
            )));
        }
        Ok(())
    }

    pub fn spend_device_code(&self) -> Result<(), VrccError> {
        let today = super::throttle::local_date_today();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.usage.roll_to(&today);
        if !state.usage.spend_device_code() {
            return Err(VrccError::QuotaExhausted(
                "本客户端今日申请连接码次数已用尽".to_string(),
            ));
        }
        Ok(())
    }

    /// 核心请求。`authed` 决定是否带 `Bearer`。
    async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
        authed: bool,
        key: &str,
        min_gap: Duration,
    ) -> Result<T, VrccError> {
        Self::validate_path(path)?;
        self.acquire(key, min_gap).await?;

        let mut builder = self
            .http
            .request(
                method
                    .parse()
                    .map_err(|_| VrccError::Transport(format!("不支持的请求方法 {method}")))?,
                format!("{}{path}", self.base),
            )
            .header("X-Client", &self.client_id)
            .header("Accept", "application/json");

        if authed {
            let token = (self.token)().ok_or(VrccError::TokenMissing)?;
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }

        if let Some(payload) = body {
            let text = payload.to_string();
            if text.len() > MAX_BODY_BYTES {
                return Err(VrccError::BodyTooLarge);
            }
            builder = builder
                .header("Content-Type", "application/json; charset=utf-8")
                .body(text);
        }

        let response = builder
            .send()
            .await
            .map_err(|error| VrccError::Transport(error.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| VrccError::Transport(error.to_string()))?;

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
            if code == 429 {
                // 官方把 429 分成两种，含义完全不同：
                // - `too_soon`：两次轮询间隔不到 25 秒。等一会儿再问就行，
                //   **不该**触发长时间退避，否则下一次正常轮询也会被自己拖慢。
                // - `too_many`：频率或当日额度用完。要退避。
                if error == "too_soon" {
                    let gap = parsed
                        .every
                        .filter(|every| *every > 0)
                        .map(|every| every as u64)
                        .unwrap_or(NOTIFICATIONS_MIN_GAP.as_secs());
                    return Err(VrccError::TooSoon {
                        retry_after_secs: gap,
                    });
                }
                let wait = self.note_rate_limited();
                return Err(VrccError::RateLimited(format!(
                    "{error}，退避 {} 秒",
                    wait.as_secs()
                )));
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
    pub async fn boards(&self, limit: u32) -> Result<VrccBoard, VrccError> {
        let limit = limit.clamp(1, 100);
        self.request(
            "GET",
            &format!("boards/maps?limit={limit}"),
            None,
            false,
            "boards",
            Duration::from_secs(60),
        )
        .await
    }

    /// 服务探活。不需要 client 标识，也不计入个人额度。
    pub async fn status(&self) -> Result<VrccStatus, VrccError> {
        self.request(
            "GET",
            "status",
            None,
            false,
            "status",
            Duration::from_secs(30),
        )
        .await
    }

    /// 单张地图详情。未上榜或未过审返回 `404 not_found`。
    pub async fn map(&self, world: &str) -> Result<VrccMapDetail, VrccError> {
        self.request(
            "GET",
            &format!("maps/{world}"),
            None,
            false,
            "map",
            Duration::from_secs(30),
        )
        .await
    }

    // -- 设备授权 ----------------------------------------------------------

    /// 申请连接码。
    ///
    /// 官方定义（`POST /api/v1/device/code`）：正文可以不带；要指定权限就带
    /// `scope`。正文也认表单，`scope` 也可以放查询串，**正文里有时以正文为准**。
    /// 不写 `scope` 时按客户端登记状态取默认值（`dev-` 是
    /// `profile notifications maps`，`profile` 总会加上）。
    ///
    /// 所以这里传 `None` 表示「用官方默认」，而不是「不要权限」。
    /// 申请连接码。
    ///
    /// 官方定义（`POST /api/v1/device/code`）：正文可以不带；要指定权限就带
    /// `scope`。不写 `scope` 时按客户端登记状态取默认值（`dev-` 是
    /// `profile notifications maps`，`profile` 总会加上）。
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
        self.spend_device_code()?;
        let path = match scope.map(str::trim).filter(|scope| !scope.is_empty()) {
            Some(scope) => format!("device/code?scope={}", encode_query_value(scope)),
            None => "device/code".to_string(),
        };
        self.request(
            "POST",
            &path,
            None,
            false,
            "device_code",
            Duration::from_secs(5),
        )
        .await
    }

    /// 用设备码换令牌。玩家还没点同意时返回 `authorization_pending`，
    /// 调用方应按 `interval` 秒后再问；`access_denied` / `expired_token` 要停下。
    pub async fn device_token(&self, device_code: &str) -> Result<VrccTokenResponse, VrccError> {
        let body = json!({ "device_code": device_code });
        self.request(
            "POST",
            "device/token",
            Some(body),
            false,
            "device_token",
            Duration::from_secs(5),
        )
        .await
    }

    /// 断开：让服务器吊销当前令牌。
    pub async fn disconnect(&self) -> Result<(), VrccError> {
        let _: serde_json::Value = self
            .request(
                "POST",
                "disconnect",
                None,
                true,
                "disconnect",
                Duration::from_secs(5),
            )
            .await?;
        Ok(())
    }

    // -- 个人接口（计每日额度） --------------------------------------------

    /// 增量轮询提醒。首次不传 `since`，之后回传上次的 `last`。
    ///
    /// 官方硬性要求两次之间至少 25 秒，这里在限流层强制。
    pub async fn notifications(&self, since: Option<i64>) -> Result<VrccNotifications, VrccError> {
        self.spend_personal()?;
        let path = match since {
            Some(cursor) => format!("me/notifications?since={cursor}"),
            None => "me/notifications".to_string(),
        };
        self.request(
            "GET",
            &path,
            None,
            true,
            "notifications",
            NOTIFICATIONS_MIN_GAP,
        )
        .await
    }

    /// 最近一天的最后几条提醒。
    pub async fn recent_notifications(
        &self,
        mark: bool,
    ) -> Result<VrccRecentNotifications, VrccError> {
        self.spend_personal()?;
        let path = if mark {
            "me/notifications/recent?mark=1"
        } else {
            "me/notifications/recent"
        };
        self.request("GET", path, None, true, "recent", Duration::from_secs(10))
            .await
    }

    /// 当前所在地图在地图榜上的状态与我自己的评分。
    ///
    /// 需要 `maps`（普通权限，`dev-` 就能用）。这是官方插件「所在地图」那块
    /// 的数据来源：能看分、看人数、看自己打了几分、现在能不能打分。
    ///
    /// `world` 只能是 `wrld_…`，不要带房间实例号。
    pub async fn my_map(&self, world: &str) -> Result<VrccMyMap, VrccError> {
        self.spend_personal()?;
        self.request(
            "GET",
            &format!("me/maps/{world}"),
            None,
            true,
            "my_map",
            Duration::from_secs(10),
        )
        .await
    }

    /// 今天这一轮路过的星 + 轮次进度。需要 `today` 权限（敏感，要审核）。
    pub async fn today(&self) -> Result<VrccToday, VrccError> {
        self.spend_personal()?;
        self.request(
            "GET",
            "me/today",
            None,
            true,
            "today",
            Duration::from_secs(30),
        )
        .await
    }

    /// 「互相连上」的星友列表。需要 `mutuals` 权限（敏感，要审核）。
    ///
    /// 条目里的 `vrcUser` 是对方的 VRChat 用户 ID，只在对方验证过且公开
    /// 「已验证」标志时才有。客户端拿它只能打开自己的玩家详情页，
    /// **不得代玩家发好友请求**——这条是官方的硬性合规要求。
    pub async fn mutuals(&self) -> Result<VrccMutuals, VrccError> {
        self.spend_personal()?;
        self.request(
            "GET",
            "me/mutuals",
            None,
            true,
            "mutuals",
            Duration::from_secs(30),
        )
        .await
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
    use vrcx_0_contracts::vrcc::{
        is_dev_client, resolve_client_id, VRCC_DEV_CLIENT, VRCC_PROD_CLIENT,
    };

    fn client() -> VrccClient {
        VrccClient::new("dev-test", Arc::new(|| Some("tok".to_string())), None)
    }

    /// 标识解析的优先级：运行时覆盖 > 正式常量 > 开发常量。
    ///
    /// 这三者的关系值得钉住，因为正式标识是平台发的、**不是** `dev-` 那个字符串
    /// 去掉前缀——`dev-…` 与 `app_…` 是两种东西。写死一处常量迟早有人手改出错。
    #[test]
    fn client_id_resolution_prefers_override_then_prod_then_dev() {
        // 覆盖值优先。
        assert_eq!(
            resolve_client_id(Some("app_0123456789abcdef")),
            "app_0123456789abcdef"
        );
        // 空白覆盖值不该被采用（配置项被清成空串是很常见的事）。
        assert_eq!(resolve_client_id(Some("   ")), {
            if VRCC_PROD_CLIENT.is_empty() {
                VRCC_DEV_CLIENT.to_string()
            } else {
                VRCC_PROD_CLIENT.to_string()
            }
        });
        assert_eq!(resolve_client_id(None), {
            if VRCC_PROD_CLIENT.is_empty() {
                VRCC_DEV_CLIENT.to_string()
            } else {
                VRCC_PROD_CLIENT.to_string()
            }
        });
    }

    /// 开发态判定只看 `dev-` 前缀，这是官方的判定方式。
    #[test]
    fn dev_client_is_detected_by_prefix() {
        assert!(is_dev_client(VRCC_DEV_CLIENT));
        assert!(is_dev_client("  dev-anything  "));
        assert!(!is_dev_client("app_0123456789abcdef"));
        assert!(!is_dev_client(""));
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

    #[tokio::test]
    async fn personal_quota_stops_before_request() {
        let client = client();
        // 把额度用光
        {
            let today = super::super::throttle::local_date_today();
            let mut state = client.state.lock().unwrap();
            state.usage.roll_to(&today);
            assert!(state.usage.spend_personal(PERSONAL_DAILY_LIMIT));
        }
        let error = client.spend_personal().unwrap_err();
        assert!(matches!(error, VrccError::QuotaExhausted(_)));
    }

    #[test]
    fn body_over_2kb_is_rejected() {
        // 官方限制 POST 正文 2 KB，客户端在发送前就拦。
        let big = "x".repeat(MAX_BODY_BYTES + 1);
        assert!(big.len() > MAX_BODY_BYTES);
    }
}
