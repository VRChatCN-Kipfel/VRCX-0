//! 星遇同频（vrcc.cc）原生集成。
//!
//! 只使用官方开放的 `/api/v1/`，不复刻网页端的未公开接口。
//!
//! 分三层：
//! - `throttle`：限流窗口、每接口最小间隔、日额度本地预扣（纯逻辑，可单测）
//! - `client`：唯一的网络出口，硬拦绝对路径，处理错误码与退避
//! - `session`：令牌与连接状态的持有者，跨命令共享

pub mod client;
pub mod session;
pub mod throttle;

pub use client::{VrccClient, VrccClientState, VrccError};
pub use session::{VrccSession, VrccSessionSnapshot};
pub use throttle::{
    backoff_for, format_local_date, local_date_today, now_millis, DailyUsage, RateLimiter,
    DEVICE_CODE_DAILY_LIMIT, NOTIFICATIONS_MIN_GAP, PERSONAL_DAILY_LIMIT, WINDOW_LIMIT,
};
pub use vrcx_0_contracts::vrcc::{
    is_dev_client, resolve_client_id, VRCC_API_BASE, VRCC_APP_NAME, VRCC_DEFAULT_CLIENT,
    VRCC_DEV_CLIENT, VRCC_PROD_CLIENT,
};

/// 配置表里存放 `X-Client` 覆盖值的键。
///
/// 正式态把平台发的 `app_…` 写进这里即可切换，不需要重新编译；
/// 留空则按开发态标识走。
pub const VRCC_CLIENT_CONFIG_KEY: &str = "vrccClientId";

/// 默认客户端标识的别名，方便调用点读起来顺一点。
pub const VRCC_DEFAULT_CLIENT_ID: &str = VRCC_DEFAULT_CLIENT;
