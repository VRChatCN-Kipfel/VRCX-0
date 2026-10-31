//! 通用远程图标缓存：给「外部事件」类通知提供图标。
//!
//! ## 为什么需要单独一层
//!
//! 通知层原有的图片通道只认 VRChat 的 `file_<uuid>` 资产
//! （`local_image_request` → [`extract_file_id`]），因为那套 `ImageCache`
//! 需要 `file_id` + `version` 才能拼出下载地址。`ActivityKind::External` 的
//! 来源是第三方服务，给的是普通 CDN 地址，不含 `file_` 前缀，于是
//! `extract_file_id` 返回 `None`，整条图片链路在第一步就退出——通知没有图。
//!
//! 这里补的是「任意 http(s) 图片 URL → 本地文件路径」，与业务无关：
//! 它不知道星遇同频，也不知道外部事件是什么，只负责抓取、落盘、复用。
//!
//! ## 设计约束
//!
//! - **失败不阻断投递**：图标是锦上添花。抓不到就返回 `None`，通知照发。
//! - **按 URL 命名文件**：一个 URL 稳定映射到一个文件名，重复使用不再联网。
//!   用 `<hash>.bin` 而不是保留扩展名：扩展名来自 URL，可能缺失、可能带查询
//!   串，也可能与实际内容不符；系统只看文件头，不看扩展名。
//! - **先写临时文件再改名**：下载中断只留下 `.tmp`，不会让一个半个的文件被
//!   当成「已缓存」长期复用——那会导致这个 URL 永远渲染不出图标。
//! - **单次尝试**：不在内存里记失败状态。下一轮再问时还是先看磁盘，因为没有
//!   磁盘文件就说明上次没成，重试是自然的；记一个「本会话放弃」的标记反而会
//!   让「启动时正好没网」变成永久失去图标。
//!
//! ## 不负责的事
//!
//! 不校验内容是不是图片、不限制大小上限、不做淘汰。前两项由调用方在拿到
//! 字节时自己判断（见 [`IconFetcher`] 的注入点）；淘汰交给应用数据目录的
//! 既有清理策略——这里只保证同名 URL 不重复落盘。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 取图的总预算。
///
/// 和 `NOTIFICATION_IMAGE_FIRST_SEND_BUDGET` 取同一个量级：通知是要「立刻」
/// 出现的，为一个图标让玩家等更久不值得。超时就当作没有图标。
pub const ICON_FETCH_BUDGET: Duration = Duration::from_secs(3);

/// 图标文件在应用数据目录下的子目录。
const ICON_CACHE_DIR: &str = "notification-icons";

/// 一次取图请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconRequest {
    /// 完整的 http(s) 地址。
    pub url: String,
}

impl IconRequest {
    /// 从 URL 构造。不是 http(s) 或为空时返回 `None`。
    pub fn parse(url: &str) -> Option<Self> {
        let url = url.trim();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return None;
        }
        Some(Self {
            url: url.to_string(),
        })
    }
}

/// 把 URL 映射成缓存文件名。
///
/// 用 64 位哈希而不是「把 URL 转义成文件名」：URL 可能很长（超文件名上限）、
/// 可能带 `?`/`#`（Windows 文件名非法）、也可能大小写不敏感地与其他 URL 冲突。
/// 哈希碰撞在这里的后果只是「两个 URL 共用一个图标」，可以接受。
pub fn cache_file_name(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}.bin", hasher.finish())
}

/// 通用远程图标缓存的句柄。
///
/// 抓取动作由外部注入（[`RemoteIconCache::resolve`] 的 `fetch` 参数），因为
/// 这一层不该知道网络栈长什么样：桌面运行时用的是带宿主代理设置的客户端，
/// 测试里换成一个假的。
#[derive(Clone)]
pub struct RemoteIconCache {
    dir: PathBuf,
    /// 宿主统一的代理设置。
    ///
    /// **必须传**：本机实测 `vrcc.cc` 直连会 DNS 超时。不接代理时图标必然
    /// 取不到，而失败是静默的（通知照发、就是没图），很难从现象反推原因。
    proxy_url: Option<String>,
}

impl RemoteIconCache {
    /// 指定缓存目录。目录不存在时会在第一次写入前创建。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            proxy_url: None,
        }
    }

    /// 接上宿主代理。
    pub fn with_proxy(mut self, proxy_url: Option<&str>) -> Self {
        self.proxy_url = proxy_url
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string);
        self
    }

    /// 用应用数据目录下的标准位置构造。
    pub fn in_app_data() -> Option<Self> {
        let base = vrcx_0_platform::app_paths::default_app_data_dir().ok()?;
        Some(Self::new(base.join(ICON_CACHE_DIR)))
    }

    /// 解析一张远程图标，抓取动作由本层自己完成（复用宿主代理）。
    ///
    /// 这是给通知层用的入口：调用方不需要知道怎么建 HTTP 客户端。
    pub async fn resolve_default(&self, url: &str) -> Option<PathBuf> {
        let client = self.http_client()?;
        self.resolve_with_budget(url, |target| async move {
            let response = client.get(&target).send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            response.bytes().await.ok().map(|bytes| bytes.to_vec())
        })
        .await
    }

    /// 按代理设置建一个一次性客户端。
    ///
    /// 不复用宿主的 `ImageFetcher`：那个按 VRChat 域名白名单校验，第三方 CDN
    /// 一律被拒；给它开口子等于拆掉一层安全边界，而这里只需要「取一次字节」。
    fn http_client(&self) -> Option<reqwest::Client> {
        let mut builder = reqwest::Client::builder().timeout(ICON_FETCH_BUDGET);
        if let Some(proxy) = self.proxy_url.as_deref() {
            // 走同一套「远端 DNS」处理：本机代理是 socks/http 混合入口，
            // 让代理解析域名才不会在本地先失败。
            builder = builder.proxy(
                reqwest::Proxy::all(vrcx_0_core::proxy::with_remote_dns(proxy).as_ref()).ok()?,
            );
        } else {
            tracing::debug!("icon cache: no proxy configured, fetching icons directly");
        }
        builder.build().ok()
    }

    /// 已经缓存过就直接返回路径，否则调 `fetch` 抓一次。
    ///
    /// `fetch` 收到的是原始 URL，成功时返回图片字节。返回 `None` 表示这次没有
    /// 图标可用——调用方应当继续投递通知，只是不带图。
    pub async fn resolve<F, Fut>(&self, url: &str, fetch: F) -> Option<PathBuf>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Option<Vec<u8>>>,
    {
        let request = IconRequest::parse(url)?;
        let target = self.dir.join(cache_file_name(&request.url));
        // 命中磁盘就不再联网：同一个 URL 稳定映射到同一个文件名。
        if target.is_file() {
            return Some(target);
        }

        let bytes = fetch(request.url.clone()).await?;
        if bytes.is_empty() {
            tracing::debug!(url = %request.url, "icon cache: response was empty");
            return None;
        }
        self.store(&target, &bytes, &request.url)
    }

    /// 把字节落盘并返回最终路径。
    ///
    /// 先写 `.tmp` 再 `rename`：中断只留下临时文件，不会让半截文件被当成缓存。
    fn store(&self, target: &Path, bytes: &[u8], url: &str) -> Option<PathBuf> {
        if let Err(error) = std::fs::create_dir_all(&self.dir) {
            tracing::debug!(%error, "icon cache: could not create directory");
            return None;
        }
        let temp = target.with_extension("tmp");
        if let Err(error) = std::fs::write(&temp, bytes) {
            tracing::debug!(%error, "icon cache: write failed");
            return None;
        }
        if let Err(error) = std::fs::rename(&temp, target) {
            let _ = std::fs::remove_file(&temp);
            tracing::debug!(%error, "icon cache: rename failed");
            return None;
        }
        tracing::info!(
            bytes = bytes.len(),
            url = %url,
            path = %target.display(),
            "icon cache: stored"
        );
        Some(target.to_path_buf())
    }

    /// 带预算地解析：超时就返回 `None`，不让图标拖慢通知。
    ///
    /// 超时不会取消已经开始的抓取任务——那需要把 `fetch` 变成可中止的形状，
    /// 而这里的抓取本来就是「成功就留着、失败就算了」，多跑一会儿无害。
    pub async fn resolve_with_budget<F, Fut>(&self, url: &str, fetch: F) -> Option<PathBuf>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Option<Vec<u8>>>,
    {
        match tokio::time::timeout(ICON_FETCH_BUDGET, self.resolve(url, fetch)).await {
            Ok(path) => path,
            Err(_) => {
                tracing::debug!(url = %url, "icon cache: fetch exceeded budget");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "vrcx0-icon-cache-{name}-{}-{nonce}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn non_http_urls_are_rejected() {
        assert!(IconRequest::parse("").is_none());
        assert!(IconRequest::parse("file:///tmp/a.png").is_none());
        assert!(IconRequest::parse("/assets/icon.png").is_none());
        assert!(IconRequest::parse("  https://a.test/i.png  ").is_some());
    }

    /// 同一个 URL 必须稳定映射到同一个文件名，不同 URL 要分开。
    ///
    /// 这条钉住「缓存能命中」：名字只要带一点不确定性，缓存就永远不命中。
    #[test]
    fn file_name_is_stable_per_url() {
        let url = "https://vrcc.cc/assets/match-icon-512.png";
        assert_eq!(cache_file_name(url), cache_file_name(url));
        assert_ne!(
            cache_file_name(url),
            cache_file_name("https://vrcc.cc/assets/match-icon-256.png")
        );
        // 文件名不能含 Windows 非法字符，也不能太长。
        let name = cache_file_name(url);
        assert!(name.len() <= 64, "unexpectedly long: {name}");
        assert!(!name.contains(['/', '\\', ':', '?', '*', '"', '<', '>', '|']));
    }

    #[tokio::test]
    async fn a_cached_icon_is_not_fetched_again() {
        let dir = temp_dir("hit");
        let cache = RemoteIconCache::new(&dir);
        let url = "https://a.test/icon.png";

        let first = cache
            .resolve(url, |_| async { Some(vec![1_u8, 2, 3]) })
            .await
            .expect("first fetch stores the file");
        assert!(first.is_file());

        // 第二次：`fetch` 若被调用就会 panic，证明走的是磁盘。
        let second = cache
            .resolve(url, |_| async {
                panic!("must not fetch a cached icon");
            })
            .await
            .expect("cache hit");
        assert_eq!(first, second);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_failed_fetch_simply_yields_no_icon() {
        let dir = temp_dir("miss");
        let cache = RemoteIconCache::new(&dir);
        let path = cache
            .resolve("https://a.test/icon.png", |_| async { None })
            .await;
        assert!(path.is_none(), "failure must not fabricate a path");
        // 失败时不该留下任何文件，否则下次会误判为「已缓存」。
        assert!(!dir
            .join(cache_file_name("https://a.test/icon.png"))
            .exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空响应不能落盘：那会变成一个永远渲染不出来的「已缓存」文件。
    #[tokio::test]
    async fn an_empty_response_is_treated_as_failure() {
        let dir = temp_dir("empty");
        let cache = RemoteIconCache::new(&dir);
        let path = cache
            .resolve("https://a.test/icon.png", |_| async { Some(Vec::new()) })
            .await;
        assert!(path.is_none());
        assert!(!dir
            .join(cache_file_name("https://a.test/icon.png"))
            .exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 中断留下的 `.tmp` 不该被当成缓存命中。
    #[tokio::test]
    async fn a_leftover_tmp_file_is_not_a_cache_hit() {
        let dir = temp_dir("tmp");
        let cache = RemoteIconCache::new(&dir);
        let url = "https://a.test/icon.png";
        std::fs::create_dir_all(&dir).expect("create dir");
        let target = dir.join(cache_file_name(url));
        std::fs::write(target.with_extension("tmp"), b"half").expect("write tmp");

        let path = cache
            .resolve(url, |_| async { Some(vec![9_u8]) })
            .await
            .expect("should fetch because only .tmp exists");
        assert_eq!(std::fs::read(&path).expect("read"), vec![9_u8]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 预算到点就放弃，不把通知拖住。
    #[tokio::test]
    async fn resolve_gives_up_after_the_budget() {
        let dir = temp_dir("budget");
        let cache = RemoteIconCache::new(&dir);
        let path = cache
            .resolve_with_budget("https://a.test/slow.png", |_| async {
                tokio::time::sleep(ICON_FETCH_BUDGET * 4).await;
                Some(vec![1_u8])
            })
            .await;
        assert!(path.is_none(), "slow fetch must not block delivery");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
