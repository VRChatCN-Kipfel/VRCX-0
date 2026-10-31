import { useMemo } from 'react';

import { useFriendRosterStore } from '@/state/friendRosterStore';

/**
 * 星友和本机好友表的关系。
 *
 * - `online`：对方在我们自己的好友表里，且现在在线
 * - `friend`：是好友，但不在线
 * - `unknown`：不是好友，或者好友表还没加载（这两种在界面上要区别对待）
 */
export type MutualPresence = 'online' | 'friend' | 'unknown';

export type MutualPresenceLookup = {
    /** 查一颗星的状态。`userId` 不是 `usr_` 开头时返回 `unknown`。 */
    of: (userId: string | null | undefined) => MutualPresence;
    /**
     * 好友表加载好了没有。
     *
     * 没加载好时**所有**人都是 `unknown`，界面此时不该显示「加好友」——
     * 那会让人以为满列表都不是好友。这个标志就是为了区分这种「还不知道」。
     */
    ready: boolean;
};

/**
 * 用本机的 VRChat 好友表判断星友的在线状态。
 *
 * 官方 VRCX 脚本读的是宿主自己的好友 store；我们这里用同一份数据
 * （`friendRosterStore`），不额外发任何请求——那本来就是应用里已有的状态。
 *
 * **合规边界**：这里只提供「是不是好友、在不在线」的事实。界面据此显示
 * 三态文案，点「加好友」仍然只是打开我们自己的玩家详情页，加好友由玩家
 * 在那里自己点，程序不代发请求。
 */
export function useMutualPresence(): MutualPresenceLookup {
    const friendsById = useFriendRosterStore((state) => state.friendsById);
    const loadStatus = useFriendRosterStore((state) => state.loadStatus);
    const ready = loadStatus === 'ready';

    // 返回值的身份必须稳定：调用方会把它放进 `useMemo` 的依赖里，每次渲染
    // 给一个新对象等于让那个 `useMemo` 白算。
    //
    // `of` 的依赖是 `friendsById`（好友表本身变了才需要重算），`ready` 单独
    // 变化时只换外面的壳，不重建那份在线集合。
    const lookup = useMemo(() => {
        // `online` 的判定和好友列表用的是同一个：`$presence` 经
        // `presenceSection` 归到 online 桶，包括「刚离线但还在宽限期」。
        const onlineIds = new Set<string>();
        for (const [userId, entry] of Object.entries(friendsById)) {
            const presence = entry?.$presence;
            if (
                presence &&
                (presence.kind === 'online' ||
                    presence.kind === 'pendingOffline')
            ) {
                onlineIds.add(userId);
            }
        }

        return (userId: string | null | undefined): MutualPresence => {
            const id = (userId ?? '').trim();
            if (!id.startsWith('usr_')) {
                return 'unknown';
            }
            if (!(id in friendsById)) {
                return 'unknown';
            }
            return onlineIds.has(id) ? 'online' : 'friend';
        };
    }, [friendsById]);

    return useMemo(() => ({ ready, of: lookup }), [ready, lookup]);
}
