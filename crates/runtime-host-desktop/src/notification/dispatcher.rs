use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use vrcx_0_application_activity::notification::{
    extract_file_version, fallback_file_version, render_delivery, NotificationResolver,
    OverlayLocale, RenderedNotification,
};
use vrcx_0_application_activity::{ActivityDelivery, ActivitySink, ActivitySnapshot};
use vrcx_0_application_core::{ImageCache, RuntimeAuthScope, TaskSupervisor};
use vrcx_0_host_desktop::tts::TtsEngine;
use vrcx_0_persistence::DatabaseService;

use super::desktop::{send_desktop_notification, DesktopNotificationAction, DesktopNotifier};
use super::icon_cache::{IconRequest, RemoteIconCache};
use super::overlay_transport::OverlayNotificationTransport;
use super::policy::LocalNotificationPolicy;
use super::tts::{notification_tts_memo_actor_user_id, send_tts_notification};
use super::NotificationDeliveryCondition;
use super::{NotificationDeliveryPlan, NotificationDeliveryPreferences};
use vrcx_0_contracts::activity::ActivityKind;
use vrcx_0_core::files::extract_file_id;
use vrcx_0_core::location::is_meaningful_world_name;
use vrcx_0_core::OwnerId;

const NOTIFICATION_IMAGE_FIRST_SEND_BUDGET: Duration = Duration::from_secs(1);
const NOTIFICATION_WORLD_NAME_FIRST_SEND_BUDGET: Duration = Duration::from_secs(2);

pub struct NotificationDispatcher {
    auth_scope: RuntimeAuthScope,
    image_cache: Arc<ImageCache>,
    resolver: Arc<NotificationResolver>,
    output: Arc<NotificationOutputContext>,
    completion_tx: mpsc::UnboundedSender<NotificationCompletion>,
    next_sequence: AtomicU64,
    tasks: TaskSupervisor,
}

pub(crate) struct NotificationDispatcherDeps {
    pub auth_scope: RuntimeAuthScope,
    pub db: Arc<DatabaseService>,
    pub image_cache: Arc<ImageCache>,
    pub resolver: Arc<NotificationResolver>,
    pub desktop: Arc<dyn DesktopNotifier>,
    pub tts: Arc<dyn TtsEngine>,
    pub tasks: TaskSupervisor,
    pub policy: Arc<LocalNotificationPolicy>,
    /// 宿主统一代理，给外部图标用。
    pub proxy_url: Option<String>,
}

struct NotificationOutputContext {
    overlay_transport: OverlayNotificationTransport,
    db: Arc<DatabaseService>,
    desktop: Arc<dyn DesktopNotifier>,
    tts: Arc<dyn TtsEngine>,
    policy: Arc<LocalNotificationPolicy>,
    /// 宿主统一代理。外部图标（非 VRChat 资产）靠它才能取到。
    proxy_url: Option<String>,
}

struct NotificationJob {
    delivery: ActivityDelivery,
    preferences: NotificationDeliveryPreferences,
    plan: NotificationDeliveryPlan,
    locale: OverlayLocale,
    endpoint: String,
    current_user_id: String,
    /// 宿主统一代理；外部图标需要它才能取到。
    proxy_url: Option<String>,
    /// 点击落点覆盖。
    ///
    /// `ActivityDelivery` 里没有「点这条通知去哪」的字段——主路径是按
    /// `ActivityKind` 推断的（群公告→群主页，其余→用户主页）。站外提醒的
    /// 落点是它自己的（打开星遇同频页签），推断不出来，所以由投递方给。
    action_override: Option<DesktopNotificationAction>,
    /// 已经在本地的图标路径，直接用作桌面通知的图。
    ///
    /// **为什么需要旁路**：通知层解析图片的入口是
    /// [`resolve_local_image_with_budget`]，它只认「VRChat `file_` 资产」和
    /// 「远程 http(s) URL」两种形态，**不接受本地路径**。星遇同频的图标是
    /// 投递方自己下载并缓存的本地文件，走那条路会被当成非法输入丢掉。
    /// 有了这个字段，投递方可以直接把「已经准备好的图」交进来，不必假装
    /// 它是一个 URL 再让下游去解析一遍。
    local_icon: Option<std::path::PathBuf>,
}

struct PreparedNotification {
    delivery: ActivityDelivery,
    preferences: NotificationDeliveryPreferences,
    plan: NotificationDeliveryPlan,
    render: RenderedNotification,
    locale: OverlayLocale,
    desktop_image: Option<String>,
    overlay_image: Option<String>,
    desktop_action: Option<DesktopNotificationAction>,
}

enum NotificationCompletion {
    Ready {
        sequence: u64,
        notification: Box<PreparedNotification>,
    },
    Skip {
        sequence: u64,
    },
}

struct OrderedDeliveryBuffer<T> {
    next_sequence: u64,
    pending: BTreeMap<u64, Option<T>>,
}

impl<T> OrderedDeliveryBuffer<T> {
    fn new(next_sequence: u64) -> Self {
        Self {
            next_sequence,
            pending: BTreeMap::new(),
        }
    }

    fn push(&mut self, sequence: u64, value: Option<T>) -> Vec<T> {
        if sequence < self.next_sequence {
            return Vec::new();
        }
        self.pending.insert(sequence, value);
        let mut ready = Vec::new();
        while let Some(value) = self.pending.remove(&self.next_sequence) {
            self.next_sequence = self.next_sequence.saturating_add(1);
            if let Some(value) = value {
                ready.push(value);
            }
        }
        ready
    }
}

impl NotificationDispatcher {
    pub(crate) fn new(deps: NotificationDispatcherDeps) -> Self {
        let output = Arc::new(NotificationOutputContext {
            overlay_transport: OverlayNotificationTransport::new(),
            db: deps.db,
            desktop: deps.desktop,
            tts: deps.tts,
            policy: deps.policy,
            proxy_url: deps.proxy_url,
        });
        let (completion_tx, completion_rx) = mpsc::unbounded_channel();
        let worker_output = Arc::clone(&output);
        deps.tasks.spawn(async move {
            run_ordered_output(completion_rx, worker_output).await;
        });
        Self {
            auth_scope: deps.auth_scope,
            image_cache: deps.image_cache,
            resolver: deps.resolver,
            output,
            completion_tx,
            next_sequence: AtomicU64::new(0),
            tasks: deps.tasks,
        }
    }
}

impl ActivitySink for NotificationDispatcher {
    fn emit_overlay_activity_snapshot(&self, _snapshot: ActivitySnapshot) {}

    fn emit_overlay_activity_delivery(&self, delivery: ActivityDelivery) {
        let settings = self.output.policy.settings();
        let plan = self.output.policy.plan(&delivery, &settings);
        if !plan.has_local_transport() {
            return;
        }
        let preferences = settings.preferences.clone();
        let locale = settings.locale;
        let (endpoint, current_user_id) = notification_session_identity(&self.auth_scope);
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let priority = delivery.entry.kind == ActivityKind::OnPlayerJoining;
        let job = NotificationJob {
            delivery,
            preferences,
            plan,
            locale,
            endpoint,
            current_user_id,
            proxy_url: self.output.proxy_url.clone(),
            action_override: None,
            local_icon: None,
        };

        if priority {
            let render = render_delivery(
                &job.delivery,
                job.locale,
                job.preferences.show_instance_id_in_location,
            );
            let prepared = prepare_rendered_notification(job, render, None, None);
            dispatch_prepared_notification(&prepared, self.output.as_ref());
            let _ = self
                .completion_tx
                .send(NotificationCompletion::Skip { sequence });
            return;
        }

        let image_cache = Arc::clone(&self.image_cache);
        let resolver = Arc::clone(&self.resolver);
        let tasks = self.tasks.clone();
        let completion_tx = self.completion_tx.clone();
        self.tasks.spawn(async move {
            let notification = prepare_notification(job, image_cache, resolver, tasks).await;
            let _ = completion_tx.send(NotificationCompletion::Ready {
                sequence,
                notification: Box::new(notification),
            });
        });
    }
}

impl NotificationDispatcher {
    /// 投递一条**站外**通知（例如星遇同频的提醒）。
    ///
    /// ## 为什么不走 `emit_overlay_activity_delivery`
    ///
    /// 那条路按「VRChat 游戏状态」决定通道（`decide_notification_plan`）：
    /// 桌面通知的默认条件是 `Never`、浮层要求 `is_steamvr_running`。那套判断
    /// 对 VRChat 活动是对的——你人在游戏里才关心谁加入了——但站外提醒不是这个
    /// 性质：它更像社交平台的动态提醒，关掉游戏照样该收到。
    ///
    /// 所以这里**只用暂停策略与通道偏好**，不碰游戏状态：
    ///
    /// - `policy.paused()` 照常生效：免打扰、隐私锁、忙碌时不打扰；
    /// - 桌面通知只要玩家没关（`desktop_toast` 不是 `Never`）就发；
    /// - 浮层不要求 `is_steamvr_running`，有没有头显由各个 overlay 通道自己判断。
    ///
    /// 除此之外——图标解析、TTS、有序投递、代理——全部复用主路径那一套，
    /// 不再有第二条投递实现。
    pub fn emit_external_notice(
        &self,
        delivery: ActivityDelivery,
        action: Option<DesktopNotificationAction>,
        local_icon: Option<&std::path::Path>,
    ) {
        let settings = self.output.policy.settings();
        // 暂停时一条都不发，和主路径一致。
        if self.output.policy.paused(&settings) {
            return;
        }
        let plan = external_notice_plan(&delivery, &settings.preferences);
        if !plan.has_local_transport() {
            return;
        }
        let preferences = settings.preferences.clone();
        let locale = settings.locale;
        let (endpoint, current_user_id) = notification_session_identity(&self.auth_scope);
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let job = NotificationJob {
            delivery,
            preferences,
            plan,
            locale,
            endpoint,
            current_user_id,
            proxy_url: self.output.proxy_url.clone(),
            action_override: action,
            local_icon: local_icon.map(std::path::Path::to_path_buf),
        };
        let image_cache = Arc::clone(&self.image_cache);
        let resolver = Arc::clone(&self.resolver);
        let tasks = self.tasks.clone();
        let completion_tx = self.completion_tx.clone();
        self.tasks.spawn(async move {
            let notification = prepare_notification(job, image_cache, resolver, tasks).await;
            let _ = completion_tx.send(NotificationCompletion::Ready {
                sequence,
                notification: Box::new(notification),
            });
        });
    }
}

/// 把 dispatcher 注册进 `ActivitySinkRegistry` 用的薄壳。
///
/// **为什么需要它**：dispatcher 有两个用途——注册成 `ActivitySink` 接 VRChat
/// 活动，以及被 vrcc 直接调用来投递站外提醒。`ActivitySinkRegistry::add` 收的
/// 是 `Arc<dyn ActivitySink>`，而 Rust 的孤儿规则不允许给 `Arc<NotificationDispatcher>`
/// 这种外部泛型实现外部 trait，所以套一层本地类型。
///
/// 这个壳**不复制** dispatcher：里面是同一个 `Arc`，所以有序投递的序列号和
/// 状态只有一份。
pub struct NotificationSink(Arc<NotificationDispatcher>);

impl NotificationSink {
    pub(crate) fn new(dispatcher: Arc<NotificationDispatcher>) -> Self {
        Self(dispatcher)
    }
}

impl ActivitySink for NotificationSink {
    fn emit_overlay_activity_snapshot(&self, snapshot: ActivitySnapshot) {
        self.0.as_ref().emit_overlay_activity_snapshot(snapshot);
    }

    fn emit_overlay_activity_delivery(&self, delivery: ActivityDelivery) {
        self.0.as_ref().emit_overlay_activity_delivery(delivery);
    }
}

async fn run_ordered_output(
    mut completion_rx: mpsc::UnboundedReceiver<NotificationCompletion>,
    output: Arc<NotificationOutputContext>,
) {
    let mut buffer = OrderedDeliveryBuffer::new(0);
    while let Some(completion) = completion_rx.recv().await {
        let (sequence, notification) = match completion {
            NotificationCompletion::Ready {
                sequence,
                notification,
            } => (sequence, Some(notification)),
            NotificationCompletion::Skip { sequence } => (sequence, None),
        };
        for notification in buffer.push(sequence, notification) {
            dispatch_prepared_notification(&notification, output.as_ref());
        }
    }
}

/// 站外提醒的通道计划。
///
/// 与 [`decide_notification_plan`] 的根本差别：**不读游戏状态**。
///
/// 那套判断是给 VRChat 活动设计的——你人没在游戏里就不必提示谁加入了——
/// 而站外提醒（星遇同频的私聊、好友请求、星轨相交）不该因为「VRChat 没开着」
/// 就被丢掉。玩家可能整天不开游戏，仍然是那个社交平台上活跃的一员。
///
/// 所以这里只响应「玩家愿不愿意收」，不看「游戏在不在跑」：
///
/// - 桌面：投递方说发就发。**不**套 `desktop_toast` 的条件值——它默认是
///   `Never`（为 VRChat 活动准备的默认值），照读会让站外提醒默认全没，
///   而那正是要避免的「功能静默消失」。
/// - 浮层：不要求 `is_steamvr_running`，也不套 `overlay_toast` 的
///   （默认 `GameRunning`）游戏条件。有没有画布由各 overlay 通道自己判断：
///   XSOverlay / OVR Toolkit 没在跑时那些调用本来就不显示，不会出错。
/// - TTS：跟随 `notification_tts`。它默认 `Never`，但这里的含义明确——
///   不要念出来，尊重它。
fn external_notice_plan(
    delivery: &ActivityDelivery,
    preferences: &NotificationDeliveryPreferences,
) -> NotificationDeliveryPlan {
    let xs = delivery.vr && preferences.xs_notifications;
    let ovrt_hud = cfg!(windows) && delivery.vr && preferences.ovrt_hud_notifications;
    let ovrt_wrist = cfg!(windows) && delivery.vr && preferences.ovrt_wrist_notifications;
    let ovrt = ovrt_hud || ovrt_wrist;
    let tts = delivery.tts
        && !matches!(
            preferences.notification_tts,
            NotificationDeliveryCondition::Never
        );

    NotificationDeliveryPlan {
        desktop: delivery.desktop,
        xs,
        ovrt,
        ovrt_hud,
        ovrt_wrist,
        tts,
    }
}

async fn prepare_notification(
    mut job: NotificationJob,
    image_cache: Arc<ImageCache>,
    resolver: Arc<NotificationResolver>,
    tasks: TaskSupervisor,
) -> PreparedNotification {
    let friend_image =
        if job.plan.desktop_image(&job.preferences) || job.plan.overlay_image(&job.preferences) {
            friend_actor_image(&resolver, &job)
        } else {
            None
        };
    if let Some((world_name, display_location)) =
        resolve_world_name_with_budget(&tasks, Arc::clone(&resolver), &job).await
    {
        job.delivery.entry.content.world_name = world_name;
        if !display_location.trim().is_empty() {
            job.delivery.entry.content.display_location = display_location;
        }
    }
    let render = render_delivery(
        &job.delivery,
        job.locale,
        job.preferences.show_instance_id_in_location,
    );
    let (desktop_url, overlay_url) =
        notification_image_urls(job.plan, &job.preferences, friend_image, &render.image_url);
    let proxy = job.proxy_url.clone();
    // 投递方已经备好本地图（站外提醒走这条）：直接用，不再走 URL 解析。
    let pre_resolved = job
        .local_icon
        .as_ref()
        .filter(|path| path.is_file())
        .and_then(|path| path.to_str())
        .map(str::to_string);
    let desktop_image = match (&pre_resolved, &desktop_url) {
        (Some(path), _) => Some(path.clone()),
        (None, Some(url)) => {
            resolve_local_image_with_budget(&tasks, Arc::clone(&image_cache), url, proxy.as_deref())
                .await
        }
        (None, None) => None,
    };
    let overlay_image = match &overlay_url {
        Some(_) if overlay_url == desktop_url => desktop_image.clone(),
        Some(url) => {
            resolve_local_image_with_budget(&tasks, image_cache, url, proxy.as_deref()).await
        }
        None => None,
    };
    prepare_rendered_notification(job, render, desktop_image, overlay_image)
}

fn notification_image_urls(
    plan: NotificationDeliveryPlan,
    preferences: &NotificationDeliveryPreferences,
    friend_image: Option<String>,
    notification_image: &str,
) -> (Option<String>, Option<String>) {
    let notification_image = notification_image.trim();
    let desktop = plan
        .desktop_image(preferences)
        .then(|| friend_image.clone())
        .flatten();
    let overlay = plan
        .overlay_image(preferences)
        .then(|| {
            friend_image.or_else(|| {
                (!notification_image.is_empty()).then(|| notification_image.to_string())
            })
        })
        .flatten();
    (desktop, overlay)
}

fn prepare_rendered_notification(
    job: NotificationJob,
    render: RenderedNotification,
    desktop_image: Option<String>,
    overlay_image: Option<String>,
) -> PreparedNotification {
    let owner_user_id = OwnerId::new(job.current_user_id);
    let desktop_action = match job.action_override {
        // 投递方明确指定了落点，就用它的（站外提醒走这条）。
        Some(action) => Some(action),
        None if job.delivery.entry.kind == ActivityKind::GroupInstanceOpened => {
            DesktopNotificationAction::open_group_profile(
                &owner_user_id,
                &job.delivery.entry.content.group_id,
            )
        }
        None => DesktopNotificationAction::open_user_profile(
            &owner_user_id,
            &job.delivery.entry.actor_user_id,
        ),
    };
    PreparedNotification {
        delivery: job.delivery,
        preferences: job.preferences,
        plan: job.plan,
        render,
        locale: job.locale,
        desktop_image,
        overlay_image,
        desktop_action,
    }
}

fn dispatch_prepared_notification(
    notification: &PreparedNotification,
    output: &NotificationOutputContext,
) {
    if output.policy.paused(&output.policy.settings()) {
        return;
    }
    let plan = notification.plan;
    if plan.tts {
        let user_memo = notification_tts_memo_actor_user_id(
            &notification.delivery,
            &notification.render,
            &notification.preferences,
            notification.locale,
        )
        .and_then(|actor_user_id| load_user_memo(output.db.as_ref(), actor_user_id));
        send_tts_notification(
            output.tts.as_ref(),
            &notification.delivery,
            &notification.render,
            &notification.preferences,
            notification.locale,
            user_memo.as_deref(),
        );
    }
    if plan.desktop {
        send_desktop_notification(
            output.desktop.as_ref(),
            &notification.render,
            notification.delivery.entry.kind,
            &notification.delivery.entry.content.group_name,
            &notification.preferences,
            notification.desktop_image.as_deref(),
            notification.desktop_action.as_ref(),
        );
    }
    output.overlay_transport.send(
        plan,
        &notification.render,
        &notification.preferences,
        notification.overlay_image.as_deref(),
    );
}

fn load_user_memo(db: &DatabaseService, actor_user_id: &str) -> Option<String> {
    let actor_user_id = actor_user_id.trim();
    if actor_user_id.is_empty() {
        return None;
    }
    match vrcx_0_persistence::memos::memo_get_user(db, actor_user_id.to_string()) {
        Ok(Some(memo)) => Some(memo.memo),
        Ok(None) => None,
        Err(error) => {
            tracing::debug!("failed to load TTS nickname memo: {error}");
            None
        }
    }
}

fn image_actor_user_id<'a>(
    delivery: &'a ActivityDelivery,
    current_user_id: &str,
) -> Option<&'a str> {
    if !delivery.entry.content.image_url.trim().is_empty() {
        return None;
    }
    let actor_user_id = delivery.entry.actor_user_id.trim();
    (actor_user_id.starts_with("usr_") && actor_user_id != current_user_id.trim())
        .then_some(actor_user_id)
}

fn friend_actor_image(resolver: &NotificationResolver, job: &NotificationJob) -> Option<String> {
    let actor_user_id = image_actor_user_id(&job.delivery, &job.current_user_id)?;
    resolver.friend_image(&job.endpoint, actor_user_id)
}

async fn resolve_world_name_with_budget(
    tasks: &TaskSupervisor,
    resolver: Arc<NotificationResolver>,
    job: &NotificationJob,
) -> Option<(String, String)> {
    if is_meaningful_world_name(&job.delivery.entry.content.world_name) {
        return None;
    }
    let endpoint = job.endpoint.clone();
    let delivery = job.delivery.clone();
    let (result_tx, result_rx) = oneshot::channel();
    tasks.spawn(async move {
        let result = resolver.world_name(&endpoint, &delivery).await;
        let _ = result_tx.send(result);
    });
    tokio::time::timeout(NOTIFICATION_WORLD_NAME_FIRST_SEND_BUDGET, result_rx)
        .await
        .ok()?
        .ok()?
}

fn notification_session_identity(auth_scope: &RuntimeAuthScope) -> (String, String) {
    let auth_scope = auth_scope.snapshot();
    if auth_scope.active {
        return (auth_scope.endpoint, auth_scope.current_user_id);
    }
    Default::default()
}

#[derive(Clone)]
struct LocalImageRequest {
    url: String,
    file_id: String,
    version: String,
}

fn local_image_request(image_url: &str) -> Option<LocalImageRequest> {
    let url = image_url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let file_id = extract_file_id(url)?;
    let version = extract_file_version(url, &file_id).unwrap_or_else(|| fallback_file_version(url));
    if version.is_empty() {
        return None;
    }
    Some(LocalImageRequest {
        url: url.to_string(),
        file_id,
        version,
    })
}

async fn resolve_local_image_with_budget(
    tasks: &TaskSupervisor,
    image_cache: Arc<ImageCache>,
    image_url: &str,
    proxy_url: Option<&str>,
) -> Option<String> {
    // VRChat 资产（`file_` + 版本）走既有 `ImageCache`：那套能拿到原图的
    // 规范地址、按版本失效，是更准的一条路。
    if let Some(request) = local_image_request(image_url) {
        let (result_tx, result_rx) = oneshot::channel();
        tasks.spawn(async move {
            let result = fetch_local_image(image_cache, request).await;
            let _ = result_tx.send(result);
        });
        let resolved = tokio::time::timeout(NOTIFICATION_IMAGE_FIRST_SEND_BUDGET, result_rx)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();
        if resolved.is_some() {
            return resolved;
        }
    }
    // 不是 VRChat 资产（外部事件给的是普通 CDN 地址，不含 `file_`）：
    // 交给通用图标缓存按 URL 抓一次并落盘。
    resolve_remote_icon(image_url, proxy_url).await
}

/// 用通用缓存解析一张远程图标。
///
/// 代理来自宿主统一设置：本机实测 `vrcc.cc` 直连会 DNS 超时，不走代理时图标
/// 必然失败，而失败是静默的（通知照发、就是没图），很难从现象反推原因。
async fn resolve_remote_icon(image_url: &str, proxy_url: Option<&str>) -> Option<String> {
    let url = image_url.trim();
    // 非 http(s) 的一律不碰网络。
    IconRequest::parse(url)?;
    let cache = RemoteIconCache::in_app_data()?.with_proxy(proxy_url);
    let path = cache.resolve_default(url).await?;
    path.to_str().map(str::to_string)
}

async fn fetch_local_image(
    image_cache: Arc<ImageCache>,
    request: LocalImageRequest,
) -> Option<String> {
    image_cache
        .get_image(&request.url, &request.file_id, &request.version)
        .await
        .ok()
}

#[cfg(test)]
mod tests;
