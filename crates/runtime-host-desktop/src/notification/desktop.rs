use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use vrcx_0_application_activity::notification::RenderedNotification;
use vrcx_0_core::location::world_id_from_location;
use vrcx_0_core::vrchat_ids::{is_group_id, is_user_id};

use super::NotificationDeliveryPreferences;
use vrcx_0_contracts::activity::ActivityKind;
use vrcx_0_core::OwnerId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopNotificationAction {
    pub owner_user_id: OwnerId,
    pub target: DesktopNotificationTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DesktopNotificationTarget {
    OpenUserProfile {
        #[serde(rename = "userId")]
        user_id: String,
    },
    OpenGroupProfile {
        #[serde(rename = "groupId")]
        group_id: String,
    },
    OpenLocation {
        location: String,
    },
    /// 星遇同频的提醒。点击后打开星遇同频页签。
    ///
    /// 只带一个可选的提醒 id：vrcc 的提醒没有 VRChat 的用户/世界 ID，
    /// 无法复用上面三种落点。带 id 是为了将来能定位到具体那一条，
    /// 现在前端只需要跳到页签。
    OpenVrcc {
        notice_id: Option<String>,
    },
}

impl DesktopNotificationAction {
    pub fn open_user_profile(owner_user_id: &OwnerId, user_id: &str) -> Option<Self> {
        if !is_user_id(owner_user_id.as_str()) || !is_user_id(user_id) {
            return None;
        }
        Some(Self {
            owner_user_id: owner_user_id.clone(),
            target: DesktopNotificationTarget::OpenUserProfile {
                user_id: user_id.to_string(),
            },
        })
    }

    pub fn open_location(owner_user_id: &OwnerId, location: &str) -> Option<Self> {
        let location = location.trim();
        if !is_user_id(owner_user_id.as_str()) || world_id_from_location(location).is_empty() {
            return None;
        }
        Some(Self {
            owner_user_id: owner_user_id.clone(),
            target: DesktopNotificationTarget::OpenLocation {
                location: location.to_string(),
            },
        })
    }

    pub fn open_group_profile(owner_user_id: &OwnerId, group_id: &str) -> Option<Self> {
        if !is_user_id(owner_user_id.as_str()) || !is_group_id(group_id) {
            return None;
        }
        Some(Self {
            owner_user_id: owner_user_id.clone(),
            target: DesktopNotificationTarget::OpenGroupProfile {
                group_id: group_id.to_string(),
            },
        })
    }

    /// 星遇同频提醒的落点。
    ///
    /// 和上面三个不同，这里**不校验 VRChat ID**——vrcc 的提醒本来就没有
    /// VRChat 用户/世界 ID，它的身份是 `owner`（当前登录的 VRChat 账号）。
    /// 仍然要求 owner 是合法用户 ID，因为点击跳转要知道是哪个账号的会话。
    pub fn open_vrcc(owner_user_id: &OwnerId, notice_id: Option<&str>) -> Option<Self> {
        if !is_user_id(owner_user_id.as_str()) {
            return None;
        }
        Some(Self {
            owner_user_id: owner_user_id.clone(),
            target: DesktopNotificationTarget::OpenVrcc {
                notice_id: notice_id
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string),
            },
        })
    }
}

pub trait DesktopNotifier: Send + Sync {
    fn show(
        &self,
        title: &str,
        body: Option<&str>,
        image: Option<&str>,
        play_sound: bool,
        action: Option<&DesktopNotificationAction>,
    ) -> Result<(), String>;
}

#[derive(Clone, Default)]
pub struct DesktopNotifierSlot {
    inner: Arc<Mutex<Option<Arc<dyn DesktopNotifier>>>>,
}

impl DesktopNotifierSlot {
    /// 取出已挂载的通知桥。
    ///
    /// 给需要直接投递的后台任务用（例如星遇同频轮询）。没有挂载时返回
    /// `None`——Headless 就是这种情况，调用方应当降级而不是把它当错误。
    pub fn inner_notifier(&self) -> Option<Arc<dyn DesktopNotifier>> {
        match self.inner.lock() {
            Ok(slot) => slot.clone(),
            Err(_) => None,
        }
    }

    pub fn set(&self, notifier: Arc<dyn DesktopNotifier>) {
        match self.inner.lock() {
            Ok(mut slot) => {
                *slot = Some(notifier);
            }
            Err(error) => {
                tracing::warn!("failed to set desktop notification bridge: {error}");
            }
        }
    }
}

impl DesktopNotifier for DesktopNotifierSlot {
    fn show(
        &self,
        title: &str,
        body: Option<&str>,
        image: Option<&str>,
        play_sound: bool,
        action: Option<&DesktopNotificationAction>,
    ) -> Result<(), String> {
        let notifier = self
            .inner
            .lock()
            .map_err(|error| format!("desktop notification bridge lock poisoned: {error}"))?
            .clone();
        let Some(notifier) = notifier else {
            return Ok(());
        };
        notifier.show(title, body, image, play_sound, action)
    }
}

pub(super) fn send_desktop_notification(
    notifier: &dyn DesktopNotifier,
    render: &RenderedNotification,
    kind: ActivityKind,
    group_name: &str,
    preferences: &NotificationDeliveryPreferences,
    local_image: Option<&str>,
    action: Option<&DesktopNotificationAction>,
) {
    let title = desktop_notification_title(kind, group_name, &render.title);
    if let Err(error) = notifier.show(
        &title,
        non_empty(&render.body),
        local_image,
        preferences.desktop_notification_sound,
        action,
    ) {
        tracing::warn!("[Desktop] notification send failed: {error}");
    }
}

fn desktop_notification_title<'a>(
    kind: ActivityKind,
    group_name: &str,
    title: &'a str,
) -> Cow<'a, str> {
    let group_name = group_name.trim();
    if kind == ActivityKind::GroupAnnouncement && !group_name.is_empty() {
        return Cow::Owned(format!("{group_name} · {title}"));
    }
    Cow::Borrowed(title)
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use vrcx_0_core::OwnerId;

    use super::{desktop_notification_title, ActivityKind, DesktopNotificationAction};

    #[test]
    fn group_announcement_title_includes_source_group() {
        assert_eq!(
            desktop_notification_title(
                ActivityKind::GroupAnnouncement,
                "Maple Club",
                "Group Announcement"
            ),
            "Maple Club · Group Announcement"
        );
        assert_eq!(
            desktop_notification_title(ActivityKind::GroupAnnouncement, "", "Group Announcement"),
            "Group Announcement"
        );
        assert_eq!(
            desktop_notification_title(
                ActivityKind::GroupInformative,
                "Maple Club",
                "Group Information"
            ),
            "Group Information"
        );
    }

    #[test]
    fn open_user_profile_action_requires_canonical_owner_and_actor_ids() {
        let action = DesktopNotificationAction::open_user_profile(
            &OwnerId::new("usr_12345678-1234-1234-1234-1234567890ab"),
            "usr_abcdefab-cdef-abcd-efab-cdefabcdefab",
        );

        assert_eq!(
            action,
            Some(DesktopNotificationAction {
                owner_user_id: OwnerId::new("usr_12345678-1234-1234-1234-1234567890ab"),
                target: super::DesktopNotificationTarget::OpenUserProfile {
                    user_id: "usr_abcdefab-cdef-abcd-efab-cdefabcdefab".into(),
                },
            })
        );
        assert!(DesktopNotificationAction::open_user_profile(
            &OwnerId::new("usr_invalid"),
            "usr_abcdefab-cdef-abcd-efab-cdefabcdefab"
        )
        .is_none());
    }

    #[test]
    fn a_place_opens_only_when_it_is_a_real_world_location() {
        let owner = OwnerId::new("usr_12345678-1234-1234-1234-1234567890ab");
        let location = "wrld_12345678-1234-1234-1234-1234567890ab:12345~region(jp)";

        assert_eq!(
            DesktopNotificationAction::open_location(&owner, location),
            Some(DesktopNotificationAction {
                owner_user_id: owner.clone(),
                target: super::DesktopNotificationTarget::OpenLocation {
                    location: location.into(),
                },
            })
        );
        assert!(DesktopNotificationAction::open_location(&owner, "private").is_none());
        assert!(DesktopNotificationAction::open_location(&owner, "").is_none());
    }
}
