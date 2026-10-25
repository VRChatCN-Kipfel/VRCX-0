//! 令牌与连接状态的持有者。
//!
//! 令牌只存在本机：由调用方（应用层）从加密存储里取出并注入，这一层不碰磁盘。
//! 这样 `integrations` 保持无持久化依赖，加密策略留在持久化层。

use std::sync::{Arc, RwLock};

use vrcx_0_contracts::vrcc::VrccConnectionSnapshot;

use super::client::VrccClient;

/// 一次连接的全部状态。
#[derive(Clone, Debug, Default)]
pub struct VrccSessionState {
    pub token: Option<String>,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
    /// 上次成功轮询提醒的毫秒时间戳。
    pub last_poll_at: Option<i64>,
    /// 提醒轮询游标。
    pub notifications_cursor: Option<i64>,
    /// 官方建议的轮询间隔（秒）。
    pub poll_every_secs: i64,
}

impl VrccSessionState {
    pub fn is_connected(&self) -> bool {
        self.token.as_deref().is_some_and(|token| !token.is_empty())
    }
}

/// 可克隆的会话句柄。
#[derive(Clone)]
pub struct VrccSession {
    state: Arc<RwLock<VrccSessionState>>,
    client_id: String,
}

impl VrccSession {
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(default_state())),
            client_id: client_id.into(),
        }
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn snapshot(&self) -> VrccSessionSnapshot {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        VrccSessionSnapshot {
            connected: state.is_connected(),
            star: state.star,
            vrc_name: state.vrc_name.clone(),
            scope: state.scope.clone(),
            client: self.client_id.clone(),
            last_poll_at: state.last_poll_at,
            notifications_cursor: state.notifications_cursor,
            poll_every_secs: state.poll_every_secs,
        }
    }

    /// 当前令牌，供客户端回调取用。
    pub fn token(&self) -> Option<String> {
        self.state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .token
            .clone()
    }

    /// 连接成功：写入令牌与玩家身份。
    pub fn connect(
        &self,
        token: String,
        star: Option<i64>,
        vrc_name: Option<String>,
        scope: Option<String>,
    ) {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        state.token = Some(token);
        state.star = star;
        state.vrc_name = vrc_name;
        state.scope = scope;
        // 换了授权，游标作废，下次从头问。
        state.notifications_cursor = None;
        state.last_poll_at = None;
    }

    /// 断开：只清本地状态。服务器侧吊销由 `client.disconnect()` 负责。
    pub fn disconnect(&self) {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        *state = default_state();
    }

    /// 记录一次成功的提醒轮询。
    pub fn note_poll(&self, at_ms: i64, cursor: Option<i64>, every_secs: Option<i64>) {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        state.last_poll_at = Some(at_ms);
        if let Some(cursor) = cursor {
            state.notifications_cursor = Some(cursor);
        }
        if let Some(every) = every_secs.filter(|value| *value > 0) {
            // 官方建议值，但不低于 25 秒硬下限。
            state.poll_every_secs = every.max(25);
        }
    }

    /// 按当前会话造一个客户端。
    pub fn client(&self) -> VrccClient {
        let session = self.clone();
        VrccClient::new(self.client_id.clone(), Arc::new(move || session.token()))
    }
}

fn default_state() -> VrccSessionState {
    VrccSessionState {
        poll_every_secs: 60,
        ..Default::default()
    }
}

/// 给前端看的会话快照。
#[derive(Clone, Debug)]
pub struct VrccSessionSnapshot {
    pub connected: bool,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
    pub client: String,
    pub last_poll_at: Option<i64>,
    pub notifications_cursor: Option<i64>,
    pub poll_every_secs: i64,
}

impl From<VrccSessionSnapshot> for VrccConnectionSnapshot {
    fn from(snapshot: VrccSessionSnapshot) -> Self {
        VrccConnectionSnapshot {
            connected: snapshot.connected,
            star: snapshot.star,
            vrc_name: snapshot.vrc_name,
            scope: snapshot.scope,
            client: snapshot.client,
            last_poll_at: snapshot.last_poll_at,
            next_poll_at: snapshot
                .last_poll_at
                .map(|last| last + snapshot.poll_every_secs * 1000),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_and_disconnect_round_trip() {
        let session = VrccSession::new("dev-test");
        assert!(!session.snapshot().connected);

        session.connect(
            "tok".into(),
            Some(77),
            Some("拾光".into()),
            Some("profile".into()),
        );
        let snapshot = session.snapshot();
        assert!(snapshot.connected);
        assert_eq!(snapshot.star, Some(77));
        assert_eq!(snapshot.vrc_name.as_deref(), Some("拾光"));

        session.disconnect();
        assert!(!session.snapshot().connected);
        assert!(session.token().is_none());
    }

    #[test]
    fn connect_resets_cursor_and_poll_time() {
        let session = VrccSession::new("dev-test");
        session.connect("tok".into(), None, None, None);
        session.note_poll(1_000, Some(42), Some(60));
        assert_eq!(session.snapshot().notifications_cursor, Some(42));

        // 重新授权后游标作废，避免用旧游标问到空结果。
        session.connect("tok2".into(), None, None, None);
        let snapshot = session.snapshot();
        assert_eq!(snapshot.notifications_cursor, None);
        assert_eq!(snapshot.last_poll_at, None);
    }

    #[test]
    fn poll_interval_never_below_hard_floor() {
        let session = VrccSession::new("dev-test");
        session.connect("tok".into(), None, None, None);
        // 官方建议 60，但服务器可能给个很小的值；25 秒是硬下限。
        session.note_poll(1_000, None, Some(5));
        assert_eq!(session.snapshot().poll_every_secs, 25);
        session.note_poll(2_000, None, Some(90));
        assert_eq!(session.snapshot().poll_every_secs, 90);
    }

    #[test]
    fn next_poll_at_derives_from_last_poll() {
        let session = VrccSession::new("dev-test");
        session.connect("tok".into(), None, None, None);
        session.note_poll(1_000_000, None, Some(60));
        let projection: VrccConnectionSnapshot = session.snapshot().into();
        assert_eq!(projection.next_poll_at, Some(1_060_000));
    }
}
