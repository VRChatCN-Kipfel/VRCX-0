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
 *
 * `sort` 进缓存键：`hot` 和 `new` 是**两份不同的列表**，共用键会让玩家切了
 * 排序却看到上一份的结果。
 */
export function vrccMyMapPageQueryOptions(
    worldId: string,
    sort: 'hot' | 'new'
) {
    return queryOptions({
        queryKey: queryKeys.worldVrccSignedIn(worldId, sort),
        queryFn: async () =>
            worldId.startsWith('wrld_')
                ? commands.appVrccMyMapPage(worldId, sort)
                : null,
        staleTime: 5 * 60 * 1000,
        gcTime: 30 * 60 * 1000,
        retry: 1,
        refetchOnWindowFocus: false
    });
}

/**
 * 给地图打分 / 写短评。**需要 `maps.rate` 敏感权限。**
 *
 * ## 这是写操作，和这个文件里其它函数都不一样
 *
 * 其它函数只造查询；这两个**会改服务器上的数据**。因此：
 *
 * - **不走 `queryOptions`、不缓存、不重试**。写操作重试一次可能造成两次提交，
 *   而读操作重试一次只是多发一个请求。
 * - **不做乐观更新**。积分榜/均分由服务端算，本地猜一个数只会显示错的。
 *   调用方成功后用返回的地图状态或重新查询来刷新。
 * - 失败原样抛出。界面需要拿到 `why`（短评被审核拒绝时会说明原因），
 *   包一层只会把它丢掉。
 *
 * ## `textMode` 的三态
 *
 * 官方对 `text` 的语义很微妙，这里显式表达，避免把「只改分数」误做成
 * 「删掉短评」：
 *
 * - `keep`  —— 不传 `text`，保留原短评
 * - `clear` —— 传空串，删掉短评
 * - `write` —— 传新内容，提交或更新短评
 */
export async function rateVrccMap(
    worldId: string,
    score: number,
    textMode: 'keep' | 'clear' | 'write',
    text: string | null
) {
    return await commands.appVrccRateMap(worldId, score, textMode, text);
}

/** 删掉自己给这张图的评分。写操作，语义同上。 */
export async function deleteVrccMapRating(worldId: string) {
    return await commands.appVrccDeleteMapRating(worldId);
}

/**
 * 给一条短评投票。**需要 `maps.write` 敏感权限。**
 *
 * `vote` 传 `'none'` 表示取消（官方 `v: 0`）。写操作，不做重试——
 * 重试一次就等于投两次。
 *
 * 官方的三条规则：`none` 取消、**不能投自己的内容**、`id` 是短评编号
 * （不是世界 ID）。第二条由界面提前禁用按钮，服务端也会拒。
 */
export async function voteVrccMapContent(
    id: number,
    vote: 'none' | 'up' | 'down'
) {
    return await commands.appVrccVoteMapContent('rate', id, vote);
}

/**
 * 当前令牌有没有某项权限。
 *
 * ## 为什么要问后端
 *
 * 「有没有 `maps.rate`」取决于**玩家当初授权时拿到的 scope 字符串**，那是
 * 令牌的属性，不在前端状态里。已经连上、但令牌是加权限之前签发的玩家，
 * 正是这里的分水岭：他们有 `maps` 却没有 `maps.rate`，服务器会回
 * `403 scope`。前端猜不出来，只能读。
 *
 * ## 判定是宽松的
 *
 * 拿不到 scope（未连接、或旧版本没记）时按**没有**处理——宁可让玩家多走
 * 一次重新连接，也不要摆一个点了必然 403 的按钮。
 *
 * 官方的 scope 分隔符兼容空格、逗号与 `+`，这里三种都认。
 */
export async function fetchVrccScopeGrants(scope: string): Promise<boolean> {
    try {
        const connection = await commands.appVrccConnection();
        const raw = connection.scope;
        if (!raw) {
            return false;
        }
        return raw
            .split(/[\s,+]+/)
            .map((part) => part.trim())
            .includes(scope);
    } catch {
        return false;
    }
}
