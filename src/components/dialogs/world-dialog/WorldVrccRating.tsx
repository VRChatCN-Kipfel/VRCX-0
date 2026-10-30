import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { VrccStars } from '@/components/vrcc/VrccStars';
import { vrccPublicMapQueryOptions } from '@/repositories/vrccMapRepository';

/**
 * 世界详情左下角的「星遇同频评分」。
 *
 * ## 放什么、不放什么
 *
 * 只放**评分本身**：星级、均分、打分人数，外加一行数据来源声明。
 *
 * - **不放名次**。名次会让作者之间产生攀比；这一块是给玩家看「这张图好不好」，
 *   不是排行榜入口。完整的名次在「星遇同频」标签页里。
 * - **不放任何人的名字或头像**。上榜作品与作者信息一概不出现。
 *
 * ## 三行的排布
 *
 * ```
 * 星遇同频评分
 *                  ★★★★☆ 9.8
 *                    27 人打分
 * 数据来源于 …
 * ```
 *
 * 星级与分数**不再和标题同行**。挤在一行时左边只剩「星遇同频评分」一个词，
 * 右边要同时放下星级、分数两样东西，标题和读数互相挤，星级也只能缩到 12px。
 * 标题独占一行之后读数有了整行宽度，星星可以放大一档。
 *
 * **右对齐保留**：读数是「数值」，数值贴右边缘才方便纵向比较，人数跟着它
 * 也贴右——这两行在视觉上是一组。只有标题和来源声明是左对齐的。
 *
 * ## 数据从哪来
 *
 * `GET /maps/{world}/page` 是公开数据，不需要令牌、不计个人额度。所以没连
 * 星遇同频的玩家也能看到分数。半小时缓存（`entityQueryPolicies.vrccMapPage`）
 * 是刻意的：判断的是「在不在榜、大概几分」这类变化很慢的事实，缓存短了的话
 * 玩家翻世界列表就等于在刷接口。
 *
 * ## 为什么不用 WorldOverviewMetric
 *
 * 那个组件在值为 `—` 时直接 `return null`，整行会消失。这里没有数据的图要
 * 显示「不在榜上」、「还没有人打分」这类明确结论，语义不同，所以单独做。
 */
export function WorldVrccRating({ worldId }: { worldId: string }) {
    const { t } = useTranslation();
    const query = useQuery({
        ...vrccPublicMapQueryOptions(worldId),
        enabled: Boolean(worldId)
    });

    // 首帧（缓存未命中）只占位一行，不做骨架屏——缓存命中时根本看不到这一帧，
    // 闪烁反而更扎眼。
    if (query.isPending) {
        return (
            <WorldVrccRatingShell
                value={
                    <span className="text-foreground text-sm font-medium tabular-nums">
                        ⋯
                    </span>
                }
                note={null}
            />
        );
    }

    // 只有「确认不在榜上」（后端把 404 转成 null）才说不在榜上。
    // 请求失败是另一回事：那时不给出结论，免得把网络问题说成「这图不在榜」。
    if (query.data === null) {
        return (
            <WorldVrccRatingShell
                value={<span className="text-muted-foreground">—</span>}
                note={t('dialog.world.vrcc.not_listed')}
            />
        );
    }

    if (!query.data) {
        return null;
    }

    const map = query.data.map;
    const avg = typeof map.avg === 'number' ? map.avg : null;
    const raters = typeof map.raters === 'number' ? map.raters : 0;

    // 没人打分时不要显示 `★★★★★ 0.0`——那是「满分」的视觉信号，会误导。
    if (avg === null || raters <= 0) {
        return (
            <WorldVrccRatingShell
                value={<span className="text-muted-foreground">—</span>}
                note={t('dialog.world.vrcc.no_rating')}
            />
        );
    }

    return (
        <WorldVrccRatingShell
            value={
                <>
                    <VrccStars score={avg} size="lg" />
                    <span className="text-foreground text-sm font-medium tabular-nums">
                        {avg.toFixed(1)}
                    </span>
                </>
            }
            note={t('dialog.world.vrcc.raters', { count: raters })}
        />
    );
}

/**
 * 那一块的统一外壳：标题左一行、读数与人数贴右两行、来源声明左一行。
 *
 * 用 `col-span-2` 占满整行——外层是两列网格，星、分数、人数塞不进半栏。
 */
function WorldVrccRatingShell({
    value,
    note
}: {
    value: React.ReactNode;
    note: string | null;
}) {
    const { t } = useTranslation();

    // 独立成块：`mt-3` 与上面的概览卡片拉开间隔，`rounded-lg border bg-card`
    // 让它自成一个圆角容器。塞在概览卡片内部时它会和上面的世界 ID、
    // 分享链接挤成一片，既看不清是两块，也读不出「这是别人家的评分」。
    //
    // **标题与人数用 `text-foreground`**，不用 `text-muted-foreground`：
    // 这一块的核心信息就是「这是谁家的评分、有多少人打过分」，压成灰字之后
    // 整块像附注，和它独立成卡片的意图自相矛盾。来源声明仍然留在弱化层。
    return (
        <div className="bg-card mt-3 min-w-0 rounded-lg border p-2.5">
            <div className="text-foreground truncate text-xs font-medium">
                {t('dialog.world.vrcc.rating')}
            </div>
            {/* 读数贴右：`justify-end` 兑现原来 `ml-auto` 的位置。
                人数是下一行同一边缘，两行成一组。 */}
            <div className="mt-1 flex min-w-0 justify-end">
                <span className="flex shrink-0 items-center gap-1.5">
                    {value}
                </span>
            </div>
            {note ? (
                <div className="text-foreground mt-1 text-right text-[11px] leading-tight">
                    {note}
                </div>
            ) : null}
            <div className="text-muted-foreground/70 mt-1 text-[10px] leading-tight">
                {t('dialog.world.vrcc.source')}
            </div>
        </div>
    );
}
