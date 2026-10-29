#![allow(non_snake_case)]

//! 星遇同频（vrcc.cc）的命令层。
//!
//! 每个命令只做「取运行时 → 调服务 → 错误映射」，业务在
//! `vrcx_0_application::vrcc`，网络在 `vrcx_0_integrations::vrcc`。
//!
//! 设备授权是两步：`begin` 拿码，`poll` 按返回的 `interval` 反复问。
//! 中间态（`device_code`）留在前端，不落盘。

use tauri::State;
use vrcx_0_application::vrcc::{VrccDeviceAuthPrompt, VrccDeviceAuthStep};
use vrcx_0_contracts::vrcc::{
    VrccAnnouncements, VrccBoard, VrccBoardSort, VrccConnectionSnapshot, VrccMapPage, VrccMyBoard,
    VrccMyBoardSort, VrccNotes, VrccRatedMaps, VrccRecentNotifications, VrccStatus, VrccStickers,
    VrccToday, VrccUnread, VrccUsageSnapshot,
};

use crate::{error::AppError, state::AppState};

/// 设备授权的起步结果。`device_code` 是唯一需要回传的字段，其余是给玩家看的。
///
/// 它同时是 `device_auth_poll` 的入参，所以既要 `Serialize` 也要 `Deserialize`。
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VrccDeviceAuthPromptOutput {
    /// 给玩家看的码，形如 `ABCD-2345`。
    pub user_code: String,
    /// 手动输码的页面。
    pub verification_uri: Option<String>,
    /// 带码的同意链接，优先用这个。
    pub verification_uri_complete: Option<String>,
    /// 多少秒内有效。
    pub expires_in_secs: i64,
    /// 官方建议的轮询间隔。
    pub interval_secs: i64,
    /// 内部用，换令牌时要回传。
    pub device_code: String,
    /// 这次要到的权限。
    pub scope: Option<String>,
}

impl From<VrccDeviceAuthPrompt> for VrccDeviceAuthPromptOutput {
    fn from(prompt: VrccDeviceAuthPrompt) -> Self {
        Self {
            user_code: prompt.user_code,
            verification_uri: prompt.verification_uri,
            verification_uri_complete: prompt.verification_uri_complete,
            expires_in_secs: prompt.expires_in_secs,
            interval_secs: prompt.interval_secs,
            device_code: prompt.device_code,
            scope: prompt.scope,
        }
    }
}

/// 轮询一次授权的三种结果。
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum VrccDeviceAuthStepOutput {
    /// 玩家还没点同意，`interval_secs` 后再问。
    Pending { interval_secs: i64 },
    /// 连上了。
    Connected { connection: VrccConnectionSnapshot },
    /// 玩家取消或缺码过期，这条流程结束。
    Terminal { message: String },
}

impl From<VrccDeviceAuthStep> for VrccDeviceAuthStepOutput {
    fn from(step: VrccDeviceAuthStep) -> Self {
        match step {
            VrccDeviceAuthStep::Pending { interval_secs } => Self::Pending { interval_secs },
            VrccDeviceAuthStep::Connected(connection) => Self::Connected { connection },
            VrccDeviceAuthStep::Terminal(message) => Self::Terminal { message },
        }
    }
}

// -- 连接状态 ---------------------------------------------------------------

#[tauri::command(async)]
#[specta::specta]
pub fn app__vrcc_connection(state: State<'_, AppState>) -> VrccConnectionSnapshot {
    state.runtime_host().vrcc().connection()
}

/// 今日用量。口径是「今日已用 N/上限」，不是「剩余」，且全部来自服务端的
/// `RateLimit-*` 响应头；还没拿到过时字段是 `null`（界面显示「未知」）。
#[tauri::command(async)]
#[specta::specta]
pub fn app__vrcc_usage(state: State<'_, AppState>) -> VrccUsageSnapshot {
    state.runtime_host().vrcc().usage()
}

// -- 开放数据 ---------------------------------------------------------------

/// 地图榜。不需要连接。`sort` 传 `null` 是总榜。
///
/// 要的是本周榜或新上榜、回答里却没有 `sort` 时，说明服务器还不支持，
/// 回的是总榜。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_boards(
    state: State<'_, AppState>,
    limit: u32,
    sort: Option<VrccBoardSort>,
) -> Result<VrccBoard, AppError> {
    state
        .runtime_host()
        .vrcc()
        .boards(limit, sort.unwrap_or_default())
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 按世界 ID 批量查地图。不需要连接。
///
/// 一次最多 20 张，**只返回在榜上的**——结果条数可能少于传入的数量，前端要
/// 按 `world` 去配，不能按下标对应。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_maps(
    state: State<'_, AppState>,
    worlds: Vec<String>,
) -> Result<VrccBoard, AppError> {
    state
        .runtime_host()
        .vrcc()
        .maps(&worlds)
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 公告和停站。不需要连接。服务器还没有这个接口时返回 `null`。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_announcements(
    state: State<'_, AppState>,
) -> Result<Option<VrccAnnouncements>, AppError> {
    state
        .runtime_host()
        .vrcc()
        .announcements()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 所有贴纸和材质名。不需要连接。服务器还没有这个接口时返回 `null`。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_stickers(
    state: State<'_, AppState>,
) -> Result<Option<VrccStickers>, AppError> {
    state
        .runtime_host()
        .vrcc()
        .stickers()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 公开地图页。不需要令牌。不在榜上时（`404`）返回 `null`。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_map_page(
    state: State<'_, AppState>,
    world: String,
) -> Result<Option<VrccMapPage>, AppError> {
    state
        .runtime_host()
        .vrcc()
        .map_page(&world)
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 玩家视角地图榜。需要 `maps` 普通权限。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_my_board(
    state: State<'_, AppState>,
    sort: Option<VrccMyBoardSort>,
) -> Result<VrccMyBoard, AppError> {
    state
        .runtime_host()
        .vrcc()
        .my_board(sort.unwrap_or_default())
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 我打过分的地图。需要 `maps` 普通权限。最近的在前。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_my_maps(state: State<'_, AppState>) -> Result<VrccRatedMaps, AppError> {
    state
        .runtime_host()
        .vrcc()
        .my_maps()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 服务探活。用来在页面上显示「星遇同频现在可用吗」。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_status(state: State<'_, AppState>) -> Result<VrccStatus, AppError> {
    state
        .runtime_host()
        .vrcc()
        .status()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

// -- 设备授权 ---------------------------------------------------------------

/// 第一步：申请连接码。`scope` 传 `null` 时按当前 `X-Client` 选权限。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_device_auth_begin(
    state: State<'_, AppState>,
    scope: Option<String>,
) -> Result<VrccDeviceAuthPromptOutput, AppError> {
    state
        .runtime_host()
        .vrcc()
        .begin_device_auth(scope)
        .await
        .map(VrccDeviceAuthPromptOutput::from)
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 第二步：问一次令牌。前端按返回的 `interval_secs` 定时重试。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_device_auth_poll(
    state: State<'_, AppState>,
    prompt: VrccDeviceAuthPromptOutput,
) -> Result<VrccDeviceAuthStepOutput, AppError> {
    let prompt = VrccDeviceAuthPrompt {
        user_code: prompt.user_code,
        verification_uri: prompt.verification_uri,
        verification_uri_complete: prompt.verification_uri_complete,
        expires_in_secs: prompt.expires_in_secs,
        interval_secs: prompt.interval_secs,
        device_code: prompt.device_code,
        scope: prompt.scope,
    };
    state
        .runtime_host()
        .vrcc()
        .poll_device_auth(&prompt)
        .await
        .map(VrccDeviceAuthStepOutput::from)
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 断开。先让服务器吊销，再清本地。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_disconnect(state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .runtime_host()
        .vrcc()
        .disconnect()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

// -- 提醒 -------------------------------------------------------------------

/// 最近的提醒。
///
/// `mark` 为 `true` 表示玩家正在看这个列表：服务器会把已读状态同步到账号，
/// 其他设备（网页、手机 App）上的红点跟着消。后台只为算数字时传 `false`。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_recent_notifications(
    state: State<'_, AppState>,
    mark: bool,
) -> Result<VrccRecentNotifications, AppError> {
    state
        .runtime_host()
        .vrcc()
        .recent_notifications(mark)
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 几处未读数（连向我、星轨相交、私聊、通知）。
///
/// `refresh` 为 `false` 时只读后台轮询的缓存、不发请求——收到
/// `vrccNoticesChanged` 时用这个。服务器还没有 `me/unread` 时返回 `null`。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_unread(
    state: State<'_, AppState>,
    refresh: bool,
) -> Result<Option<VrccUnread>, AppError> {
    state
        .runtime_host()
        .vrcc()
        .unread(refresh)
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 通知中心。读了不标已读。服务器还没有 `me/notes` 时返回 `null`。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_notes(state: State<'_, AppState>) -> Result<Option<VrccNotes>, AppError> {
    state
        .runtime_host()
        .vrcc()
        .notes()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 当前所在地图在地图榜上的状态与我自己的评分。
///
/// 地图 ID 由后端自己从游戏日志取，前端不用传：位置本来就只有后端知道，
/// 让每个调用点自己算一遍只会多出一处可能算错的地方。
///
/// 返回 `null` 的三种情况：不在任何地图里、地图被下架或不对当前玩家开放
/// （官方要求此时不显示打分面板）、以及未连接。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_my_map(
    state: State<'_, AppState>,
) -> Result<Option<vrcx_0_contracts::vrcc::VrccMyMap>, AppError> {
    let runtime = state.runtime_host();
    let world = runtime.current_world_id();
    if world.is_empty() {
        return Ok(None);
    }
    runtime
        .vrcc()
        .my_map(&world)
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 增量轮询。游标由后端维护，前端不用关心。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_poll_notifications(
    state: State<'_, AppState>,
) -> Result<vrcx_0_contracts::vrcc::VrccNotifications, AppError> {
    state
        .runtime_host()
        .vrcc()
        .poll_notifications()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 今天这一轮 + 轮次进度。需要 `today` 敏感权限（要审核）。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_today(state: State<'_, AppState>) -> Result<VrccToday, AppError> {
    state
        .runtime_host()
        .vrcc()
        .today()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}

/// 「互相连上」的星友列表。需要 `mutuals` 敏感权限（要审核）。
///
/// 条目里的 `vrcUser` 用来打开 VRCX-0 自己的玩家详情页；加好友由玩家在那里
/// 自己点。官方合规要求明文禁止程序代玩家发好友请求，所以这里只有这个命令。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_mutuals(
    state: State<'_, AppState>,
) -> Result<vrcx_0_contracts::vrcc::VrccMutuals, AppError> {
    state
        .runtime_host()
        .vrcc()
        .mutuals()
        .await
        .map_err(|failure| AppError::Vrcc { failure })
}
