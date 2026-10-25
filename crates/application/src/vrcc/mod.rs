//! 星遇同频集成的应用服务。
//!
//! 这一层负责编排：拿令牌 → 调接口 → 投影成前端/腕表能用的形状。
//! 网络细节在 `vrcx_0_integrations::vrcc`，持久化细节在 outbound-adapters，
//! 这里只做组合与口径统一。

use std::sync::Arc;

use vrcx_0_application_core::Result;
use vrcx_0_contracts::vrcc::{
    VrccBoard, VrccConnectionSnapshot, VrccRecentNotifications, VrccStatus, VrccToday,
    VrccUsageSnapshot,
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
        VrccError::RateLimited(_) | VrccError::QuotaExhausted(_) => VrccErrorKind::Throttled,
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
        }
    }

    pub fn client_id(&self) -> &str {
        self.session.client_id()
    }

    pub fn snapshot(&self) -> VrccConnectionSnapshot {
        self.session.snapshot().into()
    }

    /// 本地用量快照。措辞口径：只说「本客户端今日已用」。
    pub fn usage(&self) -> VrccUsageSnapshot {
        let today = vrcx_0_integrations::vrcc::local_date_today();
        VrccUsageSnapshot {
            personal_used_today: 0,
            personal_daily_limit: PERSONAL_DAILY_LIMIT,
            device_code_used_today: 0,
            device_code_daily_limit: vrcx_0_integrations::vrcc::DEVICE_CODE_DAILY_LIMIT,
            window_used: 0,
            window_limit: WINDOW_LIMIT as i64,
            backoff_remaining_ms: 0,
            date: today,
        }
    }

    // -- 开放数据 ----------------------------------------------------------

    /// 地图榜。不需要连接，也不计个人额度。
    pub async fn boards(&self, limit: u32) -> std::result::Result<VrccBoard, VrccError> {
        self.client.boards(limit).await
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

    /// 拉最近的提醒。首次不传游标。
    pub async fn recent_notifications(
        &self,
    ) -> std::result::Result<VrccRecentNotifications, VrccError> {
        self.ensure_connected()?;
        let result = self.client.recent_notifications(false).await?;
        self.session
            .note_poll(vrcx_0_integrations::vrcc::now_millis(), None, None);
        Ok(result)
    }

    /// 增量轮询。游标与建议间隔都自动维护。
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
