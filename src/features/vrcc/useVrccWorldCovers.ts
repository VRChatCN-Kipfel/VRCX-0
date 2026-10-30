import { useEffect, useMemo, useRef, useState } from 'react';

import {
    prefetchVrccWorldCovers,
    readVrccWorldCovers
} from '@/repositories/vrccMapRepository';

/** 每个世界对应的封面地址（来自 VRChat 自己的世界记录）。 */
export type VrccWorldCovers = Record<string, string>;

/**
 * 榜单封面的两段式加载。
 *
 * ## 两条路，各管一件事
 *
 * | 来源 | 用途 | 代价 |
 * |---|---|---|
 * | 星遇的 `img` | **立刻上屏**，保证任何一行都不空窗 | 图小；`max-age=14400`，浏览器和 Cloudflare 兜住，我们不用缓存 |
 * | VRChat 的世界图 | 清楚得多，**进本地 SQLite，越用越全** | 要先有世界记录，而 VRChat 没有批量接口 |
 *
 * ## 加载规则
 *
 * - **可视行**：立刻用星遇的小图。它不需要等 VRChat——已经有了能看的东西。
 * - **不可视行**：不给它挂星遇的图（省下那一次请求），留给下面的预载。
 *
 * ## 预载顺序：不可视优先
 *
 * 反直觉但成立：可视行的图**已经出来了**，它的 VRChat 记录晚一点拉没有任何
 * 感知；不可视行还没有任何图，优先把它们的世界记录备好，等玩家真的滚下去时
 * 就已经能显示原图，而不是先看到星遇的小图再被替换。
 *
 * 所有行**都会被拉**（可视的也拉），只是排在后面——那是纯粹为了补缓存，
 * 让下一次打开这一页时命中率更高。
 *
 * ## 一次一个、中间留间隔
 *
 * 一页约 20 行，榜单还有 top/week/new 三种排序，切一轮可能几十个不同的世界。
 * 一把梭会撞 VRChat 的接口限流，**而限流拖累的是整个客户端**，所以预载是单并发
 * 加间隔的：慢一点，但不会让别的页面陪着卡。
 *
 * ## `generation` 守卫
 *
 * 切排序会换一页世界。上一页的预载还在跑时，它的完成回调不该覆盖新一页的
 * 状态，所以用 `generation` 比对加 `AbortController` 双重收手。
 */
export function useVrccWorldCovers(
    worldIds: string[],
    visibleIds: ReadonlySet<string>
): VrccWorldCovers {
    const [covers, setCovers] = useState<VrccWorldCovers>({});

    // 数组每次渲染都是新引用，用排序后的字符串做稳定依赖，否则 effect 会每帧
    // 重跑、把预载队列反复重启。
    const key = useMemo(
        () => [...new Set(worldIds)].sort().join(','),
        [worldIds]
    );

    // 可见集合变化很频繁，不能进 effect 依赖——只在「开始预载」的那一刻读一次。
    const visibleRef = useRef(visibleIds);
    visibleRef.current = visibleIds;

    const generationRef = useRef(0);

    useEffect(() => {
        const generation = ++generationRef.current;
        const ids = key ? key.split(',') : [];
        if (!ids.length) {
            setCovers({});
            return;
        }

        const controller = new AbortController();
        let cancelled = false;
        const alive = () => !cancelled && generationRef.current === generation;

        void (async () => {
            // ① 同步读本地缓存（纯 SQLite、零网络），命中的立刻换成 VRChat 的图。
            const cached = await readVrccWorldCovers(ids);
            if (!alive()) {
                return;
            }
            setCovers(cached);

            // 等一帧，让 IntersectionObserver 把「哪几行可见」报上来——
            // 预载顺序要靠它。观察器还没跑时全部算不可视，也就等于按列表
            // 顺序拉，不会出错。
            await new Promise((resolve) => {
                if (typeof requestAnimationFrame === 'function') {
                    requestAnimationFrame(() => resolve(null));
                } else {
                    setTimeout(() => resolve(null), 0);
                }
            });
            if (!alive()) {
                return;
            }

            // ② 不可视优先，可视的排后面（它们已经有图了，只是为了补缓存）。
            const visibleNow = visibleRef.current;
            const missing = ids.filter((id) => !cached[id]);
            const ordered = [
                ...missing.filter((id) => !visibleNow.has(id)),
                ...missing.filter((id) => visibleNow.has(id))
            ];

            const stored = await prefetchVrccWorldCovers(ordered, {
                signal: controller.signal
            });
            if (!alive() || stored <= 0) {
                return;
            }

            // ③ 补完之后再读一次，把这一页剩下的行也换成原图，
            //    不必等玩家下次打开。
            const refreshed = await readVrccWorldCovers(ids);
            if (!alive()) {
                return;
            }
            setCovers(refreshed);
        })();

        return () => {
            cancelled = true;
            controller.abort();
        };
    }, [key]);

    return covers;
}
