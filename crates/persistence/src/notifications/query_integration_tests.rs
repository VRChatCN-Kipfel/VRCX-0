use std::path::PathBuf;

use serde_json::json;

use super::{
    notification_add_v1, notification_add_v2, notification_friend_requests_sync,
    notification_has_unseen_action_required, notification_has_unseen_action_required_excluding,
    notification_list_query, notification_mark_seen, NotificationListItemOutput,
    NotificationListQueryInput,
};
use crate::{DatabaseService, Error};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vrcx-0-notification-query-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn test_db(name: &str) -> Result<(TestDir, DatabaseService), Error> {
    let dir = TestDir::new(name);
    let db = DatabaseService::new(&dir.path.join("VRCX-0.sqlite3"))?;
    Ok((dir, db))
}

fn add_v1(
    db: &DatabaseService,
    user_id: &str,
    id: &str,
    created_at: &str,
    message: &str,
) -> Result<(), Error> {
    notification_add_v1(
        db,
        user_id.into(),
        json!({
            "id": id,
            "created_at": created_at,
            "type": "invite",
            "message": message,
            "senderUserId": "usr_sender_v1",
            "senderUsername": "Legacy Sender",
            "details": { "worldId": "wrld_v1" }
        }),
    )
}

fn add_v2(
    db: &DatabaseService,
    user_id: &str,
    id: &str,
    created_at: &str,
    seen: bool,
    expires_at: &str,
) -> Result<(), Error> {
    notification_add_v2(
        db,
        user_id.into(),
        json!({
            "id": id,
            "createdAt": created_at,
            "updatedAt": created_at,
            "expiresAt": expires_at,
            "type": "invite",
            "message": format!("message-{id}"),
            "title": format!("title-{id}"),
            "seen": seen,
            "senderUserId": "usr_sender_v2",
            "senderUsername": "Current Sender",
            "data": {},
            "responses": [],
            "details": { "worldId": "wrld_v2" }
        }),
    )
}

fn query(
    db: &DatabaseService,
    user_id: &str,
    per_table_limit: i64,
    limit: i64,
    include_unseen: bool,
) -> Result<Vec<NotificationListItemOutput>, Error> {
    notification_list_query(
        db,
        NotificationListQueryInput {
            user_id: user_id.into(),
            search: String::new(),
            filters: Vec::new(),
            per_table_limit,
            limit,
            include_unseen,
        },
    )
}

#[test]
fn merged_notification_list_keeps_original_v2_precedence_and_global_order() -> Result<(), Error> {
    let (_dir, db) = test_db("merge")?;
    let user_id = "usr_owner";
    add_v1(
        &db,
        user_id,
        "notif_shared",
        "2026-01-03T12:00:00Z",
        "legacy duplicate",
    )?;
    add_v2(
        &db,
        user_id,
        "notif_shared",
        "2026-01-01T10:00:00Z",
        true,
        "2099-01-01T00:00:00Z",
    )?;
    add_v1(
        &db,
        user_id,
        "notif_other",
        "2026-01-02T11:00:00Z",
        "legacy other",
    )?;

    let rows = query(&db, user_id, 10, 10, false)?;

    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        vec!["notif_other", "notif_shared"]
    );
    let shared = rows.iter().find(|row| row.id == "notif_shared").unwrap();
    assert_eq!(shared.version, 2);
    assert_eq!(shared.title, "title-notif_shared");
    assert_eq!(shared.sender_username, "Current Sender");
    Ok(())
}

#[test]
fn unseen_expansion_keeps_unexpired_rows_beyond_the_per_table_window() -> Result<(), Error> {
    let (_dir, db) = test_db("unseen")?;
    let user_id = "usr_owner";
    add_v2(
        &db,
        user_id,
        "seen-newest",
        "2026-03-03T00:00:00Z",
        true,
        "2099-01-01T00:00:00Z",
    )?;
    add_v2(
        &db,
        user_id,
        "unseen-older",
        "2026-03-02T00:00:00Z",
        false,
        "2099-01-01T00:00:00Z",
    )?;
    add_v2(
        &db,
        user_id,
        "unseen-expired",
        "2026-03-01T00:00:00Z",
        false,
        "2000-01-01T00:00:00Z",
    )?;

    assert_eq!(
        query(&db, user_id, 1, 10, false)?
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["seen-newest"]
    );
    assert_eq!(
        query(&db, user_id, 1, 10, true)?
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["seen-newest", "unseen-older"]
    );
    Ok(())
}

#[test]
fn notification_list_applies_final_limit_with_stable_id_ties() -> Result<(), Error> {
    let (_dir, db) = test_db("limit")?;
    let user_id = "usr_owner";
    for id in ["notif_a", "notif_c", "notif_b"] {
        add_v1(&db, user_id, id, "2026-04-01T00:00:00Z", "same timestamp")?;
    }

    let rows = query(&db, user_id, 10, 2, false)?;

    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        vec!["notif_c", "notif_b"]
    );
    Ok(())
}

#[test]
fn notification_tables_remain_scoped_to_the_requested_account() -> Result<(), Error> {
    let (_dir, db) = test_db("account-scope")?;
    add_v1(
        &db,
        "usr_owner_a",
        "notif_a",
        "2026-05-01T00:00:00Z",
        "owner a",
    )?;
    add_v1(
        &db,
        "usr_owner_b",
        "notif_b",
        "2026-05-02T00:00:00Z",
        "owner b",
    )?;

    let rows = query(&db, "usr_owner_a", 10, 10, true)?;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "notif_a");
    Ok(())
}

#[test]
fn seen_v1_friend_request_stays_seen_when_remote_sync_keeps_it_active() -> Result<(), Error> {
    let (_dir, db) = test_db("v1-friend-request-seen-sync")?;
    let user_id = "usr_owner";
    let friend_request = json!({
        "id": "notif_friend_request",
        "createdAt": "2026-08-20T11:00:00Z",
        "type": "friendRequest",
        "senderUserId": "usr_sender",
        "senderUsername": "Sender"
    });
    notification_add_v1(&db, user_id.into(), friend_request.clone())?;
    notification_mark_seen(&db, user_id.into(), "notif_friend_request".into(), 1)?;

    notification_friend_requests_sync(
        &db,
        user_id.into(),
        vec![friend_request],
        true,
        Vec::new(),
        true,
    )?;

    let rows = query(&db, user_id, 10, 10, true)?;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].seen);
    assert!(!rows[0].expired);
    Ok(())
}

#[test]
fn instance_closed_keeps_its_location_after_a_reload() -> Result<(), Error> {
    let (_dir, db) = test_db("instance-closed-location")?;
    let user_id = "usr_owner";
    notification_add_v1(
        &db,
        user_id.into(),
        json!({
            "id": "notif_instance_closed",
            "created_at": "2026-09-07T02:27:00Z",
            "type": "instance.closed",
            "location": "wrld_home:12345"
        }),
    )?;

    let rows = query(&db, user_id, 10, 10, true)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].location, "wrld_home:12345");
    Ok(())
}

#[test]
fn unseen_indicator_matches_frontend_action_required_rules() -> Result<(), Error> {
    let (_dir, db) = test_db("unseen-indicator")?;
    let user_id = "usr_owner";
    add_v1(
        &db,
        user_id,
        "legacy_invite",
        "2026-09-01T00:00:00Z",
        "invite",
    )?;
    assert!(!notification_has_unseen_action_required(&db, user_id)?);

    notification_add_v1(
        &db,
        user_id.into(),
        json!({
            "id": "friend_request",
            "createdAt": "2026-09-01T00:00:01Z",
            "type": "friendRequest",
            "senderUserId": "usr_sender"
        }),
    )?;
    assert!(notification_has_unseen_action_required(&db, user_id)?);

    notification_mark_seen(&db, user_id.into(), "friend_request".into(), 1)?;
    assert!(!notification_has_unseen_action_required(&db, user_id)?);

    add_v2(
        &db,
        user_id,
        "unseen_v2",
        "2026-09-01T00:00:02Z",
        false,
        "2099-01-01T00:00:00Z",
    )?;
    assert!(notification_has_unseen_action_required(&db, user_id)?);

    notification_mark_seen(&db, user_id.into(), "unseen_v2".into(), 2)?;
    add_v2(
        &db,
        user_id,
        "expired_v2",
        "2026-09-01T00:00:03Z",
        false,
        "2000-01-01T00:00:00Z",
    )?;
    assert!(!notification_has_unseen_action_required(&db, user_id)?);
    Ok(())
}

/// 站外提醒（id 以 `vrcc_` 开头）不该点亮托盘。
///
/// 托盘红点的语义是「有件事在等你处理」。星遇同频的提醒是「有人连向了你」
/// 这类读一眼就好的信息，挂上去会让图标常亮成一盏状态灯；而 v2 表此前对
/// type 完全不设过滤，任何未读行都会命中。
#[test]
fn out_of_app_notices_do_not_light_the_unseen_indicator() -> Result<(), Error> {
    let (_dir, db) = test_db("unseen-indicator-vrcc")?;
    let user_id = "usr_owner";

    add_v2(&db, user_id, "vrcc_42", "2026-09-01T00:00:02Z", false, "")?;
    // 站外提醒是未读、且永不过期，但仍然不算「需要处理」。
    assert!(!notification_has_unseen_action_required(&db, user_id)?);

    // 真正的待办出现时照样要亮。
    add_v2(
        &db,
        user_id,
        "unseen_v2",
        "2026-09-01T00:00:03Z",
        false,
        "2099-01-01T00:00:00Z",
    )?;
    assert!(notification_has_unseen_action_required(&db, user_id)?);

    // 标掉待办之后，只剩站外提醒，红点要灭。
    notification_mark_seen(&db, user_id.into(), "unseen_v2".into(), 2)?;
    assert!(!notification_has_unseen_action_required(&db, user_id)?);
    Ok(())
}

/// 排除前缀由调用方给，空串必须被丢弃。
///
/// `NOT LIKE '%'` 会匹配所有行，等于把托盘永久关死——这是最坏的失败方向：
/// 玩家再也看不到任何好友请求的红点，而且不会报错。
#[test]
fn a_blank_excluded_prefix_does_not_disable_the_indicator() -> Result<(), Error> {
    let (_dir, db) = test_db("unseen-indicator-blank-prefix")?;
    let user_id = "usr_owner";

    add_v2(
        &db,
        user_id,
        "unseen_v2",
        "2026-09-01T00:00:02Z",
        false,
        "2099-01-01T00:00:00Z",
    )?;

    assert!(notification_has_unseen_action_required_excluding(
        &db,
        user_id,
        &[""]
    )?);

    // 多个前缀并列时每个各占一个绑定参数，互不吞并。
    add_v2(&db, user_id, "vrcc_7", "2026-09-01T00:00:03Z", false, "")?;
    add_v2(&db, user_id, "other_9", "2026-09-01T00:00:04Z", false, "")?;
    assert!(notification_has_unseen_action_required_excluding(
        &db,
        user_id,
        &["vrcc_", "other_"]
    )?);

    // 只剩被排除的两类时，红点应当灭掉。
    notification_mark_seen(&db, user_id.into(), "unseen_v2".into(), 2)?;
    assert!(!notification_has_unseen_action_required_excluding(
        &db,
        user_id,
        &["vrcc_", "other_"]
    )?);
    Ok(())
}
