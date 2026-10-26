//! 客户端侧的频率与配额控制。
//!
//! 官方《频率限制》表格里对我们有效的几条：
//!
//! | 范围 | 限制 |
//! | --- | --- |
//! | 每个网络地址（总计） | 每分钟约 120 次 |
//! | 轮询提醒 `/me/notifications` | 两次之间至少 25 秒，建议按返回的 `every`（60 秒） |
//! | 最近提醒 / 星轨 / 互相连上 / 所在图 / 我打过的图 | 合计每颗星每天 300 次 |
//! | 设备授权拿码 `/device/code` | 每网络地址每天 30 个 |
//!
//! 这些都要在本地先扣，宁可少问也不越界。注意「300 次」的计数是**按令牌**
//! 在服务器侧算的，我们在本地记的只是本客户端这一份，所以对外只能说
//! 「本客户端今日已用 N 次」，不能说「还剩 N 次」。

use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// 每分钟请求总数上限（官方「120 次左右」，取 120 整数）。
pub const WINDOW_LIMIT: usize = 120;
/// 限流窗口长度。
pub const WINDOW: Duration = Duration::from_secs(60);
/// 个人接口每日合计上限。
pub const PERSONAL_DAILY_LIMIT: i64 = 300;
/// 设备授权拿码每日上限。
pub const DEVICE_CODE_DAILY_LIMIT: i64 = 30;
/// 轮询提醒的最小间隔（官方硬性要求）。
pub const NOTIFICATIONS_MIN_GAP: Duration = Duration::from_secs(25);

/// 429 之后的最大退避。
pub const MAX_BACKOFF: Duration = Duration::from_secs(600);
/// 429 之后的最小退避（官方要求「至少等一分钟」）。
pub const MIN_BACKOFF: Duration = Duration::from_secs(60);

/// 滑动窗口限流器 + 每接口最小间隔。
///
/// 逻辑照 `xingshi-app` 的 `Throttle`，但改为同步：这里的调用点本来就在
/// 异步任务里，用 `Instant` 记账比 `delay` 更容易测试。
#[derive(Debug, Default)]
pub struct RateLimiter {
    /// 最近一分钟内每次放行的时间点，最旧的在前。
    stamps: VecDeque<Instant>,
    /// 每个接口标识的上次放行时间。
    last_at: HashMap<String, Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 判断现在能否发一个 `key` 接口的请求。不修改状态。
    ///
    /// 返回 `Ok(())` 可以发；`Err(wait)` 需要等这么久。
    pub fn check(&self, key: &str, min_gap: Duration, now: Instant) -> Result<(), Duration> {
        // 窗口里过期的先算掉，但不真的删（check 是只读的）。
        let window_live = self
            .stamps
            .iter()
            .filter(|stamp| now.saturating_duration_since(**stamp) < WINDOW)
            .count();
        let gap_wait = self
            .last_at
            .get(key)
            .map(|last| min_gap.saturating_sub(now.saturating_duration_since(*last)))
            .unwrap_or_default();
        let window_wait = if window_live >= WINDOW_LIMIT {
            self.stamps
                .iter()
                .find(|stamp| now.saturating_duration_since(**stamp) < WINDOW)
                .map(|oldest| WINDOW.saturating_sub(now.saturating_duration_since(*oldest)))
                .unwrap_or_default()
        } else {
            Duration::ZERO
        };
        let wait = gap_wait.max(window_wait);
        if wait.is_zero() {
            Ok(())
        } else {
            Err(wait)
        }
    }

    /// 记录一次放行。调用点应先用 `check` 确认可以发。
    pub fn record(&mut self, key: &str, now: Instant) {
        while let Some(front) = self.stamps.front() {
            if now.saturating_duration_since(*front) >= WINDOW {
                self.stamps.pop_front();
            } else {
                break;
            }
        }
        self.stamps.push_back(now);
        self.last_at.insert(key.to_string(), now);
    }

    /// 当前窗口内已用次数。
    pub fn window_used(&self, now: Instant) -> usize {
        self.stamps
            .iter()
            .filter(|stamp| now.saturating_duration_since(**stamp) < WINDOW)
            .count()
    }
}

/// 429 退避的指数增长：60s、120s、240s、480s，封顶 600s。
pub fn backoff_for(attempt: u32) -> Duration {
    let shifted = MIN_BACKOFF.saturating_mul(1u32.checked_shl(attempt.min(4)).unwrap_or(16));
    shifted.min(MAX_BACKOFF)
}

/// 按本地日期滚动的用量计数。
///
/// 只记「已用」，不记「剩余」——服务器侧的额度是按令牌算的，我们看不见。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    date: String,
    personal: i64,
    device_code: i64,
}

impl Default for DailyUsage {
    fn default() -> Self {
        Self {
            date: String::new(),
            personal: 0,
            device_code: 0,
        }
    }
}

impl DailyUsage {
    /// 从落盘的快照恢复计数。
    ///
    /// 不用在这里判断日期是否已过期：下一次 `roll_to` 会负责清零，
    /// 而 `roll_to` 每次取用量前都会被调用。
    pub fn restore(date: &str, personal: i64, device_code: i64) -> Self {
        Self {
            date: date.to_string(),
            personal: personal.max(0),
            device_code: device_code.max(0),
        }
    }

    /// 这份计数属于哪一天（本地时区 `YYYY-MM-DD`）。
    pub fn date(&self) -> &str {
        &self.date
    }

    /// 换到 `today`（`YYYY-MM-DD`）时清零。返回是否发生了跨天。
    pub fn roll_to(&mut self, today: &str) -> bool {
        if self.date == today {
            return false;
        }
        self.date = today.to_string();
        self.personal = 0;
        self.device_code = 0;
        true
    }

    pub fn personal_used(&self) -> i64 {
        self.personal
    }

    pub fn device_code_used(&self) -> i64 {
        self.device_code
    }

    /// 个人接口还能不能再用 `amount` 次。
    pub fn can_spend_personal(&self, amount: i64) -> bool {
        self.personal + amount <= PERSONAL_DAILY_LIMIT
    }

    /// 扣一次个人接口额度。返回是否扣成功（额度不足时不动账）。
    pub fn spend_personal(&mut self, amount: i64) -> bool {
        if !self.can_spend_personal(amount) {
            return false;
        }
        self.personal += amount;
        true
    }

    pub fn can_spend_device_code(&self) -> bool {
        self.device_code < DEVICE_CODE_DAILY_LIMIT
    }

    pub fn spend_device_code(&mut self) -> bool {
        if !self.can_spend_device_code() {
            return false;
        }
        self.device_code += 1;
        true
    }
}

/// 当前时间的毫秒时间戳。会话记录轮询时间用。
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 本地时区的 `YYYY-MM-DD`。
///
/// 官方按自然日算额度，我们跟着本地时区走，跨天即清零。
/// 这里不引入 `chrono`：`integrations` 目前没有这个依赖，而算一个本地日期
/// 用 `libc` 的 `localtime_r` 语义太长；直接按 UTC+本地偏移手算了。
pub fn local_date_today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_local_date(now, local_offset_secs())
}

/// 本地时区相对 UTC 的秒数（东八区为 `+28800`）。
///
/// Windows 上没有简单可用的 tz 查询，这里读环境变量 `TZ_OFFSET_SECS`，
/// 缺省按 UTC 处理——日期只在跨天时用一次，误差最多影响当天的计数清零时机。
fn local_offset_secs() -> i64 {
    std::env::var("TZ_OFFSET_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(8 * 3600)
}

/// 把 Unix 秒按给定偏移折算成 `YYYY-MM-DD`。
pub fn format_local_date(unix_secs: i64, offset_secs: i64) -> String {
    let shifted = unix_secs + offset_secs;
    let days = shifted.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant 的 `civil_from_days`：把「1970-01-01 起的天数」转成公历年月日。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn window_blocks_after_limit() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        for _ in 0..WINDOW_LIMIT {
            assert!(limiter.check("x", Duration::ZERO, base).is_ok());
            limiter.record("x", base);
        }
        assert!(limiter.check("x", Duration::ZERO, base).is_err());
        // 一分钟后窗口滑走，重新放行。
        assert!(limiter.check("x", Duration::ZERO, at(base, 61)).is_ok());
    }

    #[test]
    fn per_key_gap_is_enforced() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        assert!(limiter
            .check("notifications", NOTIFICATIONS_MIN_GAP, base)
            .is_ok());
        limiter.record("notifications", base);
        // 25 秒内同一个接口不放行。
        assert!(limiter
            .check("notifications", NOTIFICATIONS_MIN_GAP, at(base, 24))
            .is_err());
        // 别的接口不受影响。
        assert!(limiter
            .check("boards", NOTIFICATIONS_MIN_GAP, at(base, 1))
            .is_ok());
        // 满 25 秒放行。
        assert!(limiter
            .check("notifications", NOTIFICATIONS_MIN_GAP, at(base, 25))
            .is_ok());
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_for(0), Duration::from_secs(60));
        assert_eq!(backoff_for(1), Duration::from_secs(120));
        assert_eq!(backoff_for(2), Duration::from_secs(240));
        assert_eq!(backoff_for(4), MAX_BACKOFF);
        assert_eq!(backoff_for(9), MAX_BACKOFF);
    }

    #[test]
    fn daily_usage_rolls_over_on_new_date() {
        let mut usage = DailyUsage::default();
        assert!(usage.roll_to("2026-10-25"));
        assert!(usage.spend_personal(299));
        assert!(!usage.spend_personal(2)); // 300 是上限，超了不给
        assert_eq!(usage.personal_used(), 299);
        // 跨天清零。
        assert!(usage.roll_to("2026-10-26"));
        assert_eq!(usage.personal_used(), 0);
        assert!(usage.spend_personal(300));
    }

    #[test]
    fn device_code_has_its_own_counter() {
        let mut usage = DailyUsage::default();
        usage.roll_to("2026-10-25");
        for _ in 0..DEVICE_CODE_DAILY_LIMIT {
            assert!(usage.spend_device_code());
        }
        assert!(!usage.spend_device_code());
        // 和个人接口额度互不影响。
        assert!(usage.can_spend_personal(1));
    }

    #[test]
    fn date_formatting_matches_known_dates() {
        // 1970-01-01 00:00 UTC
        assert_eq!(format_local_date(0, 0), "1970-01-01");
        // 2024-02-29 00:00 UTC（闰日）
        assert_eq!(format_local_date(1_709_164_800, 0), "2024-02-29");
        // 2026-10-26 08:00 UTC = 16:00 UTC+8
        assert_eq!(format_local_date(1_793_001_600, 0), "2026-10-26");
        assert_eq!(format_local_date(1_793_001_600, 8 * 3600), "2026-10-26");
        // 2026-10-26 20:00 UTC = 2026-10-27 04:00 UTC+8，跨天
        assert_eq!(format_local_date(1_793_044_800, 0), "2026-10-26");
        assert_eq!(format_local_date(1_793_044_800, 8 * 3600), "2026-10-27");
    }
}
