//! 星遇同频令牌的本地存储。
//!
//! 令牌属于凭据，走项目既有的 `secrets_at_rest`：写入时用
//! `seal_secret_with_status` 加密，读出时 `open_secret` 解密。
//! 不用明文存 `configs` 表，理由和 `cookies` / `secrets_at_rest` 那两个
//! 测试测的一样——磁盘上的凭据必须是密文。

use std::sync::Arc;

use vrcx_0_persistence::{secrets, DatabaseService};

/// 令牌与玩家身份的配置键。
const VRCC_TOKEN_KEY: &str = "vrccAccessToken";
const VRCC_STAR_KEY: &str = "vrccStar";
const VRCC_VRC_NAME_KEY: &str = "vrccVrcName";
const VRCC_SCOPE_KEY: &str = "vrccScope";

/// 从配置表读写的令牌仓库。
#[derive(Clone)]
pub struct LocalVrccTokenStore {
    db: Arc<DatabaseService>,
}

/// 一次读出的完整凭据。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredVrccCredential {
    pub token: String,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
}

impl LocalVrccTokenStore {
    pub fn new(db: Arc<DatabaseService>) -> Self {
        Self { db }
    }

    /// 读令牌。没有或解不开时返回 `None`。
    ///
    /// `open_secret` 对密文才解密，对明文（旧数据、未启用加密时写的）原样返回，
    /// 所以这里不需要区分两种情况。
    pub fn load(&self) -> crate::Result<Option<StoredVrccCredential>> {
        let raw = vrcx_0_persistence::config::get_string(&self.db, VRCC_TOKEN_KEY, "")
            .map_err(crate::map_persistence_error)?;
        if raw.trim().is_empty() {
            return Ok(None);
        }
        let Some(token) = secrets::open_secret(&raw) else {
            // 密文解不开：多半是换了机器或密钥轮换了。当作没连接，
            // 让玩家重新走一次设备授权，而不是把乱码当令牌发出去。
            tracing::warn!("vrcc: stored token cannot be decrypted on this machine");
            return Ok(None);
        };
        if token.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(StoredVrccCredential {
            token,
            star: self.load_i64(VRCC_STAR_KEY)?,
            vrc_name: self.load_string(VRCC_VRC_NAME_KEY)?,
            scope: self.load_string(VRCC_SCOPE_KEY)?,
        }))
    }

    fn load_string(&self, key: &str) -> crate::Result<Option<String>> {
        let value = vrcx_0_persistence::config::get_string(&self.db, key, "")
            .map_err(crate::map_persistence_error)?;
        let trimmed = value.trim();
        Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
    }

    fn load_i64(&self, key: &str) -> crate::Result<Option<i64>> {
        Ok(self
            .load_string(key)?
            .and_then(|value| value.parse::<i64>().ok()))
    }

    /// 保存令牌与身份。令牌走加密路径。
    pub fn save(&self, credential: &StoredVrccCredential) -> crate::Result<()> {
        let previous = vrcx_0_persistence::config::get_string(&self.db, VRCC_TOKEN_KEY, "")
            .map_err(crate::map_persistence_error)?;
        // `previous` 传进去是为了保留其它机器的槽位（secrets_at_rest 的约定）。
        let sealed = secrets::seal_secret_with_status(
            Some(previous.as_str()).filter(|value| !value.is_empty()),
            &credential.token,
        );
        vrcx_0_persistence::config::set_string(&self.db, VRCC_TOKEN_KEY, &sealed.stored)
            .map_err(crate::map_persistence_error)?;
        self.save_optional_string(VRCC_STAR_KEY, credential.star.map(|v| v.to_string()).as_deref())?;
        self.save_optional_string(VRCC_VRC_NAME_KEY, credential.vrc_name.as_deref())?;
        self.save_optional_string(VRCC_SCOPE_KEY, credential.scope.as_deref())?;
        Ok(())
    }

    fn save_optional_string(&self, key: &str, value: Option<&str>) -> crate::Result<()> {
        vrcx_0_persistence::config::set_string(&self.db, key, value.unwrap_or(""))
            .map_err(crate::map_persistence_error)
    }

    /// 清除凭据。断开连接时调用。
    pub fn clear(&self) -> crate::Result<()> {
        for key in [VRCC_TOKEN_KEY, VRCC_STAR_KEY, VRCC_VRC_NAME_KEY, VRCC_SCOPE_KEY] {
            vrcx_0_persistence::config::set_string(&self.db, key, "")
                .map_err(crate::map_persistence_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_round_trip_shape() {
        // 只测数据形状与默认值，真正的加密往返在 persistence 的
        // secrets_at_rest 测试里覆盖（那里有 golden vector）。
        let credential = StoredVrccCredential {
            token: "abc".into(),
            star: Some(77),
            vrc_name: Some("拾光".into()),
            scope: Some("profile maps".into()),
        };
        assert_eq!(credential.token, "abc");
        assert_eq!(credential.star, Some(77));
    }

    #[test]
    fn default_credential_is_empty() {
        let credential = StoredVrccCredential::default();
        assert!(credential.token.is_empty());
        assert!(credential.star.is_none());
        assert!(credential.vrc_name.is_none());
    }
}
