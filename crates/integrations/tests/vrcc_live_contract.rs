//! 对真实星遇同频服务验证契约的测试。
//!
//! 这些测试访问 `https://vrcc.cc/api/v1/`，只用**不需要令牌**的端点。
//! 它们价值在于：契约是从文档抄来的，而文档和线上返回可能不一致——
//! 这里用真实响应把两者钉在一起。
//!
//! # 默认不跑
//!
//! **需要 `VRCC_LIVE=1` 才执行**。理由：
//!
//! - `cargo test --workspace` 会在三个平台的 CI 上各请求一次线上服务；
//!   线上返 5xx、返 HTML 错误页、或共享 runner 的 IP 被 `429` 限流时，
//!   这些测试会 panic，红的是我们的 CI，坏的是别人的服务；
//! - `status` 不是 `ok` 时 `status_endpoint_reports_ok` 必然失败，
//!   而停站维护完全是对方的事。
//!
//! 本地想验证真契约时：
//!
//! ```text
//! VRCC_LIVE=1 cargo test -p vrcx-0-integrations --test vrcc_live_contract
//! ```
//!
//! 需要令牌的端点不在这里测：它们要有人去网页上点同意，没法自动化。

use std::sync::Arc;

use vrcx_0_contracts::vrcc::VrccBoardSort;
use vrcx_0_integrations::vrcc::VrccClient;

/// 真跑线上测试的总开关。没设成 `1` 时所有用例直接返回。
const LIVE_ENV: &str = "VRCC_LIVE";

/// 没开开关就跳过。
///
/// 返回 `true` 表示「已跳过」，调用方应立刻 `return`。
/// 用宏而不是函数是因为要在测试函数体里提前返回，而不是嵌套一层。
macro_rules! skip_unless_live {
    () => {
        if std::env::var(LIVE_ENV).ok().as_deref() != Some("1") {
            eprintln!(
                "skip: live contract tests are opt-in; set {LIVE_ENV}=1 to run against vrcc.cc"
            );
            return;
        }
    };
}

/// 建一个没有令牌的客户端。公开数据端点的 `authed = false`，用不到它。
///
/// 走环境变量里的代理：本机实测 `vrcc.cc` 直连会 DNS 超时。
/// 没有代理变量时退回直连，所以在有直连网络的环境里也能跑。
fn client() -> VrccClient {
    let proxy = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .ok();
    VrccClient::new(
        vrcx_0_integrations::vrcc::VRCC_DEV_CLIENT,
        Arc::new(|| None),
        proxy.as_deref(),
    )
}

/// 网络不通时把测试当作跳过，而不是让 CI 红。
///
/// 区分方式：传输层失败算没网；其余（尤其是 `Decode`，说明拿到了响应但
/// 结构不对）算契约问题，必须让测试失败——那才是这个文件要抓的东西。
fn is_offline(error: &vrcx_0_integrations::vrcc::VrccError) -> bool {
    // 按层级判断，不按具体变体：`Decode` 现在也归 `Transport` 层，
    // 但它意味着「拿到了 2xx 却不是 JSON」，属于必须暴露的问题，
    // 所以这里额外排除掉。
    error.layer == vrcx_0_integrations::vrcc::ErrorLayer::Transport
        && !matches!(
            error.detail,
            vrcx_0_integrations::vrcc::ErrorDetail::Decode(_)
        )
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
    skip_unless_live!();
    let boards = skip_if_offline!(client().boards(3, VrccBoardSort::Top).await);

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
    skip_unless_live!();
    let boards = skip_if_offline!(client().boards(2, VrccBoardSort::Top).await);
    // 请求 2 条，线上不应回超过 2 条（榜单可能不足 2 条，所以用 <=）。
    assert!(
        boards.maps.len() <= 2,
        "asked for 2 maps, got {}",
        boards.maps.len()
    );
}

#[tokio::test]
async fn status_endpoint_reports_ok() {
    skip_unless_live!();
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

/// `device/code` 真实可用：带 scope 也要能拿到连接码。
///
/// 这条测试的存在理由是一次实际踩坑：文档说 `device/code` 认 JSON 正文里的
/// `scope`，但现网对它回 `400 bad_request`，只有查询串和表单才工作。
/// 当时只读文档就下了结论，结果应用里「开始连接」直接失败。
///
/// 这里用默认 scope（`None`）验证一次完整成功路径：能拿到 `user_code` 与
/// `device_code`，权串里包含 `profile`（官方保证总会加上）。
///
/// 注意：这条测试**会真的向服务器申请一个连接码**。服务器对每网络地址每天
/// 限 30 个，测试跑得勤会消耗额度，所以它只在 `VRCC_LIVE_DEVICE_CODE=1`
/// 时执行——默认不跑，避免把开发额度烧光。
#[tokio::test]
async fn device_code_round_trip_works_with_scope_in_query() {
    if std::env::var("VRCC_LIVE_DEVICE_CODE").ok().as_deref() != Some("1") {
        eprintln!("skip: set VRCC_LIVE_DEVICE_CODE=1 to run (consumes daily quota)");
        return;
    }
    let code = skip_if_offline!(
        client()
            .device_code(Some("profile notifications maps"))
            .await
    );

    assert!(
        code.user_code.contains('-'),
        "user_code should look like ABCD-2345, got {:?}",
        code.user_code
    );
    assert!(
        !code.device_code.is_empty(),
        "device_code must not be empty"
    );
    assert!(
        code.expires_in.unwrap_or(0) > 0,
        "expires_in should be positive"
    );
    // 官方保证 `profile` 总会加上。
    let scope = code.scope.unwrap_or_default();
    assert!(
        scope.split_whitespace().any(|part| part == "profile"),
        "scope should include `profile`, got {scope:?}"
    );
}
