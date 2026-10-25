//! 星遇同频（vrcc.cc）开放接口的线上数据契约。
//!
//! 字段名与类型严格对照官方 `openapi.yaml` 与 `xingshi-app` 的实现，
//! 不做"我猜它应该叫这个"的推断。凡是官方允许缺失的字段，一律 `Option`，
//! 因为接口在不同 scope / 审核状态下返回的字段集合并不相同。

use serde::{Deserialize, Serialize};

/// API 基址。生产代码里只允许拼相对路径，绝对地址会在客户端层被拒绝。
pub const VRCC_API_BASE: &str = "https://vrcc.cc/api/v1/";

/// 站点根，用来把接口给的**相对路径**（如 `/match/?tab=inbox`）拼成完整地址。
pub const VRCC_SITE_ORIGIN: &str = "https://vrcc.cc";

/// 默认客户端标识。申请到审核通过的 `app_…` 后改这里。
pub const VRCC_DEFAULT_CLIENT: &str = "dev-vrcx0";

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
    /// 评分人数；官方要求满 5 人才进总榜。
    #[serde(default)]
    pub raters: Option<i64>,
    /// 名次，1 起。
    #[serde(default)]
    pub rank: Option<i64>,
    /// 口碑档位 `light` / `mid` / …
    #[serde(default)]
    pub weight: Option<String>,
    /// vrcc.cc 上的地图页。
    #[serde(default)]
    pub page: Option<String>,
    /// 封面图地址。
    #[serde(default)]
    pub img: Option<String>,
}

/// 地图榜 / 批量查地图 / 我打过的图，三者的外层结构一致。
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccBoard {
    #[serde(default)]
    pub board: Option<String>,
    /// 上总榜所需最少评分人数。
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
    /// 现在能不能打分。
    #[serde(default)]
    pub can: Option<bool>,
    /// 不能打分时缺什么：`verify` / `friend` / `go`。
    #[serde(default)]
    pub need: Option<String>,
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
    pub fn image_url(&self) -> Option<String> {
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
    /// 氛围：`安静` / `随场` / `热闹`。
    #[serde(default)]
    pub vibe: Option<String>,
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
    /// 429 `too_soon` 时给的建议重试间隔（秒）。
    ///
    /// 官方把 429 分成两种：`too_soon` 是两次轮询间隔太短，`too_many` 是
    /// 频率或当日额度用完。只有前者带这个字段，用来告诉客户端该等多久。
    #[serde(default)]
    pub every: Option<i64>,
}

// ---------------------------------------------------------------------------
// 前端用的聚合投影
// ---------------------------------------------------------------------------

/// 本客户端的本地用量。**这不是服务器数据**：官方没有"查询剩余配额"的接口，
/// 所以只能报我们自己记的计数，措辞必须诚实。
#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccUsageSnapshot {
    /// 今日个人接口已用次数（本地计数）。
    pub personal_used_today: i64,
    /// 官方对四个个人接口的每日合计上限。
    pub personal_daily_limit: i64,
    /// 今日 `/device/code` 已用次数（本地计数）。
    pub device_code_used_today: i64,
    /// 官方对每网络地址每日拿码上限。
    pub device_code_daily_limit: i64,
    /// 限流窗口内已发请求数。
    pub window_used: i64,
    /// 限流窗口容量（每分钟）。
    pub window_limit: i64,
    /// 若正在退避，还有多少毫秒恢复。
    pub backoff_remaining_ms: i64,
    /// 用量计数的日期（本地时区 `YYYY-MM-DD`）。
    pub date: String,
}

/// 连接状态投影。
#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccConnectionSnapshot {
    /// 是否持有令牌。
    pub connected: bool,
    /// 星号，已连接且拿到时才有。
    pub star: Option<i64>,
    /// 公开的 VRChat 名字。
    pub vrc_name: Option<String>,
    /// 令牌实际拿到的 scope。
    pub scope: Option<String>,
    /// 客户端标识。
    pub client: String,
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
/// - `unread` 来自「最近的提醒」的结果（页面本来就会请求）
/// - `map_rank` / `map_avg_x10` 来自「地图榜」的结果（开放数据，不计个人额度）
///
/// 刻意不去调 `me/maps/{world}` 查当前地图：那要花个人额度，而腕表是每帧
/// 都要读的展示位，不该成为额度消耗的来源。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccWristSummary {
    /// 未读提醒条数。0 表示没有未读。
    pub unread: u32,
    /// 当前所在地图在榜单上的名次；不在榜或榜单还没加载时为 `None`。
    pub map_rank: Option<i64>,
    /// 当前地图的均分，放大 10 倍存整数（避免浮点比较）。
    pub map_avg_x10: Option<i64>,
}
