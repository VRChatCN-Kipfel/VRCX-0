//! 星遇同频运行时：把应用层服务挂到桌面运行时上。

use std::sync::Arc;

use vrcx_0_application::vrcc::{StoredCredential, VrccService, VrccTokenStore};
use vrcx_0_contracts::vrcc::{
    VrccBoard, VrccConnectionSnapshot, VrccRecentNotifications, VrccStatus, VrccToday,
    VrccUsageSnapshot, VrccWristSummary,
};
use vrcx_0_integrations::vrcc::VRCC_DEFAULT_CLIENT_ID;
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

/// 桌面侧的星遇同频运行时。
pub struct DesktopVrccRuntime {
    service: Arc<VrccService>,
}

impl DesktopVrccRuntime {
    pub fn new(store: LocalVrccTokenStore) -> Self {
        let port = Arc::new(TokenStoreAdapter { inner: store });
        Self {
            service: Arc::new(VrccService::new(port, VRCC_DEFAULT_CLIENT_ID)),
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
}
