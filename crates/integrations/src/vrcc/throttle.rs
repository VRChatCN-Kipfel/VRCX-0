//! 客户端侧的频率与配额控制。
//!
//! 官方《频率限制》表格里对我们有效的几条（开放接口 1.14.0）：
//!
//! | 范围 | 限制 |
//! | --- | --- |
//! | 每个网络地址（总计） | 每分钟约 120 次 |
//! | 轮询提醒 `/me/notifications` | 两次之间至少 25 秒，建议按返回的 `every`（60 秒）；**没有每天上限** |
//! | 第一份读：最近提醒 / 星轨 / 互相连上 / 所在图 / 我打过的图 | 每个授权每天 1000 次（1.8.4 起，原来 300） |
//! | 第二份读：今天的星 / 通知中心 / … | 每个授权每天 1000 次，和第一份分开算 |
//! | 未读数 `/me/unread` | 每个授权每天 1500 次，单独算 |
//! | 设备授权拿码 `/device/code` | 每网络地址每天 30 个 |
//!
//! 1.11.0 起，带每天次数的接口回答 `RateLimit-Limit` / `RateLimit-Remaining`
//! / `RateLimit-Reset`；1.9.0 起所有 `429` 都带 `Retry-After`。服务器给了就以
//! 服务器为准（见 `client.rs`），这里的本地计数只在旧服务器没给头时兜底。

use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// 每分钟请求总数上限（官方「120 次左右」，取 120 整数）。
pub const WINDOW_LIMIT: usize = 120;
/// 限流窗口长度。
pub const WINDOW: Duration = Duration::from_secs(60);
/// 一个限流键多久没出现就可以忘掉。
///
/// 取 [`WINDOW`] 的十倍：最长的 `min_gap` 是 60 秒，超过这个时间没再出现的键
/// 已经不可能约束任何一次请求，留着只会让 `last_at` 随逛过的世界一直长。
const KEY_TTL: Duration = Duration::from_secs(600);
/// 「第一份读」每个授权每天的上限（1.8.4 起从每颗星 300 次改成每个授权 1000 次）。
///
/// 只是服务器没给 `RateLimit-Limit` 时的兜底；给了就用服务器的。
pub const PERSONAL_DAILY_LIMIT: i64 = 1000;
/// 设备授权拿码每日上限。
pub const DEVICE_CODE_DAILY_LIMIT: i64 = 30;
/// 轮询提醒的最小间隔（官方硬性要求）。
pub const NOTIFICATIONS_MIN_GAP: Duration = Duration::from_secs(25);

/// 429 之后的最大退避。
///
/// 也是「短等」和「等到明天」的分界：`Retry-After` 比它长的，是某一项每天的
/// 次数用完了，只停那一项，不让全部接口一起退避（见 `client.rs`）。
pub const MAX_BACKOFF: Duration = Duration::from_secs(600);
/// 旧服务器的 429 不带 `Retry-After` 时的最小退避（当时官方写「至少等一分钟」）。
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

    /// 窗口里已经不可能再算进任何未来窗口的记录，直接丢掉。
    ///
    /// 比 `now` 早一分钟以上的时间点，对**将来**的任何 `at`（`at >= now`）
    /// 都有 `stamp <= now - 60s <= at - 60s`，永远落在窗口外。
    fn drop_expired(&mut self, now: Instant) {
        while let Some(front) = self.stamps.front() {
            if now.saturating_duration_since(*front) >= WINDOW {
                self.stamps.pop_front();
            } else {
                break;
            }
        }
    }

    /// 丢掉很久没再出现过的键。
    ///
    /// `last_at` 是 `HashMap`，键里带资源（世界 ID、排序）时会随玩家逛过的世界
    /// 一直长。超过 [`KEY_TTL`] 没出现过的键不可能再约束任何一次请求：
    /// 最长的 `min_gap` 是 60 秒，远小于这个值。留着它们只是让内存慢慢涨。
    fn drop_stale_keys(&mut self, now: Instant) {
        self.last_at
            .retain(|_, at| now.saturating_duration_since(*at) < KEY_TTL);
    }

    /// 最早能在哪个时刻发一个 `key` 请求。只读，不改状态。
    ///
    /// 同时满足两个约束：
    /// - 同一个接口两次之间至少隔 `min_gap`；
    /// - 任意 60 秒窗口内不超过 [`WINDOW_LIMIT`] 次。
    fn earliest(&self, key: &str, min_gap: Duration, now: Instant) -> Instant {
        let gap_ready = self
            .last_at
            .get(key)
            .map(|last| *last + min_gap)
            .unwrap_or(now)
            .max(now);
        // 窗口约束：以 `at` 结尾的 60 秒里已有记录必须少于上限（要给它留位置）。
        // 不够就往后推，推到最老的几条滑出窗口为止。
        let mut at = gap_ready;
        loop {
            let window_start = at.checked_sub(WINDOW).unwrap_or(at);
            let mut in_window: Vec<Instant> = self
                .stamps
                .iter()
                .filter(|stamp| **stamp > window_start && **stamp <= at)
                .copied()
                .collect();
            if in_window.len() < WINDOW_LIMIT {
                return at;
            }
            in_window.sort_unstable();
            // 要多腾出一个位置，就得让最老的 `overflow` 条滑出窗口。
            let overflow = in_window.len() + 1 - WINDOW_LIMIT;
            at = in_window[overflow - 1] + WINDOW;
        }
    }

    /// 判断现在能否发一个 `key` 接口的请求。不修改状态。
    ///
    /// 返回 `Ok(())` 可以发；`Err(wait)` 需要等这么久。
    pub fn check(&self, key: &str, min_gap: Duration, now: Instant) -> Result<(), Duration> {
        let wait = self
            .earliest(key, min_gap, now)
            .saturating_duration_since(now);
        if wait.is_zero() {
            Ok(())
        } else {
            Err(wait)
        }
    }

    /// **原子地**预占一个发送时间槽。
    ///
    /// 返回 `Ok(at)`：槽已占用，等到 `at` 就能发。
    /// 返回 `Err(wait)`：本来要等 `wait`，因为超过 `budget` 而**没有**占槽。
    ///
    /// 存在的理由：原来调用方先 `check` 拿到 `wait`、放锁、睡完再加锁
    /// `record`。并发调用会拿到**同一个** `wait`，然后一起醒来一起发出去，
    /// `min_gap` 与窗口上限都不是原子的。预占把这两步收进同一把锁，
    /// 每个调用者拿到互不相同的时间点。
    pub fn reserve(
        &mut self,
        key: &str,
        min_gap: Duration,
        now: Instant,
        budget: Duration,
    ) -> Result<Instant, Duration> {
        self.drop_expired(now);
        self.drop_stale_keys(now);
        let at = self.earliest(key, min_gap, now);
        let wait = at.saturating_duration_since(now);
        if wait > budget {
            // 不占槽：调用方可以选择用缓存，而不是干等。
            return Err(wait);
        }
        // 按时间有序插入：`at` 未必大于最后一条（别的接口的 gap 更短时
        // 可能插到前面），而窗口统计依赖有序。
        let stamps = self.stamps.make_contiguous();
        let position = stamps.partition_point(|stamp| *stamp <= at);
        self.stamps.insert(position, at);
        self.last_at.insert(key.to_string(), at);
        Ok(at)
    }

    /// 记录一次放行。调用点应先用 `check` 确认可以发。
    pub fn record(&mut self, key: &str, now: Instant) {
        self.drop_expired(now);
        let stamps = self.stamps.make_contiguous();
        let position = stamps.partition_point(|stamp| *stamp <= now);
        self.stamps.insert(position, now);
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

/// 当前时间的毫秒时间戳。会话记录轮询时间用。
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
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

    /// **并发预占拿到的是互不相同的时间点。**
    ///
    /// 这是这次改造的核心：原来 `check` + `record` 分两步，两个并发调用会
    /// 拿到同一个 `wait`，一起睡醒、一起发出去——`min_gap` 形同虚设。
    #[test]
    fn concurrent_reservations_get_distinct_slots() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        let gap = Duration::from_secs(5);
        let budget = Duration::from_secs(600);

        let first = limiter
            .reserve("boards", gap, base, budget)
            .expect("first slot");
        // 同一时刻再要一次：不能也拿到 `first`。
        let second = limiter
            .reserve("boards", gap, base, budget)
            .expect("second slot");
        let third = limiter
            .reserve("boards", gap, base, budget)
            .expect("third slot");

        assert_eq!(first, base, "第一个请求立刻就能发");
        assert_eq!(second, base + gap, "第二个要等一个 gap");
        assert_eq!(third, base + gap * 2, "第三个再等一个 gap");
        // 三者互不相同，这才是原子性要保证的东西。
        assert_ne!(first, second);
        assert_ne!(second, third);
    }

    /// 预占会占住槽位：同一时刻再要，就得等满一个 gap。
    #[test]
    fn a_reserved_slot_is_not_handed_out_twice() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        let gap = Duration::from_secs(30);
        let _ = limiter
            .reserve("x", gap, base, Duration::from_secs(600))
            .expect("slot");
        // 同一时刻再要，且只肯等 1 秒：必须被拒，且拒的理由是「要等满 30 秒」。
        let again = limiter.reserve("x", gap, base, Duration::from_secs(1));
        assert_eq!(again, Err(gap));
    }

    /// 超过预算就不占槽，让调用方改用缓存。
    #[test]
    fn budget_exceeded_reserves_nothing() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        let gap = Duration::from_secs(60);
        // 先占一个，使下一个至少要等 60 秒。
        let _ = limiter
            .reserve("boards", gap, base, Duration::from_secs(600))
            .expect("first");

        // 只肯等 2 秒：直接拒绝，且**不留痕**。
        let denied = limiter.reserve("boards", gap, base, Duration::from_secs(2));
        assert_eq!(denied, Err(gap));
        assert_eq!(limiter.window_used(base), 1, "被拒绝的请求不该占用窗口名额");

        // 别的接口不受影响，照样能立刻发（窗口没满）。
        assert!(limiter
            .reserve(
                "status",
                Duration::from_secs(30),
                base,
                Duration::from_secs(1)
            )
            .is_ok());
    }

    /// 窗口上限也要在预占里成立：第 121 个请求必须被推后。
    #[test]
    fn reservations_respect_the_window_limit() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        for _ in 0..WINDOW_LIMIT {
            assert!(limiter
                .reserve("x", Duration::ZERO, base, Duration::from_secs(600))
                .is_ok());
        }
        // 窗口满了：这一个得等到最老的滑出去。
        let next = limiter
            .reserve("x", Duration::ZERO, base, Duration::from_secs(600))
            .expect("room after the window slides");
        assert!(
            next >= base + WINDOW,
            "第 {WINDOW_LIMIT} 之后的请求必须等到窗口滑动，实际 {next:?}"
        );
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
    fn personal_limit_matches_the_1_8_4_quota() {
        // 1.8.4 起「第一份读」是每个授权每天 1000 次。这个常量现在只用于
        // 「服务端还没说过上限」时给界面一个参考值，不再参与本地拦截。
        assert_eq!(PERSONAL_DAILY_LIMIT, 1000);
    }

    #[test]
    fn device_code_limit_matches_the_documented_network_quota() {
        // 官方：每个网络地址每天 30 个。这是**网络维度**的限制，不是本客户端的，
        // 所以只用来给界面一句提示，绝不用于本地拦截——玩家换了出口 IP，
        // 本地拦着就变成了凭空拒绝。
        assert_eq!(DEVICE_CODE_DAILY_LIMIT, 30);
    }
}
