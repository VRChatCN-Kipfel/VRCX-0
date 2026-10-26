//! 星遇同频日配额计数的本地存储。
//!
//! 为什么需要落盘：官方的日配额是**按天**算的（个人接口 300 次 / `device/code`
//! 30 个），而计数只存在进程内存里的话，**重启即清零**——本地预扣形同虚设。
//! 玩家会看到「今日已用 0/300」，服务器那边却已经扣过大半，直到撞上
//! `429 too_many` 才暴露，而那时只能等第二天。
//!
//! 这里存的**不是凭据**（配额数字不含敏感信息），所以走普通 `configs` 明文，
//! 不需要 `secrets_at_rest`。令牌的加密在 `vrcc_token.rs` 里单独处理。

use std::sync::Arc;

use vrcx_0_integrations::vrcc::{DailyUsage, DailyUsageStore};
use vrcx_0_persistence::{config, DatabaseService};

/// 日配额计数的配置键。
///
/// 存成 JSON：`{"date":"2026-10-26","personal":7,"deviceCode":1}`。
/// 用一个键而不是三个，保证读出来的是一次原子的快照，不会出现
/// 「日期是今天的、计数是昨天的」这种半更新状态。
const VRCC_USAGE_KEY: &str = "vrccDailyUsage";

/// 把日配额计数写进配置表。
#[derive(Clone)]
pub struct LocalVrccUsageStore {
    db: Arc<DatabaseService>,
}

impl LocalVrccUsageStore {
    pub fn new(db: Arc<DatabaseService>) -> Self {
        Self { db }
    }
}

impl DailyUsageStore for LocalVrccUsageStore {
    fn load(&self) -> Option<DailyUsage> {
        let raw = config::get_string(&self.db, VRCC_USAGE_KEY, "").ok()?;
        if raw.trim().is_empty() {
            return None;
        }
        match serde_json::from_str::<DailyUsage>(&raw) {
            Ok(usage) => Some(usage),
            Err(error) => {
                // 存档坏了就当作没有：从 0 开始记。清掉坏数据，
                // 否则每次启动都会重复解析失败。
                tracing::warn!(%error, "vrcc: daily usage snapshot is corrupt, resetting");
                let _ = config::set_string(&self.db, VRCC_USAGE_KEY, "");
                None
            }
        }
    }

    fn save(&self, usage: &DailyUsage) {
        let Ok(text) = serde_json::to_string(usage) else {
            tracing::warn!("vrcc: failed to serialize daily usage");
            return;
        };
        if let Err(error) = config::set_string(&self.db, VRCC_USAGE_KEY, &text) {
            // 写失败不该让请求失败：配额记不准比功能不可用轻得多。
            tracing::warn!(%error, "vrcc: failed to persist daily usage");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_key_is_stable() {
        // 键名变了等于旧计数失效，重启后又从头记——这条防止手滑改名。
        assert_eq!(VRCC_USAGE_KEY, "vrccDailyUsage");
    }

    #[test]
    fn daily_usage_serializes_with_the_expected_shape() {
        let usage = DailyUsage::restore("2026-10-26", 7, 1);
        let text = serde_json::to_string(&usage).expect("serialize");
        // 字段名是 camelCase，和前端契约保持一致的风格。
        assert!(text.contains("\"date\":\"2026-10-26\""));
        assert!(text.contains("\"personal\":7"));
        assert!(text.contains("\"deviceCode\":1"));
    }

    #[test]
    fn daily_usage_round_trips() {
        let usage = DailyUsage::restore("2026-10-26", 42, 3);
        let text = serde_json::to_string(&usage).expect("serialize");
        let back: DailyUsage = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.date(), "2026-10-26");
        assert_eq!(back.personal_used(), 42);
        assert_eq!(back.device_code_used(), 3);
    }
}
