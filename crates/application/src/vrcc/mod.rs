//! 星遇同频集成的应用服务。
//!
//! 这一层负责编排：拿令牌 → 调接口 → 投影成前端/腕表能用的形状。
//! 网络细节在 `vrcx_0_integrations::vrcc`，持久化细节在 outbound-adapters，
//! 这里只做组合与口径统一。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use vrcx_0_application_core::Result;
use vrcx_0_contracts::vrcc::{
    VrccBoard, VrccConnectionSnapshot, VrccMyMap, VrccRecentNotifications, VrccStatus, VrccToday,
    VrccUsageSnapshot, VrccWristSummary,
};
use vrcx_0_integrations::vrcc::{
    VrccClient, VrccError, VrccSession, PERSONAL_DAILY_LIMIT, WINDOW_LIMIT,
};

/// 令牌仓库端口。实现留在 outbound-adapters。
pub trait VrccTokenStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredCredential>>;
    fn save(&self, credential: &StoredCredential) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// 前端能区分的行为分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrccErrorKind {
    /// 需要玩家重新授权。
    NeedsReauth,
    /// 缺某个权限，`need` 里说明是哪项。
    MissingScope,
    /// 限流或额度用尽，等等再来。
    Throttled,
    /// 网络或服务端问题。
    Transport,
    /// 其它。
    Other,
}

/// 判定一个错误属于哪一类，并把缺的权限一并带回。
pub fn classify_vrcc_error(error: &VrccError) -> (VrccErrorKind, Option<String>) {
    let need = error.needed_scope().map(str::to_string);
    let kind = match error {
        // `TooSoon` 是「问勤了」，归到 Throttled 只是为了让前端显示
        // 「稍后再试」；它不会触发长时间退避，也不消耗额度。
        VrccError::RateLimited(_) | VrccError::QuotaExhausted(_) | VrccError::TooSoon { .. } => {
            VrccErrorKind::Throttled
        }
        VrccError::Transport(_) | VrccError::Decode(_) => VrccErrorKind::Transport,
        VrccError::TokenMissing => VrccErrorKind::NeedsReauth,
        VrccError::Api { need, .. } if need.is_some() && error.needs_reauth() => {
            VrccErrorKind::MissingScope
        }
        _ if error.needs_reauth() => VrccErrorKind::NeedsReauth,
        VrccError::AbsolutePath(_) | VrccError::BodyTooLarge | VrccError::Api { .. } => {
            VrccErrorKind::Other
        }
    };
    (kind, need)
}

/// 仓库里的凭据形状（与 outbound-adapters 的实现解耦）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredCredential {
    pub token: String,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
}

/// 一次设备授权的中间态。给前端显示用，不落盘。
#[derive(Clone, Debug)]
pub struct VrccDeviceAuthPrompt {
    pub user_code: String,
    pub verification_uri: Option<String>,
    pub verification_uri_complete: Option<String>,
    pub expires_in_secs: i64,
    pub interval_secs: i64,
    /// 内部用：换令牌时要回传，不给玩家看。
    pub device_code: String,
    pub scope: Option<String>,
}

/// 设备授权的下一步。
#[derive(Clone, Debug)]
pub enum VrccDeviceAuthStep {
    /// 玩家还没点同意，按 `interval_secs` 后再问。
    Pending { interval_secs: i64 },
    /// 拿到令牌，已保存。
    Connected(VrccConnectionSnapshot),
    /// 玩家取消或缺码过期，这条流程结束。
    Terminal(String),
}

/// 星遇同频服务。
pub struct VrccService {
    store: Arc<dyn VrccTokenStore>,
    session: VrccSession,
    client: VrccClient,
    /// 腕表摘要的缓存。
    ///
    /// 腕表每帧都会读它，所以这里必须是纯内存读取——不能在这里发请求。
    /// 内容由页面会发的那两个请求顺手填充：`recent_notifications` 提供未读数，
    /// `boards` 提供地图名次（开放数据，不占个人额度）。
    ///
    /// `None` 表示"还没拿到任何数据"，和"未读 0 条"是两件事：前者不该在腕表
    /// 上显示一个数字，后者才该显示 0。
    wrist: Arc<RwLock<Option<VrccWristSummary>>>,
    /// 最近一次地图榜：world 映射到（名次，均分 ×10）。
    ///
    /// 缓存整个榜单而不是只缓存"当前地图"：玩家随时换图，而榜单是开放数据，
    /// 一次请求就能覆盖上百张图，按图缓存没有意义。
    boards: Arc<RwLock<HashMap<String, (i64, Option<i64>)>>>,
}

impl VrccService {
    /// 从仓库恢复会话：有令牌就直接进入已连接状态。
    pub fn new(store: Arc<dyn VrccTokenStore>, client_id: impl Into<String>) -> Self {
        let session = VrccSession::new(client_id);
        if let Ok(Some(credential)) = store.load() {
            session.connect(
                credential.token,
                credential.star,
                credential.vrc_name,
                credential.scope,
            );
        }
        let client = session.client();
        Self {
            store,
            session,
            client,
            wrist: Arc::new(RwLock::new(None)),
            boards: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 腕表摘要。`world_id` 由调用方给（腕表的渲染侧知道玩家在哪张图）。
    ///
    /// 纯内存读取，不发请求。返回 `None` 的两种情况：还没拉到任何数据、
    /// 缓存锁坏了。腕表在 `None` 时整块不显示。
    ///
    /// 这里**不判断是否已连接**：地图榜是开放数据，没连接也能拉到，玩家在
    /// 腕表上看到"我在的图排第几"本身就是有用的。未读数在没有连接时保持 0，
    /// 那是事实——没有连接就收不到提醒。
    pub fn wrist_summary(&self, world_id: Option<&str>) -> Option<VrccWristSummary> {
        let mut summary = self.wrist.read().ok()?.clone()?;
        // 地图名次从榜单缓存现查：玩家换图后不必等下一次 boards 请求。
        let rank = world_id.and_then(|world| {
            self.boards
                .read()
                .ok()
                .and_then(|boards| boards.get(world).copied())
        });
        summary.map_rank = rank.map(|(rank, _)| rank);
        summary.map_avg_x10 = rank.and_then(|(_, avg)| avg);
        Some(summary)
    }

    /// 拿到 `recent` 结果后更新未读数。
    ///
    /// 顺手把缓存从 `None` 变成 `Some`：这已经是真实数据了，腕表可以显示。
    fn note_recent(&self, unread: u32) {
        let Ok(mut wrist) = self.wrist.write() else {
            return;
        };
        let entry = wrist.get_or_insert_with(VrccWristSummary::default);
        entry.unread = unread;
    }

    /// 拿到地图榜后更新名次缓存。
    ///
    /// 榜单是开放数据，不带令牌也能拉；只要拉到了就认为腕表有数据可显示，
    /// 这样"没连接但想看榜单"的场景也有内容。
    fn note_boards(&self, board: &VrccBoard) {
        let Ok(mut boards) = self.boards.write() else {
            return;
        };
        boards.clear();
        for map in &board.maps {
            let Some(rank) = map.rank else { continue };
            let avg_x10 = map.avg.map(|avg| (avg * 10.0).round() as i64);
            boards.insert(map.world.clone(), (rank, avg_x10));
        }
        drop(boards);
        if let Ok(mut wrist) = self.wrist.write() {
            wrist.get_or_insert_with(VrccWristSummary::default);
        }
    }

    pub fn client_id(&self) -> &str {
        self.session.client_id()
    }

    pub fn snapshot(&self) -> VrccConnectionSnapshot {
        self.session.snapshot().into()
    }

    /// 本地用量快照。
    ///
    /// 口径：只说「本客户端今日已用」多少。官方没有查询剩余额度的接口，
    /// 服务器按令牌计数，我们看不到那个数字，所以不能显示「剩余」。
    pub fn usage(&self) -> VrccUsageSnapshot {
        let usage = self.client.local_usage();
        VrccUsageSnapshot {
            personal_used_today: usage.personal_used_today,
            personal_daily_limit: PERSONAL_DAILY_LIMIT,
            device_code_used_today: usage.device_code_used_today,
            device_code_daily_limit: vrcx_0_integrations::vrcc::DEVICE_CODE_DAILY_LIMIT,
            window_used: usage.window_used as i64,
            window_limit: WINDOW_LIMIT as i64,
            backoff_remaining_ms: usage.backoff_remaining.as_millis() as i64,
            date: usage.date,
        }
    }

    // -- 开放数据 ----------------------------------------------------------

    /// 地图榜。不需要连接，也不计个人额度。
    pub async fn boards(&self, limit: u32) -> std::result::Result<VrccBoard, VrccError> {
        let board = self.client.boards(limit).await?;
        // 顺手填充腕表的排名缓存：这次请求本来就要发，不额外花额度。
        self.note_boards(&board);
        Ok(board)
    }

    /// 服务状态。
    pub async fn status(&self) -> std::result::Result<VrccStatus, VrccError> {
        self.client.status().await
    }

    // -- 设备授权 ----------------------------------------------------------

    /// 第一步：申请连接码。
    ///
    /// `scope` 传 `None` 表示用官方默认（`dev-` 是
    /// `profile notifications maps`）。
    pub async fn begin_device_auth(
        &self,
        scope: Option<&str>,
    ) -> std::result::Result<VrccDeviceAuthPrompt, VrccError> {
        let code = self.client.device_code(scope).await?;
        Ok(VrccDeviceAuthPrompt {
            user_code: code.user_code,
            verification_uri: code.verification_uri,
            verification_uri_complete: code.verification_uri_complete,
            expires_in_secs: code.expires_in.unwrap_or(600),
            interval_secs: code.interval.unwrap_or(5).max(5),
            device_code: code.device_code,
            scope: code.scope,
        })
    }

    /// 第二步：问一次令牌。按官方三种结果分类。
    pub async fn poll_device_auth(
        &self,
        prompt: &VrccDeviceAuthPrompt,
    ) -> std::result::Result<VrccDeviceAuthStep, VrccError> {
        match self.client.device_token(&prompt.device_code).await {
            Ok(token) => {
                let credential = StoredCredential {
                    token: token.access_token.clone(),
                    star: token.me.as_ref().and_then(|me| me.star),
                    vrc_name: token.me.as_ref().and_then(|me| me.vrc.clone()),
                    scope: token.scope.clone(),
                };
                self.store
                    .save(&credential)
                    .map_err(|error| VrccError::Transport(format!("保存令牌失败：{error}")))?;
                self.session.connect(
                    credential.token,
                    credential.star,
                    credential.vrc_name,
                    credential.scope,
                );
                Ok(VrccDeviceAuthStep::Connected(self.snapshot()))
            }
            Err(error) if error.is_authorization_pending() => Ok(VrccDeviceAuthStep::Pending {
                interval_secs: prompt.interval_secs,
            }),
            Err(error) if error.is_device_auth_terminal() => {
                Ok(VrccDeviceAuthStep::Terminal(error.to_string()))
            }
            Err(error) => Err(error),
        }
    }

    /// 断开：先让服务器吊销，再清本地。
    ///
    /// 吊销失败也照样清本地——玩家点了断开，界面上就该断开。
    pub async fn disconnect(&self) -> std::result::Result<(), VrccError> {
        let revoke = self.client.disconnect().await;
        self.store
            .clear()
            .map_err(|error| VrccError::Transport(format!("清除令牌失败：{error}")))?;
        self.session.disconnect();
        match revoke {
            Ok(()) => Ok(()),
            // 令牌本来就无效，等于已经断开了。
            Err(error) if error.needs_reauth() => Ok(()),
            Err(error) => Err(error),
        }
    }

    // -- 提醒 --------------------------------------------------------------

    /// 拉最近的提醒。
    ///
    /// `mark` 对应官方的 `?mark=1`：传 `true` 表示**玩家此刻正在看这个列表**，
    /// 服务器会把最新一条 ID 记到账号的 `read` 字段，跨设备同步已读状态。
    ///
    /// 两种调用场景必须区分：玩家打开「提醒」页时传 `true`；后台只为算数字时
    /// 传 `false`——否则等于替玩家把提醒标成看过了，而其实他没看。
    pub async fn recent_notifications(
        &self,
        mark: bool,
    ) -> std::result::Result<VrccRecentNotifications, VrccError> {
        self.ensure_connected()?;
        let result = self.client.recent_notifications(mark).await?;
        // 未读数按官方语义算：id 大于 read 的算未读。`read` 缺省时（账号上
        // 还没记过已读），把返回的条目都当作未读——这是唯一诚实的解读。
        //
        // 刚标记过 `mark=1` 时服务器会把 read 推到最新，未读随之变 0，这是对的：
        // 玩家确实看到了。
        let unread = match result.read {
            Some(read) => result.items.iter().filter(|item| item.id > read).count() as u32,
            None => result.items.len() as u32,
        };
        self.note_recent(unread);
        self.session
            .note_poll(vrcx_0_integrations::vrcc::now_millis(), None, None);
        Ok(result)
    }

    /// 当前所在地图在地图榜上的状态与我自己的评分。
    ///
    /// 需要 `maps`（普通权限）。官方的硬性要求是 `off` 为 `true` 时客户端不要
    /// 显示打分面板；这里在 `off` 时直接返回 `None`，调用方拿不到对象就不可能
    /// 误显示——比在每个 UI 分支里记得判断 `off` 更可靠。
    pub async fn my_map(&self, world: &str) -> std::result::Result<Option<VrccMyMap>, VrccError> {
        // 只认 wrld_…：带房间实例号的 location 会让接口查错图。
        let world = vrcx_0_core::location::world_id_from_location(world);
        if world.is_empty() {
            return Ok(None);
        }
        self.ensure_connected()?;
        let map = self.client.my_map(&world).await?;
        if !map.is_visible() {
            tracing::debug!(
                world = %world,
                "vrcc: this map is not shown to this player, hiding the card"
            );
            return Ok(None);
        }
        Ok(Some(map))
    }

    /// 增量轮询。游标与建议间隔都自动维护。
    ///
    /// 响应里的 `dot` 是「星轨相交尚未查看的新人数」，**不是未读提醒数**，
    /// 所以这里不拿它更新腕表的 `unread`——两者混用会显示错误的数字。
    /// 未读数只由 `recent_notifications` 按 `read` 游标算。
    pub async fn poll_notifications(
        &self,
    ) -> std::result::Result<vrcx_0_contracts::vrcc::VrccNotifications, VrccError> {
        self.ensure_connected()?;
        let cursor = self.session.snapshot().notifications_cursor;
        let result = self.client.notifications(cursor).await?;
        self.session.note_poll(
            vrcx_0_integrations::vrcc::now_millis(),
            result.last,
            result.every,
        );
        // 有新提醒到达时，未读数会变；下次打开页面拉 recent 时会修正。
        // 这里若把 items 长度加上去，会和 `read` 的语义打架。
        Ok(result)
    }

    /// 今天这一轮 + 轮次进度（需要 `today` 敏感权限）。
    pub async fn today(&self) -> std::result::Result<VrccToday, VrccError> {
        self.ensure_connected()?;
        self.client.today().await
    }

    fn ensure_connected(&self) -> std::result::Result<(), VrccError> {
        if self.session.snapshot().connected {
            Ok(())
        } else {
            Err(VrccError::TokenMissing)
        }
    }
}

#[cfg(test)]
mod tests;
