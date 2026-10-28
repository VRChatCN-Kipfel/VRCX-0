//! 不联网的契约测试：用官方文档的示例回答喂契约。
//!
//! 示例来自星遇同频开放接口文档仓库的 `examples/responses.json`（离线生成的
//! 假数据，MIT）。每个接口测三种形状：
//!
//! - **旧服务器（1.8.x）**：没有 1.11.0 以后加的字段；
//! - **现在（1.14.0）**：新字段都有，弃用中的字段也还在；
//! - **弃用字段停了以后**（2026-11-26 / 2026-12-26）：`vibe`、`can`、`need` 等不再返回。
//!
//! 三种都要能解析。官方的规则是「v1 只做加法、弃用字段至少留 60 天」，
//! 这里钉住的就是客户端这一侧：新字段当可选、不依赖弃用字段。

use vrcx_0_contracts::vrcc::{
    VrccAnnouncements, VrccBoard, VrccErrorBody, VrccMutuals, VrccMyMap, VrccNotes,
    VrccRecentNotifications, VrccToday, VrccTokenResponse, VrccUnread,
};

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_else(|error| panic!("should parse: {error}\n{json}"))
}

#[test]
fn boards_parse_old_new_and_week_shapes() {
    // 1.8.x：没有 `sort`。
    let old: VrccBoard = parse(
        r#"{"board":"maps","min":1,"maps":[{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1","name":"星夜茶屋","author":"某位作者","avg":8.5,"raters":2,"rank":1,"weight":"mid","page":"https://vrcc.cc/maps/wrld_aaaaaaaa-0000-4000-8000-0000000000a1","img":"https://vrcc.cc/maps/wrld_aaaaaaaa-0000-4000-8000-0000000000a1.jpg"}],"at":1793038216765}"#,
    );
    assert_eq!(old.sort, None);
    assert_eq!(old.maps[0].week, None);

    // 1.13.0 本周榜：多 `sort` 和每张的 `week`。
    let week: VrccBoard = parse(
        r#"{"board":"maps","sort":"week","min":1,"maps":[{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1","name":"星夜茶屋","author":"某位作者","hot":{"text":"光影很舒服","by":"某位彩蛋猎人","up":0,"went":false},"avg":8.5,"raters":2,"rank":1,"weight":"mid","page":"https://vrcc.cc/maps/wrld_aaaaaaaa-0000-4000-8000-0000000000a1","img":"https://vrcc.cc/maps/wrld_aaaaaaaa-0000-4000-8000-0000000000a1.jpg","week":2}],"at":1793038216765}"#,
    );
    assert_eq!(week.sort.as_deref(), Some("week"));
    assert_eq!(week.maps[0].week, Some(2));

    // 本周榜只按本周的打分排，`rank` 照旧是总榜名次：列表第一张可以是总榜第 7。
    let reordered: VrccBoard = parse(
        r#"{"board":"maps","sort":"week","min":1,"maps":[{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a7","rank":7,"week":5},{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1","rank":1,"week":2}],"at":1793038216765}"#,
    );
    assert_eq!(
        reordered
            .maps
            .iter()
            .map(|map| map.rank)
            .collect::<Vec<_>>(),
        vec![Some(7), Some(1)]
    );
    assert_eq!(
        week.maps[0]
            .hot
            .as_ref()
            .and_then(|hot| hot.text.as_deref()),
        Some("光影很舒服")
    );

    // 新上榜：还没人评分的图 `rank`、`avg` 是 `null`。
    let new: VrccBoard = parse(
        r#"{"board":"maps","sort":"new","min":1,"maps":[{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a2","name":"新图","author":null,"hot":null,"avg":null,"raters":0,"rank":null,"weight":null,"page":"https://vrcc.cc/maps/wrld_aaaaaaaa-0000-4000-8000-0000000000a2","img":null}],"at":1793038216767}"#,
    );
    assert_eq!(new.maps[0].rank, None);
    assert_eq!(new.maps[0].avg, None);
}

#[test]
fn my_map_parses_with_and_without_the_deprecated_fields() {
    // 1.8.x：有 `can` / `need`，没有 `imgUrl`。
    let old: VrccMyMap = parse(
        r#"{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1","name":"星夜茶屋","board":true,"pending":false,"off":false,"avg":8.5,"raters":2,"mine":9,"can":true,"need":null,"words":"光影很舒服","wordsOk":"ok","img":"/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1"}"#,
    );
    assert_eq!(old.mine, Some(9));
    assert_eq!(
        old.image_url().as_deref(),
        Some("https://vrcc.cc/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1")
    );

    // 1.14.0：多 `imgUrl`，`can` / `need` 还在（弃用中）。
    let now: VrccMyMap = parse(
        r#"{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1","name":"星夜茶屋","board":true,"pending":false,"off":false,"avg":8.5,"raters":2,"mine":9,"can":true,"need":null,"words":"光影很舒服","wordsOk":"ok","img":"/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1","imgUrl":"https://vrcc.cc/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1"}"#,
    );
    assert_eq!(
        now.img_url.as_deref(),
        Some("https://vrcc.cc/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1")
    );

    // 2026-12-26 以后：`can` / `need` 不再返回。
    let after: VrccMyMap = parse(
        r#"{"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1","name":"星夜茶屋","board":true,"pending":false,"off":false,"avg":8.5,"raters":2,"mine":null,"words":null,"wordsOk":null,"img":"/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1","imgUrl":"https://vrcc.cc/api/match?xdimg=wrld_aaaaaaaa-0000-4000-8000-0000000000a1"}"#,
    );
    assert!(after.is_visible());
    assert_eq!(after.mine, None);
}

#[test]
fn mutuals_parse_rate_when_given_and_when_not() {
    let now: VrccMutuals = parse(
        r#"{"stars":[{"n":2,"name":"彩蛋猎人","group":"G","star":102,"vrc":"Player2","vrcUser":null,"at":1792865416696,"rate":{"played":true,"mine":4}},{"n":3,"name":"拾光旅人","group":"L","star":null,"vrc":"Player3","vrcUser":"usr_00000000-0000-4000-8000-000000000003","at":1792865416696,"rate":{"played":null,"mine":4.5}},{"n":4,"name":"夜航","group":"W","star":null,"vrc":"Player4","vrcUser":null,"at":1793038216696,"rate":null}],"n":3}"#,
    );
    let rate = now.stars[0].rate.as_ref().expect("rate");
    assert_eq!(rate.played, Some(true));
    assert_eq!(rate.mine, Some(4.0));
    // 半颗星一档。
    assert_eq!(
        now.stars[1].rate.as_ref().and_then(|rate| rate.mine),
        Some(4.5)
    );
    assert_eq!(
        now.stars[1].rate.as_ref().and_then(|rate| rate.played),
        None
    );
    // 连上当天是 `null`。
    assert!(now.stars[2].rate.is_none());

    // 1.11.0 以前没有 `rate`。
    let old: VrccMutuals = parse(
        r#"{"stars":[{"n":2,"name":"彩蛋猎人","group":"G","star":102,"vrc":"Player2","vrcUser":null,"at":1792865416696}],"n":1}"#,
    );
    assert!(old.stars[0].rate.is_none());
}

#[test]
fn star_cards_parse_with_and_without_vibe() {
    let card = r#"{"n":3,"pos":"G1","name":"彩蛋猎人","line":"进了新世界先去找隐藏房间。","group":"G","groupName":"漫游星群",VIBE"intro":"喜欢在安静的世界里散步。","about":[],"common":["你们都在漫游星群"],"photo":false,"rating":null,"star":null,"verified":false,"scores":{"G":50,"W":50,"Z":50,"L":50},"deco":null,"sprout":true,"why":"cross","linked":false,"mutual":false}"#;
    let round =
        r#""round":{"at":1793038216809,"next":1793117416809,"used":0,"max":6,"remind":false}"#;
    // 现在带 `vibe`，2026-12-26 以后不带。
    for vibe in [r#""vibe":"随场","#, ""] {
        let json = format!(
            r##"{{"day":"#1","cards":[{}],{round}}}"##,
            card.replace("VIBE", vibe)
        );
        let today: VrccToday = parse(&json);
        assert_eq!(today.cards.len(), 1);
        assert_eq!(today.cards[0].group_name.as_deref(), Some("漫游星群"));
        assert_eq!(today.round.max, Some(6));
    }
}

#[test]
fn unread_notes_and_announcements_parse() {
    let unread: VrccUnread = parse(r#"{"inbox":2,"cross":1,"chats":1,"notes":2}"#);
    assert_eq!(unread.cross, Some(1));
    assert_eq!(unread.total(), 6);
    // 令牌没有 `crossings`（比如 `dev-` 标识）：服务器不给 `cross` 这个键，
    // 要和「有、但是 0」分开。
    let no_cross: VrccUnread = parse(r#"{"inbox":2,"chats":1,"notes":2}"#);
    assert_eq!(no_cross.cross, None);
    assert_eq!(no_cross.total(), 5);
    // 以后多一处红点也不该坏。
    let more: VrccUnread = parse(r#"{"inbox":0,"cross":0,"chats":0,"notes":0,"later":9}"#);
    assert_eq!(more.total(), 0);

    let notes: VrccNotes = parse(
        r#"{"notes":[{"id":3,"kind":"map","text":"你刚去了「星夜茶馆」，给它打个分吧。","at":1793038216696,"seen":true,"post":null,"world":"wrld_aaaaaaaa-0000-4000-8000-0000000000a1"},{"id":2,"kind":"yq","text":"有颗星在你的星友圈动态下留了言。","at":1793038216696,"seen":false,"post":1,"world":null},{"id":1,"kind":"gift","text":"一颗互相连上的星送了你一件礼物，在「我的仓库」里。","at":1793038216696,"seen":false,"post":null,"world":null}]}"#,
    );
    assert_eq!(notes.notes.len(), 3);
    assert_eq!(
        notes.notes[0].world.as_deref(),
        Some("wrld_aaaaaaaa-0000-4000-8000-0000000000a1")
    );
    assert_eq!(notes.notes[1].post, Some(1));
    assert_eq!(notes.notes[2].seen, Some(false));

    let open: VrccAnnouncements = parse(
        r#"{"stop":null,"posts":[{"id":1,"level":"pop","title":"地图榜开放打分","body":"去过的地图都可以打 1–10 分。","link":"https://vrcc.cc/match/?tab=maps","cover":"https://vrcc.cc/assets/posts/maps.webp","end":1793643016696}]}"#,
    );
    assert!(open.stop.is_none());
    assert_eq!(open.posts[0].level.as_deref(), Some("pop"));

    let stopped: VrccAnnouncements =
        parse(r#"{"stop":{"say":"升级数据库","until":null},"posts":[]}"#);
    assert_eq!(
        stopped.stop.as_ref().and_then(|stop| stop.say.as_deref()),
        Some("升级数据库")
    );
}

#[test]
fn recent_and_token_parse() {
    let recent: VrccRecentNotifications = parse(
        r#"{"items":[{"id":1,"title":"星遇同频","body":"有颗星连了你。","url":"/match/?tab=inbox","at":1793038216696}],"read":0}"#,
    );
    assert_eq!(
        recent.items[0].absolute_url(),
        "https://vrcc.cc/match/?tab=inbox"
    );

    let token: VrccTokenResponse = parse(
        r#"{"access_token":"31e7abc6884855a29b82f7191a684590d65f98d32befeea7","token_type":"Bearer","scope":"profile notifications maps","me":{"star":101,"vrc":null}}"#,
    );
    assert_eq!(token.me.and_then(|me| me.star), Some(101));
}

#[test]
fn error_bodies_carry_stop_and_every() {
    let stop: VrccErrorBody =
        parse(r#"{"error":"stop","say":"升级数据库，约半小时","until":1793040000000}"#);
    assert_eq!(stop.error.as_deref(), Some("stop"));
    assert_eq!(stop.until, Some(1_793_040_000_000));

    let soon: VrccErrorBody = parse(r#"{"error":"too_soon","every":60}"#);
    assert_eq!(soon.every, Some(60));
}

#[test]
fn a_malformed_hot_comment_does_not_break_the_whole_board() {
    // `hot` 不在官方 openapi 的 `BoardMap` 里，是 1.10.1 才写进文档的附加字段，
    // 随时可能改形状或下线。它坏掉时**只能少显示一句短评**，不能让整份
    // 地图榜解析失败——那会让榜单整页空白、腕表名次也不再更新。
    let board: VrccBoard = parse(
        r#"{"board":"maps","min":1,"sort":"top","maps":[
            {"world":"wrld_b1","name":"形状变了","avg":8.5,"raters":2,"rank":1,"hot":{"text":123,"by":["a"],"up":"7","went":"yes"}},
            {"world":"wrld_b2","name":"正常","avg":9.0,"raters":3,"rank":2,"hot":{"text":"很棒","by":"某位倾听者","up":9,"went":true}},
            {"world":"wrld_b3","name":"没有热评","avg":7.0,"raters":1,"rank":3}
        ],"at":1793038216765}"#,
    );

    // 三个条目都要在：形状坏掉只影响它自己那一句短评。
    assert_eq!(board.maps.len(), 3);

    // 数字被当成字符串读出来；确实是字符串的才留。
    let broken = board.maps[0].hot.as_ref().expect("hot exists");
    assert_eq!(broken.text.as_deref(), Some("123"));
    assert_eq!(broken.by, None, "数组不是字符串，当作没有");
    assert_eq!(broken.up, Some(7), "字符串里的数字能读出来");
    assert_eq!(broken.went, None, "\"yes\" 不是 bool，当作没有");

    let normal = board.maps[1].hot.as_ref().expect("hot exists");
    assert_eq!(normal.text.as_deref(), Some("很棒"));
    assert_eq!(normal.went, Some(true));

    assert!(board.maps[2].hot.is_none());
}

/// 星轨相交：列表要能解析，且 `why` 的七种取值都要分类正确（外加未知值）。
///
/// `why` 的分类**直接影响界面行为**：`hide` 要求客户端把整个模块藏起来
/// （16–17 岁星池、或玩家在隐私设置里关了星轨相交），而 `verify` / `wait`
/// 只是「还没准备好」，要显示原因而不是隐藏。两者搞反的后果不对称——
/// 把 `hide` 当 `verify` 会向不该看到的人暴露「这里有星轨相交」这个事实。
#[test]
fn crossings_parse_and_the_why_reasons_classify_correctly() {
    use vrcx_0_contracts::vrcc::{VrccCrossingWhy, VrccCrossings};

    // 正常一条：有擦肩记录，还没连线。
    let list: VrccCrossings = parse(
        r#"{"stars":[
            {"n":77,"name":"拾光旅人","line":"放慢脚步逛世界","group":"g1","star":31,
             "intro":"平时喜欢在安静的世界拍照、听音乐","vrc":null,
             "linked":false,"mutual":false,"at":1792830000000,"fresh":true},
            {"n":78,"name":"已连上的","group":"g1","vrc":"StarTraveler",
             "linked":true,"mutual":true,"at":1792830000001,"fresh":false}
        ],"n":2,"why":null,"me":{"n":12,"name":"夜航星"}}"#,
    );

    assert_eq!(list.stars.len(), 2);
    assert_eq!(list.why, None);

    let first = &list.stars[0];
    assert_eq!(first.n, 77);
    assert_eq!(first.name.as_deref(), Some("拾光旅人"));
    assert_eq!(first.line.as_deref(), Some("放慢脚步逛世界"));
    assert_eq!(first.intro.as_deref(), Some("平时喜欢在安静的世界拍照、听音乐"));
    assert!(!first.linked && !first.mutual);
    assert!(first.fresh);
    // 未双向连线时 `vrc` 是 null——这是「连上才解锁」的信息。
    assert!(first.vrc.is_none());

    let second = &list.stars[1];
    assert!(second.linked && second.mutual);
    assert_eq!(second.vrc.as_deref(), Some("StarTraveler"));

    assert_eq!(list.me.as_ref().and_then(|me| me.n), Some(12));
    assert_eq!(list.me.as_ref().and_then(|me| me.name.as_deref()), Some("夜航星"));

    // 七种 why 取值 + 未知值 + 缺省值。
    assert_eq!(VrccCrossingWhy::parse(None), VrccCrossingWhy::Available);
    assert_eq!(VrccCrossingWhy::parse(Some("")), VrccCrossingWhy::Available);
    assert_eq!(VrccCrossingWhy::parse(Some("hide")), VrccCrossingWhy::Hidden);
    assert_eq!(
        VrccCrossingWhy::parse(Some("verify")),
        VrccCrossingWhy::Unverified
    );
    assert_eq!(
        VrccCrossingWhy::parse(Some("wait")),
        VrccCrossingWhy::WaitingVerify
    );
    assert_eq!(VrccCrossingWhy::parse(Some("queued")), VrccCrossingWhy::Queued);
    assert_eq!(VrccCrossingWhy::parse(Some("sent")), VrccCrossingWhy::Sent);
    assert_eq!(VrccCrossingWhy::parse(Some("gone")), VrccCrossingWhy::Gone);
    assert_eq!(
        VrccCrossingWhy::parse(Some("friend")),
        VrccCrossingWhy::NotFriend
    );
    // 服务器以后加的新原因：认得成 Unknown，但不能解析失败。
    assert_eq!(
        VrccCrossingWhy::parse(Some("something-new")),
        VrccCrossingWhy::Unknown
    );

    // 只有 `hide` 要求整个模块隐藏。
    assert!(!VrccCrossingWhy::Hidden.should_show());
    for reason in [
        VrccCrossingWhy::Available,
        VrccCrossingWhy::Unverified,
        VrccCrossingWhy::WaitingVerify,
        VrccCrossingWhy::Queued,
        VrccCrossingWhy::Sent,
        VrccCrossingWhy::Gone,
        VrccCrossingWhy::NotFriend,
        VrccCrossingWhy::Unknown,
    ] {
        assert!(reason.should_show(), "{reason:?} should stay visible");
    }

    // 需要先去 VRChat 侧准备的那几种，提示要走向「去验证/去加机器人」
    // 而不是「去开权限」。
    for reason in [
        VrccCrossingWhy::Unverified,
        VrccCrossingWhy::WaitingVerify,
        VrccCrossingWhy::Queued,
        VrccCrossingWhy::Sent,
        VrccCrossingWhy::Gone,
        VrccCrossingWhy::NotFriend,
    ] {
        assert!(reason.needs_vrchat_setup(), "{reason:?} needs setup");
    }
    assert!(!VrccCrossingWhy::Available.needs_vrchat_setup());
    assert!(!VrccCrossingWhy::Hidden.needs_vrchat_setup());

    // 旧服务器/空列表也要能解析（字段全缺省）。
    let empty: VrccCrossings = parse(r#"{"stars":[],"n":0,"why":null,"me":null}"#);
    assert!(empty.stars.is_empty());
    assert!(empty.me.is_none());
}
