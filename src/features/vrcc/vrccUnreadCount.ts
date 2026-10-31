import type {
    VrccRecentNotifications,
    VrccUnread
} from '@/features/vrcc/vrccApi';

/**
 * 星遇同频几处红点的合计。
 *
 * 服务器有 `me/unread`（1.13.0 起）时直接加总；旧服务器上没有这个接口，
 * 退回「最近的提醒」里 `id > read` 的条数（`read` 没有时都算未读）。
 *
 * 放在这里而不是页面上，是因为侧边栏那个条目也要显示同一个数字：两处各自
 * 算一遍迟早会算歪，而「侧边栏和页面标题显示不同数字」很难被发现。
 */
export function countVrccUnread(
    unread: VrccUnread | null,
    recent: VrccRecentNotifications | null
): number {
    if (unread) {
        return (
            (unread.inbox ?? 0) +
            (unread.cross ?? 0) +
            (unread.chats ?? 0) +
            (unread.notes ?? 0)
        );
    }
    const read = recent?.read ?? 0;
    return (recent?.items ?? []).filter((item) => (item.id ?? 0) > read).length;
}
