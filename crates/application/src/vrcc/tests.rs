//! 应用层测试：用内存仓库替换持久化，验证编排逻辑。

use std::sync::{Arc, Mutex};

use super::*;
use vrcx_0_contracts::vrcc::{VrccBoardMap, VrccUnread};

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

/// 假网络端口：只记录调用、按需返回预设结果。
///
/// 这正是端口层的意义：应用层的编排（未读怎么算、什么时候清令牌、
/// 配额报什么数）可以完全不碰网络就测掉。过去这些测试靠真的
/// `VrccSession`，一联网就拖慢、还会碰真实限流。
#[derive(Default)]
struct FakeRemote {
    /// 当前连接状态，由 `connect`/`disconnect`/`expire` 改。
    connection: Mutex<VrccConnection>,
    /// 下一次个人调用返回的错误。
    next_error: Mutex<Option<VrccError>>,
    /// `recent_notifications` 的返回。
    recent: Mutex<Option<VrccRecentNotifications>>,
    /// `unread` 的返回。
    unread: Mutex<Option<VrccUnread>>,
    /// 记录 `note_poll` 收到的参数。
    notes_polled: Mutex<Vec<(Option<i64>, Option<i64>)>>,
}

impl FakeRemote {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            connection: Mutex::new(VrccConnection {
                client: "dev-test".to_string(),
                poll_every_secs: 60,
                ..VrccConnection::default()
            }),
            ..Self::default()
        })
    }

    /// 让假端口进入「已连接」。
    ///
    /// 接收 `&self` 而不是 `self`：`new()` 已经返回 `Arc`，再取所有权就没法用了。
    fn mark_connected(&self) {
        self.connection.lock().unwrap().connected = true;
    }

    /// 预设下一次 `recent_notifications` 的返回。
    fn set_recent(&self, recent: VrccRecentNotifications) {
        *self.recent.lock().unwrap() = Some(recent);
    }

    /// 预设下一次 `unread` 的返回。
    fn set_unread(&self, unread: VrccUnread) {
        *self.unread.lock().unwrap() = Some(unread);
    }

    /// 让下一次个人调用失败。
    fn set_error(&self, error: VrccError) {
        *self.next_error.lock().unwrap() = Some(error);
    }

    fn take_error(&self) -> Option<VrccError> {
        self.next_error.lock().unwrap().take()
    }
}

impl VrccRemote for FakeRemote {
    fn boards<'a>(&'a self, _limit: u32, _sort: VrccBoardSort) -> VrccFuture<'a, VrccBoard> {
        Box::pin(async move { Err(VrccError::Transport("fake".into())) })
    }
    fn status<'a>(&'a self) -> VrccFuture<'a, VrccStatus> {
        Box::pin(async move { Err(VrccError::Transport("fake".into())) })
    }
    fn fetch_site_asset<'a>(&'a self, _path: &'a str) -> VrccFuture<'a, Vec<u8>> {
        Box::pin(async move { Err(VrccError::Transport("fake".into())) })
    }
    fn announcements<'a>(&'a self) -> VrccFuture<'a, VrccAnnouncements> {
        Box::pin(async move { Err(VrccError::Transport("fake".into())) })
    }
    fn device_code<'a>(&'a self, _scope: Option<&'a str>) -> VrccFuture<'a, VrccDeviceCode> {
        Box::pin(async move { Err(VrccError::Transport("fake".into())) })
    }
    fn device_token<'a>(&'a self, _code: &'a str) -> VrccFuture<'a, VrccTokenResponse> {
        Box::pin(async move { Err(VrccError::Transport("fake".into())) })
    }
    fn revoke<'a>(&'a self) -> VrccFuture<'a, ()> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Ok(())
        })
    }
    fn recent_notifications<'a>(&'a self, _mark: bool) -> VrccFuture<'a, VrccRecentNotifications> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            self.recent
                .lock()
                .unwrap()
                .clone()
                .ok_or(VrccError::TokenMissing)
        })
    }
    fn notifications<'a>(&'a self, _since: Option<i64>) -> VrccFuture<'a, VrccNotifications> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(VrccError::TokenMissing)
        })
    }
    fn my_map<'a>(&'a self, _world: &'a str) -> VrccFuture<'a, VrccMyMap> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(VrccError::TokenMissing)
        })
    }
    fn unread<'a>(&'a self) -> VrccFuture<'a, VrccUnread> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            self.unread
                .lock()
                .unwrap()
                .clone()
                .ok_or(VrccError::TokenMissing)
        })
    }
    fn notes<'a>(&'a self) -> VrccFuture<'a, VrccNotes> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(VrccError::TokenMissing)
        })
    }
    fn today<'a>(&'a self) -> VrccFuture<'a, VrccToday> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(VrccError::TokenMissing)
        })
    }
    fn mutuals<'a>(&'a self) -> VrccFuture<'a, VrccMutuals> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(VrccError::TokenMissing)
        })
    }

    fn connect(
        &self,
        _token: String,
        star: Option<i64>,
        vrc_name: Option<String>,
        scope: Option<String>,
    ) {
        let mut connection = self.connection.lock().unwrap();
        connection.connected = true;
        connection.expired = false;
        connection.star = star;
        connection.vrc_name = vrc_name;
        connection.scope = scope;
        connection.notifications_cursor = None;
    }
    fn disconnect(&self) {
        let mut connection = self.connection.lock().unwrap();
        *connection = VrccConnection {
            client: connection.client.clone(),
            poll_every_secs: 60,
            ..VrccConnection::default()
        };
    }
    fn expire(&self) {
        let mut connection = self.connection.lock().unwrap();
        connection.connected = false;
        connection.expired = true;
        connection.notifications_cursor = None;
    }
    fn connection(&self) -> VrccConnection {
        self.connection.lock().unwrap().clone()
    }
    fn client_id(&self) -> String {
        self.connection.lock().unwrap().client.clone()
    }
    fn local_usage(&self) -> VrccLocalUsage {
        VrccLocalUsage {
            reads: Some((12, 1000)),
            reset_secs: Some(3600),
            ..VrccLocalUsage::default()
        }
    }
    fn note_poll(&self, _at_ms: i64, cursor: Option<i64>, every_secs: Option<i64>) {
        self.notes_polled.lock().unwrap().push((cursor, every_secs));
    }
    fn window_limit(&self) -> i64 {
        120
    }
    fn now_millis(&self) -> i64 {
        1_700_000_000_000
    }
}

#[test]
fn new_restores_connected_session_from_store() {
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        star: Some(77),
        vrc_name: Some("拾光".into()),
        scope: Some("profile maps".into()),
        client_id: None,
    });
    let service = VrccService::new(store, FakeRemote::new());
    let snapshot = service.snapshot();
    assert!(snapshot.connected);
    assert_eq!(snapshot.star, Some(77));
    assert_eq!(snapshot.vrc_name.as_deref(), Some("拾光"));
    assert_eq!(snapshot.client, "dev-test");
}

#[test]
fn new_without_stored_token_is_disconnected() {
    let store = Arc::new(MemoryStore::default());
    let service = VrccService::new(store, FakeRemote::new());
    assert!(!service.snapshot().connected);
}

#[test]
fn usage_reports_what_the_server_said() {
    // 口径要求：只说「今日已用 N/上限」，不说「还剩」。
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    let usage = service.usage();
    // 服务端 `RateLimit-*` 说的那一份。
    assert_eq!(usage.personal_used_today, Some(12));
    assert_eq!(usage.personal_daily_limit, Some(1000));
    assert_eq!(usage.window_limit, 120);
    // 重置时间来自服务端（到北京时间零点的秒数），不是本地推算的日期。
    assert_eq!(usage.reset_secs, Some(3600));
    // 未读数那一份还没拿到过响应头，不显示。
    assert_eq!(usage.unread_used_today, None);
    assert_eq!(usage.unread_daily_limit, None);
}

#[test]
fn a_gone_token_is_dropped_and_remembered_as_expired() {
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        star: Some(77),
        vrc_name: None,
        scope: Some("profile notifications maps".into()),
        client_id: None,
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(store.clone(), remote);
    service.note_unread(&VrccUnread {
        inbox: 2,
        ..Default::default()
    });

    let error = service.settle(VrccError::Api {
        status: 404,
        error: "gone".into(),
        need: None,
        why: None,
    });
    assert!(error.is_gone());

    let snapshot = service.snapshot();
    assert!(!snapshot.connected);
    assert!(snapshot.expired, "界面要能说出是失效，不是玩家自己断开的");
    // 本地存的令牌也清掉了，重启后不会拿着死令牌再问。
    assert!(store.load().unwrap().is_none());
    // 未读数不再属于任何人；令牌也没了，腕表整块消失（底栏回落到玩家数）。
    assert!(service.cached_unread().is_none());
    assert!(
        service.wrist_summary(None).is_none(),
        "令牌失效后腕表不该再显示"
    );
}

#[test]
fn other_404s_leave_the_token_alone() {
    // `star_gone`（对方不在了）、`expired`（签名图片失效）、`not_found`（旧服务器
    // 没有这个接口）都不是令牌的事。
    for code in ["star_gone", "expired", "not_found"] {
        let store = MemoryStore::with(StoredCredential {
            token: "tok".into(),
            ..Default::default()
        });
        let remote = FakeRemote::new();
        remote.mark_connected();
        let service = VrccService::new(store.clone(), remote);
        service.settle(VrccError::Api {
            status: 404,
            error: code.into(),
            need: None,
            why: None,
        });
        assert!(service.snapshot().connected, "{code}");
        assert!(!service.snapshot().expired, "{code}");
        assert!(store.load().unwrap().is_some(), "{code}");
    }
}

#[test]
fn wrist_prefers_the_unread_counts_over_recent() {
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(
        Arc::new(MemoryStore::default()),
        Arc::clone(&remote) as Arc<dyn VrccRemote>,
    );
    // 旧服务器：只有「最近的提醒」的未读条数。
    service.note_recent(2);
    assert_eq!(service.wrist_summary(None).unwrap().unread, 2);

    // 有了 `me/unread`：腕表显示几处红点的合计。
    let counts = VrccUnread {
        inbox: 1,
        cross: Some(1),
        chats: 2,
        notes: 3,
    };
    assert!(service.note_unread(&counts), "第一次拿到算变化");
    assert_eq!(service.wrist_summary(None).unwrap().unread, 7);
    // 之后「最近的提醒」不再覆盖它。
    service.note_recent(0);
    assert_eq!(service.wrist_summary(None).unwrap().unread, 7);
    // 一样的数不算变化，后台就不必通知前端。
    assert!(!service.note_unread(&counts));
    assert_eq!(service.cached_unread(), Some(counts));
}

#[test]
fn wrist_shows_nothing_until_the_player_connects() {
    // 没连接就不该在腕表上占位：底栏原本显示的是玩家数，
    // 不该被一个从没连过的功能顶掉。
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    assert!(service.wrist_summary(None).is_none());

    // 就算拉到了开放榜单（不需要连接），也不该突然冒出摘要。
    service.note_boards(&VrccBoard {
        maps: vec![VrccBoardMap {
            world: "wrld_x".into(),
            rank: Some(3),
            avg: Some(8.8),
            ..Default::default()
        }],
        ..Default::default()
    });
    assert!(
        service.wrist_summary(Some("wrld_x")).is_none(),
        "没连接时榜单不该让腕表出现"
    );
}

#[test]
fn wrist_shows_the_map_rank_once_connected() {
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(
        Arc::new(MemoryStore::default()),
        Arc::clone(&remote) as Arc<dyn VrccRemote>,
    );
    // 连接后先有一条提醒，腕表才有数据可显示。
    service.note_recent(0);
    service.note_boards(&VrccBoard {
        maps: vec![VrccBoardMap {
            world: "wrld_x".into(),
            rank: Some(3),
            avg: Some(8.8),
            ..Default::default()
        }],
        ..Default::default()
    });
    let summary = service.wrist_summary(Some("wrld_x")).expect("connected");
    assert_eq!(summary.map_rank, Some(3));
    assert_eq!(summary.map_avg_x10, Some(88));
}

#[test]
fn stop_is_classified_as_a_passing_problem() {
    let paused = VrccError::Paused {
        say: Some("维护".into()),
        until: None,
    };
    assert_eq!(classify_vrcc_error(&paused).0, VrccErrorKind::Transport);
}

#[tokio::test]
async fn unread_and_notes_require_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    assert!(matches!(
        service.refresh_unread().await.unwrap_err(),
        VrccError::TokenMissing
    ));
    assert!(matches!(
        service.notes().await.unwrap_err(),
        VrccError::TokenMissing
    ));
}

#[test]
fn usage_snapshot_reflects_the_remote_counters() {
    // 快照必须是真实读上来的，不是写死的 0。
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    let usage = service.usage();
    assert_eq!(usage.personal_used_today, Some(12));
    assert_eq!(usage.window_used, 0);
    assert_eq!(usage.backoff_remaining_ms, 0);
}

#[test]
fn wrist_summary_is_none_before_any_data_arrives() {
    // 「还没拿到数据」和「未读 0 条」是两件事，前者不该在腕表上显示数字。
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    assert!(service.wrist_summary(None).is_none());
    assert!(service.wrist_summary(Some("wrld_abc")).is_none());
}

#[test]
fn wrist_summary_reports_map_rank_from_board_cache() {
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(
        Arc::new(MemoryStore::default()),
        Arc::clone(&remote) as Arc<dyn VrccRemote>,
    );
    // 腕表要先有数据（比如拉过一次提醒）才会显示；榜单负责补名次。
    service.note_recent(0);
    // 直接喂一份榜单，模拟页面拉过地图榜。
    let board = VrccBoard {
        board: Some("maps".into()),
        sort: None,
        min: Some(5),
        maps: vec![
            VrccBoardMap {
                world: "wrld_target".into(),
                rank: Some(3),
                avg: Some(8.75),
                ..Default::default()
            },
            VrccBoardMap {
                world: "wrld_other".into(),
                rank: Some(9),
                avg: Some(6.0),
                ..Default::default()
            },
        ],
        at: Some(1),
    };
    service.note_boards(&board);

    let summary = service
        .wrist_summary(Some("wrld_target"))
        .expect("榜单已加载，应当有摘要");
    assert_eq!(summary.map_rank, Some(3));
    // 均分放大 10 倍存整数，8.75 → 88（四舍五入到一位小数）。
    assert_eq!(summary.map_avg_x10, Some(88));
    // 没拉过 recent，未读数还没数据，保持 0（未连接时收不到提醒）。
    assert_eq!(summary.unread, 0);

    // 不在榜上的图不报名次，而不是报一个假名次。
    let off_board = service
        .wrist_summary(Some("wrld_missing"))
        .expect("仍应有摘要");
    assert_eq!(off_board.map_rank, None);
    assert_eq!(off_board.map_avg_x10, None);
}

#[test]
fn wrist_summary_unread_comes_from_recent_read_cursor() {
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(
        Arc::new(MemoryStore::default()),
        Arc::clone(&remote) as Arc<dyn VrccRemote>,
    );
    service.note_recent(4);
    let summary = service.wrist_summary(None).expect("有未读数据了");
    assert_eq!(summary.unread, 4);
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
    let service = VrccService::new(store, FakeRemote::new());
    // 未连接时不应发请求，直接报没令牌。
    let error = service.recent_notifications(false).await.unwrap_err();
    assert!(matches!(error, VrccError::TokenMissing));
}

#[tokio::test]
async fn poll_notifications_requires_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    let error = service.poll_notifications().await.unwrap_err();
    assert!(matches!(error, VrccError::TokenMissing));
}

#[tokio::test]
async fn today_requires_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    let error = service.today().await.unwrap_err();
    assert!(matches!(error, VrccError::TokenMissing));
}

#[tokio::test]
async fn unread_is_counted_from_the_read_cursor_not_the_item_count() {
    // 官方的「未读」是 `id > read`，不是「返回了几条」。
    // 过去这里靠真的 `VrccSession` 才能测，现在端口一换就能脱离网络验证。
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    remote.set_recent(VrccRecentNotifications {
        items: vec![recent_item(3), recent_item(2), recent_item(1)],
        read: Some(2),
    });
    let service = VrccService::new(store, Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let recent = service.recent_notifications(false).await.expect("recent");
    assert_eq!(recent.items.len(), 3);
    // read=2 时只有 id=3 是未读。
    assert_eq!(
        service.wrist_summary(None).map(|summary| summary.unread),
        Some(1)
    );
}

#[tokio::test]
async fn a_missing_read_cursor_treats_every_item_as_unread() {
    // 账号上还没记过已读时，只能把返回的都当未读——这是唯一诚实的解读。
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    remote.set_recent(VrccRecentNotifications {
        items: vec![recent_item(2), recent_item(1)],
        read: None,
    });
    let service = VrccService::new(store, Arc::clone(&remote) as Arc<dyn VrccRemote>);

    service.recent_notifications(false).await.expect("recent");
    assert_eq!(
        service.wrist_summary(None).map(|summary| summary.unread),
        Some(2)
    );
}

#[tokio::test]
async fn unread_prefers_the_server_counts_over_the_recent_items() {
    // 服务器有 `me/unread` 时，腕表用它；「最近的提醒」的条数不再覆盖它。
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    remote.set_unread(VrccUnread {
        inbox: 2,
        chats: 3,
        ..Default::default()
    });
    let service = VrccService::new(store, Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let (counts, changed) = service
        .refresh_unread()
        .await
        .expect("unread")
        .expect("counts");
    assert!(changed);
    assert_eq!(counts.total(), 5);
    assert_eq!(
        service.wrist_summary(None).map(|summary| summary.unread),
        Some(5)
    );
}

#[tokio::test]
async fn a_transport_failure_is_reported_but_keeps_the_token() {
    // 网络错误不是令牌的事：报出去，但不清凭据。
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    remote.set_error(VrccError::Transport("dns timeout".into()));
    let service = VrccService::new(store.clone(), Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let error = service.refresh_unread().await.unwrap_err();
    assert!(matches!(error, VrccError::Transport(_)));
    assert!(service.snapshot().connected);
    assert!(store.load().unwrap().is_some());
}

/// 造一条「最近的提醒」条目。只关心 id，其余字段取默认。
fn recent_item(id: i64) -> vrcx_0_contracts::vrcc::VrccRecentNotificationItem {
    vrcx_0_contracts::vrcc::VrccRecentNotificationItem {
        id,
        title: Some(format!("提醒 {id}")),
        body: None,
        at: Some(1_700_000_000_000),
        url: None,
    }
}

// -- 断开时的吊销 ------------------------------------------------------------

/// **吊销请求没发出去时必须保留令牌。**
///
/// 这是真实风险：网络不通、或恰好在全局退避里，`POST /disconnect` 到不了
/// 服务器。如果这时候把本地令牌删了，服务器上那份授权就再也吊销不了——
/// 玩家在网页「第三方客户端」里会一直看到一个早就该消失的客户端。
#[tokio::test]
async fn a_retriable_revoke_failure_keeps_the_token() {
    for error in [
        VrccError::Transport("network unreachable".into()),
        VrccError::RateLimited("退避中".into()),
        VrccError::TooSoon {
            retry_after_secs: 12,
        },
    ] {
        let store = MemoryStore::with(StoredCredential {
            token: "tok".into(),
            ..Default::default()
        });
        let remote = FakeRemote::new();
        remote.mark_connected();
        remote.set_error(error.clone());
        let service = VrccService::new(store.clone(), Arc::clone(&remote) as Arc<dyn VrccRemote>);

        let returned = service.disconnect().await.unwrap_err();
        assert_eq!(
            std::mem::discriminant(&returned),
            std::mem::discriminant(&error),
            "错误要原样交出去"
        );
        // 令牌还在，服务器上那份授权还有机会被吊销。
        assert!(
            store.load().unwrap().is_some(),
            "可重试的吊销失败不该清掉令牌：{error}"
        );
        // 也仍然算连着：界面要如实显示，不能骗玩家说已经断开了。
        assert!(service.snapshot().connected, "本地也不该断开：{error}");
    }
}

/// 吊销成功 → 清本地。
#[tokio::test]
async fn a_successful_revoke_clears_the_token() {
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(store.clone(), Arc::clone(&remote) as Arc<dyn VrccRemote>);

    service.disconnect().await.expect("disconnect");
    assert!(store.load().unwrap().is_none());
    assert!(!service.snapshot().connected);
}

/// 服务器说「这个令牌早就不算数了」→ 当作已断开，清本地。
#[tokio::test]
async fn a_gone_token_counts_as_already_disconnected() {
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    remote.set_error(VrccError::Api {
        status: 404,
        error: "gone".into(),
        need: None,
        why: None,
    });
    let service = VrccService::new(store.clone(), Arc::clone(&remote) as Arc<dyn VrccRemote>);

    service
        .disconnect()
        .await
        .expect("令牌本来就无效，等于已经断开了");
    assert!(store.load().unwrap().is_none());
    assert!(!service.snapshot().connected);
}

/// 服务器答复里的错误不算「可重试」：请求已经到了，重试也不会变。
#[test]
fn only_pre_send_failures_are_retriable() {
    assert!(retriable_revoke_failure(&VrccError::Transport("x".into())));
    assert!(retriable_revoke_failure(&VrccError::RateLimited(
        "x".into()
    )));
    assert!(retriable_revoke_failure(&VrccError::TooSoon {
        retry_after_secs: 1
    }));

    assert!(!retriable_revoke_failure(&VrccError::Api {
        status: 500,
        error: "server_error".into(),
        need: None,
        why: None,
    }));
    assert!(!retriable_revoke_failure(&VrccError::QuotaExhausted(
        "用尽".into()
    )));
    assert!(!retriable_revoke_failure(&VrccError::Paused {
        say: None,
        until: None,
    }));
}
