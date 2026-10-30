import { queryOptions } from '@tanstack/react-query';

import { entityQueryPolicies, queryKeys } from '@/lib/entityQueryCache';
import { commands } from '@/platform/tauri/bindings';

/**
 * 星遇同频地图榜上的公开信息。
 *
 * ## 为什么放在 repositories 而不是 features/vrcc
 *
 * 世界详情对话框属于 `src/components/`，架构约束（`.oxlintrc.json`）规定
 * `src/components/**` **不能依赖** `@/features/**`。而对话框需要读这张图在
 * 榜上的分数，所以这条读取必须落在更低的层——`repositories/` 可以依赖
 * `platform`，`components` 依赖 `repositories` 是允许的。
 *
 * ## 为什么直接用 command 而不是 `features/vrcc/vrccApi`
 *
 * 那个包装层同样在 `features/` 下，`repositories` 也引用不到。这里直接调
 * `commands.appVrccMapPage`——它本来就是后端导出的绑定，等于绕过了
 * 一层不必要的中间层，没有多写任何逻辑。
 *
 * ## 公开数据，不需要连接
 *
 * `GET /maps/{world}/page` 是开放接口：不需要令牌，也不计入玩家每天的个人
 * 额度。所以未连接星遇同频的玩家同样能看到分数——地图质量本来就是公开事实。
 *
 * 已连接的玩家要看自己的评分、写短评，走的是 `/me/maps/{world}` 那条路，
 * 不在这个仓库里混。
 */
export function vrccPublicMapQueryOptions(worldId: string) {
    return queryOptions({
        // 键走 `queryKeys.worldVrcc`，内部是 `withEndpoint(['world', id, 'vrcc'], '')`。
        // **不要手拼数组**：A2 那个 bug 就是手拼键漏掉一段，长度对不上，
        // 缓存永远命中不了，「零请求」是假的。
        queryKey: queryKeys.worldVrcc(worldId),
        queryFn: async () =>
            worldId.startsWith('wrld_')
                ? commands.appVrccMapPage(worldId)
                : null,
        ...entityQueryPolicies.vrccMapPage
    });
}

/**
 * 带令牌的地图页：完整短评列表 + 我的评分 + 地图介绍。
 *
 * 需要 `maps` 普通权限（已有，不需要玩家重新授权）。未连接时会失败——**这
 * 是预期行为**：公开的那一半照常显示，只是没有别人的短评和自己那一条。
 *
 * 缓存策略**比公开版短**：短评会随时间增加，30 分钟太滞钝。这里沿用
 * `vrccMapPage` 的 30 分钟只用于「在不在榜」这种慢变事实，短评不行。
 */
export function vrccMyMapPageQueryOptions(worldId: string) {
    return queryOptions({
        queryKey: queryKeys.worldVrccSignedIn(worldId),
        queryFn: async () =>
            worldId.startsWith('wrld_')
                ? commands.appVrccMyMapPage(worldId, null)
                : null,
        staleTime: 5 * 60 * 1000,
        gcTime: 30 * 60 * 1000,
        retry: 1,
        refetchOnWindowFocus: false
    });
}
