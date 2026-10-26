//! 星遇同频运行时：把应用层服务挂到桌面运行时上。

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use vrcx_0_application::vrcc::{StoredCredential, VrccService, VrccTokenStore};
use vrcx_0_contracts::vrcc::{
    VrccBoard, VrccConnectionSnapshot, VrccRecentNotifications, VrccStatus, VrccToday,
    VrccUsageSnapshot, VrccWristSummary,
};
use vrcx_0_integrations::vrcc::resolve_client_id;
use vrcx_0_outbound_adapters::LocalVrccTokenStore;

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
        }))
    }

    fn save(&self, credential: &StoredCredential) -> vrcx_0_application_core::Result<()> {
        self.inner
            .save(&vrcx_0_outbound_adapters::StoredVrccCredential {
                token: credential.token.clone(),
                star: credential.star,
                vrc_name: credential.vrc_name.clone(),
                scope: credential.scope.clone(),
            })
    }

    fn clear(&self) -> vrcx_0_application_core::Result<()> {
        self.inner.clear()
    }
}

/// 星遇同频提醒发生变化时发给前端的事件名。
///
/// 只作为「该刷新了」的信号，不带正文——玩家内容没必要经手额外的事件层。
pub const VRCC_NOTICES_CHANGED_EVENT: &str = "vrccNoticesChanged";

/// 桌面侧的星遇同频运行时。
pub struct DesktopVrccRuntime {
    service: Arc<VrccService>,
}

impl DesktopVrccRuntime {
    /// 后台轮询的基准间隔。
    ///
    /// 官方建议 60 秒，并明确 `/me/notifications` 有 25 秒的最小间隔。
    /// 取 60 是照官方建议，不是我们保守。
    const BACKGROUND_POLL_BASE: Duration = Duration::from_secs(60);
    /// 抖动上限，避免所有客户端撞在同一秒（官方脚本也这样做）。
    const BACKGROUND_POLL_JITTER: Duration = Duration::from_secs(20);

    pub fn new(store: LocalVrccTokenStore) -> Self {
        Self::with_client_override(store, None, None)
    }

    /// 用指定的 `X-Client` 标识与代理构造。
    ///
    /// `client_override` 来自配置：正式态把平台发的 `app_…` 填进去即可，
    /// 不用重新编译。`None` 时按 `resolve_client_id` 的优先级回落到常量。
    ///
    /// `proxy_url` 必须传宿主应用的统一代理（`runtime_host().proxy_url()`）：
    /// 本机实测 `vrcc.cc` 直连 DNS 超时，不接代理时所有接口都会「暂时不可用」。
    pub fn with_client_override(
        store: LocalVrccTokenStore,
        client_override: Option<&str>,
        proxy_url: Option<&str>,
    ) -> Self {
        let usage_store = Arc::new(vrcx_0_outbound_adapters::LocalVrccUsageStore::new(
            Arc::clone(store.db()),
        ));
        let port = Arc::new(TokenStoreAdapter { inner: store });
        let client_id = resolve_client_id(client_override);
        Self {
            service: Arc::new(VrccService::with_usage_store(
                port,
                client_id,
                proxy_url.map(str::to_string),
                Some(usage_store),
            )),
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

    pub fn client_id(&self) -> &str {
        self.service.client_id()
    }

    /// 地图榜：开放数据，不需要连接。
    pub async fn boards(&self, limit: u32) -> Result<VrccBoard, String> {
        self.service
            .boards(limit)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn status(&self) -> Result<VrccStatus, String> {
        self.service
            .status()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn begin_device_auth(
        &self,
        scope: Option<String>,
    ) -> Result<vrcx_0_application::vrcc::VrccDeviceAuthPrompt, String> {
        self.service
            .begin_device_auth(scope.as_deref())
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn poll_device_auth(
        &self,
        prompt: &vrcx_0_application::vrcc::VrccDeviceAuthPrompt,
    ) -> Result<vrcx_0_application::vrcc::VrccDeviceAuthStep, String> {
        self.service
            .poll_device_auth(prompt)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        self.service
            .disconnect()
            .await
            .map_err(|error| error.to_string())
    }

    /// 最近的提醒。`mark` 为 `true` 时同时把已读状态同步到账号（跨设备）。
    pub async fn recent_notifications(
        &self,
        mark: bool,
    ) -> Result<VrccRecentNotifications, String> {
        self.service
            .recent_notifications(mark)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn poll_notifications(
        &self,
    ) -> Result<vrcx_0_contracts::vrcc::VrccNotifications, String> {
        self.service
            .poll_notifications()
            .await
            .map_err(|error| error.to_string())
    }

    /// 当前所在地图在地图榜上的状态。`off` 的地图返回 `None`（不给看）。
    pub async fn my_map(
        &self,
        world: &str,
    ) -> Result<Option<vrcx_0_contracts::vrcc::VrccMyMap>, String> {
        self.service
            .my_map(world)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn today(&self) -> Result<VrccToday, String> {
        self.service
            .today()
            .await
            .map_err(|error| error.to_string())
    }

    /// 「互相连上」的星友列表。需要 `mutuals` 敏感权限。
    pub async fn mutuals(&self) -> Result<vrcx_0_contracts::vrcc::VrccMutuals, String> {
        self.service
            .mutuals()
            .await
            .map_err(|error| error.to_string())
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
    /// `event_sink` 用来告诉前端「有新数据了」。前端不再自己轮询——双轨会让
    /// 两边各拉一次、`mark` 语义互相打架，而且隐藏窗口时前端定时器会被节流。
    pub fn spawn_background_poll(
        &self,
        supervisor: &vrcx_0_application_core::TaskSupervisor,
        notifier: Arc<dyn crate::notification::DesktopNotifier>,
        event_sink: Arc<dyn vrcx_0_application_core::RuntimeEventSink>,
    ) -> vrcx_0_application_core::TaskSpawnOutcome {
        let service = Arc::clone(&self.service);
        let base = Self::BACKGROUND_POLL_BASE;
        let jitter = Self::BACKGROUND_POLL_JITTER;

        supervisor.spawn_cancellable(move |stop_token| {
            let service = Arc::clone(&service);
            let notifier = Arc::clone(&notifier);
            let event_sink = Arc::clone(&event_sink);
            async move {
                loop {
                    if stop_token.is_stop_requested() {
                        return;
                    }
                    // 未连接时不必轮询：`recent_notifications` 会直接报错。
                    if service.snapshot().connected {
                        match poll_once(&service, notifier.as_ref()).await {
                            Ok(changed) => {
                                if changed {
                                    // 只发一个「变了」的信号，具体数据前端自己拉。
                                    // 不在事件里带提醒正文：那会把玩家内容写进
                                    // 事件经手的所有层，多一条泄露面。
                                    event_sink.emit(VRCC_NOTICES_CHANGED_EVENT, json!({}));
                                }
                            }
                            Err(error) => {
                                // 不把错误抛出去：一次失败不该杀掉整个轮询。
                                tracing::debug!(%error, "vrcc: background poll failed");
                            }
                        }
                    }
                    // 用带抖动的固定间隔。这里刻意用 `sleep` 而不是绝对时刻对齐：
                    // 唤醒后最多补一次，不会像"补 N 个周期"那样突发请求。
                    let delay = base + Duration::from_millis(jitter_millis(&jitter));
                    tokio::time::sleep(delay).await;
                }
            }
        })
    }
}

/// 一次轮询：拉最近提醒，把「还没弹过」的发成系统通知。
///
/// 返回 `true` 表示「有新提醒出现」，调用方据此通知前端刷新。
///
/// **未读的判定**：`recent` 里的 `read` 是玩家最后已读的提醒 id（顶层游标），
/// 不是每条条目上的标记。所以 `item.id > read` 才算未读——和服务端语义一致，
/// 也解释了为什么 `read` 为 `None` 时所有条目都算未读。
async fn poll_once(
    service: &VrccService,
    notifier: &dyn crate::notification::DesktopNotifier,
) -> Result<bool, String> {
    let recent = service
        .recent_notifications(false)
        .await
        .map_err(|error| error.to_string())?;

    // 没有游标 = 全都没读过。
    let read_cursor = recent.read.unwrap_or(0);

    let mut notified = 0usize;
    for item in recent.items.iter() {
        // 只弹未读：已读说明玩家在网页或手机上已经看过，再弹是打扰。
        if item.id <= read_cursor {
            continue;
        }
        if notified < MAX_NOTIFIED_PER_POLL {
            let title = item.title.clone().unwrap_or_else(|| "星遇同频".to_string());
            let body = item.body.clone();
            if let Err(error) = notifier.show(&title, body.as_deref(), None, true, None) {
                // Headless 或未挂载通知桥时走到这里，记录即可。
                tracing::debug!(%error, "vrcc: desktop notification not delivered");
            }
            notified += 1;
        }
    }
    // 有未读就值得让前端刷新（哪怕因为限流没全部弹成通知）。
    Ok(notified > 0 || recent.items.iter().any(|item| item.id > read_cursor))
}

/// 每次轮询最多弹几条。
///
/// 首次连接时可能一次性拉到很多历史提醒，全弹出来会刷屏。
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
