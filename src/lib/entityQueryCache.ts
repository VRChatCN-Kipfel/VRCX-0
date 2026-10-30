import type { QueryKey } from '@tanstack/react-query';

import { queryClient } from '@/lib/queryClient';
import type {
    CalendarListParams,
    ReleaseStatusFilter,
    VrchatGroupGalleryInput,
    VrchatGroupMembersInput,
    VrchatWorldListByUserInput
} from '@/platform/tauri/bindings';
import { MINUTE_MS, SECOND_MS } from '@/shared/constants/time';
import { normalizeVrchatEndpointKey } from '@/shared/vrchatEndpoint';

type EntityQueryPolicy = Readonly<{
    staleTime: number;
    gcTime: number;
    retry: number;
    refetchOnWindowFocus: boolean;
}>;

type FetchWithEntityPolicyOptions<TData = unknown> = {
    queryKey: QueryKey;
    policy: EntityQueryPolicy;
    queryFn: () => Promise<TData> | TData;
    force?: boolean;
};

export const entityQueryPolicies = Object.freeze({
    instance: Object.freeze({
        staleTime: 0,
        gcTime: 90 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    group: Object.freeze({
        staleTime: 5 * MINUTE_MS,
        gcTime: 30 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    groupDialog: Object.freeze({
        staleTime: 120 * SECOND_MS,
        gcTime: 30 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    // 地图在星遇同频榜上的公开信息。半小时是刻意的：判断的是「在不在榜、
    // 大概几分」这类变化很慢的事实，而且官方自己最多 5 分钟才更新一次。
    // 缓存短了，玩家翻世界列表就等于在刷接口。
    vrccMapPage: Object.freeze({
        staleTime: 30 * MINUTE_MS,
        gcTime: 120 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    groupCollection: Object.freeze({
        staleTime: 60 * SECOND_MS,
        gcTime: 300 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    economy: Object.freeze({
        staleTime: 10 * MINUTE_MS,
        gcTime: 30 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    economyBalance: Object.freeze({
        staleTime: 120 * SECOND_MS,
        gcTime: 600 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    groupCalendarEvent: Object.freeze({
        staleTime: 120 * SECOND_MS,
        gcTime: 600 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    worldCollection: Object.freeze({
        staleTime: 60 * SECOND_MS,
        gcTime: 300 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    avatarGallery: Object.freeze({
        staleTime: 30 * SECOND_MS,
        gcTime: 120 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    inventoryCollection: Object.freeze({
        staleTime: 20 * SECOND_MS,
        gcTime: 120 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    inventoryTemplate: Object.freeze({
        staleTime: 60 * MINUTE_MS,
        gcTime: 240 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    fileAnalysis: Object.freeze({
        staleTime: 120 * MINUTE_MS,
        gcTime: 240 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    avatarStyles: Object.freeze({
        staleTime: 60 * MINUTE_MS,
        gcTime: 240 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    representedGroup: Object.freeze({
        staleTime: 60 * SECOND_MS,
        gcTime: 300 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    userDialogTabCounts: Object.freeze({
        staleTime: 0,
        gcTime: 10 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    worldPersistData: Object.freeze({
        staleTime: 30 * MINUTE_MS,
        gcTime: 120 * MINUTE_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    userAppearanceProfile: Object.freeze({
        staleTime: 60 * SECOND_MS,
        gcTime: 300 * SECOND_MS,
        retry: 1,
        refetchOnWindowFocus: false
    }),
    userAvatarLookup: Object.freeze({
        staleTime: 30 * MINUTE_MS,
        gcTime: 5 * MINUTE_MS,
        retry: 0,
        refetchOnWindowFocus: false
    }),
    boopEmojiLookup: Object.freeze({
        staleTime: 30 * MINUTE_MS,
        gcTime: 30 * MINUTE_MS,
        retry: 0,
        refetchOnWindowFocus: false
    })
});

type UserDialogTabCountQueryParams = {
    userId: string;
    currentUserId: string;
    avatarReleaseStatus: ReleaseStatusFilter;
    includeMutualFriends: boolean;
};

type AggregateCalendarQueryParams = {
    date: string;
    includeFeatured: boolean;
    userId: string | null;
};

function withEndpoint(queryKey: QueryKey, endpoint: string = ''): QueryKey {
    const normalizedEndpoint = normalizeVrchatEndpointKey(endpoint);
    return normalizedEndpoint
        ? [...queryKey, { endpoint: normalizedEndpoint }]
        : queryKey;
}

function stableParams(params: object = {}): Record<string, unknown> {
    return Object.fromEntries(
        Object.entries(params)
            .filter(([, value]) => value !== undefined)
            .sort(([left], [right]) => left.localeCompare(right))
    );
}

export const queryKeys = Object.freeze({
    user: (userId: string, endpoint: string = '') =>
        withEndpoint(['user', userId], endpoint),
    userAppearanceProfile: (userId: string, endpoint: string = '') =>
        withEndpoint(['user', userId, 'appearanceProfile'], endpoint),
    userGroups: (userId: string, endpoint: string = '') =>
        withEndpoint(['user', userId, 'groups'], endpoint),
    userGroupsOverview: (userId: string, endpoint: string = '') =>
        withEndpoint(['user', userId, 'groupsOverview'], endpoint),
    instance: (worldId: string, instanceId: string, endpoint: string = '') =>
        withEndpoint(['instance', worldId, instanceId], endpoint),
    instanceShortName: (
        worldId: string,
        instanceId: string,
        endpoint: string = ''
    ) => withEndpoint(['instance', worldId, instanceId, 'shortName'], endpoint),
    group: (
        groupId: string,
        includeRoles: boolean = false,
        endpoint: string = ''
    ) => withEndpoint(['group', groupId, includeRoles], endpoint),
    worldsByUser: (params: VrchatWorldListByUserInput, endpoint: string = '') =>
        withEndpoint(
            ['worlds', 'user', params.userId, stableParams(params)],
            endpoint
        ),
    // 这张地图在星遇同频地图榜上的公开信息。`endpoint` 这里恒为空串，但
    // **不能省掉这一段**：`withEndpoint` 只在非空时才追加，所以传空串的
    // 结果本身是干净的；而一旦有人手拼键、漏掉这一段，键的长度就和这里
    // 不一致，缓存永远命中不了——A2 那个 bug 就是这么来的。
    worldVrcc: (worldId: string, endpoint: string = '') =>
        withEndpoint(['world', worldId, 'vrcc'], endpoint),
    // 带令牌的地图页。**必须与 `worldVrcc` 分开**：两条路的缓存时长、失效
    // 条件、以及「未连接时会失败」这个性质都不同，共用一个键会让公开数据
    // 被登录数据顶掉，反之亦然。`sort` 也进键：`hot` 和 `new` 是两份不同的
    // 列表，玩家切了排序不该看到上一份。
    worldVrccSignedIn: (
        worldId: string,
        sort: 'hot' | 'new' = 'hot',
        endpoint: string = ''
    ) => withEndpoint(['world', worldId, 'vrcc', 'signedIn', sort], endpoint),
    // 当前令牌是否有某项权限。全局一份，按 scope 名分键。
    vrccScope: (scope: string) => ['vrcc', 'scope', scope] as const,
    groupMembers: (params: VrchatGroupMembersInput, endpoint: string = '') =>
        withEndpoint(
            ['group', params.groupId, 'members', stableParams(params)],
            endpoint
        ),
    groupGallery: (params: VrchatGroupGalleryInput, endpoint: string = '') =>
        withEndpoint(
            [
                'group',
                params.groupId,
                'gallery',
                params.galleryId,
                stableParams(params)
            ],
            endpoint
        ),
    groupCalendarList: (
        kind: 'aggregate' | 'group' | 'following',
        params:
            | AggregateCalendarQueryParams
            | CalendarListParams
            | { groupId: string },
        endpoint: string = ''
    ) => withEndpoint(['calendar', kind, stableParams(params)], endpoint),
    groupCalendarEvent: (
        {
            groupId = '',
            eventId = ''
        }: { eventId?: string; groupId?: string } = {},
        endpoint: string = ''
    ) => withEndpoint(['calendar', groupId, eventId], endpoint),
    economyPurchases: (userId: string, endpoint: string = '') =>
        withEndpoint(['economy', 'purchases', userId], endpoint),
    economyTransactions: (userId: string, endpoint: string = '') =>
        withEndpoint(['economy', 'transactions', userId], endpoint),
    economyBalance: (userId: string, endpoint: string = '') =>
        withEndpoint(['economy', 'balance', userId], endpoint),
    economyStore: (storeId: string, endpoint: string = '') =>
        withEndpoint(['economy', 'store', storeId], endpoint),
    licensedAvatars: (userId: string, endpoint: string = '') =>
        withEndpoint(['avatars', 'licensed', userId], endpoint),
    worldRooms: (worldId: string, endpoint: string = '') =>
        withEndpoint(['world', worldId, 'rooms'], endpoint),
    avatarGallery: (avatarId: string, endpoint: string = '') =>
        withEndpoint(['avatar', avatarId, 'gallery'], endpoint),
    userInventoryItem: (
        {
            inventoryId = '',
            userId = ''
        }: { inventoryId?: string; userId?: string } = {},
        endpoint: string = ''
    ) => withEndpoint(['inventory', 'item', userId, inventoryId], endpoint),
    inventoryTemplate: (inventoryTemplateId: string, endpoint: string = '') =>
        withEndpoint(['inventory', 'template', inventoryTemplateId], endpoint),
    boopEmoji: (userId: string, inventoryId: string, endpoint: string = '') =>
        withEndpoint(['boopEmoji', userId, inventoryId], endpoint),
    fileAnalysis: (
        {
            fileId = '',
            version = 0,
            variant = ''
        }: { fileId?: string; variant?: string; version?: number } = {},
        endpoint: string = ''
    ) => withEndpoint(['analysis', fileId, version, variant], endpoint),
    avatarStyles: (endpoint: string = '') =>
        withEndpoint(['avatar', 'styles'], endpoint),
    representedGroup: (userId: string, endpoint: string = '') =>
        withEndpoint(['user', userId, 'representedGroup'], endpoint),
    userMutualFriends: (userId: string, endpoint: string = '') =>
        withEndpoint(['user', userId, 'mutualFriends'], endpoint),
    userDialogTabCounts: (
        params: UserDialogTabCountQueryParams,
        endpoint: string = ''
    ) =>
        withEndpoint(
            ['user', params.userId, 'dialogTabCounts', stableParams(params)],
            endpoint
        ),
    worldPersistData: (
        {
            userId = '',
            worldId = ''
        }: { userId?: string; worldId?: string } = {},
        endpoint: string = ''
    ) => withEndpoint(['world', worldId, 'persistData', userId], endpoint)
});

function toQueryOptions(
    policy: EntityQueryPolicy,
    overrides: Record<string, unknown> = {}
) {
    return {
        staleTime: policy.staleTime,
        gcTime: policy.gcTime,
        retry: policy.retry,
        refetchOnWindowFocus: policy.refetchOnWindowFocus,
        ...overrides
    };
}

async function fetchWithEntityPolicy<TData = unknown>({
    queryKey,
    policy,
    queryFn,
    force = false
}: FetchWithEntityPolicyOptions<TData>): Promise<{
    data: TData;
    cache: boolean;
}> {
    const staleTime = force ? 0 : policy.staleTime;
    const queryState = queryClient.getQueryState(queryKey);
    const cache =
        !force &&
        Boolean(queryState?.dataUpdatedAt) &&
        staleTime > 0 &&
        Date.now() - (queryState?.dataUpdatedAt ?? 0) < staleTime;

    const data = await queryClient.fetchQuery<TData>({
        queryKey,
        queryFn,
        ...toQueryOptions(policy, { staleTime })
    });

    return {
        data,
        cache
    };
}

export async function fetchCachedData<TData = unknown>(
    options: FetchWithEntityPolicyOptions<TData>
): Promise<TData> {
    const { data } = await fetchWithEntityPolicy(options);
    return data;
}

export function setCachedQueryData<TData = unknown>(
    queryKey: QueryKey,
    data: TData
) {
    queryClient.setQueryData(queryKey, data);
}

export function getCachedQueryData<TData = unknown>(queryKey: QueryKey) {
    return queryClient.getQueryData<TData>(queryKey);
}

export function invalidateEntityQueries(queryKey: QueryKey) {
    return queryClient.invalidateQueries({
        queryKey,
        refetchType: 'active'
    });
}

export async function clearEntityQueryCache() {
    await queryClient.cancelQueries();
    queryClient.clear();
}
