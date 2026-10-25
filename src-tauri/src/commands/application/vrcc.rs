#![allow(non_snake_case)]

//! 星遇同频（vrcc.cc）的命令层。
//!
//! 每个命令只做「取运行时 → 调服务 → 错误映射」，业务在
//! `vrcx_0_application::vrcc`，网络在 `vrcx_0_integrations::vrcc`。
//!
//! 设备授权是两步：`begin` 拿码，`poll` 按返回的 `interval` 反复问。
//! 中间态（`device_code`）留在前端，不落盘。

use tauri::State;
use vrcx_0_application::vrcc::{VrccDeviceAuthPrompt, VrccDeviceAuthStep, VrccErrorKind};
use vrcx_0_contracts::vrcc::{
    VrccBoard, VrccConnectionSnapshot, VrccRecentNotifications, VrccStatus, VrccToday,
    VrccUsageSnapshot,
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

/// 错误分类，供前端决定显示「重新连接」还是「稍后再试」。
///
/// 前端拿到的是 `AppError`（字符串），但「是否需要重新授权」这种判断不该
/// 让前端去解析文案。这个类型把 `classify_vrcc_error` 的结论带过命令边界。
#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum VrccErrorKindOutput {
    NeedsReauth,
    MissingScope,
    Throttled,
    Transport,
    Other,
}

impl From<VrccErrorKind> for VrccErrorKindOutput {
    fn from(kind: VrccErrorKind) -> Self {
        match kind {
            VrccErrorKind::NeedsReauth => Self::NeedsReauth,
            VrccErrorKind::MissingScope => Self::MissingScope,
            VrccErrorKind::Throttled => Self::Throttled,
            VrccErrorKind::Transport => Self::Transport,
            VrccErrorKind::Other => Self::Other,
        }
    }
}

/// 把一行错误信息归类，并把缺的权限一并带回。
///
/// 运行时的命令把 `VrccError` 折成了字符串（`AppError::Custom`），这里按
/// 官方错误码表的关键字分类。这不是理想的形状——理想是让 `VrccError` 自己
/// 跨过命令边界——但改那个要动 `AppError` 的变体，留到有明确需求时再做。
#[tauri::command(async)]
#[specta::specta]
pub fn app__vrcc_classify_error(message: String) -> VrccErrorKindOutput {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("gone")
        || lowered.contains("client_mismatch")
        || lowered.contains("client_unknown")
        || lowered.contains("client_off")
        || lowered.contains("token 缺失")
    {
        return VrccErrorKindOutput::NeedsReauth;
    }
    if lowered.contains("限流") || lowered.contains("额度") || lowered.contains("too_many") {
        return VrccErrorKindOutput::Throttled;
    }
    if lowered.contains("暂时不可用") || lowered.contains("timeout") || lowered.contains("无法解析")
    {
        return VrccErrorKindOutput::Transport;
    }
    VrccErrorKindOutput::Other
}

// -- 连接状态 ---------------------------------------------------------------

#[tauri::command(async)]
#[specta::specta]
pub fn app__vrcc_connection(state: State<'_, AppState>) -> VrccConnectionSnapshot {
    state.runtime_host().vrcc().connection()
}

/// 本地用量。口径是「本客户端今日已用 N/300」，不是「剩余」——
/// 服务器按令牌算额度，我们看不见那个数字。
#[tauri::command(async)]
#[specta::specta]
pub fn app__vrcc_usage(state: State<'_, AppState>) -> VrccUsageSnapshot {
    state.runtime_host().vrcc().usage()
}

// -- 开放数据 ---------------------------------------------------------------

/// 地图榜。不需要连接。
#[tauri::command]
#[specta::specta]
pub async fn app__vrcc_boards(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<VrccBoard, AppError> {
    state
        .runtime_host()
        .vrcc()
        .boards(limit)
        .await
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
}

// -- 设备授权 ---------------------------------------------------------------

/// 第一步：申请连接码。`scope` 传 `null` 表示用官方默认。
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
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
        .map_err(AppError::Custom)
}
