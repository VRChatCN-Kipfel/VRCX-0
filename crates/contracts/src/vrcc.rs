//! 星遇同频（vrcc.cc）开放接口的线上数据契约。
//!
//! 字段名与类型严格对照官方 `openapi.yaml` 与 `xingshi-app` 的实现，
//! 不做"我猜它应该叫这个"的推断。凡是官方允许缺失的字段，一律 `Option`，
//! 因为接口在不同 scope / 审核状态下返回的字段集合并不相同。
//!
//! 对照的是开放接口 1.14.0。官方的兼容规则是「v1 只做加法，要删的字段先标
//! 弃用、至少再留 60 天」，所以这里：
//! - 新字段一律 `#[serde(default)]`，旧服务器没给也能解析；
//! - **不读弃用中的字段**（`vibe`、`can`、`need`、`teen`、`adj`、
//!   `counts.regenToday`、`counts.styles`），到期不再返回时什么都不会坏；
//! - 不写 `deny_unknown_fields`，服务器多给的字段直接忽略。

use serde::{Deserialize, Serialize};

/// 官方限制 POST 正文最大 2 KB。
///
/// 放在契约层是因为 [`ErrorDetail::BodyTooLarge`] 的错误文案要引用它。
pub const VRCC_MAX_BODY_BYTES: usize = 2048;

/// 星遇同频的错误。
///
/// **为什么定义在契约层而不是网络层**：应用层要按它分类（要不要重新授权、
/// 是不是限流），并在公开 API 上返回它；如果它留在 `integrations`，
/// 应用层就得依赖网络实现，架构测试（`backend_architecture`）不允许。
/// 这个类型本身只是数据 + 判据，不含任何 IO，放在这里不违反「契约层
/// 只放同步数据与协议」的约束。
///
/// `Clone` 是给「先判断能不能重试、再把原错误交出去」这类调用点用的
/// （见 `VrccService::disconnect`）。
///
/// # 为什么是 struct 而不是 enum
///
/// 早先这里是一个 12 个变体的 enum，把**四种不同层级**的失败平铺在一起：
/// 我们自己的编码 bug、传输层失败、服务器拒绝、会话状态。结果是玩家会看到
/// 「请求正文超过 2048 字节上限」这种本不该外露的内部错误，而「网络不通」
/// 和「服务器拒绝」也分不开。
///
/// 现在只回答两件事：[`ErrorLayer`] 说**发生在哪一层**，[`ErrorDetail`]
/// 说**具体是什么**。层级判断只做一次，消费方不必再逐个变体去猜。
///
/// # 为什么 HTTP 语义收敛成一个变体
///
/// 早年按错误码加了 `RateLimited` / `TooSoon` / `QuotaExhausted` / `Paused`
/// / `NotReady` / `ConnectPaused` 六个变体，每来一个新错误码就要加一个。
/// 但官方错误码有三十多个，这条路走不完。
///
/// 现在统一记成 `Http { status, error, … }`，**错误只忠实记录 HTTP 事实，
/// 业务判断交给消费方**。同一个码在不同调用点反应本来就可能不同——
/// 比如 `409 busy` 表示「星卡还在生成」，而 `429 busy` 表示「连接功能暂停
/// 到次日」，官方文档明说这两种只能按状态码区分（README「网页上的动作和
/// 打分的错误码」）。把这种判断焊死在产生错误的地方是错的。
#[derive(Clone, Debug, thiserror::Error)]
#[error("{}", .detail)]
pub struct VrccError {
    /// 失败发生在哪一层。**先看这个，再看 `detail`。**
    pub layer: ErrorLayer,
    pub detail: ErrorDetail,
}

impl VrccError {
    pub fn new(layer: ErrorLayer, detail: ErrorDetail) -> Self {
        Self { layer, detail }
    }

    /// 拿到 HTTP 语义细节；不是服务器拒绝时为 `None`。
    pub fn http(&self) -> Option<&HttpDetail> {
        match &self.detail {
            ErrorDetail::Http(http) => Some(http),
            _ => None,
        }
    }

    /// 会话丢失：没有令牌，或令牌已失效。
    ///
    /// 和 [`Self::is_gone`] 的区别：那个特指官方 `404 gone`；这个是更宽的
    /// 「会话这一层出了问题」，包含从没连过的情况。
    pub fn is_session_lost(&self) -> bool {
        matches!(self.detail, ErrorDetail::TokenMissing) || self.is_gone()
    }

    /// 是不是被本机限流拦下的（请求没有发出去）。
    ///
    /// 本机最小间隔（`Gap`）和上一轮 429 的全局退避（`Backoff`）都算。
    /// 这种时候服务可能是好的，只是现在不该问——后台轮询应当静默跳过这一轮。
    pub fn is_throttled(&self) -> bool {
        matches!(self.detail, ErrorDetail::ThrottledLocally { .. })
    }

    /// 是不是「同一接口的最小间隔还没到」。
    ///
    /// 和 [`Self::is_our_bug`] 不同，这个是正常情况：调用方可以改用缓存，
    /// 而不是把它当成错误弹给玩家。`boards` 的 60 秒间隔就靠它接住。
    pub fn is_local_gap(&self) -> bool {
        matches!(
            self.detail,
            ErrorDetail::ThrottledLocally {
                reason: LocalThrottle::Gap,
                ..
            }
        )
    }

    /// 这个错误是不是「我们自己的 bug」，**不该显示给玩家**。
    ///
    /// 拼错路径、正文超限都是我们自己构造请求时出的错。以前它们和
    /// 「额度用尽」挤在同一个 toast 队列里，玩家看到的是
    /// 「请求正文超过 2048 字节上限」——和自己毫无关系的信息。
    pub fn is_our_bug(&self) -> bool {
        self.layer == ErrorLayer::Input
    }
}

/// 失败发生在哪一层。
///
/// 这就是「网络层还是应用层」这个问题的答案，一个字段即可，不必从变体去猜。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorLayer {
    /// **我们自己的 bug**：路径拼错、正文超限。玩家不该看到这类错误。
    Input,
    /// **本机限流**：请求根本没发出去，被本地限流器或退避拦下了。
    ///
    /// 和 [`Self::Server`] 分开是因为两者含义相反：本机限流时服务可能是好的，
    /// 只是我们问得太勤；把它标成「服务不可用」是在冤枉服务器。
    Client,
    /// **传输层**：请求送出去了，但没拿到任何响应。
    Transport,
    /// **服务器拒绝**：拿到了响应，但状态码不是 2xx。
    Server,
    /// **会话状态**：令牌缺失或失效，和网络无关。
    Session,
}

/// 传输层失败卡在哪个环节。
///
/// 判据全部来自 `reqwest::Error` 的类型判断（`is_dns` / `is_connect` /
/// `is_timeout` / `is_body`），不是对错误文案做字符串匹配。
///
/// **注意 `is_dns` 的盲区**：走 SOCKS5 代理时代理串会被改写成 `socks5h://`
/// （见 `vrcx_0_core::proxy::with_remote_dns`），域名由代理那边解析。
/// 那种情况下本机根本不做 DNS 查询，解析失败会表现为 `Connect` 而不是 `Dns`。
/// 所以看到 `Connect` 时不能断言「不是 DNS 问题」。
///
/// 另外：RST、端口拒绝、运营商封锁在操作系统层面表现一致，都是建连失败，
/// 这里无法进一步区分，也不会假装能区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    /// 本机域名解析失败。
    Dns,
    /// 建立连接失败（端口不通、RST、代理未启动、SOCKS5 握手失败等）。
    Connect,
    /// 连接或读取超时。
    Timeout,
    /// 响应体读到一半中断。
    Body,
    /// 其它传输错误。
    Other,
}

/// 服务器拒绝时，官方在正文里给的细节。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpDetail {
    pub status: u16,
    /// 官方的错误码，如 `too_soon` / `quota` / `busy` / `map_wait`。
    /// **这个串是稳定的**，业务分支应当按它（配合 `status`）判断。
    pub error: String,
    /// 缺哪一项权限或前置条件。
    pub need: Option<String>,
    /// 补充原因（哪些错误码会给：`yq_words`、`msg_words` 等）。
    pub why: Option<String>,
    /// `Retry-After` 响应头的秒数。1.9.0 起所有 429 都带。
    pub retry_after: Option<u64>,
    /// 发起这次调用的是哪个接口（`device_code` / `today` …）。
    ///
    /// 同一个错误码在不同接口下含义可能不同，消费方需要它才能给出准确提示。
    /// 典型例子：`429 too_many` 在 `device_code` 上是「按网络出口 IP 算的
    /// 每天 30 个，换网络可立刻再用」，在别的接口上才是「你自己的次数用完了」。
    pub op: Option<&'static str>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum ErrorDetail {
    #[error("星遇同频 token 缺失，请先连接")]
    TokenMissing,
    #[error("路径必须相对 /api/v1/：{0}")]
    AbsolutePath(String),
    /// **本地参数不成立**：请求压根不该发出去，也发不出去。
    ///
    /// 与 [`Self::AbsolutePath`] 的区别是：那个是路径拼错（实现缺陷），
    /// 这个是**调用方传进来的东西本身不完整**，例如要打分却没有世界 ID。
    ///
    /// 之所以单列而不是复用别的变体：写操作遇到这种情况**不能安静地成功**
    /// ——玩家会以为提交了。所以它需要一条玩家看得懂、又能指向我们代码的文案。
    #[error("星遇同频请求缺少必要参数：{0}")]
    MissingArgument(&'static str),
    /// **参数格式不合法**：值给了，但不是这个接口认的形状。
    ///
    /// 和 [`Self::MissingArgument`] 分开：那个是「没给」，这个是「给了但不能用」
    /// ——例如世界 ID 不是官方规定的 `wrld_` 加 36 位十六进制。两者都必须让请求
    /// 在本地停下：世界 ID 要拼进 URL 路径，形状不对就没有发出去的理由。
    #[error("星遇同频请求参数格式不对：{0}")]
    InvalidArgument(&'static str),
    #[error("请求正文超过 {VRCC_MAX_BODY_BYTES} 字节上限")]
    BodyTooLarge,
    /// **本机限流**：请求没有发出去。
    ///
    /// 两种来源：发出前的全局退避还没结束，或者本地最小间隔还没到。
    /// 后者通常意味着「刚问过，没必要再问」，调用方可以改用缓存。
    #[error("星遇同频请求被本机限流拦下，{retry_after_secs} 秒后再问")]
    ThrottledLocally {
        retry_after_secs: u64,
        /// `backoff`：上一轮 429 的全局退避中；`gap`：同一接口的最小间隔未到。
        reason: LocalThrottle,
    },
    #[error("星遇同频接口暂时不可用：{message}")]
    Transport {
        kind: TransportKind,
        /// 原始错误文案，**只用于日志与排查**。玩家看到的文案应当按 `kind` 出。
        message: String,
    },
    #[error("星遇同频返回无法解析：{0}")]
    Decode(String),
    #[error("{}", http_display(.0))]
    Http(HttpDetail),
}

/// 本机拦截的原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalThrottle {
    /// 上一轮 429 之后的全局退避还没结束。
    Backoff,
    /// 同一个接口两次请求的最小间隔还没到（例如地图榜 60 秒）。
    Gap,
}

/// `Http` 的显示文案。
///
/// 只保留「服务器说不行」这个事实和官方错误码，不再拼中文解释——那些
/// 交给前端按 `(status, error, op)` 出 i18n。早先这里拼出了
/// 「今天的次数用完了，约 3 小时 后恢复」这种高度定制的句子，一旦文案要改
/// 就得改后端，而且没法本地化。
fn http_display(http: &HttpDetail) -> String {
    match http.retry_after {
        Some(secs) => format!(
            "星遇同频接口返回 HTTP {}（{}），{} 秒后可重试",
            http.status, http.error, secs
        ),
        None => format!("星遇同频接口返回 HTTP {}（{}）", http.status, http.error),
    }
}

impl VrccError {
    /// 官方错误码表里的 `404 gone`：**令牌失效**（玩家断开了、同一个客户端又
    /// 授权了一次、星被删除，1.10.0 起还有「90 天没调用过任何接口自动断开」）。
    /// 要停下轮询并让玩家重新授权。
    ///
    /// 注意区分 `star_gone`（连线/私聊的对方不在了，令牌没事）、`expired`
    /// （1.9.0 起签名图片失效用这个码，原来也叫 `gone`）和 `not_found`
    /// （查的地图不在榜上、或旧服务器还没有这个接口），这些都**不该**触发重新授权。
    pub fn is_gone(&self) -> bool {
        self.http()
            .is_some_and(|http| http.status == 404 && http.error == "gone")
    }

    /// `404 not_found`：没有这个接口，或查的东西不在。
    ///
    /// 新接口（`me/unread`、`me/notes`、`announcements`）在还没更新的服务器上
    /// 就是这样回答的，调用方据此安静地不显示，而不是报错。
    pub fn is_not_found(&self) -> bool {
        self.http()
            .is_some_and(|http| http.status == 404 && http.error == "not_found")
    }

    /// 「等一下再试」这一类：不是故障，也不是玩家做错了什么。
    ///
    /// `409 busy`（这一轮的星卡还在生成）。界面应显示「稍后再试」，
    /// **不要**引导玩家重新授权，也不要渲染成错误横幅。
    ///
    /// 必须**连带状态码**判断：`429 busy` 用的是同一个 `error` 串，含义却是
    /// 「全站连接功能暂停到次日」，两者只能按状态码区分（官方文档原话）。
    pub fn is_not_ready(&self) -> bool {
        self.http()
            .is_some_and(|http| http.status == 409 && http.error == "busy")
    }

    /// 需要重新授权的情况。照官方错误码表逐条来：
    /// - `404 gone`：令牌失效
    /// - `403 client_mismatch`：令牌不是发给这个 `X-Client` 的
    /// - `403 client_unknown` / `client_off` / `client_missing`：标识本身有问题
    /// - `401 token`：没带令牌或格式不对
    /// - `403 scope`：缺权限，要玩家重新授权时勾上 `need` 那一项
    pub fn needs_reauth(&self) -> bool {
        if self.is_gone() || matches!(self.detail, ErrorDetail::TokenMissing) {
            return true;
        }
        let Some(http) = self.http() else {
            return false;
        };
        http.status == 401
            || (http.status == 403
                && matches!(
                    http.error.as_str(),
                    "client_mismatch"
                        | "client_unknown"
                        | "client_off"
                        | "client_missing"
                        | "scope"
                        | "scope_unreviewed"
                ))
    }

    /// 缺哪个权限。前端据此提示「需要重新授权并勾上 X」。
    pub fn needed_scope(&self) -> Option<&str> {
        self.http().and_then(|http| http.need.as_deref())
    }

    /// 设备授权期间「玩家还没点同意」，调用方应等 `interval` 秒再来问。
    pub fn is_authorization_pending(&self) -> bool {
        self.http()
            .is_some_and(|http| http.status == 400 && http.error == "authorization_pending")
    }

    /// 设备授权走到终态：玩家取消，或码过期/已用过。
    pub fn is_device_auth_terminal(&self) -> bool {
        self.http().is_some_and(|http| {
            http.status == 400 && (http.error == "access_denied" || http.error == "expired_token")
        })
    }
}

/// API 基址。生产代码里只允许拼相对路径，绝对地址会在客户端层被拒绝。
pub const VRCC_API_BASE: &str = "https://vrcc.cc/api/v1/";

/// 站点根，用来把接口给的**相对路径**（如 `/match/?tab=inbox`）拼成完整地址。
pub const VRCC_SITE_ORIGIN: &str = "https://vrcc.cc";

/// 开发态客户端标识。
///
/// 官方对 `dev-` 开头的标识只做前缀判定（`client_unknown` 的定义是「不是登记
/// 名单里的标识、不是 `client_id`、**也不是 `dev-` 开头**」），没有长度或字符
/// 限制，所以这里可以带上项目名。已实测该值对公开端点返回 200。
///
/// `dev-` 标识在玩家同意页会显示「（测试中）」与「未登记客户端（仅测试用）」，
/// 这是官方设计，不是问题。
pub const VRCC_DEV_CLIENT: &str = "dev-VRCX-0-Galaxies";

/// 正式态的应用**名称**。
///
/// 注意这不是 `X-Client` 的值。官方对应用名称的规定是「2–20 个字，不能带
/// 「星遇同频」「VRCC」「VRChat」「官方」「管理员」」，`VRCX-0-Galaxies` 符合。
/// 登记通过后拿到的 `client_id`（`app_` + 16 位十六进制）才是正式态的
/// `X-Client`，届时用 [`VRCC_BUILD_CLIENT`] 在构建期注入。
pub const VRCC_APP_NAME: &str = "VRCX-0-Galaxies";

/// 构建期注入的正式客户端标识。
///
/// 官方 `client_id`（`app_` + 16 位十六进制）由平台发放，**不入库**：
///
/// ```text
/// 构建时：  VRCC_CLIENT_ID=app_xxxxxxxxxxxxxxxx  npm run tauri:build
/// 未注入：  ""
/// ```
///
/// `option_env!` 而不是 `env!`：源码树里默认没有正式标识，这必须是合法状态，
/// 不是构建错误。cargo 会因这个变量的变化而重编本 crate，不需要额外声明。
///
/// 走的是和 [`crate::telemetry`] 里 `VRCX_0_TELEMETRY_ENDPOINT` 同一套约定——
/// 构建期注入、源码里不留真实值。
///
/// **不要**把它改成字面量，也不要写进 `.cargo/config.toml`（那个文件是入库的）。
pub const VRCC_BUILD_CLIENT: &str = match option_env!("VRCC_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// 源码默认的客户端标识：构建期没注入时用它。
///
/// 克隆下来直接 `tauri:dev` 就该能连上公开接口，所以默认值必须是能跑的。
/// 需要正式标识时用 [`VRCC_BUILD_CLIENT`] 在构建期注入，别改这里。
pub const VRCC_DEFAULT_CLIENT: &str = VRCC_DEV_CLIENT;

/// 申请设备授权时**显式请求**的权限集合。
///
/// **为什么不能传 `None` 交给服务器决定**：不写 `scope` 时，拿到哪些权限
/// 完全取决于客户端是怎么登记的——
/// - 自助登记的 `app_…` 只给 `profile`，于是 `/me/notifications/recent`、
///   `/me/maps/{world}` 全都 `403 scope`，提醒和所在地图直接不可用；
/// - 开 Issue 登记的客户端又会默认带上本功能用不到的 `crossings` 和
///   `maps.rate`，违反官方《使用要求》第 10 条「只申请用得到的权限」。
///
/// 这里只列界面真正用到的：提醒、所在地图、「连上」页（`mutuals`）、
/// 「星轨相交」页（`crossings`）、给地图打分（`maps.rate`）、给短评投票
/// （`maps.write`）。
///
/// `mutuals`、`crossings`、`maps.rate`、`maps.write` 都是敏感权限，只有正式
/// 标识能要，`dev-` 用 [`VRCC_DEV_DEVICE_AUTH_SCOPE`]。实际要哪一串用
/// [`device_auth_scope`] 选。
///
/// **`crossings` 必须在这里**：令牌的权限是签发时定下的，事后要再拿到它
/// 只能重新授权。不列进来，界面上的「星轨相交」页签对每一个玩家都永远
/// 是「缺权限」——功能等于没做。
pub const VRCC_DEVICE_AUTH_SCOPE: &str =
    "profile notifications maps mutuals crossings maps.rate maps.write";

/// `dev-` 标识申请的权限：只能要普通权限，要了敏感的 `device/code`
/// 会直接回答 `403 scope_unreviewed`，连码都拿不到。
pub const VRCC_DEV_DEVICE_AUTH_SCOPE: &str = "profile notifications maps";

/// 按当前 `X-Client` 选设备授权要的权限。
pub fn device_auth_scope(client_id: &str) -> &'static str {
    if is_dev_client(client_id) {
        VRCC_DEV_DEVICE_AUTH_SCOPE
    } else {
        VRCC_DEVICE_AUTH_SCOPE
    }
}

/// 令牌的权限串里有没有 `needed`。
///
/// 不知道权限（旧存档没记）时当作有，交给服务器判断。官方用空格分开，
/// 也认逗号和 `+`。
pub fn scope_grants(scope: Option<&str>, needed: &str) -> bool {
    match scope {
        None => true,
        Some(scope) => scope
            .split(|c: char| c.is_whitespace() || c == ',' || c == '+')
            .any(|item| item == needed),
    }
}

/// 实际要发的 `X-Client`。
///
/// **构建即锁定**：只有两层，没有运行时开关。
///
/// ```text
/// 1. VRCC_CLIENT_ID 环境变量       ← 构建期注入，不入库
/// 2. dev-VRCX-0-Galaxies          ← 源码默认
/// ```
///
/// 为什么不留运行时覆盖：`X-Client` 是「这个二进制以谁的身份和平台通信」的
/// 全部凭据。留一个配置项等于允许用户在配置文件里填上正式标识，去冒充一个
/// 通过审核的客户端——那是绕过平台审核，不是便利。锁定在构建期后，产物用
/// 的是哪个标识只看它怎么被打出来，配置文件改不动它。
///
/// 想换标识就重新打包，并且应重新走一次设备授权：令牌绑签发它的那个标识。
///
/// 开发态和正式态的标识**不是同一个字符串的加前缀关系**（`dev-…` 与
/// `app_…` 是两种东西），写死一处常量迟早有人手改出错。
pub fn resolve_client_id() -> String {
    build_client_id().to_string()
}

/// 构建期注入的标识；没注入时回落到开发态。
///
/// 注入值也要 trim：CI 里从变量拼进来的串偶尔带空白，而 `X-Client` 带空白
/// 会吃 `403 client_unknown`，且现象是「所有接口都 403」，极难看出是空格。
fn build_client_id() -> &'static str {
    let injected = VRCC_BUILD_CLIENT.trim();
    if injected.is_empty() {
        VRCC_DEV_CLIENT
    } else {
        injected
    }
}

/// 当前标识是否为开发态（用于界面提示与日志）。
pub fn is_dev_client(client_id: &str) -> bool {
    client_id.trim().starts_with("dev-")
}

// ---------------------------------------------------------------------------
// 开放数据 · 地图榜
// ---------------------------------------------------------------------------

/// `GET /boards/maps`、`GET /maps`、`GET /me/maps` 共用的地图条目。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccBoardMap {
    /// 世界 ID，形如 `wrld_…`。
    pub world: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// 平均分（1–10）。
    #[serde(default)]
    pub avg: Option<f64>,
    /// 评分人数；有一个评分就有名次（`min` 现在是 1）。
    #[serde(default)]
    pub raters: Option<i64>,
    /// 总榜名次，1 起；新上榜里还没人评分的是 `null`。
    ///
    /// 本周榜里也是总榜名次：本周榜只按本周的打分重新排顺序，本周的名次是
    /// 它在列表里的位置。
    #[serde(default)]
    pub rank: Option<i64>,
    /// 多小咕实测的轻重：`light` / `mid` / `heavy`，没测过是 `null`。
    #[serde(default)]
    pub weight: Option<String>,
    /// vrcc.cc 上的地图页。
    #[serde(default)]
    pub page: Option<String>,
    /// 封面图地址。
    #[serde(default)]
    pub img: Option<String>,
    /// 同一张封面的完整网址。
    ///
    /// **官方 `boards/maps` 至今不返回这个字段**（只有 `me/board`、地图页、
    /// `me/maps/{world}` 才有），所以这里读到的通常是 `None`。仍然留着它，
    /// 是因为哪天官方统一了就直接能接上，不用再改一轮契约。
    #[serde(default)]
    pub img_url: Option<String>,
    /// 热评：最热的一条短评（1.10.1 起写进文档），没有时是 `null`。
    #[serde(default)]
    pub hot: Option<VrccMapHot>,
    /// 只在本周榜（`sort=week`）里有：本周几人打分（1.13.0 起）。
    #[serde(default)]
    pub week: Option<i64>,
}

/// 地图榜条目上的热评 `{text, by, up, went}`。
///
/// **`hot` 不在官方 `openapi.yaml` 的 `BoardMap` 里**，是 1.10.1 才写进文档的
/// 附加字段；README「开放数据」一节也写明公开榜单「不给谁打了分，也不给短评」。
/// 也就是说它随时可能改形状或下线。
///
/// 因此每个字段都容忍「形状不对」：`text` / `by` 若不是字符串就当没有，
/// 而不是让整个 `VrccBoard` 解析失败——那会让地图榜整页空白、腕表上的名次
/// 也不再更新，代价远大于少显示一句短评。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapHot {
    /// 那句短评。
    #[serde(default, deserialize_with = "lenient_optional_string")]
    pub text: Option<String>,
    /// 出处（形如「某位炉边倾听者」）。
    #[serde(default, deserialize_with = "lenient_optional_string")]
    pub by: Option<String>,
    /// 点赞数。
    #[serde(default, deserialize_with = "lenient_optional_i64")]
    pub up: Option<i64>,
    /// 写的人去过这张图。
    #[serde(default, deserialize_with = "lenient_optional_bool")]
    pub went: Option<bool>,
}

/// 把「类型不对」一律读成 `None` 而不是报错。
///
/// 用在这些**非契约必需**的附加字段上：字段缺失、`null`、或是别的类型，
/// 三种都当作没有，绝不让它连累整份回答的解析。
fn lenient_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(text)) => Some(text),
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        _ => None,
    })
}

fn lenient_optional_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(number)) => number.as_i64(),
        Some(serde_json::Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    })
}

fn lenient_optional_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Bool(flag)) => Some(flag),
        _ => None,
    })
}

/// 地图榜的排序（1.13.0 起）：总榜、本周榜、新上榜。
///
/// 和网页地图榜的「总榜」「本周」「新上榜」一样，只是不分星池。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum VrccBoardSort {
    #[default]
    Top,
    Week,
    New,
}

impl VrccBoardSort {
    /// 查询串里的写法。
    pub fn as_query(self) -> &'static str {
        match self {
            VrccBoardSort::Top => "top",
            VrccBoardSort::Week => "week",
            VrccBoardSort::New => "new",
        }
    }
}

/// `GET /me/board` 的排序。比公开榜多一个 `went`（我去过的）。
///
/// 单独一个枚举而不是复用 [`VrccBoardSort`]：多出来的那一个值在公开榜上
/// 不存在，塞进同一个枚举会让公开榜的调用点也「支持」一个它其实发不出去的
/// 值。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum VrccMyBoardSort {
    #[default]
    Top,
    Week,
    New,
    /// 我去过的。
    Went,
}

impl VrccMyBoardSort {
    /// 查询串里的写法。
    pub fn as_query(self) -> &'static str {
        match self {
            VrccMyBoardSort::Top => "top",
            VrccMyBoardSort::Week => "week",
            VrccMyBoardSort::New => "new",
            VrccMyBoardSort::Went => "went",
        }
    }
}

/// 地图榜 / 批量查地图 / 我打过的图，三者的外层结构一致。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccBoard {
    #[serde(default)]
    pub board: Option<String>,
    /// 这次用的排序：`top` / `week` / `new`（1.13.0 起）。
    ///
    /// 旧服务器不认 `sort`、照旧回总榜，回答里也没有这个字段。所以要的是
    /// 本周榜或新上榜、回来的却没有 `sort` 时，说明服务器还不支持，不能把
    /// 总榜当成本周榜显示。
    #[serde(default)]
    pub sort: Option<String>,
    /// 有名次最少需要几人评分（当前为 1）。
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub maps: Vec<VrccBoardMap>,
    /// 榜单快照时间（毫秒）。
    #[serde(default)]
    pub at: Option<i64>,
}

/// 单张地图详情：`GET /maps/{world}`。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapDetail {
    #[serde(default)]
    pub map: Option<VrccBoardMap>,
    #[serde(default)]
    pub at: Option<i64>,
}

// ---------------------------------------------------------------------------
// 连接身份
// ---------------------------------------------------------------------------

/// 通用的"我"：点亮编号 + VRChat 名字。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMe {
    /// 星号（点亮编号）。
    #[serde(default)]
    pub star: Option<i64>,
    /// 公开的 VRChat 名字。
    #[serde(default)]
    pub vrc: Option<String>,
}

// ---------------------------------------------------------------------------
// 设备授权（RFC 8628）
// ---------------------------------------------------------------------------

/// `POST /device/code` 的返回：给玩家看的码 + 同意链接。
#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
pub struct VrccDeviceCode {
    #[serde(rename = "device_code")]
    pub device_code: String,
    #[serde(rename = "user_code")]
    pub user_code: String,
    #[serde(rename = "verification_uri", default)]
    pub verification_uri: Option<String>,
    /// 带码的同意链接，优先用这个。
    #[serde(rename = "verification_uri_complete", default)]
    pub verification_uri_complete: Option<String>,
    /// 有效期秒数。
    #[serde(rename = "expires_in", default)]
    pub expires_in: Option<i64>,
    /// 官方建议的轮询间隔秒数。
    #[serde(default)]
    pub interval: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// `POST /device/token` 成功后返回的令牌。
#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
pub struct VrccTokenResponse {
    #[serde(rename = "access_token")]
    pub access_token: String,
    #[serde(rename = "token_type", default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub me: Option<VrccMe>,
}

// ---------------------------------------------------------------------------
// 提醒
// ---------------------------------------------------------------------------

/// 一条提醒，对应官方的 `Notice`。
///
/// 官方定义只有 `id` / `title` / `body` / `at` 四个字段。`/me/notifications/recent`
/// 会在此之上多给一个 `url`（见 [`VrccRecentNotificationItem`]）——
/// `/me/notifications` 的轮询**没有** `url`，两者不能混用同一个类型。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccNotificationItem {
    /// 提醒递增编号。
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// 产生时间（毫秒时间戳）。
    #[serde(default)]
    pub at: Option<i64>,
}

/// `GET /me/notifications`：带增量游标的轮询接口。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccNotifications {
    /// 下一次轮询要回传的游标。
    #[serde(default)]
    pub last: Option<i64>,
    #[serde(default)]
    pub items: Vec<VrccNotificationItem>,
    /// 官方建议的轮询间隔（秒），通常 60。
    #[serde(default)]
    pub every: Option<i64>,
    /// 未读角标数。
    #[serde(default)]
    pub dot: Option<i64>,
}

/// `GET /me/notifications/recent`：最近 24 小时内的最后 5 条（新的在前）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccRecentNotifications {
    #[serde(default)]
    pub items: Vec<VrccRecentNotificationItem>,
    /// 玩家最后已读的提醒 ID；大于它的可视为未读。
    #[serde(default)]
    pub read: Option<i64>,
}

/// `recent` 里的提醒条目：`Notice` 加上站内跳转路径。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccRecentNotificationItem {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// 产生时间（毫秒时间戳）。
    #[serde(default)]
    pub at: Option<i64>,
    /// 对应的网页**相对路径**（以 `/match/` 开头），无跳转时为 `null`。
    ///
    /// 注意：这是相对路径，不是完整 URL。前面要拼上 [`VRCC_SITE_ORIGIN`]。
    #[serde(default)]
    pub url: Option<String>,
}

impl VrccRecentNotificationItem {
    /// 拼出可点击的完整地址。`url` 为空时回到站点首页。
    pub fn absolute_url(&self) -> String {
        match self
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            Some(url) if url.starts_with("http://") || url.starts_with("https://") => {
                url.to_string()
            }
            Some(url) => format!("{}{}", VRCC_SITE_ORIGIN, url),
            None => format!("{}/", VRCC_SITE_ORIGIN),
        }
    }
}

// ---------------------------------------------------------------------------
// 地图评分
// ---------------------------------------------------------------------------

/// `GET /me/maps/{world}`：玩家**当前所在地图**在地图榜上的状态与本人评分。
///
/// 字段照官方 `MyMap`。三个布尔各有明确含义，客户端行为直接依赖它们：
///
/// | 字段 | 为 `true` 时 |
/// | --- | --- |
/// | `board` | 已正式在榜上展示 |
/// | `pending` | 已提交收录、等审核 |
/// | `off` | **不给当前玩家看**（下架或年龄池不符）→ 客户端**必须隐藏**这张图的卡片与打分入口 |
///
/// 三者皆 `false` 表示地图榜还没有这张图；玩家首次打分时会自动提交审核。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMyMap {
    #[serde(default)]
    pub world: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// 是否已正式在地图榜上展示。
    #[serde(default)]
    pub board: Option<bool>,
    /// 是否已提交收录、正在等待审核。
    #[serde(default)]
    pub pending: Option<bool>,
    /// 是否不给当前玩家看；为 `true` 时客户端应隐藏这张图。
    #[serde(default)]
    pub off: Option<bool>,
    #[serde(default)]
    pub avg: Option<f64>,
    #[serde(default)]
    pub raters: Option<i64>,
    /// 自己打的分（1–10）。
    #[serde(default)]
    pub mine: Option<i64>,
    // 不读 `can` / `need`：官方 1.10.1 起标为弃用（2026-10-24 起谁都能打分，
    // 恒为 `true` / `null`），2026-12-26 起不再返回。
    /// 自己的短评。
    #[serde(default)]
    pub words: Option<String>,
    /// 短评的审核状态：`wait` 审核中 / `ok` 已通过 / `hidden` 未通过；
    /// 没有短评时为 `null`。
    #[serde(default)]
    pub words_ok: Option<String>,
    /// 封面**相对路径**（接在 [`VRCC_SITE_ORIGIN`] 之后使用）。
    ///
    /// 官方给的示例是 `/api/match?xdimg=…`，也就是说这个图片来自网页端那套
    /// 内部接口。我们只把它当图片地址加载，不通过它取任何数据。
    #[serde(default)]
    pub img: Option<String>,
    /// 同一张封面的完整网址（1.11.0 起）。有它就不用自己拼。
    #[serde(default)]
    pub img_url: Option<String>,
}

impl VrccMyMap {
    /// 客户端是否应当显示这张地图。
    ///
    /// 官方的硬性要求：`off` 为 `true` 时不要显示打分面板。把判断收在这里，
    /// 免得每个调用点各写一遍、漏掉一处就等于没做。
    pub fn is_visible(&self) -> bool {
        !self.off.unwrap_or(false)
    }

    /// 封面的完整地址；没有封面时返回 `None`。
    ///
    /// 优先用服务器给的 `imgUrl`，旧服务器没有时再拿 `img` 自己拼。
    pub fn image_url(&self) -> Option<String> {
        if let Some(url) = self
            .img_url
            .as_deref()
            .map(str::trim)
            .filter(|url| url.starts_with("https://"))
        {
            return Some(url.to_string());
        }
        let img = self.img.as_deref()?.trim();
        if img.is_empty() {
            return None;
        }
        if img.starts_with("http://") || img.starts_with("https://") {
            return Some(img.to_string());
        }
        Some(format!(
            "{}{}",
            VRCC_SITE_ORIGIN,
            if img.starts_with('/') {
                img.to_string()
            } else {
                format!("/{img}")
            }
        ))
    }
}

// ---------------------------------------------------------------------------
// 星卡 / 今天（敏感 scope，需审核）
// ---------------------------------------------------------------------------

/// 一张星卡：`/me/today`、`/me/inbox` 里的条目。
///
/// 字段照官方 `StarCard` schema。`additionalProperties: true`，所以还有文档
/// 没列出的字段存在，这里只取我们用得到的那些；多出来的字段解析时被忽略。
///
/// 注意：这张卡**没有** `at` 时间字段（官方 `StarCard` 不提供），时间由
/// 外层结构给。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccStarCard {
    /// 星的编号，连线、私聊都用它。
    #[serde(default)]
    pub n: i64,
    #[serde(default)]
    pub name: Option<String>,
    /// 星位的一句话描述。
    #[serde(default)]
    pub line: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    /// 星群名称。
    #[serde(default)]
    pub group_name: Option<String>,
    // 不读 `vibe`：网页的星卡上不显示，官方 1.10.1 起标为弃用，
    // 2026-12-26 起不再返回。
    #[serde(default)]
    pub intro: Option<String>,
    /// 点亮编号（对方没公开时为 `null`）。
    #[serde(default)]
    pub star: Option<i64>,
    /// 对方验证过 VRChat 账号。
    #[serde(default)]
    pub verified: Option<bool>,
    /// 我连向了 TA。
    #[serde(default)]
    pub linked: Option<bool>,
    /// 已经互相连上。
    #[serde(default)]
    pub mutual: Option<bool>,
    /// 对方的 VRChat 名字，**只在互相连上时才有**。
    #[serde(default)]
    pub vrc: Option<String>,
    /// 对方的 VRChat 用户 ID，只在互相连上且对方公开已验证时才有。
    #[serde(default)]
    pub vrc_user: Option<String>,
    /// 共同的星友数（对方不公开时为 `null`）。
    #[serde(default)]
    pub friends: Option<i64>,
}

/// 「今天」的轮次信息。这一轮连了几颗、下一轮什么时候开始。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccRound {
    /// 本轮开始时间（毫秒）。
    #[serde(default)]
    pub at: Option<i64>,
    /// 下一轮开始时间（毫秒）。
    #[serde(default)]
    pub next: Option<i64>,
    /// 本轮已连几颗。
    #[serde(default)]
    pub used: Option<i64>,
    /// 本轮最多能连几颗。
    #[serde(default)]
    pub max: Option<i64>,
    #[serde(default)]
    pub remind: Option<bool>,
}

/// 「互相连上」的一颗星，对应 `GET /me/mutuals` 的条目。
///
/// `vrc_user` 是对方的 VRChat 用户 ID（`usr_…`），**只在对方已验证且公开
/// 「已验证」标志时才有**。有它才能打开 VRCX 自己的玩家详情页。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMutualStar {
    /// 星的编号。
    #[serde(default)]
    pub n: i64,
    /// 星位名称。
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    /// 点亮编号。
    #[serde(default)]
    pub star: Option<i64>,
    /// 对方的 VRChat 显示名。
    #[serde(default)]
    pub vrc: Option<String>,
    /// 对方的 VRChat 用户 ID；对方没公开时为 `null`。
    ///
    /// **合规提醒**：官方要求客户端若提供「加好友」入口，只能打开客户端自己
    /// 的玩家页面让玩家手动点添加，**不得由程序代发好友请求**。这里只把 ID
    /// 交出去用于打开玩家详情页，不提供任何自动发送好友请求的通道。
    #[serde(default)]
    pub vrc_user: Option<String>,
    /// 互相连上的时间（毫秒时间戳）。
    #[serde(default)]
    pub at: Option<i64>,
    /// 我给 TA 的分（1.12.0 起）。连上的第二天起才有，当天和旧服务器都是 `null`。
    #[serde(default)]
    pub rate: Option<VrccMutualRate>,
}

/// 我给一颗互相连上的星的评价，和网页上互相连上的卡一样。
///
/// 只有我给 TA 的，没有 TA 给我的。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMutualRate {
    /// 一起玩过没有；`null` 是还没回答。
    #[serde(default)]
    pub played: Option<bool>,
    /// 我给的分，0–5，半颗星一档；`0` 是没打。
    #[serde(default)]
    pub mine: Option<f64>,
}

/// `GET /me/mutuals`：与当前玩家双向连线的星友列表。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMutuals {
    #[serde(default)]
    pub stars: Vec<VrccMutualStar>,
    /// 互相连上的总数。
    #[serde(default)]
    pub n: Option<i64>,
}

// ---------------------------------------------------------------------------
// 星轨相交（`GET /me/crossings`，需要 crossings 敏感权限）
// ---------------------------------------------------------------------------

/// `GET /me/crossings`：在同一个 VRChat 房间偶遇过的星遇同频玩家。
///
/// 对应网页「连向我 → 星轨相交」。**需要 `crossings` 敏感权限**；`dev-`
/// 标识要不到（回 `403 scope_unreviewed`），16–17 岁星池没有这一项。
///
/// 与「互相连上」(`mutuals`) 的区别：互相连上是**双向已连线**的星友，
/// 星轨相交只是**同房间擦肩**，绝大多数还没有连线。所以这里每条要带上
/// `linked` / `mutual` 两个状态，让界面能区分「我已连 TA」「已双向」「都还没」。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccCrossings {
    #[serde(default)]
    pub stars: Vec<VrccCrossingStar>,
    /// 列表里的星数量。
    #[serde(default)]
    pub n: Option<i64>,
    /// 星轨相交不可用或为空的原因，见 [`VrccCrossingWhy`]。
    ///
    /// 保留服务器的原始字符串（界面用它挑文案），判定统一走 [`Self::hidden`]。
    #[serde(default)]
    pub why: Option<String>,
    /// 是否必须**整个模块都不显示**（由应用层按 [`VrccCrossingWhy`] 算好）。
    ///
    /// 给成字段而不是让每个消费方各判一次 `why == "hide"`：这条判定的两边
    /// 后果不对称——把 `hide` 当普通空列表，等于向不该看到的人确认「这里有
    /// 星轨相交」这个事实。只在一处算，才不会有一处漏。
    #[serde(default)]
    pub hidden: bool,
    /// 当前玩家自己的星位摘要。
    #[serde(default)]
    pub me: Option<VrccCrossingMe>,
}

/// 一条星轨相交记录（`stars[]` 的条目）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccCrossingStar {
    /// 星的编号；网页直达地址是 `https://vrcc.cc/match/?tab=inbox&cross={n}`。
    #[serde(default)]
    pub n: i64,
    /// 星位名称。
    #[serde(default)]
    pub name: Option<String>,
    /// 星位的一句话描述。
    #[serde(default)]
    pub line: Option<String>,
    /// 星群（内部编号，展示时按 `name`）。
    #[serde(default)]
    pub group: Option<String>,
    /// `/light/` 点亮编号；对方未公开时为 `null`。
    #[serde(default)]
    pub star: Option<i64>,
    /// 匿名星卡介绍的前 60 个字。
    #[serde(default)]
    pub intro: Option<String>,
    /// 对方的 VRChat 名字。
    ///
    /// **只在 `mutual` 为 `true`（双向连线）后才返回**，否则为 `null`。
    /// 也就是说这里的 `vrc` 是「连上之后才解锁」的信息，不能当通用字段用。
    #[serde(default)]
    pub vrc: Option<String>,
    /// 我是否已经连向对方。
    #[serde(default)]
    pub linked: bool,
    /// 是否已经双向连上。
    #[serde(default)]
    pub mutual: bool,
    /// 相交发生的时间（毫秒时间戳）。
    #[serde(default)]
    pub at: Option<i64>,
    /// 是否为上次查看后新产生的记录。
    #[serde(default)]
    pub fresh: bool,
}

/// 星轨相交里的「我」的摘要（`GET /me/crossings` 的 `me` 字段）。
///
/// 只取界面用得到的部分：契约允许服务端返回更多字段，多的直接忽略。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccCrossingMe {
    /// 我的星位编号。
    #[serde(default)]
    pub n: Option<i64>,
    /// 我的星位名称。
    #[serde(default)]
    pub name: Option<String>,
}

/// 星轨相交不可用的原因（`why`）。
///
/// 按官方文档列的七种取值分类；服务器以后加新的，落到 `Unknown` 而不是
/// 解析失败——一个没见过的原因不该让整页崩掉。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VrccCrossingWhy {
    /// 正常开放（`null`）。
    #[default]
    Available,
    /// 不对该玩家开放（16–17 岁星池，或隐私设置里关了）。**界面应整个隐藏**。
    Hidden,
    /// 尚未验证 VRChat 账号。
    Unverified,
    /// VRChat 账号正在等待验证。
    WaitingVerify,
    /// 机器人「VRCC 的小助手」的好友请求在排队。
    Queued,
    /// 机器人好友请求已发出，等待玩家在 VRChat 内同意。
    Sent,
    /// 玩家在 VRChat 内删除了机器人好友。
    Gone,
    /// 尚未开启机器人好友。
    NotFriend,
    /// 服务器新加的原因，本地还不认识。
    Unknown,
}

impl VrccCrossingWhy {
    /// 从服务端的 `why` 字符串解析。
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            None | Some("") => Self::Available,
            Some("hide") => Self::Hidden,
            Some("verify") => Self::Unverified,
            Some("wait") => Self::WaitingVerify,
            Some("queued") => Self::Queued,
            Some("sent") => Self::Sent,
            Some("gone") => Self::Gone,
            Some("friend") => Self::NotFriend,
            Some(_) => Self::Unknown,
        }
    }

    /// 是否应当**整个模块都不显示**。
    ///
    /// `Hidden` 是唯一要整个隐藏的情况——官方文档明确说「客户端不应显示
    /// 此模块」。其余原因都是「功能在但暂时不可用」，应当显示原因而不是
    /// 让页面空着。
    pub fn is_hidden(self) -> bool {
        matches!(self, Self::Hidden)
    }
}

impl VrccCrossings {
    /// 分类后的 `why`。
    pub fn reason(&self) -> VrccCrossingWhy {
        VrccCrossingWhy::parse(self.why.as_deref())
    }

    /// 按 `why` 定下 `hidden`。
    ///
    /// 由应用层在拿到响应后调用一次，之后所有消费方只看 `hidden` 字段，
    /// 不再各自去比 `why == "hide"` 字符串。
    pub fn settle_visibility(&mut self) {
        self.hidden = self.reason().is_hidden();
    }
}

/// `GET /me/crossings/cards`：星轨相交的完整星卡（1.13.0 起）。
///
/// 与 [`VrccCrossings`] 的区别：那个是**列表**（谁跟我擦过肩），这个是
/// **完整星卡**（网页「连向我 → 星轨相交」的样子），另带今天还能连几颗。
/// 只有带 `?mark=1` 才标成看过。算第二份读的次数。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccCrossingCards {
    #[serde(default)]
    pub cards: Vec<VrccStarCard>,
    /// 上次看过时最新那次擦肩的时间；`at` 比它新就是新的。
    #[serde(default)]
    pub seen: Option<i64>,
    /// 今天还能从星轨相交连几颗。
    #[serde(default)]
    pub left: Option<i64>,
    /// 每天最多几颗。
    #[serde(default)]
    pub max: Option<i64>,
}

/// `GET /me/today`：这一轮路过的星 + 轮次进度。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccToday {
    /// 第几轮，形如 `#12`。
    #[serde(default)]
    pub day: Option<String>,
    #[serde(default)]
    pub cards: Vec<VrccStarCard>,
    #[serde(default)]
    pub round: VrccRound,
}

// ---------------------------------------------------------------------------
// 服务状态
// ---------------------------------------------------------------------------

/// `GET /status`：不需要 client 标识的探活接口。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccStatus {
    /// 毫秒时间戳。
    #[serde(default)]
    pub at: Option<i64>,
    /// `ok` 表示正常。
    #[serde(default)]
    pub state: Option<String>,
}

// ---------------------------------------------------------------------------
// 未读数、通知中心、公告（1.12.0 / 1.13.0 起）
// ---------------------------------------------------------------------------

/// `GET /me/unread`：网页上几个红点的数字（1.13.0 起，`notifications` 权限）。
///
/// 读了不标已读。每个授权每天 1500 次，和别的读接口分开算，适合后台定时问。
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccUnread {
    /// 上次看「连向我」以后，新连向我的星。
    #[serde(default)]
    pub inbox: i64,
    /// 星轨相交里还没看过的。
    ///
    /// 只在令牌有 `crossings` 权限时才有这个键（16–17 岁没有星轨相交，恒为 0
    /// 就能看出星池）。没有时是 `None`，界面不显示这一项，而不是显示 0。
    #[serde(default)]
    pub cross: Option<i64>,
    /// 私聊未读的消息。
    #[serde(default)]
    pub chats: i64,
    /// 通知中心里未读的：`me/notes` 给的那些里玩家还没看过的。
    #[serde(default)]
    pub notes: i64,
}

impl VrccUnread {
    /// 几处加起来。
    pub fn total(&self) -> i64 {
        [self.inbox, self.cross.unwrap_or(0), self.chats, self.notes]
            .into_iter()
            .map(|count| count.max(0))
            .sum()
    }
}

/// 通知中心的一条（`GET /me/notes`，1.13.0 起）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccNote {
    #[serde(default)]
    pub id: i64,
    /// `link` / `mutual` / `gift` / `invite` / `yq` / `map` / `about`，
    /// 以后可能加新的：不认识的照样显示 `text`。
    #[serde(default)]
    pub kind: Option<String>,
    /// 网页上显示的那句话（纯文字）。
    #[serde(default)]
    pub text: Option<String>,
    /// 毫秒时间戳。
    #[serde(default)]
    pub at: Option<i64>,
    /// 玩家在网页上看过没有。
    #[serde(default)]
    pub seen: Option<bool>,
    /// 点开去的星友圈动态编号。
    #[serde(default)]
    pub post: Option<i64>,
    /// 点开去的地图（`wrld_…`）。
    #[serde(default)]
    pub world: Option<String>,
}

/// `GET /me/notes`：网页「通知」那张表，最多 60 条，新的在前。读了不标已读。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccNotes {
    #[serde(default)]
    pub notes: Vec<VrccNote>,
}

/// 一条公告（`GET /announcements` 的 `posts[]`）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccAnnouncement {
    #[serde(default)]
    pub id: i64,
    /// 网页上怎么显示：`bar` 顶上一条、`pop` 弹窗、`force` 强制。
    #[serde(default)]
    pub level: Option<String>,
    /// 纯文字，显示时不要当 HTML。
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// 「查看」打开的完整网址。
    #[serde(default)]
    pub link: Option<String>,
    /// 卡片顶上的图（完整网址）。
    #[serde(default)]
    pub cover: Option<String>,
    /// 什么时候撤下（毫秒），`null` 是没定。
    #[serde(default)]
    pub end: Option<i64>,
}

/// 停站说明。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccStop {
    #[serde(default)]
    pub say: Option<String>,
    /// 预计恢复的时间（毫秒），没定时是 `null`。
    #[serde(default)]
    pub until: Option<i64>,
}

/// `GET /announcements`：给所有玩家的公告和停站说明（1.12.0 起，不用令牌）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccAnnouncements {
    /// 停站时才有。
    #[serde(default)]
    pub stop: Option<VrccStop>,
    /// 正在显示的公告，新的在前。
    #[serde(default)]
    pub posts: Vec<VrccAnnouncement>,
}

/// 一张贴纸：`GET /stickers` 里 `stickers` 的一项。
///
/// 官方没保证这三个字段一定存在，所以都按「可能没有」处理——少一段装饰
/// 素材不该让整页解析失败。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccSticker {
    /// 贴纸名，展示用。
    #[serde(default)]
    pub name: Option<String>,
    /// 这张贴纸支持的材质代号（`paper` / `glitter` / …）。
    #[serde(default)]
    pub finishes: Vec<String>,
    /// 贴纸图的地址；`GET /stickers/{id}?fin=…` 会按材质和磨损画。
    #[serde(default)]
    pub url: Option<String>,
}

/// `GET /stickers`：所有贴纸和材质名（**不需要令牌，也不需要 `client`**）。
///
/// 两个字段都是「代号 → 值」的字典而不是数组：键是贴纸 ID 或材质代号，
/// 顺序没有意义，用 `BTreeMap` 让迭代顺序稳定。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccStickers {
    /// 贴纸 ID → 贴纸。
    #[serde(default)]
    pub stickers: std::collections::BTreeMap<String, VrccSticker>,
    /// 材质代号 → 材质名（`paper` → 纸质）。
    #[serde(default)]
    pub finishes: std::collections::BTreeMap<String, String>,
}

/// 我打过分的某张地图：`GET /me/maps` 里的一条。
///
/// **和 [`VrccBoardMap`] 不是同一个形状**：这里说的是「我给它打了多少分、
/// 写了什么」，所以有 `score` / `words` / `at`，没有 `avg` / `rank`。
/// 把两者塞进同一个结构体会让一半字段永远是空的，调用方也分不清「没有」
/// 和「不适用」。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccRatedMap {
    /// 世界 ID，形如 `wrld_…`。
    pub world: String,
    /// 地图名；官方不知道时是 `null`。
    #[serde(default)]
    pub name: Option<String>,
    /// 我打的分数（1–10）。
    #[serde(default)]
    pub score: Option<i64>,
    /// 我写的短评；没写是 `null`。
    #[serde(default)]
    pub words: Option<String>,
    /// 短评的审核状态：`wait` / `ok` / `hidden`。
    #[serde(default)]
    pub words_ok: Option<String>,
    /// 打分的时间（毫秒）。
    #[serde(default)]
    pub at: Option<i64>,
    /// vrcc.cc 上的地图页。
    #[serde(default)]
    pub page: Option<String>,
}

/// `GET /me/maps`：我打过分的地图，**最近打的在前**，最多 200 张。
///
/// 需要 `maps` 普通权限。计入个人每天 1000 次。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccRatedMaps {
    #[serde(default)]
    pub maps: Vec<VrccRatedMap>,
}

/// 玩家视角地图榜里的一行（`GET /me/board` 的 `maps` 条目）。
///
/// 比公开榜单多了「我自己的视角」：去过几次、我有没有打过分、热评、名次、
/// 介绍首行。**不复用 [`VrccBoardMap`]**：那边没有这些字段，硬塞会让
/// 调用方分不清「没有」和「不适用」。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMyBoardMap {
    /// 世界 ID（1.11.0 起也有同值的 `world`）。
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub world: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// 房间人数上限。
    #[serde(default)]
    pub cap: Option<i64>,
    /// 封面**相对路径**。
    #[serde(default)]
    pub img: Option<String>,
    /// 同一张封面的完整网址（1.11.0 起）。
    #[serde(default)]
    pub img_url: Option<String>,
    /// 榜单状态。
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub avg: Option<f64>,
    #[serde(default)]
    pub raters: Option<i64>,
    /// 去过的人数。
    #[serde(default)]
    pub went: Option<i64>,
    /// 总榜或本周榜上的名次；有人打分的才有。筛选不改名次。
    #[serde(default)]
    pub rank: Option<i64>,
    /// 赞最多的那段介绍的第一行。
    #[serde(default)]
    pub intro: Option<String>,
    /// 最热的一条短评。
    #[serde(default)]
    pub hot: Option<VrccMapHot>,
    /// 最多 3 条短评。
    #[serde(default)]
    pub hots: Vec<VrccMapHot>,
}

/// `GET /me/board`：玩家看到的地图榜。
///
/// 需要 `maps` 普通权限。和网页「榜单」一样，带玩家自己的评分、去过、热评。
/// `dot` 是「有新内容」的提示，`min` 是进榜需要几人评分。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMyBoard {
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub min: Option<i64>,
    /// 有新内容的提示。
    #[serde(default)]
    pub dot: Option<VrccBoardDot>,
    #[serde(default)]
    pub maps: Vec<VrccMyBoardMap>,
}

/// `GET /me/board` 的 `dot`：哪一块有新内容。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccBoardDot {
    #[serde(default)]
    pub maps: bool,
    #[serde(default)]
    pub went: bool,
}

/// 分数分布的一段：`9–10` 分占百分之几。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccScoreBand {
    /// 这一段的分数下限。
    #[serde(default)]
    pub from: i64,
    /// 这一段的分数上限。
    #[serde(default)]
    pub to: i64,
    /// 占百分之几（整数）。
    #[serde(default)]
    pub pct: i64,
}

/// 多小咕的实测数据（`perf`）。**整个可以没有**，里面每一项也可以是 `null`。
///
/// 所以除了 `from` 之外全部是 `Option`——没测过的图不该显示成一堆 0。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapPerf {
    /// 实测方式：`test`（测地图本身）/ `hang`（挂房的平均）。
    #[serde(default)]
    pub from: Option<String>,
    /// 轻重档位：`light` / `mid` / `heavy`。
    #[serde(default)]
    pub weight: Option<String>,
    /// 显存（MB）。
    #[serde(default)]
    pub vram: Option<f64>,
    /// 内存（MB）。
    #[serde(default)]
    pub mem: Option<f64>,
    /// 加载时间（毫秒）。
    #[serde(default)]
    pub load: Option<f64>,
    /// 下载大小（字节）。
    #[serde(default)]
    pub size: Option<f64>,
    /// 支持的平台：`PC` / `Quest` / `iOS`。
    #[serde(default)]
    pub platforms: Vec<String>,
    /// 挂房的分钟数（实测时为 `null`）。
    #[serde(default)]
    pub minutes: Option<i64>,
}

/// `GET /maps/{world}/page`：公开地图页（1.13.0 起，**不需要令牌**）。
///
/// 和 vrcc.cc 上每张地图的页面一样：榜上那一行、分数分布、去过的人数、
/// 房间上限、多小咕实测。短评和介绍不在这里。
///
/// 不在榜上或没过审时是 `404 not_found`——那是**正常状态**，不是故障。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapPage {
    /// 榜上的那一行。
    pub map: VrccBoardMap,
    /// 分数分布：9–10、7–8、5–6、3–4、1–2 各占百分之几。
    #[serde(default)]
    pub spread: Vec<VrccScoreBand>,
    /// 去过的人数。
    #[serde(default)]
    pub went: i64,
    /// 房间人数上限。
    #[serde(default)]
    pub cap: Option<i64>,
    /// 多小咕实测；没测过是 `null`。
    #[serde(default)]
    pub perf: Option<VrccMapPerf>,
    /// 服务器生成这份数据的时刻（毫秒）。
    #[serde(default)]
    pub at: i64,
}

/// 一条打分记录（短评）：`/me/maps/{world}/page` 的 `rates[]` 与 `mine.rate`。
///
/// 公开的地图页**不给**这个列表，只有带令牌的 `/me/…/page` 才有。官方还会按
/// 令牌权限调整写者身份：有 `mutuals` 且客户端审核通过时给对方的 `n`，
/// 否则一律匿名成 `{"name": "某位<星位>", "group": "G"}`。**我们只照原样显示
/// `who`，不自己拼匿名格式**——那是服务器的职责，我们拼就可能在权限变化时
/// 显示错的东西。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapRate {
    /// 这条打分的编号。
    #[serde(default)]
    pub id: Option<i64>,
    /// 短评正文；没写短评时是 `null`。
    #[serde(default)]
    pub text: Option<String>,
    /// 打的分（1–10）。
    #[serde(default)]
    pub score: Option<i64>,
    /// 赞、踩的计数。
    #[serde(default)]
    pub up: Option<i64>,
    #[serde(default)]
    pub down: Option<i64>,
    /// 打分时间（毫秒）。
    #[serde(default)]
    pub at: Option<i64>,
    /// 写者。字段形状由服务器按权限决定，这里宽松处理。
    #[serde(default)]
    pub who: Option<VrccMapRateWho>,
    /// 是不是我写的。
    #[serde(default)]
    pub mine: Option<bool>,
    /// 我投的票：`1` 赞 / `-1` 踩 / `0` 没投。
    #[serde(default)]
    pub vote: Option<i64>,
    /// 被折叠（内容或作者被隐藏）。
    #[serde(default)]
    pub folded: Option<bool>,
    /// 只有 `mine.rate` 才有：审核状态（`wait` / `ok` / `hidden`）。
    #[serde(default)]
    pub check: Option<String>,
}

/// 一条打分里「写者」的身份。
///
/// 官方按权限决定给多少：审核通过的客户端 + `mutuals` 能看到别人的 `n` 和
/// VRChat 名字；否则只有匿名的「某位<星位>」。**这里全部是 `Option`**——
/// 缺字段是常态，不是异常。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapRateWho {
    #[serde(default)]
    pub n: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    /// 星群代号（`G` / `W` / `Z` / `L` / `A`）。
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub photo: Option<bool>,
    /// 互相连上时对方的 VRChat 名字。
    #[serde(default)]
    pub vrc: Option<String>,
    #[serde(default)]
    pub mutual: Option<bool>,
    /// 是不是我。
    #[serde(default)]
    pub me: Option<bool>,
}

/// `/me/maps/{world}/page`：带令牌的地图页（1.13.0 起）。
///
/// 比公开版多出：完整短评列表、地图介绍、正在开的房，以及**我自己的评分**。
/// 需要 `maps` 普通权限（我们已有），因此不需要重新授权。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMyMapPage {
    /// 榜上的那一行。
    #[serde(default)]
    pub map: Option<VrccBoardMap>,
    /// 地图介绍；没有时是 `null`。
    #[serde(default)]
    pub intro: Option<VrccMapIntro>,
    /// 短评列表，按 `sort` 排。
    #[serde(default)]
    pub rates: Vec<VrccMapRate>,
    /// 我自己的评分与短评。
    #[serde(default)]
    pub mine: Option<VrccMyRate>,
    /// 当前的排序方式（`hot` / `new`）。
    #[serde(default)]
    pub sort: Option<String>,
}

/// 地图介绍。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMapIntro {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub up: Option<i64>,
    #[serde(default)]
    pub down: Option<i64>,
    #[serde(default)]
    pub at: Option<i64>,
    #[serde(default)]
    pub who: Option<VrccMapRateWho>,
}

/// 「我的」那一小块：自己打的分数与短评。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccMyRate {
    /// 我的打分记录；没打过分时是 `null`。
    #[serde(default)]
    pub rate: Option<VrccMapRate>,
    /// 我写的地图介绍；没有时是 `null`。
    #[serde(default)]
    pub intro: Option<VrccMapIntro>,
}

/// 短评在打分请求里的三种语义。
///
/// 官方文档写得很细，而这三者差别极大：
///
/// | 传什么 | 结果 |
/// |---|---|
/// | 非空字符串 | 提交或更新短评（会审核） |
/// | 空字符串 `""` | **删掉已有短评**，分数保留 |
/// | 完全不传这个字段 | 只改分数，**保留原短评** |
///
/// 所以不能用 `Option<String>`：那样 `None` 与「明确传空」在序列化上都是
/// `null` 或缺失，无法区分，而错一个就把玩家的短评删了。
///
/// 这个类型只用于**请求**，不参与响应解析。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VrccRatingText {
    /// 完全不传 `text` 字段：只改分数，原有短评原样保留。
    Keep,
    /// 传空字符串：删除已有短评。
    Clear,
    /// 传这段文字：提交或更新短评。
    Write(String),
}

impl VrccRatingText {
    /// 序列化成请求体里的 `text` 字段。
    ///
    /// `Keep` 返回 `None`，调用方据此**不把这个键放进 JSON**——放一个
    /// `null` 进去与官方语义不符。
    pub fn as_body_text(&self) -> Option<&str> {
        match self {
            Self::Keep => None,
            Self::Clear => Some(""),
            Self::Write(text) => Some(text.as_str()),
        }
    }
}

/// 打分请求。
///
/// 分数必须是 1–10 的整数（官方 `minimum: 1` / `maximum: 10`）。这里用
/// `i64` 并在发送前夹取，而不是靠界面保证——越界会被服务器 `400
/// bad_request` 打回来，那对玩家是个莫名其妙的报错。
#[derive(Clone, Debug)]
pub struct VrccRatingInput {
    pub world: String,
    pub score: i64,
    pub text: VrccRatingText,
}

impl VrccRatingInput {
    /// 分数的合法范围。
    pub const MIN_SCORE: i64 = 1;
    pub const MAX_SCORE: i64 = 10;
    /// 短评的最长字数（官方 `maxLength: 30`）。
    pub const MAX_TEXT_CHARS: usize = 30;

    /// 夹取到合法范围。界面可以先用它做即时反馈，发送前仍会再夹一次。
    pub fn clamped_score(&self) -> i64 {
        self.score.clamp(Self::MIN_SCORE, Self::MAX_SCORE)
    }
}

/// 投票的对象类型。
///
/// 官方接口同一套投票同时服务于短评（`rate`）和地图介绍（`intro`）。我们
/// 目前只做短评的投票，但**把两种都建模**：请求体里的 `kind` 是必填的，
/// 枚举化成字面量可以减少「写错字符串」这类低级错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum VrccVoteKind {
    /// 给一条短评投票。
    Rate,
    /// 给地图介绍投票。
    Intro,
}

impl VrccVoteKind {
    /// 请求体里 `kind` 字段的字面量。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rate => "rate",
            Self::Intro => "intro",
        }
    }
}

/// 我对一条内容的投票。
///
/// 三种取值对应官方 `v`：`1` 赞、`-1` 踩、`0` 取消。
///
/// **单独建类型而不是用 `i64`**：界面会频繁地在「当前票值」和「点一下之后
/// 该发什么」之间换算（点已赞的赞 = 取消 = 0），用枚举能把这件事写清楚，
/// 也挡住传 `2` 这种服务器必然拒绝的值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum VrccVote {
    /// 取消投票。
    None,
    /// 赞。
    Up,
    /// 踩。
    Down,
}

impl VrccVote {
    /// 请求体里 `v` 字段的整数值。
    pub fn as_body_value(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Up => 1,
            Self::Down => -1,
        }
    }

    /// 从服务器返回的整数解析。认不出的值按「没投」处理——那比报错温和，
    /// 而且界面只会少高亮一个按钮。
    pub fn from_body_value(value: i64) -> Self {
        match value {
            1 => Self::Up,
            -1 => Self::Down,
            _ => Self::None,
        }
    }

    /// 点一下同一个按钮之后应该变成什么（点已赞的赞 = 取消）。
    ///
    /// 这段逻辑放在契约层，是因为界面和服务端两处都要用到同一个判断，
    /// 各写一遍迟早会分叉。
    pub fn toggled_from(self, tapped: Self) -> Self {
        if self == tapped {
            Self::None
        } else {
            tapped
        }
    }
}

/// 投票结果（`POST /me/map-votes` 的 200）。
///
/// 官方返回的是**更新之后**的计数，所以调用方可以直接拿它刷新界面，
/// 不必再查一次列表。
///
/// ## `my_vote` 为什么不叫 `mine`
///
/// 官方响应里这个字段叫 `mine`，但同名的 `VrccMapRate::mine` 表示「这条内容
/// 是不是我写的」，语义完全不同（一个是布尔，一个是我投的票）。两个字段
/// 放在同一个界面里用，叫同一个名字迟早会有人把「我写的」当成「我投了」，
/// 所以这里改名，并且注明它来自官方的 `mine`。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccVoteResult {
    /// 对象的类型（`rate` / `intro`）。
    #[serde(default)]
    pub kind: Option<String>,
    /// 对象的编号。
    #[serde(default)]
    pub id: Option<i64>,
    /// 更新后的赞数。
    #[serde(default)]
    pub up: Option<i64>,
    /// 更新后的踩数。
    #[serde(default)]
    pub down: Option<i64>,
    /// 我现在的票；来自官方的 `mine` 字段（整数）。
    #[serde(default, rename = "mine")]
    pub my_vote: Option<i64>,
}

// ---------------------------------------------------------------------------
// 错误体
// ---------------------------------------------------------------------------

/// 官方错误体：`{"error": "…", "need": "…", "why": "…"}`。
///
/// 三个字段都可能缺，所以整体是 Option 链而不是结构体默认值。
/// `every` 只在 429 `too_soon` 时出现，是官方建议的重试间隔（秒）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
pub struct VrccErrorBody {
    #[serde(default)]
    pub error: Option<String>,
    /// 缺哪一项权限或前置条件。
    #[serde(default)]
    pub need: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    /// 429 `too_soon` 时给的建议轮询间隔（秒）。
    ///
    /// 要等多久以 `Retry-After` 响应头为准（1.9.0 起所有 429 都带）；
    /// 旧服务器没给头时才用它兜底。
    #[serde(default)]
    pub every: Option<i64>,
    /// `503 stop`（停站维护）时的说明。
    #[serde(default)]
    pub say: Option<String>,
    /// `503 stop` 时预计恢复的时间（毫秒）。
    #[serde(default)]
    pub until: Option<i64>,
}

// ---------------------------------------------------------------------------
// 前端用的聚合投影
// ---------------------------------------------------------------------------

/// 本客户端今天的用量。
///
/// **全部来自服务端响应头，既不落盘也不自我推算。**
///
/// 1.11.0 起，带每天次数的接口会回答 `RateLimit-Limit` / `RateLimit-Remaining`
/// / `RateLimit-Reset`，那是服务器按这个授权算的准数。官方没有「查询额度」的
/// 接口，这组头只在你发某个请求的那一刻回一次——所以界面上显示的是「最近一次
/// 请求时服务端说的余量」，不是实时值，也不需要是实时值。
///
/// 为什么不再自己记账：官方的时间基准是**北京时间零点**（`RateLimit-Reset`
/// 是到那时候的秒数），而本地推算只能拿到本机时区的自然日。两套时钟对非
/// UTC+8 的玩家必然错开，本地记的数会变成「昨天下班前用了 980」这种误导，
/// 还可能在服务端早就重置之后继续把请求拦在本地。宁可显示「未知」，也不显示
/// 一个自己编的数。
#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccUsageSnapshot {
    /// 今日「第一份读」已用次数（最近的提醒、星轨相交、互相连上、所在的地图、
    /// 我打过分的地图合计）。还没拿到过服务端响应头时为 `null`。
    pub personal_used_today: Option<i64>,
    /// 「第一份读」每个授权每天的上限（1.8.4 起是 1000）。
    pub personal_daily_limit: Option<i64>,
    /// 今日 `me/unread` 已用次数；服务器还没给过响应头时为 `null`。
    pub unread_used_today: Option<i64>,
    /// `me/unread` 每天的上限（1500）；同上，没拿到时为 `null`。
    pub unread_daily_limit: Option<i64>,
    /// 限流窗口内已发请求数。
    pub window_used: i64,
    /// 限流窗口容量（每分钟）。
    pub window_limit: i64,
    /// 若正在退避，还有多少毫秒恢复。
    pub backoff_remaining_ms: i64,
    /// 服务端下一次重置额度还有多少秒（`RateLimit-Reset`）；没拿到时为 `null`。
    ///
    /// 这是**服务端**的时间基准（北京时间零点），界面据此显示「几点恢复」，
    /// 不要拿本机时区去推。
    pub reset_secs: Option<i64>,
}

/// 连接状态投影。
#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccConnectionSnapshot {
    /// 是否持有令牌。
    pub connected: bool,
    /// 上一个令牌是不是被服务器判了失效（`404 gone`）后清掉的。
    ///
    /// 玩家在网页上断开、同一个客户端又授权了一次、星被删除，以及 1.10.0 起
    /// 「90 天没调用过任何接口自动断开」，都会这样。界面据此提示「连接已失效，
    /// 请重新连接」，而不是只显示一个「未连接」让玩家摸不着头脑。
    pub expired: bool,
    /// 星号，已连接且拿到时才有。
    pub star: Option<i64>,
    /// 公开的 VRChat 名字。
    pub vrc_name: Option<String>,
    /// 令牌实际拿到的 scope。
    pub scope: Option<String>,
    /// 客户端标识。
    pub client: String,
    /// 当前是否在用 `dev-` 开发态标识。
    ///
    /// 给界面用：`dev-` 标识在玩家同意页会被标成「（测试中）」，且只能要普通
    /// 权限。界面上让开发者和测试者看到这件事，比事后排查 403 省事。
    pub client_is_dev: bool,
    /// 上次成功轮询提醒的时间（毫秒）。
    pub last_poll_at: Option<i64>,
    /// 下一次建议轮询时间（毫秒）。
    pub next_poll_at: Option<i64>,
}

/// 腕表上显示的星遇同频摘要。
///
/// 定义在契约层而不是 `overlay-runtime`：填充它的是 `application` 层的服务，
/// 而 `application` 不能反向依赖 `overlay-runtime`。放在这里两边都能用。
///
/// 只包含**不额外消耗额度**能拿到的数据：
/// - `unread` 优先来自后台轮询的 `me/unread`（网页上几个红点的合计，单独的
///   每天 1500 次）；旧服务器没有这个接口时，退回「最近的提醒」里 `id > read`
///   的条数
/// - `map_rank` / `map_avg_x10` 来自「地图榜」的结果（开放数据，不计个人额度）
///
/// 刻意不去调 `me/maps/{world}` 查当前地图：那要花个人额度，而腕表是每帧
/// 都要读的展示位，不该成为额度消耗的来源。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccWristSummary {
    /// 未读数。0 表示没有未读。
    pub unread: u32,
    /// 当前所在地图在榜单上的名次；不在榜或榜单还没加载时为 `None`。
    pub map_rank: Option<i64>,
    /// 当前地图的均分，放大 10 倍存整数（避免浮点比较）。
    pub map_avg_x10: Option<i64>,
}
