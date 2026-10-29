//! 星遇同频运行时：把应用层服务挂到桌面运行时上。

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use vrcx_0_application::vrcc::{StoredCredential, VrccFailure, VrccService, VrccTokenStore};
use vrcx_0_contracts::vrcc::{
    scope_grants, VrccAnnouncements, VrccBoard, VrccBoardSort, VrccConnectionSnapshot, VrccMapPage,
    VrccMyBoard, VrccMyBoardSort, VrccNotes, VrccRatedMaps, VrccRecentNotifications, VrccStatus,
    VrccStickers, VrccToday, VrccUnread, VrccUsageSnapshot, VrccWristSummary,
};
use vrcx_0_integrations::vrcc::{resolve_client_id, VrccError};
use vrcx_0_outbound_adapters::LocalVrccTokenStore;
use vrcx_0_persistence::DatabaseService;

/// 把本地加密仓库适配成应用层端口。
///
/// 适配层只做类型转换：契约形状在 `application`，密文读写在 outbound-adapters。
struct TokenStoreAdapter {
    inner: LocalVrccTokenStore,
}

impl VrccTokenStore for TokenStoreAdapter {
    fn load(&self) -> vrcx_0_application_core::Result<Option<StoredCredential>> {
        let stored = self.inner.load()?;
        Ok(stored.map(|credential| StoredCredential {
            token: credential.token,
            star: credential.star,
            vrc_name: credential.vrc_name,
            scope: credential.scope,
            client_id: credential.client_id,
        }))
    }

    fn save(&self, credential: &StoredCredential) -> vrcx_0_application_core::Result<()> {
        self.inner
            .save(&vrcx_0_outbound_adapters::StoredVrccCredential {
                token: credential.token.clone(),
                star: credential.star,
                vrc_name: credential.vrc_name.clone(),
                scope: credential.scope.clone(),
                client_id: credential.client_id.clone(),
            })
    }

    fn clear(&self) -> vrcx_0_application_core::Result<()> {
        self.inner.clear()
    }
}

/// 星遇同频提醒或未读数发生变化时发给前端的事件名。
///
/// 只作为「该刷新了」的信号，不带正文——玩家内容没必要经手额外的事件层。
pub const VRCC_NOTICES_CHANGED_EVENT: &str = "vrccNoticesChanged";

/// 令牌被服务器判了失效（`404 gone`）、本地已经清掉时发给前端的事件名。
///
/// 前端收到后重读连接状态，显示「连接已失效，请重新连接」。
pub const VRCC_CONNECTION_CHANGED_EVENT: &str = "vrccConnectionChanged";

/// 桌面侧的星遇同频运行时。
pub struct DesktopVrccRuntime {
    service: Arc<VrccService>,
    /// 通知中心那一行要落到当前账号的库表里，句柄与令牌存储共用同一个。
    db: Arc<DatabaseService>,
    /// 读当前登录账号。装配发生在登录之前，所以取闭包而不是固定 id。
    owner: Arc<dyn Fn() -> Option<vrcx_0_core::OwnerId> + Send + Sync>,
}

impl DesktopVrccRuntime {
    /// 后台轮询的基准间隔。
    ///
    /// 官方建议按 `me/notifications` 返回的 `every`（通常 60 秒）问，最少隔
    /// 25 秒。这里取 60 秒和 `every` 里大的那个：`me/unread` 每天 1500 次，
    /// 一分钟问一次正好够一整天。
    const BACKGROUND_POLL_BASE: Duration = Duration::from_secs(60);
    /// 抖动上限，避免所有客户端撞在同一秒（官方脚本也这样做）。
    const BACKGROUND_POLL_JITTER: Duration = Duration::from_secs(20);

    /// 星遇企划图标在站点上的路径。
    ///
    /// 512×512：Windows toast 会把它缩到 48×48（随 DPI 放大），源图更大
    /// 降采样后更锐利。
    pub const NOTICE_ICON_PATH: &str = "/assets/match-icon-512.png";
    /// 图标缓存文件名，落在应用数据目录。
    const NOTICE_ICON_FILE: &str = "vrcc-notice-icon.png";

    /// 通知中心里那一行的 id 前缀。
    ///
    /// 行 id 的形态是 `<前缀><提醒编号>`，写库和回标已读都用同一个形态。
    /// 前缀让这些行和 VRChat 自己的提醒（`not_…`）在同一个 id 空间里不撞车。
    const NOTICE_ROW_ID_PREFIX: &str = "vrcc_";
    /// 通知中心里那一行的类型。
    ///
    /// 前端按 `view.notification.filters.<type>` 取标签，所以这个名字同时
    /// 是本地化键的一部分，改这里要一起改文案。
    const NOTICE_ROW_TYPE: &str = "vrcc.notice";
    /// 点击这一行时跳到的位置。前端按 scheme 分派，见 `openNotificationLink`。
    const NOTICE_ROW_LINK: &str = "vrcc:notifications";
    /// 行的发送者与标题。两者都取这个名字是有意的，见 `notice_row`。
    const NOTICE_ROW_SENDER: &str = "星遇同频";

    /// 构造运行时。
    ///
    /// `X-Client` 由 [`resolve_client_id`] 在**构建期**定死，这里不接受任何
    /// 覆盖入口：标识属于「这个二进制是哪个客户端」，不该由配置或命令行改写。
    ///
    /// `proxy_url` 必须传宿主应用的统一代理（`runtime_host().proxy_url()`）：
    /// 本机实测 `vrcc.cc` 直连 DNS 超时，不接代理时所有接口都会「暂时不可用」。
    ///
    /// `owner` 与令牌存储用的是同一个来源：通知中心那一行要写进当前账号的表，
    /// 标记已读也要按同一个账号找行。
    pub fn new(
        store: LocalVrccTokenStore,
        proxy_url: Option<&str>,
        owner: Arc<dyn Fn() -> Option<vrcx_0_core::OwnerId> + Send + Sync>,
    ) -> Self {
        let db = Arc::clone(store.db());
        let port = Arc::new(TokenStoreAdapter { inner: store });
        let client_id = resolve_client_id();
        // 网络端口的实现放在 outbound-adapters：应用层不认识 `VrccSession`，
        // 这里只把它当作 `Arc<dyn VrccRemote>` 注入。
        let remote: Arc<dyn vrcx_0_application::vrcc::VrccRemote> =
            Arc::new(vrcx_0_outbound_adapters::VrccRemoteAdapter::new(
                client_id,
                proxy_url.map(str::to_string),
            ));
        Self {
            service: Arc::new(VrccService::new(port, remote)),
            db,
            owner,
        }
    }

    /// 腕表摘要。纯内存读取，不发请求。
    pub fn wrist_summary(&self, world_id: Option<&str>) -> Option<VrccWristSummary> {
        self.service.wrist_summary(world_id)
    }

    pub fn connection(&self) -> VrccConnectionSnapshot {
        self.service.snapshot()
    }

    pub fn usage(&self) -> VrccUsageSnapshot {
        self.service.usage()
    }

    pub fn client_id(&self) -> String {
        self.service.client_id()
    }

    /// 地图榜：开放数据，不需要连接。
    pub async fn boards(&self, limit: u32, sort: VrccBoardSort) -> Result<VrccBoard, VrccFailure> {
        self.service
            .boards(limit, sort)
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 批量查地图：开放数据，不需要连接。只返回在榜上的。
    pub async fn maps(&self, worlds: &[String]) -> Result<VrccBoard, VrccFailure> {
        self.service
            .maps(worlds)
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    pub async fn status(&self) -> Result<VrccStatus, VrccFailure> {
        self.service
            .status()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 公告和停站。服务器还没有这个接口时为 `None`。
    pub async fn announcements(&self) -> Result<Option<VrccAnnouncements>, VrccFailure> {
        self.service
            .announcements()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 所有贴纸和材质名。服务器还没有这个接口时为 `None`。
    pub async fn stickers(&self) -> Result<Option<VrccStickers>, VrccFailure> {
        self.service
            .stickers()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    pub async fn begin_device_auth(
        &self,
        scope: Option<String>,
    ) -> Result<vrcx_0_application::vrcc::VrccDeviceAuthPrompt, VrccFailure> {
        self.service
            .begin_device_auth(scope.as_deref())
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    pub async fn poll_device_auth(
        &self,
        prompt: &vrcx_0_application::vrcc::VrccDeviceAuthPrompt,
    ) -> Result<vrcx_0_application::vrcc::VrccDeviceAuthStep, VrccFailure> {
        self.service
            .poll_device_auth(prompt)
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    pub async fn disconnect(&self) -> Result<(), VrccFailure> {
        self.service
            .disconnect()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 最近的提醒。`mark` 为 `true` 时同时把已读状态同步到账号（跨设备）。
    ///
    /// 拿到结果后顺手让通知中心里那几行跟上服务端的已读游标，见
    /// [`Self::settle_notice_rows`]。
    pub async fn recent_notifications(
        &self,
        mark: bool,
    ) -> Result<VrccRecentNotifications, VrccFailure> {
        let recent = self
            .service
            .recent_notifications(mark)
            .await
            .map_err(|error| VrccFailure::from_error(&error))?;
        self.settle_notice_rows(mark, recent.read);
        Ok(recent)
    }

    /// 让通知中心里那几行跟上服务端的已读游标。
    ///
    /// 服务端的 `read` 是唯一的已读权威：`id > read` 才算未读。玩家打开
    /// 「提醒」页时前端传 `mark=true`，服务器把 `read` 推到最新；本地那些行
    /// 到这里就该跟着变已读，否则角标只增不减。
    ///
    /// `read` 缺失（账号上还没记过已读）时只在 `viewed` 为真这一种情况下
    /// 全部标已读——那表示玩家确实看过了整张列表，而不是「不知道」。
    fn settle_notice_rows(&self, viewed: bool, read: Option<i64>) {
        let Some(owner) = (self.owner)() else {
            return;
        };
        let cutoff = match read {
            Some(read) => read,
            None if viewed => i64::MAX,
            None => return,
        };
        if let Err(error) = vrcx_0_persistence::notifications::notification_mark_seen_up_to(
            &self.db,
            owner.as_str().to_string(),
            Self::NOTICE_ROW_ID_PREFIX,
            cutoff,
        ) {
            tracing::warn!(%error, "vrcc: could not settle notification-centre rows");
        }
    }

    pub async fn poll_notifications(
        &self,
    ) -> Result<vrcx_0_contracts::vrcc::VrccNotifications, VrccFailure> {
        self.service
            .poll_notifications()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 几处未读数。
    ///
    /// `refresh` 为 `false` 时只读后台轮询留下的缓存，不发请求——前端收到
    /// 「有变化」的事件时用这个，后台刚问过，不必再花一次。服务器还没有
    /// `me/unread` 时为 `None`。
    pub async fn unread(&self, refresh: bool) -> Result<Option<VrccUnread>, VrccFailure> {
        if !refresh {
            return Ok(self.service.cached_unread());
        }
        self.service
            .refresh_unread()
            .await
            .map(|result| result.map(|(counts, _)| counts))
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 通知中心。服务器还没有这个接口时为 `None`。
    pub async fn notes(&self) -> Result<Option<VrccNotes>, VrccFailure> {
        self.service
            .notes()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 当前所在地图在地图榜上的状态。`off` 的地图返回 `None`（不给看）。
    pub async fn my_map(
        &self,
        world: &str,
    ) -> Result<Option<vrcx_0_contracts::vrcc::VrccMyMap>, VrccFailure> {
        self.service
            .my_map(world)
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 公开地图页。不在榜上时（`404`）返回 `None`。
    pub async fn map_page(&self, world: &str) -> Result<Option<VrccMapPage>, VrccFailure> {
        self.service
            .map_page(world)
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 玩家视角地图榜。需要 `maps` 普通权限。
    pub async fn my_board(&self, sort: VrccMyBoardSort) -> Result<VrccMyBoard, VrccFailure> {
        self.service
            .my_board(sort)
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 我打过分的地图。需要 `maps` 普通权限。
    pub async fn my_maps(&self) -> Result<VrccRatedMaps, VrccFailure> {
        self.service
            .my_maps()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    pub async fn today(&self) -> Result<VrccToday, VrccFailure> {
        self.service
            .today()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 「互相连上」的星友列表。需要 `mutuals` 敏感权限。
    pub async fn mutuals(&self) -> Result<vrcx_0_contracts::vrcc::VrccMutuals, VrccFailure> {
        self.service
            .mutuals()
            .await
            .map_err(|error| VrccFailure::from_error(&error))
    }

    /// 起一个后台轮询任务，把新提醒推成系统通知。
    ///
    /// **为什么要放到 Rust 侧**：原来轮询在前端（`useVrccController` 的
    /// `setTimeout`）。窗口隐藏到托盘后 WebView 仍存活（`window.hide()` 不销毁
    /// WebView），但 Chromium 会节流隐藏页面的定时器，60 秒的间隔可能被拉到
    /// 几分钟——而"后台也能收到提醒"恰恰是这个功能最重要的场景。
    ///
    /// 这里用 `TaskSupervisor::spawn_cancellable`，让任务随应用退出一起停，
    /// 不用自己管生命周期。
    ///
    /// 通知走宿主注入的 `DesktopNotifier`：Headless 下没人挂载它，
    /// `show()` 返回 `Err`，这里只记日志，不会 panic。
    ///
    /// `event_sink` 用来告诉前端「有新数据了」或「连接失效了」。前端不再自己
    /// 轮询——双轨会让两边各拉一次、`mark` 语义互相打架，而且隐藏窗口时前端
    /// 定时器会被节流。
    pub fn spawn_background_poll(
        &self,
        supervisor: &vrcx_0_application_core::TaskSupervisor,
        dispatcher: Arc<crate::notification::NotificationDispatcher>,
        event_sink: Arc<dyn vrcx_0_application_core::RuntimeEventSink>,
        owner: vrcx_0_core::OwnerId,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) -> vrcx_0_application_core::TaskSpawnOutcome {
        let service = Arc::clone(&self.service);
        let db = Arc::clone(&self.db);
        let base = Self::BACKGROUND_POLL_BASE;
        let jitter = Self::BACKGROUND_POLL_JITTER;

        supervisor.spawn_cancellable(move |stop_token| {
            let service = Arc::clone(&service);
            let db = Arc::clone(&db);
            let dispatcher = Arc::clone(&dispatcher);
            let event_sink = Arc::clone(&event_sink);
            let owner = owner.clone();
            let stop = Arc::clone(&stop);
            async move {
                // 去重状态在**所有轮次之间共享**，这是关键：服务端游标长期不动，
                // 只靠它判断会让同一条提醒每轮重弹一次（实测几十次）。
                let notified = NotifiedIds::default();
                // 通知图标也在轮次之间共享：第一次成功下载后就只用本地文件。
                let mut notice_icon = NoticeIcon::default();
                // 启动探针：没有它，分不清「轮询没起来」和「起来了但没数据」。
                tracing::info!(
                    interval_secs = base.as_secs(),
                    jitter_secs = jitter.as_secs(),
                    owner = %owner.as_str(),
                    "vrcc: background poll task started"
                );
                let mut round: u64 = 0;
                loop {
                    // 两个停止信号：应用退出（supervisor 的 token）或换账号
                    // （我们自己那个 atomic）。后者让「一个任务只服务一个账号」
                    // 成为可能——换号时旧任务自行退出，新任务另起。
                    if stop_token.is_stop_requested() {
                        tracing::info!(round, "vrcc: background poll stop requested");
                        return;
                    }
                    if stop.load(std::sync::atomic::Ordering::Acquire) {
                        tracing::info!(round, "vrcc: background poll superseded by account change");
                        return;
                    }
                    round = round.wrapping_add(1);
                    // 未连接时不必轮询：个人接口会直接报错。
                    // owner 由任务自身固定持有（见 spawn 的注释），不必再判空。
                    let snapshot = service.snapshot();
                    if snapshot.connected
                        && !scope_grants(snapshot.scope.as_deref(), "notifications")
                    {
                        // 令牌没有提醒权限（旧版本连上的只有 `profile`）：每轮都只会
                        // 换来 `403 scope`。玩家重新连接、拿到新令牌后自动恢复。
                        tracing::debug!(
                            round,
                            "vrcc: background poll skipped (token lacks notifications)"
                        );
                    } else if snapshot.connected {
                        match poll_once(
                            &service,
                            dispatcher.as_ref(),
                            &db,
                            &notified,
                            &mut notice_icon,
                            &owner,
                        )
                        .await
                        {
                            Ok(outcome) => {
                                // 每一轮都记录结果，但用 debug：正常运行时不该刷屏，
                                // 排查时打开 RUST_LOG=debug 就有完整时间线。
                                tracing::debug!(
                                    round,
                                    fresh = outcome.fresh,
                                    persisted = outcome.persisted,
                                    unread = ?outcome.unread,
                                    toast_attempted = outcome.toast_attempted,
                                    changed = outcome.changed,
                                    "vrcc: background poll round finished"
                                );
                                // 只发一个「变了」的信号，具体数据前端自己拉。
                                // 不在事件里带提醒正文：那会把玩家内容写进
                                // 事件经手的所有层，多一条泄露面。
                                if let Some(event) = outcome.event_to_emit() {
                                    event_sink.emit(event, json!({}));
                                }
                            }
                            Err(error) if error.is_gone() => {
                                // 令牌失效，应用层已经把它清掉了：之后几轮会因为
                                // 未连接而跳过，等玩家重新连接。告诉前端改显示。
                                tracing::info!(
                                    round,
                                    "vrcc: token gone, background poll idle until reconnect"
                                );
                                event_sink.emit(VRCC_CONNECTION_CHANGED_EVENT, json!({}));
                            }
                            Err(error) => {
                                // 不把错误抛出去：一次失败不该杀掉整个轮询。
                                // 用 warn 而不是 debug：轮询连续失败是需要被看到的，
                                // 但它不是致命错误，所以不升级成 error 去污染错误日志。
                                tracing::warn!(round, %error, "vrcc: background poll failed");
                            }
                        }
                    } else {
                        tracing::debug!(round, "vrcc: background poll skipped (not connected)");
                    }
                    // 用带抖动的固定间隔。这里刻意用 `sleep` 而不是绝对时刻对齐：
                    // 唤醒后最多补一次，不会像"补 N 个周期"那样突发请求。
                    let every = Duration::from_secs(service.poll_every_secs().max(0) as u64);
                    let delay = base.max(every) + Duration::from_millis(jitter_millis(&jitter));
                    tracing::debug!(
                        round,
                        delay_ms = delay.as_millis() as u64,
                        "vrcc: background poll sleeping"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        })
    }
}

/// 一次轮询的结果，用于日志与事件判定。
///
/// 拆成结构体而不是裸 `bool`：排查时最常问的问题分别是「来了几条新的」、
/// 「现在几处未读」、「弹出去几次」，只回一个布尔值等于什么都没说。
#[derive(Clone, Copy, Debug, Default)]
struct PollOutcome {
    /// 这一轮 `me/notifications` 给的新提醒条数。
    fresh: usize,
    /// 实际尝试弹系统通知的次数（受 `MAX_NOTIFIED_PER_POLL` 限制）。
    toast_attempted: usize,
    /// `me/unread` 的合计；服务器还没有这个接口时为 `None`。
    unread: Option<i64>,
    /// 写进通知中心的条数。和 `fresh` 不同：这里包含超出弹窗上限的那几条。
    persisted: usize,
    /// 是否需要让前端刷新。
    changed: bool,
}

impl PollOutcome {
    /// 该给前端发哪个事件；`None` 表示这一轮没有变化，不必打扰前端。
    ///
    /// 抽成方法而不是在循环里写 `if outcome.changed`：这是「要不要通知前端」
    /// 的唯一判断点，值得单独钉住——写反了会导致界面永远不刷新（静默），
    /// 或者每轮都刷新（浪费）。
    fn event_to_emit(&self) -> Option<&'static str> {
        if self.changed {
            Some(VRCC_NOTICES_CHANGED_EVENT)
        } else {
            None
        }
    }
}

/// 一次轮询。
///
/// 1. `me/notifications?since=`：官方给桌面客户端设计的轮询，**没有每天上限**，
///    每次只给游标之后的新提醒，所以每条只弹一次。第一次不带游标，服务器只
///    告诉「现在到哪了」，不给旧提醒——启动时不会把一天的旧提醒全弹一遍。
/// 2. `me/unread`（1.13.0 起）：几处红点的数字，每天 1500 次单独计数，给腕表和
///    页面角标。
/// 3. 旧服务器没有 `me/unread` 时，只在刚连上或来了新提醒时拉一次「最近的提醒」
///    （每天 1000 次的第一份读），腕表的未读数靠它。
///
/// 原来每轮都拉「最近的提醒」：一分钟一次一整天要一千多次，超过第一份读的
/// 每天上限；而且只要玩家没在网页上看过，同一条提醒每轮都会再弹一次。
async fn poll_once(
    service: &VrccService,
    dispatcher: &crate::notification::NotificationDispatcher,
    db: &DatabaseService,
    notified: &NotifiedIds,
    icon: &mut NoticeIcon,
    owner: &vrcx_0_core::OwnerId,
) -> Result<PollOutcome, VrccError> {
    let first_poll = service.notifications_cursor().is_none();
    let fresh = match service.poll_notifications().await {
        Ok(result) => result.items,
        // 离上一次不到 25 秒（比如刚重新连上）：这一轮没有新的，下一轮补上。
        //
        // 认 `Throttled` 这一类而不是某个具体变体：本机最小间隔（`Gap`）和
        // 上一轮 429 的退避（`Backoff`）都算「现在不该问」，两种都静默跳过。
        Err(error) if error.is_throttled() => Vec::new(),
        Err(error) => return Err(error),
    };

    // 落库要放在选通知之前：系统通知每轮最多弹 `MAX_NOTIFIED_PER_POLL` 条，
    // 但**这一轮拉到的每一条**都该进通知中心，否则超出的那几条就凭空消失了
    // ——游标已经过去，下一轮不会再给。
    let persisted = persist_notices(db, owner, &fresh);
    if persisted > 0 {
        tracing::debug!(
            persisted,
            "vrcc: notices written to the notification centre"
        );
    }

    // 只有真有新提醒时才去解析图标：没新提醒就不该产生任何额外流量。
    let icon_path = if fresh.is_empty() {
        None
    } else {
        icon.resolve(service).await
    };

    // 拉取与处理分开：`handle_recent_notices` 只接收数据、不看网络，
    // 于是「拿到什么样的数据 → 弹哪些通知」可以脱离服务端直接测。
    let mut outcome =
        handle_recent_notices(&fresh, dispatcher, notified, icon_path, Some(owner.clone()));
    outcome.persisted = persisted;

    match service.refresh_unread().await {
        Ok(Some((counts, changed))) => {
            outcome.unread = Some(counts.total());
            outcome.changed |= changed;
        }
        Ok(None) => {
            if first_poll || !fresh.is_empty() {
                match service.recent_notifications(false).await {
                    Ok(_) => outcome.changed = true,
                    Err(error) if error.is_gone() => return Err(error),
                    Err(error) => tracing::warn!(%error, "vrcc: recent notices refresh failed"),
                }
            }
        }
        Err(error) if error.is_gone() => return Err(error),
        // 未读数拿不到不影响提醒：记一笔，下一轮再试。
        Err(error) => tracing::warn!(%error, "vrcc: unread count failed"),
    }
    Ok(outcome)
}

/// 已弹过通知的提醒 id 集合。
///
/// **为什么需要它**：服务端的 `read` 游标只在玩家主动标记已读时前进，而后台
/// 轮询用的是 `mark=false`（不该替他标已读，那是对的）。于是游标长期不动，
/// 同一条提醒在每一轮里都会被判成"未读"——实测每 60 秒重弹一次，同一条提醒
/// 弹了几十次。
///
/// 所以「未读」和「还没弹过」是两件事：
/// - 未读 = `id > read`，决定界面角标；
/// - 还没弹过 = 不在这个集合里，决定要不要发系统通知。
///
/// 只在内存里：进程重启后集合清空，此时服务端游标通常已经因为玩家打开页面
/// 而前进，最多重弹最近一条。为跨重启保存它需要落库，代价与收益不成比例。
#[derive(Default)]
struct NotifiedIds {
    seen: std::sync::Mutex<std::collections::HashSet<i64>>,
}

impl NotifiedIds {
    /// 记下这批 id，返回其中**之前没见过**的那些。
    ///
    /// 判定与写入在同一次加锁内完成，避免两轮轮询交错时重复弹。
    fn take_new(&self, ids: impl IntoIterator<Item = i64>) -> Vec<i64> {
        let mut seen = match self.seen.lock() {
            Ok(seen) => seen,
            Err(error) => error.into_inner(),
        };
        let mut fresh = Vec::new();
        for id in ids {
            if seen.insert(id) {
                fresh.push(id);
            }
        }
        fresh
    }
}

/// 把一条提醒写成通知中心的一行。
///
/// 纯函数：不碰库、不读时钟（时间由调用方给）。行的形状在这里收口，
/// 于是「写进去会长什么样」可以脱离数据库单独验。
///
/// `fallback_created_at` 用于提醒没带 `at` 的场合。`at` 是毫秒时间戳，
/// 而列里要的是 RFC3339：`created_at` 既参与 SQL 排序，也被前端 `Date.parse`
/// 读，写成裸数字两边都不认。
fn notice_row(
    notice: &vrcx_0_contracts::vrcc::VrccNotificationItem,
    fallback_created_at: &str,
) -> serde_json::Value {
    let created_at = notice
        .at
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(vrcx_0_core::time::iso_millis)
        .unwrap_or_else(|| fallback_created_at.to_string());
    let title = notice.title.as_deref().unwrap_or_default().trim();
    let body = notice.body.as_deref().unwrap_or_default().trim();
    // 正文一栏只放正文，不拼接标题。
    //
    // 抽屉行经 `getNotificationMessage` 会把 `title` 与 `message` 两列**再拼
    // 一次**；先前把标题拼进正文，又让 `title` 与发送者同名，于是同一条提醒
    // 在抽屉里会显示成「星遇同频 星遇同频 …」。两者只留一处：标题走 `title`，
    // 正文走 `message`。正文缺失时退回标题，免得留下空白行。
    let message = if body.is_empty() { title } else { body };
    let sender = DesktopVrccRuntime::NOTICE_ROW_SENDER;
    json!({
        "id": format!("{}{}", DesktopVrccRuntime::NOTICE_ROW_ID_PREFIX, notice.id),
        "createdAt": created_at,
        "updatedAt": created_at.clone(),
        // 空串表示不过期：这些行是给玩家回查的历史，不该自己消失。
        "expiresAt": "",
        "type": DesktopVrccRuntime::NOTICE_ROW_TYPE,
        // 提醒没有 VRChat 的用户或世界可跳，落点只能写在 link 上。
        "link": DesktopVrccRuntime::NOTICE_ROW_LINK,
        "linkText": String::new(),
        // 发送者同时写进 `title` 与 `senderUsername`：抽屉行经 `getSenderName`
        // 读后者，表格页经 `view.actor.name`（= `firstText(title)`）读前者，
        // 两处各取一路。
        "title": sender,
        "message": message,
        // 未读：抽屉的红点、侧边栏角标都认这个字段。
        "seen": false,
        // 留空，`openSender` 据此落到 `link` 分支，而不是去开一个不存在的玩家资料。
        "senderUserId": String::new(),
        "senderUsername": sender,
        "data": {},
        "responses": [],
        "details": {},
    })
}

/// 把这一轮的提醒写进通知中心。
///
/// 逐条写、单条失败只记日志：一条写不进去不该让整批作废，更不该影响本轮
/// 已经发出去的系统通知。返回写成功的条数，只用于日志。
///
/// 重复写同一条是无害的：`notification_add_v2` 按 id 冲突合并，而
/// `seen` 取两边较大的那个，所以不会把玩家已经读过的行又变回未读。
fn persist_notices(
    db: &DatabaseService,
    owner: &vrcx_0_core::OwnerId,
    items: &[vrcx_0_contracts::vrcc::VrccNotificationItem],
) -> usize {
    if items.is_empty() {
        return 0;
    }
    let now = vrcx_0_core::time::now_iso();
    let mut written = 0usize;
    for item in items {
        let row = notice_row(item, &now);
        match vrcx_0_persistence::notifications::notification_add_v2(
            db,
            owner.as_str().to_string(),
            row,
        ) {
            Ok(()) => written += 1,
            Err(error) => {
                // 只记编号，不记正文：提醒内容属于玩家数据。
                tracing::warn!(
                    notice_id = item.id,
                    %error,
                    "vrcc: notice could not be written to the notification centre"
                );
            }
        }
    }
    written
}

/// 处理一批刚拉到的新提醒：挑出要弹的、交给通知分发器。
///
/// 这个函数不发请求也不读时钟，是排查「为什么没弹」时最该被测试覆盖的一段。
///
/// **为什么改成走 `NotificationDispatcher`**：早先这里直接调
/// `DesktopNotifier::show()`，等于绕过了整条通知链——免打扰、隐私锁、
/// 通道偏好、图标解析、有序投递全都不生效，而且「VR 内也提醒」根本没实现
/// （`show()` 只弹桌面 toast）。现在把每条提醒包装成一个 `ActivityDelivery`，
/// 交给同一套实现。
///
/// `icon` 是通知图标的本地路径（Windows toast 只接受本地文件，不接受 URL）。
/// 传 `None` 也能正常发通知，只是没有图。
fn handle_recent_notices(
    items: &[vrcx_0_contracts::vrcc::VrccNotificationItem],
    dispatcher: &crate::notification::NotificationDispatcher,
    notified: &NotifiedIds,
    icon: Option<&std::path::Path>,
    owner: Option<vrcx_0_core::OwnerId>,
) -> PollOutcome {
    tracing::debug!(fresh = items.len(), "vrcc: fetched new notices");

    // 决策与投递分开：`select_notices_to_notify` 是纯函数，可以脱离网络和
    // 通知桥单独测。这里只负责把它选出来的结果包装成通知。
    let selection = select_notices_to_notify(items, MAX_NOTIFIED_PER_POLL);

    // 真正要弹的是「选中 且 之前没弹过」的那些。`?since=` 由服务端保证每条只给
    // 一次，但重连、换号、或服务器重发同一批时仍可能重复，本地这一层不能省。
    let candidates = selection.notices;
    let fresh_ids = notified.take_new(candidates.iter().map(|notice| notice.id));
    let notices: Vec<_> = candidates
        .into_iter()
        .filter(|notice| fresh_ids.contains(&notice.id))
        .collect();
    // 本轮被本地去重挡下的条数：非零说明「服务端又给了一遍已经弹过的」，
    // 值得在 debug 里看见。
    let already_notified = fresh_ids.len().saturating_sub(notices.len());

    if selection.skipped > 0 {
        // 一轮来了太多：老的那几条不弹，在「提醒」页里看得到。
        tracing::warn!(
            fresh = items.len(),
            limit = MAX_NOTIFIED_PER_POLL,
            skipped = selection.skipped,
            "vrcc: new notices exceed per-poll toast limit; older ones not toasted"
        );
    }
    if already_notified > 0 {
        tracing::debug!(
            already_notified,
            "vrcc: skipped notices already notified in a previous round"
        );
    }

    let mut toast_attempted = 0usize;
    for (notice, action) in plan_notice_deliveries(&notices, owner.as_ref()) {
        // **必须带 action**，不只是为了点击跳转。
        //
        // `TauriDesktopNotifier::show` 有两条分支：
        // - action 为 Some → winrt（`Toast::icon(path, …)`，真能带图）
        // - action 为 None → tauri-plugin-notification，它调的是
        //   `notify_rust::Notification::icon()`，而那个方法在 Windows 上是
        //   空操作（Windows 需要的是 `image_path()`）。于是图标被静默吞掉。
        //
        // 实测证据：不带 action 时，系统通知库里的 toast XML 里没有
        // `<image>` 元素，尽管我们确实传了标的文件。
        //
        // 拿不到 owner（未登录）时退回 None：那时没有可跳转的目标，
        // 图标也就跟着没有——比弹出一个点了没反应的通知要好。
        dispatcher.emit_external_notice(external_notice_delivery(&notice), action, icon);
        // 只记 id 与长度，不记正文：提醒内容属于玩家数据。
        tracing::info!(
            notice_id = notice.id,
            body_len = notice.body.as_deref().map(str::len).unwrap_or(0),
            icon = icon.is_some(),
            "vrcc: notice handed to the notification dispatcher"
        );
        toast_attempted += 1;
    }

    PollOutcome {
        fresh: items.len(),
        toast_attempted,
        unread: None,
        persisted: 0,
        changed: !items.is_empty(),
    }
}

/// 把一条星遇同频提醒包装成活动投递对象。
///
/// ## 为什么用 `ActivityKind::External`
///
/// 这个 kind 存在的意义就是「来自 VRChat 之外的事件」，渲染时走
/// `literal_body`——标题正文原样照抄，不做任何键值替换。正好是本功能需要的
/// 语义，不必新增 kind，也不必给渲染层加分支。
///
/// 标题用「星遇同频」：那是对方的**服务名**（不是社区名，也不是 logo），
/// 只作文字归属使用，符合其《使用要求》对文字署名的允许。
///
/// ## 三个通道布尔值
///
/// - `desktop: true`：站外提醒的核心诉求，不该因为「VRChat 没开」就被丢掉；
/// - `vr: true`：戴着头显时也该看到（具体有没有画布由各 overlay 通道自己判断）；
/// - `tts: true`：交给偏好决定，`notification_tts` 为 `Never` 时不会念。
///
/// `webhook` / `widget` 保持 `false`：这两条通道不在当前功能范围内。
fn external_notice_delivery(
    notice: &NoticeToNotify,
) -> vrcx_0_application_activity::ActivityDelivery {
    use vrcx_0_application_activity::{
        ActivityActorRelation, ActivityCategory, ActivityContent, ActivityDelivery, ActivityEntry,
        ActivityText,
    };
    use vrcx_0_contracts::activity::ActivityKind;

    ActivityDelivery {
        entry: ActivityEntry {
            // 提醒 id 本身就是稳定的单调递增值，直接用。
            sequence: notice.id.unsigned_abs(),
            source_id: format!("vrcc:{}", notice.id),
            kind: ActivityKind::External,
            // 星遇同频的提醒不属于 VRChat 的任何分类，归到最中性的那一档。
            category: ActivityCategory::SystemSafety,
            created_at: String::new(),
            // 星遇同频的提醒没有 VRChat 用户/世界 ID，这三个字段留空。
            actor_user_id: String::new(),
            actor_display_name: String::new(),
            actor_relation: ActivityActorRelation::None,
            content: ActivityContent {
                icon: "system".to_string(),
                title: ActivityText::literal("星遇同频"),
                body: ActivityText::literal(notice.body.as_deref().unwrap_or_default()),
                // 图标是**已经缓存好的本地文件**，不是 URL。通知层只从
                // `image_url` 出发去解析（VRChat 资产或远程 URL），都不接受
                // 本地路径，所以这里留空；本地图标由 `icon_path` 那条旁路交给
                // toast（见 `emit_external_notice` 的 action + icon 处理）。
                image_url: String::new(),
                ..ActivityContent::default()
            },
        },
        desktop: true,
        vr: true,
        hmd: false,
        webhook: false,
        tts: true,
        widget: false,
    }
}

/// 把选中的提醒配上各自的点击落点。
///
/// 纯函数：不碰网络、不碰通知桥、不读时钟。它的存在价值是让「点了通知会
/// 跳到哪」可以脱离投递链单独验证——那是玩家能直接感知的一段。
///
/// 拿不到 owner（未登录）时 action 为 `None`：那时没有可跳转的会话，
/// 弹一个点了没反应的通知比没有图更糟。
fn plan_notice_deliveries(
    notices: &[NoticeToNotify],
    owner: Option<&vrcx_0_core::OwnerId>,
) -> Vec<(
    NoticeToNotify,
    Option<crate::notification::DesktopNotificationAction>,
)> {
    notices
        .iter()
        .map(|notice| {
            let action = owner.and_then(|owner| {
                crate::notification::DesktopNotificationAction::open_vrcc(
                    owner,
                    Some(&notice.id.to_string()),
                )
            });
            (notice.clone(), action)
        })
        .collect()
}

/// 每次轮询最多弹几条。
///
/// `me/notifications` 一次最多给 5 条，全弹出来会刷屏。
const MAX_NOTIFIED_PER_POLL: usize = 3;

/// 在 `0..jitter` 里取一个伪随机毫秒数。
///
/// 不用 `rand` crate：只要一个够散的抖动，混入当前时间纳秒即可，
/// 避免为这点需求引入依赖。
fn jitter_millis(jitter: &Duration) -> u64 {
    let span = jitter.as_millis().max(1) as u64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as u64)
        .unwrap_or(0);
    now % span
}

/// 通知图标的本地缓存状态。
///
/// **为什么下载而不是打包进仓库**：官方《使用要求》第 17 条把「星遇同频的
/// 图标」列为不可作为我方品牌使用，它是对方的美术资产。运行时取用并在本地
/// 缓存不构成再分发；顺带官方换图标时我们也能跟着变。
///
/// 失败**不阻断通知**：图标是锦上添花，不是发通知的前置条件。失败后下一轮
/// 会再试（而不是本会话放弃），因为「启动时正好没网」不该永久失去图标。
#[derive(Default)]
struct NoticeIcon {
    path: Option<std::path::PathBuf>,
}

impl NoticeIcon {
    /// 拿到图标的本地路径；已缓存则直接返回，不再联网。
    async fn resolve(&mut self, service: &VrccService) -> Option<&std::path::Path> {
        if self.path.is_some() {
            return self.path.as_deref();
        }
        match ensure_notice_icon(service).await {
            Some(path) => {
                self.path = Some(path);
                self.path.as_deref()
            }
            None => None,
        }
    }
}

/// 确保通知图标已缓存在本地，返回它的路径。
async fn ensure_notice_icon(service: &VrccService) -> Option<std::path::PathBuf> {
    let dir = vrcx_0_platform::app_paths::default_app_data_dir().ok()?;
    let target = dir.join(DesktopVrccRuntime::NOTICE_ICON_FILE);
    if target.is_file() {
        return Some(target);
    }

    let bytes = match service
        .fetch_site_asset(DesktopVrccRuntime::NOTICE_ICON_PATH)
        .await
    {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::debug!(%error, "vrcc: notice icon fetch failed; notifications will have no icon");
            return None;
        }
    };
    if bytes.is_empty() {
        tracing::debug!("vrcc: notice icon response was empty");
        return None;
    }

    // 先写临时文件再改名：避免下载中断留下半个 PNG —— 那会让后续每一轮
    // 都认为「已缓存」，却渲染不出图标。
    let temp = dir.join(format!("{}.tmp", DesktopVrccRuntime::NOTICE_ICON_FILE));
    if let Err(error) = std::fs::write(&temp, &bytes) {
        tracing::debug!(%error, "vrcc: notice icon write failed");
        return None;
    }
    if let Err(error) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        tracing::debug!(%error, "vrcc: notice icon rename failed");
        return None;
    }
    tracing::info!(
        bytes = bytes.len(),
        path = %target.display(),
        "vrcc: notice icon cached"
    );
    Some(target)
}

/// 一条准备好投递的通知（已经从契约类型里摘出来，便于测试构造）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct NoticeToNotify {
    id: i64,
    title: String,
    body: Option<String>,
}

/// `select_notices_to_notify` 的结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NoticeSelection {
    /// 本轮要弹的通知，已按 `limit` 截断，旧的在前。
    notices: Vec<NoticeToNotify>,
    /// 因超出 `limit` 而没弹的条数。
    ///
    /// 游标已经过去了，它们不会在下一轮再来，只能在「提醒」页里看到。
    /// 单独暴露而不是让调用方再数一遍：日志里要靠它说话。
    skipped: usize,
}

/// 决定这一轮该弹哪些通知。
///
/// 纯函数：不碰网络、不碰通知桥、不读时钟。
///
/// 规则：
/// - `items` 都是游标之后的新提醒（旧的在前），不用再判已读；
/// - 最多弹 `limit` 条，挑最新的那几条，按时间顺序弹，最新的落在最上面；
/// - 标题为空时用「星遇同频」，避免弹出一个没有标题的空通知。
fn select_notices_to_notify(
    items: &[vrcx_0_contracts::vrcc::VrccNotificationItem],
    limit: usize,
) -> NoticeSelection {
    let skipped = items.len().saturating_sub(limit);
    let notices = items[skipped..]
        .iter()
        .map(|item| NoticeToNotify {
            id: item.id,
            title: item
                .title
                .clone()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "星遇同频".to_string()),
            body: item.body.clone(),
        })
        .collect();
    NoticeSelection { notices, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrcx_0_contracts::vrcc::VrccNotificationItem;

    fn item(id: i64, title: Option<&str>, body: Option<&str>) -> VrccNotificationItem {
        VrccNotificationItem {
            id,
            title: title.map(str::to_string),
            body: body.map(str::to_string),
            at: Some(1_700_000_000_000),
        }
    }

    #[test]
    fn every_new_notice_is_selected_in_order() {
        let items = vec![item(41, Some("a"), None), item(42, Some("b"), None)];
        let selection = select_notices_to_notify(&items, 10);
        assert_eq!(
            selection.notices.iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![41, 42]
        );
        assert_eq!(selection.skipped, 0);
    }

    #[test]
    fn nothing_new_selects_nothing() {
        let selection = select_notices_to_notify(&[], 3);
        assert!(selection.notices.is_empty());
        assert_eq!(selection.skipped, 0);
    }

    #[test]
    fn over_the_limit_keeps_the_newest_and_counts_the_rest() {
        let items = (1..=5).map(|id| item(id, None, None)).collect::<Vec<_>>();
        let selection = select_notices_to_notify(&items, 3);
        // 弹最新的三条，按时间顺序。
        assert_eq!(
            selection.notices.iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(selection.skipped, 2);
    }

    #[test]
    fn blank_title_falls_back_so_no_notification_is_titleless() {
        let items = vec![item(1, Some("   "), Some("body"))];
        let selection = select_notices_to_notify(&items, 10);
        assert_eq!(selection.notices[0].title, "星遇同频");
    }

    #[test]
    fn keeps_the_body_verbatim() {
        let items = vec![item(1, Some("标题"), Some("正文内容"))];
        let selection = select_notices_to_notify(&items, 10);
        assert_eq!(selection.notices[0].body.as_deref(), Some("正文内容"));
    }

    #[test]
    fn jitter_stays_below_the_span() {
        // 抖动必须是 0..span，否则会在基准间隔之外多等一截。
        let span = Duration::from_secs(20);
        for _ in 0..200 {
            assert!(jitter_millis(&span) < 20_000);
        }
        // 极小 span 不能返回 0 以外的值（除零保护）。
        assert_eq!(jitter_millis(&Duration::from_millis(0)), 0);
    }

    // -- 通知投递：包装成活动对象、点哪里 --------------------------------

    /// 每条提醒都要包装成 `ActivityKind::External`。
    ///
    /// 这个 kind 的语义就是「来自 VRChat 之外的事件」，渲染时用
    /// `literal_body` 原样照抄正文。用别的 kind 会让界面套上不相干的
    /// 模板文案（例如「加入了你所在的世界」）。
    #[test]
    fn notices_are_wrapped_as_external_activity() {
        use vrcx_0_contracts::activity::ActivityKind;

        let delivery = external_notice_delivery(&NoticeToNotify {
            id: 42,
            title: "星遇同频".to_string(),
            body: Some("有人回复了你".to_string()),
        });

        assert_eq!(delivery.entry.kind, ActivityKind::External);
        assert_eq!(delivery.entry.source_id, "vrcc:42");
        // 桌面和 VR 都要发：站外提醒不该因为「VRChat 没开」被丢掉。
        assert!(delivery.desktop);
        assert!(delivery.vr);
        // 正文原样保留，不做任何替换。
        assert_eq!(
            delivery.entry.content.body,
            vrcx_0_application_activity::ActivityText::literal("有人回复了你")
        );
        // 标题是对方的**服务名**，只作文字归属。
        assert_eq!(
            delivery.entry.content.title,
            vrcx_0_application_activity::ActivityText::literal("星遇同频")
        );
        // 星遇同频的提醒没有 VRChat 用户/世界 ID，别伪造一个。
        assert!(delivery.entry.actor_user_id.is_empty());
        assert!(delivery.entry.actor_display_name.is_empty());
    }

    /// 正文缺失时不能崩，也不能塞进 `"None"` 这种字符串。
    #[test]
    fn a_notice_without_body_wraps_as_empty_text() {
        let delivery = external_notice_delivery(&NoticeToNotify {
            id: 7,
            title: "星遇同频".to_string(),
            body: None,
        });
        assert_eq!(
            delivery.entry.content.body,
            vrcx_0_application_activity::ActivityText::literal("")
        );
    }

    /// 有 owner 时每条都带「打开星遇同频」的落点。
    ///
    /// **这不只是点击跳转**：`TauriDesktopNotifier` 只有在 action 为 `Some`
    /// 时才走 winrt 那条分支，而只有那条分支会把图标真的写进 toast
    /// （`notify_rust::Notification::icon()` 在 Windows 上是空操作）。
    #[test]
    fn every_notice_gets_an_open_vrcc_action() {
        let owner = vrcx_0_core::OwnerId::new("usr_00000000-0000-0000-0000-000000000001");
        let notices = vec![
            NoticeToNotify {
                id: 1,
                title: "a".to_string(),
                body: None,
            },
            NoticeToNotify {
                id: 2,
                title: "b".to_string(),
                body: None,
            },
        ];
        let planned = plan_notice_deliveries(&notices, Some(&owner));
        assert_eq!(planned.len(), 2);
        for (notice, action) in &planned {
            let action = action.as_ref().expect("action must be present");
            assert_eq!(action.owner_user_id, owner);
            assert_eq!(
                action.target,
                crate::notification::DesktopNotificationTarget::OpenVrcc {
                    notice_id: Some(notice.id.to_string()),
                }
            );
        }
    }

    /// 没有 owner（未登录）时不给落点：点了也没地方去。
    #[test]
    fn no_owner_means_no_action() {
        let notices = vec![NoticeToNotify {
            id: 1,
            title: "a".to_string(),
            body: None,
        }];
        let planned = plan_notice_deliveries(&notices, None);
        assert_eq!(planned.len(), 1);
        assert!(planned[0].1.is_none());
    }

    // -- 跨轮次行为：轮流跑多遍会发生什么 --------------------------------

    /// 同一条提醒在跨轮次之间只该被选中一次。
    ///
    /// 服务端游标长期不动时，只靠它判断会让同一条提醒每轮重弹一次——实测
    /// 出现过几十次。这里钉住的是本地那一层去重。
    #[test]
    fn a_notice_is_selected_once_across_rounds() {
        let notified = NotifiedIds::default();
        let items = vec![item(1, Some("t"), Some("b"))];

        let first: Vec<_> = notified
            .take_new(
                select_notices_to_notify(&items, MAX_NOTIFIED_PER_POLL)
                    .notices
                    .iter()
                    .map(|n| n.id),
            )
            .into_iter()
            .collect();
        assert_eq!(first, vec![1]);

        // 服务端又给了一遍同一条。
        let second: Vec<_> = notified
            .take_new(
                select_notices_to_notify(&items, MAX_NOTIFIED_PER_POLL)
                    .notices
                    .iter()
                    .map(|n| n.id),
            )
            .into_iter()
            .collect();
        assert!(second.is_empty(), "同一提醒不该弹第二次");
    }

    // -- event_to_emit：要不要通知前端 ------------------------------------

    #[test]
    fn changed_outcome_emits_the_notices_event() {
        let outcome = PollOutcome {
            fresh: 1,
            toast_attempted: 1,
            unread: Some(3),
            changed: true,
            ..PollOutcome::default()
        };
        assert_eq!(outcome.event_to_emit(), Some(VRCC_NOTICES_CHANGED_EVENT));
    }

    #[test]
    fn unchanged_outcome_emits_nothing() {
        // 没有变化时绝不能发事件：否则前端每轮都白刷新一次。
        let outcome = PollOutcome::default();
        assert_eq!(outcome.event_to_emit(), None);
    }

    /// 事件名必须和前端订阅的一致，否则前端永远收不到。
    ///
    /// 这条钉的是跨语言契约：Rust 侧常量改名而前端没跟上时，编译和类型检查
    /// 都不会报错，只会表现为「界面不刷新」，极难定位。
    #[test]
    fn event_names_match_the_frontend_contract() {
        assert_eq!(VRCC_NOTICES_CHANGED_EVENT, "vrccNoticesChanged");
        assert_eq!(VRCC_CONNECTION_CHANGED_EVENT, "vrccConnectionChanged");
    }

    // -- 通知中心那一行 ---------------------------------------------------

    fn notice_at(
        id: i64,
        title: Option<&str>,
        body: Option<&str>,
        at: Option<i64>,
    ) -> VrccNotificationItem {
        VrccNotificationItem {
            id,
            title: title.map(str::to_string),
            body: body.map(str::to_string),
            at,
        }
    }

    /// 行 id 必须能解析回提醒编号：点通知时要靠它定位到具体一条。
    #[test]
    fn a_row_is_addressed_by_the_notice_number() {
        let row = notice_row(
            &notice_at(42, Some("t"), None, None),
            "2026-01-01T00:00:00.000Z",
        );
        let id = row["id"].as_str().unwrap();
        assert_eq!(id, "vrcc_42");
        assert_eq!(
            id.strip_prefix(DesktopVrccRuntime::NOTICE_ROW_ID_PREFIX)
                .unwrap()
                .parse::<i64>()
                .unwrap(),
            42
        );
    }

    /// 落库时必须还是未读，否则红点永远不会出现。
    #[test]
    fn a_row_stays_unread_and_never_expires() {
        let row = notice_row(
            &notice_at(1, Some("t"), Some("b"), None),
            "2026-01-01T00:00:00.000Z",
        );
        assert_eq!(row["seen"], json!(false));
        // 空串 = 不过期。写成日期会让历史行在几天后从列表里消失。
        assert_eq!(row["expiresAt"], json!(""));
    }

    /// 点击落点写在 `link` 上：提醒没有 VRChat 用户可跳，`senderUserId` 留空。
    #[test]
    fn a_row_carries_the_link_the_frontend_dispatches_on() {
        let row = notice_row(&notice_at(7, None, None, None), "2026-01-01T00:00:00.000Z");
        assert_eq!(row["link"], json!("vrcc:notifications"));
        assert_eq!(row["type"], json!("vrcc.notice"));
        assert_eq!(row["senderUserId"], json!(""));
    }

    /// `at` 是毫秒时间戳，列里要的是 RFC3339。
    #[test]
    fn a_row_renders_the_timestamp_as_rfc3339() {
        let row = notice_row(
            &notice_at(1, None, Some("b"), Some(1_700_000_000_000)),
            "2026-01-01T00:00:00.000Z",
        );
        assert_eq!(row["createdAt"], json!("2023-11-14T22:13:20.000Z"));
        assert_eq!(row["updatedAt"], row["createdAt"]);
    }

    /// 提醒没带 `at` 时退回调用方给的时间，而不是写一个空 `created_at`
    /// ——空值会让这一行在排序里垫底。
    #[test]
    fn a_row_without_a_timestamp_falls_back() {
        let row = notice_row(&notice_at(1, None, None, None), "2026-01-01T00:00:00.000Z");
        assert_eq!(row["createdAt"], json!("2026-01-01T00:00:00.000Z"));
    }

    /// 正文一栏只放正文，标题单独走 `title` 列。
    ///
    /// 抽屉行会把 `title` 与 `message` 再拼一次；早先把标题拼进正文、又让
    /// `title` 与发送者同名，于是同一条提醒在抽屉里显示成「星遇同频 星遇同频
    /// …」。这条测试钉住「两者不重复」。
    #[test]
    fn a_row_keeps_the_title_and_the_body_apart() {
        let both = notice_row(&notice_at(1, Some("标题"), Some("正文"), None), "");
        // 正文只有正文，不含标题。
        assert_eq!(both["message"], json!("正文"));
        assert_eq!(both["title"], json!(DesktopVrccRuntime::NOTICE_ROW_SENDER));

        let title_only = notice_row(&notice_at(1, Some("标题"), None, None), "");
        assert_eq!(title_only["message"], json!("标题"));

        let body_only = notice_row(&notice_at(1, None, Some("正文"), None), "");
        assert_eq!(body_only["message"], json!("正文"));

        // 都为空时至少不要留下 " " 这种只有空白的一栏。
        let neither = notice_row(&notice_at(1, Some("  "), None, None), "");
        assert_eq!(neither["message"], json!(""));
    }

    /// 标题与正文不会在抽屉里重复：`getNotificationMessage` 拼的是
    /// `title + message`，所以两者不能有相同内容。
    #[test]
    fn a_row_does_not_repeat_the_sender_in_the_body() {
        let row = notice_row(
            &notice_at(1, Some("星遇同频"), Some("有人连向了你"), None),
            "",
        );
        let title = row["title"].as_str().unwrap_or_default();
        let message = row["message"].as_str().unwrap_or_default();
        assert_eq!(title, DesktopVrccRuntime::NOTICE_ROW_SENDER);
        assert_eq!(message, "有人连向了你");
        // 抽屉拼起来是「星遇同频 有人连向了你」，服务名只出现一次。
        assert!(!message.contains(title));
    }
}
