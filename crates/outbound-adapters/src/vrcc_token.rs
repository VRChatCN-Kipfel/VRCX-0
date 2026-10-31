//! 星遇同频令牌的本地存储。
//!
//! 令牌属于凭据，走项目既有的 `secrets_at_rest`：写入时用
//! `seal_secret_with_status` 加密，读出时 `open_secret` 解密。
//! 不用明文存 `configs` 表，理由和 `cookies` / `secrets_at_rest` 那两个
//! 测试测的一样——磁盘上的凭据必须是密文。

use std::sync::Arc;

use vrcx_0_persistence::{secrets, DatabaseService};

/// 令牌与玩家身份的配置键（**不含** owner 前缀）。
const VRCC_TOKEN_KEY: &str = "vrccAccessToken";
const VRCC_STAR_KEY: &str = "vrccStar";
const VRCC_VRC_NAME_KEY: &str = "vrccVrcName";
const VRCC_SCOPE_KEY: &str = "vrccScope";

/// 签发这个令牌时用的 `X-Client`。
///
/// 令牌只能配签发它的那个 client 使用（官方《设备授权》一节），换了标识会
/// 得到 `403 client_mismatch`。存下来才能在换标识后认出「这个令牌作废了」，
/// 而不是启动时照样恢复成「已连接」、之后每次请求都 403。
const VRCC_CLIENT_ID_KEY: &str = "vrccClientId";

/// 从配置表读写的令牌仓库。
///
/// **按 VRChat 账号隔离**：星遇同频的令牌和其它玩家数据一样属于某个人，
/// 不是这台机器。原来的实现把令牌存成全局键（`vrcx_vrccaccesstoken`），
/// 于是切换 VRChat 账号后会继续沿用上一个账号的星遇连接——同一台机器上
/// 两个账号看到的是同一个星遇身份，断开一个人的连接会连带断掉另一个人的。
///
/// 现在每个账号一组键：`vrcc_{user_prefix}_{stem}`，`user_prefix` 用项目
/// 既有的 `normalize_user_table_prefix`（与 `usr{hash}_*` 那套表同一算法），
/// 保持命名一致。
///
/// **账号是每次读写时现取的**，不是构造时固定的：这个仓库在应用装配阶段
/// 就被创建，那时还没有登录用户。把它做成"绑死某个账号"，就没法表达
/// 「切号之后读写另一组键」这件事。
#[derive(Clone)]
pub struct LocalVrccTokenStore {
    db: Arc<DatabaseService>,
    /// 当前登录账号的来源。返回 `None` 表示还没有人登录。
    current_owner: Arc<dyn Fn() -> Option<vrcx_0_core::OwnerId> + Send + Sync>,
}

/// 一次读出的完整凭据。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredVrccCredential {
    pub token: String,
    pub star: Option<i64>,
    pub vrc_name: Option<String>,
    pub scope: Option<String>,
    /// 签发这个令牌时用的 X-Client；旧存档没有这一项。
    pub client_id: Option<String>,
}

impl LocalVrccTokenStore {
    /// 用「当前账号」的来源构造。
    ///
    /// 传闭包而不是具体 user id，是因为装配发生在登录之前；每次读写时
    /// 现取一次，切号自然生效。
    pub fn new(
        db: Arc<DatabaseService>,
        current_owner: Arc<dyn Fn() -> Option<vrcx_0_core::OwnerId> + Send + Sync>,
    ) -> Self {
        Self { db, current_owner }
    }

    /// 当前账号的键前缀；未登录时为 `None`。
    ///
    /// 未登录时**所有读写都落到"没有凭据"**：既不读别人的令牌，也不把
    /// 令牌写到无名键上。
    fn owner_prefix(&self) -> Option<String> {
        let owner = (self.current_owner)()?;
        vrcx_0_persistence::realtime::normalize_user_table_prefix(owner.as_str()).ok()
    }

    /// 把逻辑键拼成实际存储键。
    ///
    /// `stem` 是去掉 `vrcc` 前缀后的小写余下部分，
    /// 例如 `vrccAccessToken` → `accesstoken`。
    /// 无 owner 时返回 `None`——调用方据此跳过读写。
    fn store_key(&self, key: &str) -> Option<String> {
        let prefix = self.owner_prefix()?;
        let lowered = key.to_ascii_lowercase();
        let stem = lowered.strip_prefix("vrcc").unwrap_or(&lowered);
        Some(format!("vrcc_{prefix}_{stem}"))
    }

    /// 共享同一个数据库句柄。
    ///
    /// 给同一层的其它 vrcc 存储（日配额）复用连接用，避免为了拿一个
    /// `DatabaseService` 再走一遍装配链。
    pub fn db(&self) -> &Arc<DatabaseService> {
        &self.db
    }

    /// 读令牌。没有或解不开时返回 `None`。
    ///
    /// `open_secret` 对密文才解密，对明文（旧数据、未启用加密时写的）原样返回，
    /// 所以这里不需要区分两种情况。
    pub fn load(&self) -> crate::Result<Option<StoredVrccCredential>> {
        // 还没登录：没有"这个账号的令牌"，直接当未连接。
        let Some(token_key) = self.store_key(VRCC_TOKEN_KEY) else {
            return Ok(None);
        };
        let raw = vrcx_0_persistence::config::get_string(&self.db, &token_key, "")
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
            client_id: self.load_string(VRCC_CLIENT_ID_KEY)?,
        }))
    }

    fn load_string(&self, key: &str) -> crate::Result<Option<String>> {
        let Some(store_key) = self.store_key(key) else {
            return Ok(None);
        };
        let value = vrcx_0_persistence::config::get_string(&self.db, &store_key, "")
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
    ///
    /// 未登录时**报错而不是丢弃**：令牌必须挂在某个人名下，静默丢掉会让
    /// 玩家以为连上了、重启后却又要重连。
    pub fn save(&self, credential: &StoredVrccCredential) -> crate::Result<()> {
        let Some(token_key) = self.store_key(VRCC_TOKEN_KEY) else {
            return Err(crate::Error::Database(
                "vrcc: cannot store a token without a signed-in VRChat user".into(),
            ));
        };
        let previous = vrcx_0_persistence::config::get_string(&self.db, &token_key, "")
            .map_err(crate::map_persistence_error)?;
        // `previous` 传进去是为了保留其它机器的槽位（secrets_at_rest 的约定）。
        let sealed = secrets::seal_secret_with_status(
            Some(previous.as_str()).filter(|value| !value.is_empty()),
            &credential.token,
        );
        vrcx_0_persistence::config::set_string(&self.db, &token_key, &sealed.stored)
            .map_err(crate::map_persistence_error)?;
        self.save_optional_string(
            VRCC_STAR_KEY,
            credential.star.map(|v| v.to_string()).as_deref(),
        )?;
        self.save_optional_string(VRCC_VRC_NAME_KEY, credential.vrc_name.as_deref())?;
        self.save_optional_string(VRCC_SCOPE_KEY, credential.scope.as_deref())?;
        self.save_optional_string(VRCC_CLIENT_ID_KEY, credential.client_id.as_deref())?;
        Ok(())
    }

    fn save_optional_string(&self, key: &str, value: Option<&str>) -> crate::Result<()> {
        let Some(store_key) = self.store_key(key) else {
            return Ok(());
        };
        vrcx_0_persistence::config::set_string(&self.db, &store_key, value.unwrap_or(""))
            .map_err(crate::map_persistence_error)
    }

    /// 清除凭据。断开连接时调用。
    ///
    /// 只清**本账号**的键：断开一个账号的星遇连接，不该动到同一台机器上
    /// 另一个账号的凭据。
    pub fn clear(&self) -> crate::Result<()> {
        for key in [
            VRCC_TOKEN_KEY,
            VRCC_STAR_KEY,
            VRCC_VRC_NAME_KEY,
            VRCC_SCOPE_KEY,
        ] {
            let Some(store_key) = self.store_key(key) else {
                continue;
            };
            vrcx_0_persistence::config::set_string(&self.db, &store_key, "")
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
            client_id: Some("dev-test".into()),
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

    // -- 按账号隔离 --------------------------------------------------------

    /// 临时目录，Drop 时删除。
    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("dsh-vrcc-token-{nonce}"));
            std::fs::create_dir_all(&path).expect("temp dir");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn test_db() -> (Arc<DatabaseService>, TempDir) {
        let dir = TempDir::new();
        let db = Arc::new(
            DatabaseService::new(&dir.path().join("VRCX-0.sqlite3")).expect("test database"),
        );
        (db, dir)
    }

    fn store_for(db: Arc<DatabaseService>, user_id: Option<&'static str>) -> LocalVrccTokenStore {
        let owner = Arc::new(move || user_id.map(|id| vrcx_0_core::OwnerId::new(id.to_string())));
        LocalVrccTokenStore::new(db, owner)
    }

    fn credential(token: &str) -> StoredVrccCredential {
        StoredVrccCredential {
            token: token.to_string(),
            star: Some(645),
            vrc_name: None,
            scope: Some("profile notifications maps".into()),
            client_id: Some("dev-test".into()),
        }
    }

    /// **两个账号各存各的**，互不覆盖。
    ///
    /// 这条钉的是本次修复的核心：原来令牌存全局键，切号后会沿用上一个人的
    /// 星遇连接，断开一个人会连带断掉另一个人。
    #[test]
    fn credentials_are_isolated_per_vrchat_account() {
        let (db, _dir) = test_db();

        let alice = store_for(Arc::clone(&db), Some("usr_aaaa"));
        alice.save(&credential("token-alice")).expect("save alice");

        let bob = store_for(Arc::clone(&db), Some("usr_bbbb"));
        bob.save(&credential("token-bob")).expect("save bob");

        // 各自的令牌互不影响。
        assert_eq!(
            alice.load().unwrap().map(|c| c.token),
            Some("token-alice".to_string())
        );
        assert_eq!(
            bob.load().unwrap().map(|c| c.token),
            Some("token-bob".to_string())
        );

        // 断开 Alice 不应动到 Bob。
        alice.clear().expect("clear alice");
        assert!(alice.load().unwrap().is_none());
        assert_eq!(
            bob.load().unwrap().map(|c| c.token),
            Some("token-bob".to_string())
        );
    }

    /// 未登录时读不到任何令牌，也存不进去。
    ///
    /// 存的时候**报错而不是静默丢弃**：令牌必须挂在某个人名下，
    /// 悄悄丢掉会让玩家以为连上了、重启后却又要重连。
    #[test]
    fn unsigned_in_store_neither_reads_nor_writes() {
        let (db, _dir) = test_db();

        // 先让某个账号存一份，确认未登录的 store 读不到它。
        store_for(Arc::clone(&db), Some("usr_aaaa"))
            .save(&credential("token-alice"))
            .expect("save alice");

        let anonymous = store_for(Arc::clone(&db), None);
        assert!(anonymous.load().expect("load").is_none());

        let error = anonymous.save(&credential("orphan"));
        assert!(error.is_err(), "未登录时保存必须报错");
    }
}
