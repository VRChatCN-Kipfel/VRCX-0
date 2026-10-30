import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';
import { vrccPublicMapQueryOptions } from '@/repositories/vrccMapRepository';

/**
 * 五颗星。
 *
 * 颜色走 `text-primary`，也就是 `var(--primary)`——它在 `globals.css` 里由
 * `--vrcx-theme-primary` 一路派生下来，玩家换强调色、装社区主题时星星会跟着变。
 * **不写死任何色值**：官方站点那套金色是它们的视觉资产，我们不用。
 *
 * 分数是 1–10，按 `score / 2` 折成 0–5 星，不足一颗的按比例部分填充。
 */
function StarRating({
    score,
    className
}: {
    score: number;
    className?: string;
}) {
    const filled = Math.max(0, Math.min(5, score / 2));

    return (
        <span
            className={cn('inline-flex items-center gap-px', className)}
            role="img"
            aria-label={score.toFixed(1)}
        >
            {[0, 1, 2, 3, 4].map((index) => {
                // 每颗星占 1 分，超出的比例决定这颗星填多少
                const ratio = Math.max(0, Math.min(1, filled - index));
                return (
                    <span
                        key={index}
                        className="relative inline-block size-3 leading-none"
                    >
                        <StarGlyph className="text-muted-foreground/35 absolute inset-0" />
                        {ratio > 0 ? (
                            <span
                                className="absolute inset-0 overflow-hidden"
                                style={{ width: `${ratio * 100}%` }}
                            >
                                <StarGlyph className="text-primary size-3" />
                            </span>
                        ) : null}
                    </span>
                );
            })}
        </span>
    );
}

function StarGlyph({ className }: { className?: string }) {
    return (
        <svg
            viewBox="0 0 24 24"
            aria-hidden="true"
            className={cn('size-3', className)}
            fill="currentColor"
        >
            <path d="M12 3.4l2.6 5.5 6 .8-4.4 4.2 1.1 6L12 17l-5.3 2.9 1.1-6-4.4-4.2 6-.8z" />
        </svg>
    );
}

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
                    <span className="text-foreground font-medium tabular-nums">
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
                <span className="flex items-center gap-1.5">
                    <StarRating score={avg} />
                    <span className="text-foreground font-medium tabular-nums">
                        {avg.toFixed(1)}
                    </span>
                </span>
            }
            note={t('dialog.world.vrcc.raters', { count: raters })}
        />
    );
}

/**
 * 那一块的统一外壳：标签 + 值 + 一行说明 + 来源声明。
 *
 * 用 `col-span-2` 占满整行——外层是两列网格，星级加数字再加两行小字塞不进半栏。
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
            <div className="flex min-w-0 items-baseline gap-1.5">
                <span className="text-foreground truncate text-xs font-medium">
                    {t('dialog.world.vrcc.rating')}
                </span>
                <span className="ml-auto shrink-0 text-sm">{value}</span>
            </div>
            {note ? (
                // 靠右：它描述的是上一行右侧那个分数（「8.4」是 412 人打出来的），
                // 右对齐后两行在视觉上是一组，左对齐会读成标题的续行。
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
