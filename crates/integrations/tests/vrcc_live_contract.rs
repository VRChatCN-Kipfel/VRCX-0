//! 对真实星遇同频服务验证契约的测试。
//!
//! 这些测试访问 `https://vrcc.cc/api/v1/`，只用**不需要令牌**的端点，
//! 所以不需要配置任何凭据就能跑。它们价值在于：契约是从文档抄来的，
//! 而文档和线上返回可能不一致——这里用真实响应把两者钉在一起。
//!
//! 默认会跑（不是 `--ignored`），但网络不通时应当跳过而不是判失败：
//! 一个离线环境不该让「接口形状变了」和「没联网」看起来一样。
//!
//! 需要令牌的端点不在这里测：它们要有人去网页上点同意，没法自动化。

use std::sync::Arc;

use vrcx_0_integrations::vrcc::VrccClient;

/// 建一个没有令牌的客户端。公开数据端点的 `authed = false`，用不到它。
fn client() -> VrccClient {
    VrccClient::new("dev-vrcx0", Arc::new(|| None))
}

/// 网络不通时把测试当作跳过，而不是让 CI 红。
///
/// 区分方式：抓取失败（`Transport`）算没网；解析失败（`Decode`）算契约问题。
fn is_offline(error: &vrcx_0_integrations::vrcc::VrccError) -> bool {
    matches!(error, vrcx_0_integrations::vrcc::VrccError::Transport(_))
}

macro_rules! skip_if_offline {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) if is_offline(&error) => {
                eprintln!("skip: network unavailable ({error})");
                return;
            }
            Err(error) => panic!("contract mismatch against live API: {error}"),
        }
    };
}

#[tokio::test]
async fn boards_live_response_parses_into_the_contract() {
    let boards = skip_if_offline!(client().boards(3).await);

    assert_eq!(boards.board.as_deref(), Some("maps"));
    // 至少能拿到条目；空榜单也算合法，所以不强求非空。
    for map in &boards.maps {
        // 官方 `BoardMap` 的必填字段：world / name / author / avg / raters /
        // rank / weight / page / img。我们只断言真的用到的那些，
        // 其余允许为 None（文档标了可空）。
        assert!(
            map.world.starts_with("wrld_"),
            "world id should be a wrld_ id, got {:?}",
            map.world
        );
        if let Some(avg) = map.avg {
            assert!(
                (0.0..=10.0).contains(&avg),
                "avg should be within 1..=10, got {avg}"
            );
        }
        if let Some(rank) = map.rank {
            assert!(rank >= 1, "rank should be 1-based, got {rank}");
        }
        if let Some(raters) = map.raters {
            assert!(raters >= 0, "raters should not be negative, got {raters}");
        }
        // `hot` 是文档未记录的字段，但线上有。能解析出来说明契约接得住；
        // 万一哪天服务端去掉了，`None` 也不报错。
        if let Some(hot) = &map.hot {
            assert!(
                hot.text.is_some() || hot.by.is_some(),
                "hot should carry at least a text or a by line"
            );
        }
    }
}

#[tokio::test]
async fn boards_limit_is_honoured() {
    let boards = skip_if_offline!(client().boards(2).await);
    // 请求 2 条，线上不应回超过 2 条（榜单可能不足 2 条，所以用 <=）。
    assert!(
        boards.maps.len() <= 2,
        "asked for 2 maps, got {}",
        boards.maps.len()
    );
}

#[tokio::test]
async fn status_endpoint_reports_ok() {
    let status = skip_if_offline!(client().status().await);
    // 官方定义：`state` 为 `ok` 表示网站与接口都可用。
    // 服务真的挂了才会不是 ok，那时这个断言会提醒我们。
    assert_eq!(
        status.state.as_deref(),
        Some("ok"),
        "vrcc.cc status is not ok: {:?}",
        status
    );
}
