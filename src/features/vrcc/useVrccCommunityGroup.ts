import { useEffect, useState } from 'react';

import type { GroupProfileRecord } from '@/domain/entities/group';
import { getCachedQueryData, queryKeys } from '@/lib/entityQueryCache';
import groupProfileRepository from '@/repositories/groupProfileRepository';
import { VRCC_COMMUNITY_GROUP_ID } from '@/shared/constants/vrccCommunity';
import { normalizeString } from '@/shared/utils/string';
import { DEFAULT_VRCHAT_API_ENDPOINT } from '@/shared/vrchatEndpoint';

export type CommunityGroupState = {
    /** 是否已经加入。`null` 表示还不知道（拉取中或失败），界面按「未加入」处理。 */
    joined: boolean | null;
    /** 加载中。用来在按钮上显示期待状态，而不是先给一个可能错的灰态。 */
    loading: boolean;
};

/**
 * 判断当前玩家有没有加入中文社区群组。
 *
 * ## 为什么先读缓存
 *
 * `queryClient` 是纯内存的（没有挂 persister），但群组对话框本身会把
 * `GET groups/{id}?include_roles=true` 的结果放进同一份缓存。我们用
 * `getCachedQueryData` **同步读**已经有的那份——命中时一个网络请求都不发。
 *
 * 缓存没命中才发一次请求；这一次同样会写进缓存，玩家点开群组对话框时
 * 直接复用，不是额外浪费。
 *
 * ## 为什么 `includeRoles` 固定传 `true`
 *
 * 缓存键是 `['group', groupId, includeRoles]`，`includeRoles` 是键的一部分。
 * 群组对话框走的是 `getGroupProfile({ groupId, dialog: true })`，而
 * `includeRoles` 的默认值就是 `true`——只有同样传 `true`，两边才是**同一个键**，
 * 缓存才共享得起来。
 *
 * ## 已知限制（不是 bug）
 *
 * - 冷启动第一次仍会有一次请求（缓存纯内存，重启即空）。
 * - 群组缓存的 `gcTime` 是 30 分钟：玩家在别处退群后，最长 30 分钟内这里
 *   仍会显示「已加入」。所以它是**提示性状态**，界面文案不该写成绝对判断。
 */
export function useVrccCommunityGroup(): CommunityGroupState {
    const [state, setState] = useState<CommunityGroupState>({
        joined: null,
        loading: false
    });

    useEffect(() => {
        let active = true;
        // **必须带 endpoint。** `queryKeys.group` 会把它拼进键的最后一段，而
        // `getGroupProfile` 自己传的是 `DEFAULT_VRCHAT_API_ENDPOINT`；少这一段
        // 键的长度都不一样，等于永远命中不了缓存——那样「零请求」是假的，
        // 每次都会真去拉一遍。
        const key = queryKeys.group(
            VRCC_COMMUNITY_GROUP_ID,
            true,
            DEFAULT_VRCHAT_API_ENDPOINT
        );

        // 1) 纯同步读，不发请求。命中就直接用。
        const cached = getCachedQueryData<GroupProfileRecord>(key);
        if (cached) {
            setState({ joined: isMember(cached), loading: false });
            return () => {
                active = false;
            };
        }

        // 2) 没命中才拉一次；这次结果与群组对话框共用缓存。
        setState((previous) => ({ ...previous, loading: true }));
        groupProfileRepository
            .getGroupProfile({
                groupId: VRCC_COMMUNITY_GROUP_ID,
                includeRoles: true,
                dialog: true
            })
            .then((profile) => {
                if (!active) {
                    return;
                }
                setState({ joined: isMember(profile), loading: false });
            })
            .catch(() => {
                if (!active) {
                    return;
                }
                // 拉不到就当「不知道」，界面按未加入显示——**不能**因为一次
                // 网络失败就给玩家一个假的「已加入」。
                setState({ joined: null, loading: false });
            });

        return () => {
            active = false;
        };
    }, []);

    return state;
}

/**
 * 读成员身份。
 *
 * 只认顶层的 `membershipStatus`：`normalizeGroupProfile()` 的返回值里
 * **没有** `myMember`（那个字段只出现在原始响应上），所以不能照搬群组
 * 对话框视图里 `group.myMember?.membershipStatus` 的写法。
 *
 * VRChat 的取值里只有 `member` 表示在群；`inactive` 是「不在群」，
 * `userblocked` 是被拉黑，两者都按未加入处理。
 */
function isMember(profile: GroupProfileRecord): boolean {
    return normalizeString(profile.membershipStatus).toLowerCase() === 'member';
}

export { VRCC_COMMUNITY_GROUP_ID };

// 让调用方不必单独知道仓库层的类型细节。
export type { GroupProfileRecord };
