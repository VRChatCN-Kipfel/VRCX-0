//! 应用层测试：用内存仓库替换持久化，验证编排逻辑。

use std::sync::{Arc, Mutex};

use super::*;

/// 内存令牌仓库。
#[derive(Default)]
struct MemoryStore {
    inner: Mutex<Option<StoredCredential>>,
}

impl MemoryStore {
    fn with(credential: StoredCredential) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Some(credential)),
        })
    }
}

impl VrccTokenStore for MemoryStore {
    fn load(&self) -> Result<Option<StoredCredential>> {
        Ok(self.inner.lock().unwrap().clone())
    }

    fn save(&self, credential: &StoredCredential) -> Result<()> {
        *self.inner.lock().unwrap() = Some(credential.clone());
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
}

#[test]
fn new_restores_connected_session_from_store() {
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        star: Some(77),
        vrc_name: Some("拾光".into()),
        scope: Some("profile maps".into()),
    });
    let service = VrccService::new(store, "dev-test");
    let snapshot = service.snapshot();
    assert!(snapshot.connected);
    assert_eq!(snapshot.star, Some(77));
    assert_eq!(snapshot.vrc_name.as_deref(), Some("拾光"));
    assert_eq!(snapshot.client, "dev-test");
}

#[test]
fn new_without_stored_token_is_disconnected() {
    let store = Arc::new(MemoryStore::default());
    let service = VrccService::new(store, "dev-test");
    assert!(!service.snapshot().connected);
}

#[test]
fn usage_reports_used_not_remaining() {
    // 口径要求：只说「本客户端今日已用 N/300」，不说「还剩」。
    let service = VrccService::new(Arc::new(MemoryStore::default()), "dev-test");
    let usage = service.usage();
    assert_eq!(usage.personal_daily_limit, 300);
    assert_eq!(usage.device_code_daily_limit, 30);
    assert_eq!(usage.window_limit, 120);
    assert!(!usage.date.is_empty());
    // 结构里没有 "remaining" 这类字段，是刻意的。
}

#[test]
fn classify_splits_reauth_from_throttle() {
    let throttled = VrccError::RateLimited("too_many".into());
    assert_eq!(classify_vrcc_error(&throttled).0, VrccErrorKind::Throttled);

    let quota = VrccError::QuotaExhausted("用尽".into());
    assert_eq!(classify_vrcc_error(&quota).0, VrccErrorKind::Throttled);

    let gone = VrccError::Api {
        status: 404,
        error: "gone".into(),
        need: None,
        why: None,
    };
    assert_eq!(classify_vrcc_error(&gone).0, VrccErrorKind::NeedsReauth);

    let missing = VrccError::Api {
        status: 403,
        error: "scope".into(),
        need: Some("today".into()),
        why: None,
    };
    let (kind, need) = classify_vrcc_error(&missing);
    assert_eq!(kind, VrccErrorKind::MissingScope);
    assert_eq!(need.as_deref(), Some("today"));

    let transport = VrccError::Transport("timeout".into());
    assert_eq!(classify_vrcc_error(&transport).0, VrccErrorKind::Transport);
}

#[test]
fn map_not_went_is_not_a_reauth_case() {
    // 打分前置条件不满足，令牌没问题。
    let error = VrccError::Api {
        status: 403,
        error: "map_not_went".into(),
        need: Some("verify".into()),
        why: None,
    };
    let (kind, _) = classify_vrcc_error(&error);
    assert_ne!(kind, VrccErrorKind::NeedsReauth);
}

#[tokio::test]
async fn connected_guard_blocks_personal_calls_when_disconnected() {
    let store = Arc::new(MemoryStore::default());
    let service = VrccService::new(store, "dev-test");
    // 未连接时不应发请求，直接报没令牌。
    let error = service.recent_notifications().await.unwrap_err();
    assert!(matches!(error, VrccError::TokenMissing));
}

#[tokio::test]
async fn poll_notifications_requires_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), "dev-test");
    let error = service.poll_notifications().await.unwrap_err();
    assert!(matches!(error, VrccError::TokenMissing));
}

#[tokio::test]
async fn today_requires_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), "dev-test");
    let error = service.today().await.unwrap_err();
    assert!(matches!(error, VrccError::TokenMissing));
}
