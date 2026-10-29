//! 星遇同频集成的应用服务。
//!
//! 这一层负责编排：拿令牌 → 调接口 → 投影成前端/腕表能用的形状。
//! 网络细节通过 [`VrccRemote`] 端口注入，持久化通过 [`VrccTokenStore`] 注入，
//! 两者的实现都在 outbound-adapters。本层不认识任何具体协议或存储。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use vrcx_0_application_core::Result;
use vrcx_0_contracts::vrcc::{
    device_auth_scope, is_dev_client, VrccAnnouncements, VrccBoard, VrccBoardSort,
    VrccConnectionSnapshot, VrccDeviceCode, VrccError, VrccMutuals, VrccMyMap, VrccNotes,
    VrccNotifications, VrccRecentNotifications, VrccStatus, VrccToday, VrccTokenResponse,
    VrccUnread, VrccUsageSnapshot, VrccWristSummary,
};

/// 服务器回答 `me/unread` 是 `404 not_found`（还没更新到 1.13.0）之后，
/// 多久再试一次。服务器更新了，客户端不用重启也能接上。
const UNREAD_RETRY_AFTER_NOT_FOUND: Duration = Duration::from_secs(3600);

/// 未读数的缓存。
#[derive(Debug, Default)]
struct UnreadCache {
    /// 最近一次 `me/unread` 的结果；还没问过、服务器没有这个接口时为 `None`。
    counts: Option<VrccUnread>,
    /// 服务器说没有这个接口时，到这个时刻之前不再问。
    skip_until: Option<Instant>,
}

/// 令牌仓库端口。实现留在 outbound-adapters。
pub trait VrccTokenStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredCredential>>;
    fn save(&self, credential: &StoredCredential) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// 一次异步调用的结果。前端/上层都按 [`VrccError`] 分类，所以这里固定用它的
/// 错误类型，而不是 `vrcx_0_application_core::Error`——两者语义不同：
/// `VrccError` 要能区分「重新授权 / 缺权限 / 限流 / 网络」，那是业务判定。
pub type VrccFuture<'a, T> = futures_util::future::BoxFuture<'a, std::result::Result<T, VrccError>>;

/// 会话状态快照（应用层视角）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VrccConnection {
    pub connected: bool,
    /// 令牌被服务器判过失效（`404 gone`），界面要说清是「连接已失效」。
    pub expired: bool,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
    /// 当前使用的 `X-Client` 标识。
    pub client: String,
    /// `me/notifications` 的游标；连上以后还没问过时为 `None`。
    pub notifications_cursor: Option<i64>,
    /// 上次成功轮询提醒的时间（毫秒）。
    pub last_poll_at: Option<i64>,
    /// 服务器建议的轮询间隔（秒，不低于 25）。
    pub poll_every_secs: i64,
}

/// 本客户端的用量快照（应用层视角）。
///
/// 全部来自服务端的 `RateLimit-*`。没拿到过就是 `None`——界面显示「未知」，
/// 不要拿本地日期推算一个数出来（官方按北京时间零点重置，本地算不准）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VrccLocalUsage {
    pub reads: Option<(i64, i64)>,
    pub unread: Option<(i64, i64)>,
    pub window_used: usize,
    pub backoff_remaining_ms: i64,
    pub reset_secs: Option<i64>,
}

/// 星遇同频网络端口。
///
/// **为什么需要这一层**：应用层要编排（先鉴权、再按语义算未读、决定要不要
/// 让玩家重连），但具体协议在 `integrations`。`backend_architecture` 规定
/// 应用层不得依赖出向实现，所以这里只声明能力，实现放在 `outbound-adapters`。
///
/// 所有方法都返回 [`VrccError`]，因为分类是应用层的事：同一个 404，
/// `gone` 要让玩家重连、`not_found` 只是接口还没上线。
pub trait VrccRemote: Send + Sync {
    // -- 开放数据（不需要连接） -------------------------------------------
    fn boards<'a>(&'a self, limit: u32, sort: VrccBoardSort) -> VrccFuture<'a, VrccBoard>;
    fn status<'a>(&'a self) -> VrccFuture<'a, VrccStatus>;
    fn fetch_site_asset<'a>(&'a self, path: &'a str) -> VrccFuture<'a, Vec<u8>>;
    fn announcements<'a>(&'a self) -> VrccFuture<'a, VrccAnnouncements>;

    // -- 设备授权 ----------------------------------------------------------
    fn device_code<'a>(&'a self, scope: Option<&'a str>) -> VrccFuture<'a, VrccDeviceCode>;
    fn device_token<'a>(&'a self, device_code: &'a str) -> VrccFuture<'a, VrccTokenResponse>;
    fn revoke<'a>(&'a self) -> VrccFuture<'a, ()>;

    // -- 个人接口（需要连接） ----------------------------------------------
    fn recent_notifications<'a>(&'a self, mark: bool) -> VrccFuture<'a, VrccRecentNotifications>;
    fn notifications<'a>(&'a self, since: Option<i64>) -> VrccFuture<'a, VrccNotifications>;
    fn my_map<'a>(&'a self, world: &'a str) -> VrccFuture<'a, VrccMyMap>;
    fn unread<'a>(&'a self) -> VrccFuture<'a, VrccUnread>;
    fn notes<'a>(&'a self) -> VrccFuture<'a, VrccNotes>;
    fn today<'a>(&'a self) -> VrccFuture<'a, VrccToday>;
    fn mutuals<'a>(&'a self) -> VrccFuture<'a, VrccMutuals>;

    // -- 本地状态（不联网） -------------------------------------------------
    /// 把令牌交给实现方持有。之后所有个人请求自动带上它。
    fn connect(
        &self,
        token: String,
        star: Option<i64>,
        vrc_name: Option<String>,
        scope: Option<String>,
    );
    /// 玩家主动断开：只清本地。
    fn disconnect(&self);
    /// 令牌被判失效（`404 gone`）：清本地并记下原因。
    fn expire(&self);
    fn connection(&self) -> VrccConnection;
    fn client_id(&self) -> String;
    fn local_usage(&self) -> VrccLocalUsage;
    /// 记一次轮询结果（游标与建议间隔）。
    fn note_poll(&self, at_ms: i64, cursor: Option<i64>, every_secs: Option<i64>);
    /// 限流窗口容量（每分钟）。
    fn window_limit(&self) -> i64;
    /// 当前毫秒时间戳，由实现方提供以保持「时间来自一处」。
    fn now_millis(&self) -> i64;
}

/// 吊销失败是否属于「请求没到服务器、值得再试一次」。
///
/// - `RateLimited` / `TooSoon`：被限流挡住，请求根本没发出去；
/// - `Transport`：网络或发送阶段失败。
///
/// 这三种都不该连带清掉本地令牌，否则服务器上的授权永远吊销不了。
/// 服务器答复里的错误（`Api` / `QuotaExhausted` / `Paused`）**不算**——
/// 那些说明请求已经到了，服务器自己决定拒绝，重试也不会变。
fn retriable_revoke_failure(error: &VrccError) -> bool {
    matches!(
        error,
        VrccError::RateLimited(_) | VrccError::TooSoon { .. } | VrccError::Transport(_)
    )
}

/// 前端能区分的行为分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrccErrorKind {
    /// 需要玩家重新授权。
    NeedsReauth,
    /// 缺某个权限，`need` 里说明是哪项。
    MissingScope,
    /// 限流或额度用尽，等等再来。
    Throttled,
    /// 服务端还没准备好，稍后再试（`409 busy`：这一轮的星卡还在生成）。
    ///
    /// 和 [`Self::Throttled`] 分开：那个是「你问太多了」，这个是「那边还没好」，
    /// 玩家能做的动作不同（前者该等，后者该过一会儿重试，都不用重连）。
    NotReady,
    /// 网络或服务端问题。
    Transport,
    /// 其它。
    Other,
}

/// 跨层传递的错误：**分类结论 + 缺的权限 + 原文**。
///
/// 为什么不是 `String`：错误分类必须在产生错误的地方算（那里知道 `VrccError`
/// 的确切变体），而消费它的地方（命令层、前端）不该靠解析中文文案反推。
/// 早先 runtime 层把错误折成 `to_string()`，前端再对文案做子串匹配，结果
/// `401 token` 落进 `Other`、`need` 在转字符串时直接丢失、`missing_scope`
/// 提示永远不显示——都是这一个设计问题带出来的。
///
/// `message` 保留原文只为日志与兜底显示；判断分支一律看 `kind` / `need`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VrccFailure {
    pub kind: VrccErrorKind,
    /// 缺哪个权限（`kind == MissingScope` 时通常有值）。
    pub need: Option<String>,
    /// 人可读的错误原文，用于日志与兜底显示。
    pub message: String,
}

impl VrccFailure {
    /// 从应用层错误造一个。分类在这里一次算清，后面不再重算。
    pub fn from_error(error: &VrccError) -> Self {
        let (kind, need) = classify_vrcc_error(error);
        Self {
            kind,
            need,
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for VrccFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// 判定一个错误属于哪一类，并把缺的权限一并带回。
pub fn classify_vrcc_error(error: &VrccError) -> (VrccErrorKind, Option<String>) {
    let need = error.needed_scope().map(str::to_string);
    let kind = match error {
        // `TooSoon` 是「问勤了」，归到 Throttled 只是为了让前端显示
        // 「稍后再试」；它不会触发长时间退避，也不消耗额度。
        VrccError::RateLimited(_) | VrccError::QuotaExhausted(_) | VrccError::TooSoon { .. } => {
            VrccErrorKind::Throttled
        }
        // `409 busy`：那边还没准备好，不是玩家的问题，也不是本机限流。
        VrccError::NotReady => VrccErrorKind::NotReady,
        // `429 busy`：全站连接功能暂停到次日。本机一次都没用，所以同样
        // 不算 Throttled（那会显示「今天的次数用完了」，是错的）。
        VrccError::ConnectPaused => VrccErrorKind::NotReady,
        // 停站维护：令牌没事，等一会儿就好，和网络问题一样处理。
        VrccError::Transport(_) | VrccError::Decode(_) | VrccError::Paused { .. } => {
            VrccErrorKind::Transport
        }
        VrccError::TokenMissing => VrccErrorKind::NeedsReauth,
        VrccError::Api { need, .. } if need.is_some() && error.needs_reauth() => {
            VrccErrorKind::MissingScope
        }
        _ if error.needs_reauth() => VrccErrorKind::NeedsReauth,
        VrccError::AbsolutePath(_) | VrccError::BodyTooLarge | VrccError::Api { .. } => {
            VrccErrorKind::Other
        }
    };
    (kind, need)
}

/// 仓库里的凭据形状（与 outbound-adapters 的实现解耦）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredCredential {
    pub token: String,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
    /// 签发这个令牌时用的 `X-Client`；旧存档没有这一项。
    ///
    /// 令牌只能配签发它的那个 client 使用，换了标识会 `403 client_mismatch`。
    /// 加载时和当前构建期标识比对，不一致就丢弃——
    /// 否则换标识后旧令牌会在启动时被恢复成「已连接」，之后每次请求都 403。
    pub client_id: Option<String>,
}

/// 一次设备授权的中间态。给前端显示用，不落盘。
#[derive(Clone, Debug)]
pub struct VrccDeviceAuthPrompt {
    pub user_code: String,
    pub verification_uri: Option<String>,
    pub verification_uri_complete: Option<String>,
    pub expires_in_secs: i64,
    pub interval_secs: i64,
    /// 内部用：换令牌时要回传，不给玩家看。
    pub device_code: String,
    pub scope: Option<String>,
}

/// 设备授权的下一步。
#[derive(Clone, Debug)]
pub enum VrccDeviceAuthStep {
    /// 玩家还没点同意，按 `interval_secs` 后再问。
    Pending { interval_secs: i64 },
    /// 拿到令牌，已保存。
    Connected(VrccConnectionSnapshot),
    /// 玩家取消或缺码过期，这条流程结束。
    Terminal(String),
}

/// 地图榜缓存：world 映射到（名次，均分 ×10）。
type BoardRanks = HashMap<String, (i64, Option<i64>)>;

/// 星遇同频服务。
pub struct VrccService {
    store: Arc<dyn VrccTokenStore>,
    /// 网络端口。实现是 `outbound-adapters` 里的适配器，本层不知道协议细节。
    remote: Arc<dyn VrccRemote>,
    /// 腕表摘要的缓存。
    ///
    /// 腕表每帧都会读它，所以这里必须是纯内存读取——不能在这里发请求。
    /// 内容由本来就会发的请求顺手填充：后台轮询的 `me/unread`（旧服务器上是
    /// `recent_notifications`）提供未读数，`boards` 提供地图名次（开放数据，
    /// 不占个人额度）。
    ///
    /// `None` 表示"还没拿到任何数据"，和"未读 0 条"是两件事：前者不该在腕表
    /// 上显示一个数字，后者才该显示 0。
    wrist: Arc<RwLock<Option<VrccWristSummary>>>,
    /// 最近一次地图榜：world 映射到（名次，均分 ×10）。
    ///
    /// 缓存整个榜单而不是只缓存"当前地图"：玩家随时换图，而榜单是开放数据，
    /// 一次请求就能覆盖上百张图，按图缓存没有意义。
    boards: Arc<RwLock<BoardRanks>>,
    /// 最近一次 `me/unread`。后台轮询填，页面和腕表读。
    unread: Arc<RwLock<UnreadCache>>,
    /// 最近一次成功拉到的地图榜，按排序方式分别记一份。
    ///
    /// 用来在限流间隔内接住 `TooSoon`：`boards` 的 `min_gap` 是 60 秒，
    /// 用户在这个窗口里点刷新不该看到错误，也不该干等一分钟。官方说开放
    /// 数据最多 5 分钟更新一次，旧的那份完全够用。
    boards_cache: Arc<RwLock<HashMap<VrccBoardSort, (u32, VrccBoard)>>>,
}

impl VrccService {
    /// 从仓库恢复会话：有令牌就直接进入已连接状态。
    pub fn new(store: Arc<dyn VrccTokenStore>, remote: Arc<dyn VrccRemote>) -> Self {
        if let Ok(Some(credential)) = store.load() {
            // 令牌只能配签发它的那个 `X-Client` 使用。存的标识和现在的不一样
            // （换了正式标识、或改了配置覆盖），这个令牌必然被服务器回
            // `403 client_mismatch`——与其恢复成「已连接」再让每次请求都失败，
            // 不如当作没连，让玩家走一次新的设备授权。
            let signed_by = credential.client_id.as_deref();
            let current = remote.client_id();
            let usable = match signed_by {
                // 旧存档没记标识：当时只有 `dev-` 一种，按兼容处理继续用。
                None => true,
                Some(id) => id == current,
            };
            if usable {
                remote.connect(
                    credential.token,
                    credential.star,
                    credential.vrc_name,
                    credential.scope,
                );
            } else {
                tracing::info!(
                    stored = signed_by.unwrap_or(""),
                    current = %current,
                    "vrcc: stored token was issued by a different X-Client; asking for a new authorization"
                );
                // 顺手清掉，免得每次启动都重新判断一遍。
                let _ = store.clear();
            }
        }
        Self {
            store,
            remote,
            wrist: Arc::new(RwLock::new(None)),
            boards: Arc::new(RwLock::new(HashMap::new())),
            unread: Arc::new(RwLock::new(UnreadCache::default())),
            boards_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 腕表摘要。`world_id` 由调用方给（腕表的渲染侧知道玩家在哪张图）。
    ///
    /// 纯内存读取，不发请求。返回 `None` 的三种情况：还没连上、还没拉到
    /// 任何数据、缓存锁坏了。腕表在 `None` 时整块不显示，底栏继续显示
    /// 原本的玩家数。
    ///
    /// **必须检查连接状态**：地图榜是开放数据，没连接也能拉到，而
    /// `note_boards` 一拉到就会把缓存从 `None` 变成 `Some`。若不在这里拦，
    /// 一个「从没连过星遇同频、只打开过一次页面」的玩家进入上榜地图后，
    /// 腕表底栏会从「12 名玩家」变成「#3 8.8」，而且没有任何设置能恢复。
    pub fn wrist_summary(&self, world_id: Option<&str>) -> Option<VrccWristSummary> {
        if !self.remote.connection().connected {
            return None;
        }
        let mut summary = self.wrist.read().ok()?.clone()?;
        // 地图名次从榜单缓存现查：玩家换图后不必等下一次 boards 请求。
        let rank = world_id.and_then(|world| {
            self.boards
                .read()
                .ok()
                .and_then(|boards| boards.get(world).copied())
        });
        summary.map_rank = rank.map(|(rank, _)| rank);
        summary.map_avg_x10 = rank.and_then(|(_, avg)| avg);
        Some(summary)
    }

    /// 拿到 `recent` 结果后更新未读数。
    ///
    /// 顺手把缓存从 `None` 变成 `Some`：这已经是真实数据了，腕表可以显示。
    ///
    /// 服务器有 `me/unread` 时，腕表显示的是那几处红点的合计，这里就不覆盖它；
    /// 只有旧服务器（没有 `me/unread`）才用「最近的提醒」里 `id > read` 的条数。
    fn note_recent(&self, unread: u32) {
        if self.cached_unread().is_some() {
            return;
        }
        let Ok(mut wrist) = self.wrist.write() else {
            return;
        };
        let entry = wrist.get_or_insert_with(VrccWristSummary::default);
        entry.unread = unread;
    }

    /// 拿到 `me/unread` 后更新缓存和腕表。返回和上一次比有没有变。
    fn note_unread(&self, counts: &VrccUnread) -> bool {
        let changed = {
            let Ok(mut cache) = self.unread.write() else {
                return false;
            };
            let changed = cache.counts.as_ref() != Some(counts);
            cache.counts = Some(counts.clone());
            cache.skip_until = None;
            changed
        };
        if let Ok(mut wrist) = self.wrist.write() {
            let entry = wrist.get_or_insert_with(VrccWristSummary::default);
            entry.unread = u32::try_from(counts.total()).unwrap_or(u32::MAX);
        }
        changed
    }

    /// 断开或令牌失效后，未读数不再属于任何人：清掉，腕表回到 0。
    fn forget_unread(&self) {
        if let Ok(mut cache) = self.unread.write() {
            cache.counts = None;
        }
        if let Ok(mut wrist) = self.wrist.write() {
            if let Some(entry) = wrist.as_mut() {
                entry.unread = 0;
            }
        }
    }

    /// 最近一次 `me/unread` 的结果，纯内存读取。
    pub fn cached_unread(&self) -> Option<VrccUnread> {
        self.unread.read().ok()?.counts.clone()
    }

    /// 收拾一个错误：`404 gone` 表示令牌已经失效，清掉它。
    ///
    /// 官方的要求是「收到 `404 gone` 就停下轮询，让玩家重新授权」。不清的话，
    /// 后台轮询会拿着一个死令牌每分钟问一次，界面上也一直显示「已连接」。
    /// 1.10.0 起「90 天没调用过任何接口」也会这样，所以这不是罕见情况。
    fn settle(&self, error: VrccError) -> VrccError {
        if error.is_gone() {
            tracing::info!(
                "vrcc: token is gone (disconnected on the site, re-authorized elsewhere, or unused for 90 days); dropping it"
            );
            if let Err(clear_error) = self.store.clear() {
                tracing::warn!(%clear_error, "vrcc: failed to clear the dead token");
            }
            self.remote.expire();
            self.forget_unread();
        }
        error
    }

    /// 拿到地图榜后更新名次缓存。
    ///
    /// 榜单是开放数据，不带令牌也能拉。但这里**不把腕表缓存设成 `Some`**：
    /// 那会让「从没连过、只打开过一次页面」的玩家也在腕表底栏看到名次，
    /// 把原本的玩家数顶掉。名次只在已经连上、且腕表本来就有数据时才显示，
    /// 由 `wrist_summary` 现查缓存补上。
    fn note_boards(&self, board: &VrccBoard) {
        let Ok(mut boards) = self.boards.write() else {
            return;
        };
        boards.clear();
        for map in &board.maps {
            let Some(rank) = map.rank else { continue };
            let avg_x10 = map.avg.map(|avg| (avg * 10.0).round() as i64);
            boards.insert(map.world.clone(), (rank, avg_x10));
        }
    }

    /// 记下这一份榜单，供限流窗口内复用。
    fn remember_board(&self, limit: u32, sort: VrccBoardSort, board: &VrccBoard) {
        let Ok(mut cache) = self.boards_cache.write() else {
            return;
        };
        cache.insert(sort, (limit, board.clone()));
    }

    /// 取上次那份榜单，条件严格：**排序方式和条数都要一样**。
    ///
    /// 条数不一样就不给：请求 50 条却拿到 20 条的旧数据，界面会以为这就是
    /// 全部内容。
    fn cached_board(&self, limit: u32, sort: VrccBoardSort) -> Option<VrccBoard> {
        let cache = self.boards_cache.read().ok()?;
        let (cached_limit, board) = cache.get(&sort)?;
        (*cached_limit == limit).then(|| board.clone())
    }

    pub fn client_id(&self) -> String {
        self.remote.client_id()
    }

    pub fn snapshot(&self) -> VrccConnectionSnapshot {
        let connection = self.remote.connection();
        let client_is_dev = is_dev_client(&connection.client);
        VrccConnectionSnapshot {
            connected: connection.connected,
            expired: connection.expired,
            star: connection.star,
            vrc_name: connection.vrc_name,
            scope: connection.scope,
            client: connection.client,
            client_is_dev,
            last_poll_at: connection.last_poll_at,
            next_poll_at: connection
                .last_poll_at
                .map(|last| last + connection.poll_every_secs * 1000),
        }
    }

    /// 服务器建议的提醒轮询间隔（秒，不低于 25），后台轮询用。
    pub fn poll_every_secs(&self) -> i64 {
        self.remote.connection().poll_every_secs
    }

    /// `me/notifications` 的游标；连上以后还没问过时为 `None`。
    pub fn notifications_cursor(&self) -> Option<i64> {
        self.remote.connection().notifications_cursor
    }

    /// 用量快照。
    ///
    /// 口径：说「今日已用」多少，**全部来自服务端的 `RateLimit-*`**。还没拿到
    /// 过那组头时字段是 `None`，界面显示「未知」——不要拿本地日期推算，
    /// 官方按北京时间零点重置，本地算出来的是错的。
    pub fn usage(&self) -> VrccUsageSnapshot {
        let usage = self.remote.local_usage();
        VrccUsageSnapshot {
            personal_used_today: usage.reads.map(|(used, _)| used),
            personal_daily_limit: usage.reads.map(|(_, limit)| limit),
            unread_used_today: usage.unread.map(|(used, _)| used),
            unread_daily_limit: usage.unread.map(|(_, limit)| limit),
            window_used: usage.window_used as i64,
            window_limit: self.remote.window_limit(),
            backoff_remaining_ms: usage.backoff_remaining_ms,
            reset_secs: usage.reset_secs,
        }
    }

    // -- 开放数据 ----------------------------------------------------------

    /// 地图榜。不需要连接，也不计个人额度。
    ///
    /// `sort` 是本周榜或新上榜、而回答里没有 `sort` 时，说明服务器还不认
    /// 这个参数、回的是总榜：照样返回，由界面判断「还不支持」。
    pub async fn boards(
        &self,
        limit: u32,
        sort: VrccBoardSort,
    ) -> std::result::Result<VrccBoard, VrccError> {
        match self.remote.boards(limit, sort).await {
            Ok(board) => {
                // 顺手填充腕表的排名缓存：这次请求本来就要发，不额外花额度。
                // 只用总榜：名次指的是总榜名次，本周榜和新上榜只列一部分地图。
                if sort == VrccBoardSort::Top || board.sort.is_none() {
                    self.note_boards(&board);
                }
                self.remember_board(limit, sort, &board);
                Ok(board)
            }
            // 距上一次请求不到限流间隔：把上次那份还给他。
            //
            // 官方说开放数据最多 5 分钟才更新一次，一份旧榜单完全够用；
            // 而让用户点一下刷新就弹错误，比给旧数据更糟。客户端侧因此
            // 把「等太久」直接报成 `TooSoon`（见 `MAX_ACQUIRE_WAIT`），
            // 这里接住它。没有缓存时才真的报错。
            Err(VrccError::TooSoon { retry_after_secs }) => match self.cached_board(limit, sort) {
                Some(board) => Ok(board),
                None => Err(VrccError::TooSoon { retry_after_secs }),
            },
            Err(error) => Err(error),
        }
    }

    /// 服务状态。
    pub async fn status(&self) -> std::result::Result<VrccStatus, VrccError> {
        self.remote.status().await
    }

    /// 取站点上的静态资源（通知图标之类）。
    ///
    /// 不需要连接：那是站点资源，和玩家令牌无关。仍然走端口，于是复用
    /// 同一份代理配置与「只和 vrcc.cc 说话」的硬拦。
    pub async fn fetch_site_asset(&self, path: &str) -> std::result::Result<Vec<u8>, VrccError> {
        self.remote.fetch_site_asset(path).await
    }

    /// 公告和停站（1.12.0 起）。不需要连接。
    ///
    /// 旧服务器没有这个接口（`404 not_found`）时返回 `None`，界面就不显示。
    pub async fn announcements(&self) -> std::result::Result<Option<VrccAnnouncements>, VrccError> {
        match self.remote.announcements().await {
            Ok(announcements) => Ok(Some(announcements)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error),
        }
    }

    // -- 设备授权 ----------------------------------------------------------

    /// 第一步：申请连接码。
    ///
    /// `scope` 传 `None` 时按当前标识选（见 [`device_auth_scope`]）。
    /// 不能交给服务器默认：自助登记的 `app_…` 不写只有 `profile`，
    /// 提醒、所在地图、「连上」全都会 `403 scope`。
    pub async fn begin_device_auth(
        &self,
        scope: Option<&str>,
    ) -> std::result::Result<VrccDeviceAuthPrompt, VrccError> {
        let client_id = self.remote.client_id();
        let scope = scope
            .map(str::trim)
            .filter(|scope| !scope.is_empty())
            .unwrap_or_else(|| device_auth_scope(&client_id));
        let code = self.remote.device_code(Some(scope)).await?;
        Ok(VrccDeviceAuthPrompt {
            user_code: code.user_code,
            verification_uri: code.verification_uri,
            verification_uri_complete: code.verification_uri_complete,
            expires_in_secs: code.expires_in.unwrap_or(600),
            interval_secs: code.interval.unwrap_or(5).max(5),
            device_code: code.device_code,
            scope: code.scope,
        })
    }

    /// 第二步：问一次令牌。按官方三种结果分类。
    pub async fn poll_device_auth(
        &self,
        prompt: &VrccDeviceAuthPrompt,
    ) -> std::result::Result<VrccDeviceAuthStep, VrccError> {
        match self.remote.device_token(&prompt.device_code).await {
            Ok(token) => {
                let credential = StoredCredential {
                    token: token.access_token.clone(),
                    star: token.me.as_ref().and_then(|me| me.star),
                    vrc_name: token.me.as_ref().and_then(|me| me.vrc.clone()),
                    scope: token.scope.clone(),
                    // 记下是哪个 `X-Client` 换来的：将来换标识时才能认出这个
                    // 令牌已经不可用，而不是继续拿它请求、收一堆 403。
                    client_id: Some(self.remote.client_id()),
                };
                self.store
                    .save(&credential)
                    .map_err(|error| VrccError::Transport(format!("保存令牌失败：{error}")))?;
                self.remote.connect(
                    credential.token,
                    credential.star,
                    credential.vrc_name,
                    credential.scope,
                );
                Ok(VrccDeviceAuthStep::Connected(self.snapshot()))
            }
            Err(error) if error.is_authorization_pending() => Ok(VrccDeviceAuthStep::Pending {
                interval_secs: prompt.interval_secs,
            }),
            Err(error) if error.is_device_auth_terminal() => {
                Ok(VrccDeviceAuthStep::Terminal(error.to_string()))
            }
            Err(error) => Err(error),
        }
    }

    /// 断开：先让服务器吊销，再清本地。
    ///
    /// 断开：先让服务器吊销，再清本地。
    ///
    /// **可重试的失败不清本地**。这条很重要：如果吊销请求根本没发出去
    /// （网络不通、或恰好在限流退避里），令牌却已经删了，那服务器上那份授权
    /// 就**永远**吊销不了了——玩家在网页「第三方客户端」里会一直看到一个
    /// 早就该消失的客户端。所以这种情况保留令牌、把错误交出去，让玩家再点一次。
    ///
    /// 其余失败照旧清本地：玩家点了断开，界面上就该断开。`needs_reauth`
    /// （`gone` / `401` / `client_mismatch`）说明服务器那边早已不认这个令牌，
    /// 等于本来就断着。
    pub async fn disconnect(&self) -> std::result::Result<(), VrccError> {
        let revoke = self.remote.revoke().await;
        // 服务器没收到吊销请求：什么都别动，等玩家重试。
        if let Err(error) = &revoke {
            if retriable_revoke_failure(error) {
                tracing::info!(
                    %error,
                    "vrcc: revoke did not reach the server; keeping the token so it can be retried"
                );
                return Err(error.clone());
            }
        }
        self.store
            .clear()
            .map_err(|error| VrccError::Transport(format!("清除令牌失败：{error}")))?;
        self.remote.disconnect();
        self.forget_unread();
        match revoke {
            Ok(()) => Ok(()),
            // 令牌本来就无效，等于已经断开了。
            Err(error) if error.needs_reauth() => Ok(()),
            Err(error) => Err(error),
        }
    }

    // -- 提醒 --------------------------------------------------------------

    /// 拉最近的提醒。
    ///
    /// `mark` 对应官方的 `?mark=1`：传 `true` 表示**玩家此刻正在看这个列表**，
    /// 服务器会把最新一条 ID 记到账号的 `read` 字段，跨设备同步已读状态。
    ///
    /// 两种调用场景必须区分：玩家打开「提醒」页时传 `true`；后台只为算数字时
    /// 传 `false`——否则等于替玩家把提醒标成看过了，而其实他没看。
    pub async fn recent_notifications(
        &self,
        mark: bool,
    ) -> std::result::Result<VrccRecentNotifications, VrccError> {
        self.ensure_connected()?;
        let result = self
            .remote
            .recent_notifications(mark)
            .await
            .map_err(|error| self.settle(error))?;
        // 未读数按官方语义算：id 大于 read 的算未读。`read` 缺省时（账号上
        // 还没记过已读），把返回的条目都当作未读——这是唯一诚实的解读。
        //
        // 刚标记过 `mark=1` 时服务器会把 read 推到最新，未读随之变 0，这是对的：
        // 玩家确实看到了。
        let unread = match result.read {
            Some(read) => result.items.iter().filter(|item| item.id > read).count() as u32,
            None => result.items.len() as u32,
        };
        self.note_recent(unread);
        self.remote.note_poll(self.remote.now_millis(), None, None);
        Ok(result)
    }

    /// 当前所在地图在地图榜上的状态与我自己的评分。
    ///
    /// 需要 `maps`（普通权限）。官方的硬性要求是 `off` 为 `true` 时客户端不要
    /// 显示打分面板；这里在 `off` 时直接返回 `None`，调用方拿不到对象就不可能
    /// 误显示——比在每个 UI 分支里记得判断 `off` 更可靠。
    pub async fn my_map(&self, world: &str) -> std::result::Result<Option<VrccMyMap>, VrccError> {
        // 只认 wrld_…：带房间实例号的 location 会让接口查错图。
        let world = vrcx_0_core::location::world_id_from_location(world);
        if world.is_empty() {
            return Ok(None);
        }
        self.ensure_connected()?;
        let map = self
            .remote
            .my_map(&world)
            .await
            .map_err(|error| self.settle(error))?;
        if !map.is_visible() {
            tracing::debug!(
                world = %world,
                "vrcc: this map is not shown to this player, hiding the card"
            );
            return Ok(None);
        }
        Ok(Some(map))
    }

    /// 问一次几处未读数（`me/unread`，1.13.0 起），结果进缓存。
    ///
    /// 返回 `(结果, 和上次比变没变)`。旧服务器没有这个接口（`404 not_found`）
    /// 时返回 `None`，之后一小时内不再问——服务器更新以后不用重启就能接上。
    pub async fn refresh_unread(
        &self,
    ) -> std::result::Result<Option<(VrccUnread, bool)>, VrccError> {
        self.ensure_connected()?;
        let skip = self
            .unread
            .read()
            .ok()
            .and_then(|cache| cache.skip_until)
            .is_some_and(|until| Instant::now() < until);
        if skip {
            return Ok(None);
        }
        match self.remote.unread().await {
            Ok(counts) => {
                let changed = self.note_unread(&counts);
                Ok(Some((counts, changed)))
            }
            Err(error) if error.is_not_found() => {
                tracing::debug!("vrcc: me/unread is not on this server yet");
                if let Ok(mut cache) = self.unread.write() {
                    cache.counts = None;
                    cache.skip_until = Some(Instant::now() + UNREAD_RETRY_AFTER_NOT_FOUND);
                }
                Ok(None)
            }
            Err(error) => Err(self.settle(error)),
        }
    }

    /// 通知中心（`me/notes`，1.13.0 起，第二份读）。读了不标已读。
    ///
    /// 旧服务器没有这个接口时返回 `None`，界面就不显示这一块。
    pub async fn notes(&self) -> std::result::Result<Option<VrccNotes>, VrccError> {
        self.ensure_connected()?;
        match self.remote.notes().await {
            Ok(notes) => Ok(Some(notes)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(self.settle(error)),
        }
    }

    /// 增量轮询。游标与建议间隔都自动维护。
    ///
    /// 这是官方给桌面客户端设计的轮询方式：两次之间至少 25 秒，**没有每天
    /// 上限**，每次只给 `since` 之后的新提醒。后台轮询就用它来弹系统通知。
    ///
    /// 响应里的 `dot` 是「星轨相交尚未查看的新人数」，**不是未读提醒数**，
    /// 所以这里不拿它更新腕表的 `unread`——两者混用会显示错误的数字。
    pub async fn poll_notifications(
        &self,
    ) -> std::result::Result<vrcx_0_contracts::vrcc::VrccNotifications, VrccError> {
        self.ensure_connected()?;
        let cursor = self.remote.connection().notifications_cursor;
        let polled = self
            .remote
            .notifications(cursor)
            .await
            .map_err(|error| self.settle(error))?;
        self.remote
            .note_poll(self.remote.now_millis(), polled.last, polled.every);
        // 有新提醒到达时，未读数会变；下次打开页面拉 recent 时会修正。
        // 这里若把 items 长度加上去，会和 `read` 的语义打架。
        Ok(polled)
    }

    /// 今天这一轮 + 轮次进度（需要 `today` 敏感权限）。
    pub async fn today(&self) -> std::result::Result<VrccToday, VrccError> {
        self.ensure_connected()?;
        self.remote
            .today()
            .await
            .map_err(|error| self.settle(error))
    }

    /// 「互相连上」的星友列表（需要 `mutuals` 敏感权限）。
    ///
    /// 条目带 `vrcUser` 时，前端可以打开 VRCX-0 自己的玩家详情页。按官方合规
    /// 要求，这里**不提供**代玩家发送好友请求的通道——加好友必须由玩家在
    /// 原生界面里自己点。
    pub async fn mutuals(&self) -> std::result::Result<VrccMutuals, VrccError> {
        self.ensure_connected()?;
        self.remote
            .mutuals()
            .await
            .map_err(|error| self.settle(error))
    }

    fn ensure_connected(&self) -> std::result::Result<(), VrccError> {
        if self.remote.connection().connected {
            Ok(())
        } else {
            Err(VrccError::TokenMissing)
        }
    }
}

#[cfg(test)]
mod tests;
