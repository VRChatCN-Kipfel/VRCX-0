//! 应用层测试：用内存仓库替换持久化，验证编排逻辑。

use std::sync::{Arc, Mutex};

use super::*;
use vrcx_0_contracts::vrcc::{HttpDetail, LocalThrottle, VrccBoardMap, VrccUnread};
// ---------------------------------------------------------------------------
// 造错误的几个便捷函数
//
// 重构后错误是「层级 + 细节」两段，直接写全每次都很啰嗦。这几个函数只用于
// 测试，名字直接说明造的是哪一层的错。
// ---------------------------------------------------------------------------

/// 传输层失败（没拿到响应）。
fn transport(message: &str) -> VrccError {
    VrccError::new(
        ErrorLayer::Transport,
        ErrorDetail::Transport {
            kind: TransportKind::Connect,
            message: message.to_string(),
        },
    )
}

/// 服务器拒绝（拿到了响应，状态码不是 2xx）。
fn http(status: u16, error: &str) -> VrccError {
    VrccError::new(
        ErrorLayer::Server,
        ErrorDetail::Http(HttpDetail {
            status,
            error: error.to_string(),
            need: None,
            why: None,
            retry_after: None,
            op: None,
        }),
    )
}

/// 缺权限的 403。
fn missing_scope(need: &str) -> VrccError {
    VrccError::new(
        ErrorLayer::Server,
        ErrorDetail::Http(HttpDetail {
            status: 403,
            error: "scope".to_string(),
            need: Some(need.to_string()),
            why: None,
            retry_after: None,
            op: None,
        }),
    )
}

/// 本机限流（请求没发出去）。
fn throttled_locally(secs: u64, reason: LocalThrottle) -> VrccError {
    VrccError::new(
        ErrorLayer::Client,
        ErrorDetail::ThrottledLocally {
            retry_after_secs: secs,
            reason,
        },
    )
}

/// 没有令牌。
fn token_missing() -> VrccError {
    VrccError::new(ErrorLayer::Session, ErrorDetail::TokenMissing)
}

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
    /// 记录 `device_code` 收到的 `scope`。
    device_code_scopes: Mutex<Vec<Option<String>>>,
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
        Box::pin(async move { Err(transport("fake")) })
    }
    fn maps<'a>(&'a self, _worlds: &'a [String]) -> VrccFuture<'a, VrccBoard> {
        Box::pin(async move { Err(transport("fake")) })
    }
    fn status<'a>(&'a self) -> VrccFuture<'a, VrccStatus> {
        Box::pin(async move { Err(transport("fake")) })
    }
    fn fetch_site_asset<'a>(&'a self, _path: &'a str) -> VrccFuture<'a, Vec<u8>> {
        Box::pin(async move { Err(transport("fake")) })
    }
    fn announcements<'a>(&'a self) -> VrccFuture<'a, VrccAnnouncements> {
        Box::pin(async move { Err(transport("fake")) })
    }
    fn stickers<'a>(&'a self) -> VrccFuture<'a, VrccStickers> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Ok(VrccStickers::default())
        })
    }
    fn device_code<'a>(&'a self, scope: Option<&'a str>) -> VrccFuture<'a, VrccDeviceCode> {
        self.device_code_scopes
            .lock()
            .unwrap()
            .push(scope.map(str::to_string));
        Box::pin(async move { Err(transport("fake")) })
    }
    fn device_token<'a>(&'a self, _code: &'a str) -> VrccFuture<'a, VrccTokenResponse> {
        Box::pin(async move { Err(transport("fake")) })
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
                .ok_or_else(token_missing)
        })
    }
    fn notifications<'a>(&'a self, _since: Option<i64>) -> VrccFuture<'a, VrccNotifications> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
        })
    }
    fn my_map<'a>(&'a self, _world: &'a str) -> VrccFuture<'a, VrccMyMap> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
        })
    }
    fn my_maps<'a>(&'a self) -> VrccFuture<'a, VrccRatedMaps> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
        })
    }
    fn map_page<'a>(&'a self, _world: &'a str) -> VrccFuture<'a, VrccMapPage> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            // 公开接口：没有令牌也能答。默认给一份空页面，让「404 转 None」
            // 这类测试只要注入错误就能覆盖到。
            Ok(VrccMapPage::default())
        })
    }
    fn my_map_page<'a>(
        &'a self,
        _world: &'a str,
        _sort: Option<&'a str>,
    ) -> VrccFuture<'a, VrccMyMapPage> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            // 带令牌的页面：默认给一份空的，让「404 转 None」「未连接先拒绝」
            // 这类测试只要注入错误就能覆盖到。
            Ok(VrccMyMapPage::default())
        })
    }
    fn rate_map<'a>(
        &'a self,
        _world: &'a str,
        _score: i64,
        _text: &'a VrccRatingText,
    ) -> VrccFuture<'a, VrccMyMap> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            // 写操作默认成功，返回一份空的地图状态。
            Ok(VrccMyMap::default())
        })
    }
    fn delete_map_rating<'a>(&'a self, _world: &'a str) -> VrccFuture<'a, ()> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Ok(())
        })
    }
    fn my_board<'a>(&'a self, _sort: VrccMyBoardSort) -> VrccFuture<'a, VrccMyBoard> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
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
                .ok_or_else(token_missing)
        })
    }
    fn notes<'a>(&'a self) -> VrccFuture<'a, VrccNotes> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
        })
    }
    fn today<'a>(&'a self) -> VrccFuture<'a, VrccToday> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
        })
    }
    fn mutuals<'a>(&'a self) -> VrccFuture<'a, VrccMutuals> {
        Box::pin(async move {
            if let Some(error) = self.take_error() {
                return Err(error);
            }
            Err(token_missing())
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

    let error = service.settle(http(404, "gone"));
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
        service.settle(http(404, code));
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
    // 停站维护：令牌没事，等 `until` 就好，和网络问题一样处理。
    let paused = http(503, "stop");
    assert_eq!(classify_vrcc_error(&paused).0, VrccErrorKind::Transport);
    assert_eq!(paused.layer, ErrorLayer::Server);
}

#[tokio::test]
async fn unread_and_notes_require_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    assert!(service
        .refresh_unread()
        .await
        .unwrap_err()
        .is_session_lost());
    assert!(service.notes().await.unwrap_err().is_session_lost());
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
    // 本机限流：请求没发出去，玩家该等。
    let client_side = throttled_locally(30, LocalThrottle::Gap);
    assert_eq!(
        classify_vrcc_error(&client_side).0,
        VrccErrorKind::Throttled
    );

    // 服务器说 429：同样是「问太多了」，也归 Throttled。
    let too_many = http(429, "too_many");
    assert_eq!(classify_vrcc_error(&too_many).0, VrccErrorKind::Throttled);

    let quota = http(429, "quota");
    assert_eq!(classify_vrcc_error(&quota).0, VrccErrorKind::Throttled);

    let gone = http(404, "gone");
    assert_eq!(classify_vrcc_error(&gone).0, VrccErrorKind::NeedsReauth);

    let missing = missing_scope("today");
    let (kind, need) = classify_vrcc_error(&missing);
    assert_eq!(kind, VrccErrorKind::MissingScope);
    assert_eq!(need.as_deref(), Some("today"));

    let transport = transport("timeout");
    assert_eq!(classify_vrcc_error(&transport).0, VrccErrorKind::Transport);
}

/// `409 busy` 和 `429 busy` 用的是同一个 `error` 串，含义却相反：
/// 前者是「那边的星卡还在生成，等一下」，后者是「全站连接功能今天关了」。
///
/// 两者都不该被当成需要重新授权，也不该说成「今天的次数用完了」。
/// 这正是把 HTTP 语义收敛成一个 `Http` 变体之后必须守住的一条：
/// **官方的 `busy` 只能按状态码区分**。
#[test]
fn busy_never_reads_as_throttled_or_reauth() {
    let not_ready = http(409, "busy");
    assert_eq!(classify_vrcc_error(&not_ready).0, VrccErrorKind::NotReady);
    assert!(not_ready.is_not_ready());

    let connect_paused = http(429, "busy");
    // 429 busy 也是「那边没准备好」，不是「你额度用完了」。
    assert_eq!(
        classify_vrcc_error(&connect_paused).0,
        VrccErrorKind::NotReady
    );
    // 但 429 busy 不是 is_not_ready（那只认 409），两者靠状态码分开。
    assert!(!connect_paused.is_not_ready());

    for error in [not_ready, connect_paused] {
        assert_ne!(classify_vrcc_error(&error).0, VrccErrorKind::NeedsReauth);
        assert_ne!(classify_vrcc_error(&error).0, VrccErrorKind::Throttled);
        assert!(!error.needs_reauth());
    }
}

/// 我们自己构造请求出的错，不该走到玩家面前。
#[test]
fn our_own_bugs_are_marked_as_input_layer() {
    let bad_path = VrccError::new(
        ErrorLayer::Input,
        ErrorDetail::AbsolutePath("http://elsewhere".into()),
    );
    assert!(bad_path.is_our_bug());
    assert_eq!(classify_vrcc_error(&bad_path).0, VrccErrorKind::Other);

    let too_big = VrccError::new(ErrorLayer::Input, ErrorDetail::BodyTooLarge);
    assert!(too_big.is_our_bug());

    // 反过来：网络问题不是我们的锅。
    assert!(!transport("timeout").is_our_bug());
}

#[test]
fn map_not_went_is_not_a_reauth_case() {
    // 打分前置条件不满足，令牌没问题。
    let error = missing_scope("verify");
    let (kind, _) = classify_vrcc_error(&error);
    assert_ne!(kind, VrccErrorKind::NeedsReauth);
}

#[tokio::test]
async fn connected_guard_blocks_personal_calls_when_disconnected() {
    let store = Arc::new(MemoryStore::default());
    let service = VrccService::new(store, FakeRemote::new());
    // 未连接时不应发请求，直接报没令牌。
    let error = service.recent_notifications(false).await.unwrap_err();
    assert!(error.is_session_lost());
}

/// 不传 `scope` 时要按标识显式地要：交给服务器默认的话，自助登记的
/// `app_…` 只有 `profile`，提醒和「连上」全是 `403 scope`。
#[tokio::test]
async fn device_auth_asks_for_the_scope_that_fits_the_client() {
    let cases = [
        ("dev-test", "profile notifications maps"),
        (
            "app_0123456789abcdef",
            "profile notifications maps mutuals maps.rate",
        ),
    ];
    for (client, expected) in cases {
        let remote = FakeRemote::new();
        remote.connection.lock().unwrap().client = client.to_string();
        let service = VrccService::new(Arc::new(MemoryStore::default()), remote.clone());

        let _ = service.begin_device_auth(None).await;
        let _ = service.begin_device_auth(Some("  ")).await;

        let asked = remote.device_code_scopes.lock().unwrap().clone();
        assert_eq!(
            asked,
            vec![Some(expected.to_string()), Some(expected.to_string())],
            "{client}"
        );
    }
}

#[tokio::test]
async fn device_auth_keeps_an_explicit_scope() {
    let remote = FakeRemote::new();
    let service = VrccService::new(Arc::new(MemoryStore::default()), remote.clone());

    let _ = service.begin_device_auth(Some("profile card")).await;

    let asked = remote.device_code_scopes.lock().unwrap().clone();
    assert_eq!(asked, vec![Some("profile card".to_string())]);
}

#[tokio::test]
async fn poll_notifications_requires_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    let error = service.poll_notifications().await.unwrap_err();
    assert!(error.is_session_lost());
}

#[tokio::test]
async fn today_requires_connection() {
    let service = VrccService::new(Arc::new(MemoryStore::default()), FakeRemote::new());
    let error = service.today().await.unwrap_err();
    assert!(error.is_session_lost());
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
    remote.set_error(transport("dns timeout"));
    let service = VrccService::new(store.clone(), Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let error = service.refresh_unread().await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Transport);
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
        transport("network unreachable"),
        throttled_locally(30, LocalThrottle::Backoff),
        // 最小间隔没到也是「没发出去」，同样该保留令牌。
        throttled_locally(12, LocalThrottle::Gap),
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
        // `VrccError` 现在是 struct，不能再用 `discriminant` 比「同一个变体」；
        // 改成比层级，这才是「原样交出去」在结构上唯一说得通的等值判据。
        assert_eq!(returned.layer, error.layer, "错误要原样交出去");
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
    remote.set_error(http(404, "gone"));
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
    // 没发出去的：传输失败、本机限流。
    assert!(retriable_revoke_failure(&transport("x")));
    assert!(retriable_revoke_failure(&throttled_locally(
        1,
        LocalThrottle::Backoff
    )));
    assert!(retriable_revoke_failure(&throttled_locally(
        1,
        LocalThrottle::Gap
    )));

    // 服务器已经答复的：重试没用。
    assert!(!retriable_revoke_failure(&http(500, "server_error")));
    assert!(!retriable_revoke_failure(&http(429, "quota")));
    assert!(!retriable_revoke_failure(&http(503, "stop")));
}

// ---------------------------------------------------------------------------
// 地图页 / 我打过的图 / 玩家视角榜 / 贴纸
//
// 这四条都随 A 批一起进来，行为都是「某种失败要变成可显示的状态」，
// 最容易在以后的重构里被悄悄改掉，所以钉住。
// ---------------------------------------------------------------------------

fn connected_service() -> (VrccService, Arc<FakeRemote>, Arc<MemoryStore>) {
    let store = MemoryStore::with(StoredCredential {
        token: "tok".into(),
        ..Default::default()
    });
    let remote = FakeRemote::new();
    remote.mark_connected();
    let service = VrccService::new(store.clone(), Arc::clone(&remote) as Arc<dyn VrccRemote>);
    (service, remote, store)
}

/// 地图页是没有令牌也能读的公开接口。
///
/// 带令牌的地图页：404 同样是「不在榜上」，不是故障。
///
/// 这一条和公开版分开测，因为两条路径的 `ensure_connected` 行为不同：
/// 公开版不要求连接，带令牌版**要求**。
#[tokio::test]
async fn the_signed_in_map_page_treats_404_as_absent() {
    let (service, remote, _store) = connected_service();
    remote.set_error(http(404, "not_found"));

    assert!(
        service.my_map_page("wrld_x", None).await.unwrap().is_none(),
        "404 should read as 'this world is not on the board', not as a failure"
    );

    // 真的故障仍然要报出去。
    remote.set_error(http(500, "server"));
    let error = service.my_map_page("wrld_x", None).await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Server);
}

/// 未连接时直接拒绝，不把一个必然 `TokenMissing` 的请求发出去。
#[tokio::test]
async fn the_signed_in_map_page_refuses_before_sending_when_disconnected() {
    // 空的 store + 没 mark_connected 的 remote = 从没连过的玩家。
    // **不能**用 `connected_service()` 再 `disconnect()`：那个 store 里已经有
    // 令牌，`ensure_connected` 看的是 store，断开并不会让检查失败。
    let store: Arc<dyn VrccTokenStore> = Arc::new(MemoryStore::default());
    let remote = FakeRemote::new();
    let service = VrccService::new(store, Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let error = service.my_map_page("wrld_x", None).await.unwrap_err();
    assert!(error.needs_reauth());
    // 没注入过错误且没有走到网络，说明请求是被本地拦下的。
    assert!(remote.take_error().is_none());
}

/// 写操作遇到空世界 ID 必须**失败**，不能安静成功。
///
/// 读接口在这种情况下返回 `Ok(None)` 是合理的（「没有这一页」），但写操作
/// 「什么都没发生」是最坏的结局——玩家会以为打分提交了。
#[tokio::test]
async fn rating_a_map_without_a_world_id_fails_instead_of_silently_succeeding() {
    let (service, _remote, _store) = connected_service();

    let error = service
        .rate_map("", 8, &VrccRatingText::Keep)
        .await
        .unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Input);

    let error = service.delete_map_rating("").await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Input);
}

/// 未连接时写操作先拒绝，不把必然失败的请求发出去。
#[tokio::test]
async fn rating_refuses_before_sending_when_disconnected() {
    let store: Arc<dyn VrccTokenStore> = Arc::new(MemoryStore::default());
    let remote = FakeRemote::new();
    let service = VrccService::new(store, Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let error = service
        .rate_map("wrld_x", 8, &VrccRatingText::Keep)
        .await
        .unwrap_err();
    assert!(error.needs_reauth());
    assert!(remote.take_error().is_none());
}

/// 删评分遇到 `404` 当作成功：目标本来就已达成。
///
/// 玩家看到「删除失败」会以为评分还在，然后反复点。
#[tokio::test]
async fn deleting_a_rating_that_is_already_gone_is_not_an_error() {
    let (service, remote, _store) = connected_service();
    remote.set_error(http(404, "not_found"));

    assert!(service.delete_map_rating("wrld_x").await.is_ok());
}

/// 短评被审核拒绝时，错误要能一路带到界面。
///
/// `yq_words` 不是网络故障，服务器会在 `why` 里说明原因，那句话对玩家比
/// 「操作失败」有用得多。
#[tokio::test]
async fn a_rejected_comment_keeps_its_reason() {
    let (service, remote, _store) = connected_service();
    remote.set_error(http(400, "yq_words"));

    let error = service
        .rate_map("wrld_x", 8, &VrccRatingText::Write("留个联系方式".into()))
        .await
        .unwrap_err();
    // 这是服务器拒绝，不是传输失败；界面据此显示「提交失败」+ 原因
    assert_eq!(error.layer, ErrorLayer::Server);
}

/// 不在榜上时服务器回 `404 not_found`——那是**这张图还没上榜**，不是故障。
/// 如果这里改成把 404 当错误抛出去，一张新图就会显示成「服务坏了」。
#[tokio::test]
async fn a_map_that_is_not_on_the_board_is_absent_not_broken() {
    let (service, remote, _store) = connected_service();
    remote.set_error(http(404, "not_found"));

    assert!(
        service.map_page("wrld_x").await.unwrap().is_none(),
        "404 should read as 'no page for this world', not as a failure"
    );

    // 真的故障（5xx）仍然要报出去，别把两者都吞掉。
    remote.set_error(http(500, "server"));
    let error = service.map_page("wrld_x").await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Server);
}

/// 空的 world 直接短路，不发请求。
#[tokio::test]
async fn an_empty_world_never_reaches_the_network() {
    let (service, remote, _store) = connected_service();
    // 留一个「下一次一定失败」的错误：真发出去就会拿到它。
    remote.set_error(transport("should not be sent"));

    assert!(service.map_page("").await.unwrap().is_none());
    assert!(service.map_page("   ").await.unwrap().is_none());
}

/// 贴纸是装饰素材：旧服务器没有这个接口（404）时给 `None`，
/// 「我的」页签不该因此报错。
#[tokio::test]
async fn stickers_go_missing_quietly_on_an_old_server() {
    let (service, remote, _store) = connected_service();

    // 正常有数据。
    assert!(service.stickers().await.unwrap().is_some());

    remote.set_error(http(404, "not_found"));
    assert!(service.stickers().await.unwrap().is_none());

    // 其它错误照常上报。
    remote.set_error(http(500, "server"));
    assert!(service.stickers().await.unwrap_err().layer == ErrorLayer::Server);
}

/// 需要令牌的两条：没连接就当场拒绝，不把必然 `TokenMissing` 的请求发出去。
///
/// 发出去的后果很实在：那个错误会被判成「要重新授权」，于是还没连过的玩家
/// 打开页面就看见一句莫名其妙的「请重新连接」。
#[tokio::test]
async fn personal_map_lists_refuse_before_sending_when_disconnected() {
    // 注意这里**不**调用 `mark_connected`。
    let store = MemoryStore::with(StoredCredential::default());
    let remote = FakeRemote::new();
    let service = VrccService::new(store, Arc::clone(&remote) as Arc<dyn VrccRemote>);

    let error = service.my_maps().await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Session);

    let error = service.my_board(VrccMyBoardSort::Went).await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Session);
}

/// 连上之后这两条照常可用。
///
/// fake 在这两条上回 `token_missing`，但连接状态已经就绪——所以拿到的错误
/// 必须来自**远端**，而不是本地那道提前拒绝。两者同为 `Session` 层，这里
/// 靠「换一个远端错误能否透传」来区分。
#[tokio::test]
async fn personal_map_lists_work_once_connected() {
    let (service, remote, _store) = connected_service();

    // 注入一个远端错误：能原样拿到，就说明请求确实走到了远端。
    remote.set_error(http(403, "scope"));
    let error = service.my_maps().await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Server);

    remote.set_error(http(403, "scope"));
    let error = service.my_board(VrccMyBoardSort::Top).await.unwrap_err();
    assert_eq!(error.layer, ErrorLayer::Server);
}
