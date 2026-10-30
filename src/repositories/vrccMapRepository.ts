import { queryOptions } from '@tanstack/react-query';

import { entityQueryPolicies, queryKeys } from '@/lib/entityQueryCache';
import { normalizeVrccError } from '@/lib/vrccError';
import { commands } from '@/platform/tauri/bindings';
import type { VrccMyMap } from '@/platform/tauri/bindings';

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
 * 一张地图在榜上的**状态**与我自己的评分。需要 `maps` 普通权限。
 *
 * ## 为什么界面必须调这一条
 *
 * 公开的 `/maps/{world}/page` 对「还没被收录」和「已投稿、等审核」**都回
 * `404`**，两者分不开。玩家投完稿回来看到的还是「不在榜上」，会以为失败
 * 然后反复投。只有这条带令牌的接口会用 `board` / `pending` 两个布尔把它们
 * 区分开。
 *
 * ## 四种状态
 *
 * | `off` | `board` | `pending` | 状态 |
 * |---|---|---|---|
 * | true | — | — | 不给看（后端已转成 `null`） |
 * | false | true | — | 已上榜 |
 * | false | false | true | 已投稿，待审核 |
 * | false | false | false | 未收录 |
 *
 * `off: true` 在服务层就被转成 `null`（官方要求此时隐藏整块），所以调用方
 * 拿到 `null` 时**不要**显示任何打分/投稿入口。
 */
export async function readVrccMyMap(
    worldId: string
): Promise<VrccMyMap | null> {
    if (!worldId.startsWith('wrld_')) {
        return null;
    }
    try {
        return await commands.appVrccMyMap(worldId);
    } catch (error) {
        // **必须归一化再抛出**：直接抛原始 `AppError` 的话，调用方拿到的是
        // 一坨没有分类的对象，只能笼统说「出错了」——而这里的失败要分好几种：
        // 令牌失效要重新连接、缺 `maps` 权限要勾同意、传输失败要查网络。
        // 分类是后端算好随错误过来的，扔掉它等于让界面去猜。
        throw await normalizeVrccError(error);
    }
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
 * 榜单封面的「便宜路」：批量读本地世界缓存，**零网络**。
 *
 * `appWorldSummariesGet` 背后是 `world_summaries_get`，直接查 SQLite 的
 * `cache_world`，不发任何请求。命中的行可以直接用 VRChat 自己的原图，
 * 比星遇那种小缩略图清楚得多。
 *
 * 未命中的 id 不会出现在返回的 map 里——这是**预期**，调用方据此回退到
 * 星遇的图，同时触发 [`prefetchVrccWorldCovers`] 去补。
 */
export async function readVrccWorldCovers(
    worldIds: string[]
): Promise<Record<string, string>> {
    if (!worldIds.length) {
        return {};
    }
    try {
        const summaries = await commands.appWorldSummariesGet(worldIds);
        const covers: Record<string, string> = {};
        for (const [id, summary] of Object.entries(summaries)) {
            if (!summary) {
                continue;
            }
            // `imageUrl` 是完整原图，`thumbnailImageUrl` 是更小的缩略图。
            // 榜单一行只有 32px，用缩略图就够；两者都没有就不记这一项。
            const url = summary.thumbnailImageUrl || summary.imageUrl;
            if (url) {
                covers[id] = url;
            }
        }
        return covers;
    } catch {
        // 读缓存失败不该让榜单显示不出来——退回全部用星遇的图。
        return {};
    }
}

/**
 * 可被取消信号打断的等待。
 *
 * 预载途中切排序或关页面时，`AbortController` 会立刻叫醒这里，不用等满一个间隔
 * ——否则最多白等一个 500 毫秒才收手。
 */
function sleep(ms: number, signal?: AbortSignal): Promise<void> {
    if (ms <= 0 || signal?.aborted) {
        return Promise.resolve();
    }
    return new Promise((resolve) => {
        let timer: ReturnType<typeof setTimeout> | undefined;
        const finish = () => {
            if (timer !== undefined) {
                clearTimeout(timer);
            }
            signal?.removeEventListener('abort', finish);
            resolve();
        };
        timer = setTimeout(finish, ms);
        signal?.addEventListener('abort', finish, { once: true });
    });
}

/**
 * 榜单封面的「贵的路」：从 VRChat 拉世界记录，让下次能用原图。
 *
 * ## 为什么要有这一条
 *
 * 星遇返回的 `img` 是**很小**的缩略图，够看清但不够好；而 VRChat 自己的
 * 原图清楚得多。问题是要拿到它得先有世界记录，VRChat 又**没有批量接口**，
 * 一次只能问一个。
 *
 * 所以策略是：**这一页先用星遇的小图顶上，后台慢慢把 VRChat 记录补进本地
 * 缓存**。看得越多，下次命中率越高，最终整页都能显示原图，而星遇那层始终
 * 是「不空窗」的保证。
 *
 * ## 一次只跑一个，两次之间留间隔
 *
 * 20 行就是 20 个请求，榜单还有 top/week/new 三种排序，切一轮可能几十个不同的
 * 世界。三个并发、中间不留空隙，等于一口气把几十个请求打给 VRChat——**它的限流
 * 拖累的是整个客户端**，不只是这个榜单。所以默认单并发、两次之间隔 500 毫秒，
 * 一页大约十秒补完，全程在后台，玩家看不到等待。
 *
 * 也考虑过「只补视口附近那几行」：那样列表尾部永远补不到，玩家滚到底还是小图，
 * 下次打开又从整页的小图开始。放慢速度能让它最终收敛——慢，但会好。
 *
 * ## 不缓存图片字节
 *
 * 这里缓存的是**世界记录**（含图片地址），不是图片本身。所以「用 VRChat
 * 原图」仍然是一次从 VRChat CDN 的图片请求，省掉的是查记录那一步。
 *
 * 星遇的图同样不需要我们缓存：它带 `Cache-Control: max-age=14400`，
 * 浏览器和 Cloudflare 都会兜住。
 *
 * @returns 成功写入缓存的数量，仅供调试/测试观察
 */
export async function prefetchVrccWorldCovers(
    worldIds: string[],
    options: {
        /** 已缓存、无需再拉的世界；调用方从 [`readVrccWorldCovers`] 得到。 */
        known?: Readonly<Record<string, string>>;
        /** 并发上限，默认 1。 */
        concurrency?: number;
        /** 两次请求之间的间隔（毫秒），默认 500。 */
        gapMs?: number;
        /** 外部取消信号：切排序或关页面时用来及时收手。 */
        signal?: AbortSignal;
    } = {}
): Promise<number> {
    const known = options.known ?? {};
    const concurrency = Math.max(1, options.concurrency ?? 1);
    const gapMs = Math.max(0, options.gapMs ?? 500);
    const pending = [...new Set(worldIds)].filter(
        (id) => id.startsWith('wrld_') && !known[id]
    );
    if (!pending.length) {
        return 0;
    }

    let cursor = 0;
    let stored = 0;
    let aborted = false;

    async function worker() {
        while (!aborted && cursor < pending.length) {
            if (options.signal?.aborted) {
                aborted = true;
                return;
            }
            const id = pending[cursor++];
            if (!id) {
                return;
            }
            try {
                // `appWorldGet` 会顺带写进 `cache_world`，下一次
                // `readVrccWorldCovers` 就能读到。
                await commands.appWorldGet({ worldId: id, force: false });
                stored += 1;
            } catch {
                // 单个失败就跳过：世界可能已删除、可能限流。下一轮再来。
            }
            // 无论成败都停一下再问下一个：撞上 VRChat 的限流要整个客户端一起等。
            await sleep(gapMs, options.signal);
        }
    }

    await Promise.all(
        Array.from({ length: Math.min(concurrency, pending.length) }, () =>
            worker()
        )
    );
    return stored;
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
