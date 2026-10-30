import { useQuery } from '@tanstack/react-query';
import { ExternalLinkIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';
import { vrccPublicMapQueryOptions } from '@/repositories/vrccMapRepository';
import { Button } from '@/ui/shadcn/button';

import { VrccFactRow, VrccScoreSpread } from './WorldVrccPanelParts';

/**
 * 「星遇同频」标签页。
 *
 * ## 这一版只做公开数据
 *
 * 数据来自 `GET /maps/{world}/page`（不需要令牌，也不计个人额度），因此
 * **没连星遇同频的玩家也能看到完整的一页**：均分、星级、分数分布、去过人数、
 * 房间上限、实测配置、一条热评。
 *
 * 公开接口**不给完整短评列表**，也不给「我打了几分」「我的短评」。那两样要
 * 令牌，属于下一批（`/me/maps/{world}/page`）。所以这一页现在的缺口是
 * 「互动」而不是「信息」——不做一个假的输入框占位，那比没有更糟。
 *
 * ## 名次在这里显示
 *
 * 左下角的概览刻意不显示名次（`WorldVrccRating` 里有说明），因为那里是
 * 「这图好不好」的速览。这里是专门点进来的一页，名次是玩家预期之内的信息。
 */
export function WorldVrccPanel({ worldId }: { worldId: string }) {
    const { t } = useTranslation();
    const query = useQuery({
        ...vrccPublicMapQueryOptions(worldId),
        enabled: Boolean(worldId)
    });

    if (query.isPending) {
        return (
            <div className="text-muted-foreground flex h-40 items-center justify-center text-sm">
                {t('common.loading', '加载中…')}
            </div>
        );
    }

    // 请求失败是「我们读不到」，与「这张图不在榜上」不同。分开说。
    if (query.isError) {
        return (
            <div className="text-muted-foreground flex h-40 items-center justify-center text-sm">
                {t('dialog.world.vrcc.load_failed')}
            </div>
        );
    }

    // 后端把 404 转成 null：这张图不在榜上（含未过审）。
    if (query.data === null) {
        return (
            <div className="flex h-40 flex-col items-center justify-center gap-1 text-center">
                <div className="text-foreground text-sm font-medium">
                    {t('dialog.world.vrcc.not_listed')}
                </div>
                <div className="text-muted-foreground max-w-70 text-xs leading-relaxed">
                    {t('dialog.world.vrcc.not_listed_hint')}
                </div>
            </div>
        );
    }

    if (!query.data) {
        return null;
    }

    const { map, went, cap, perf } = query.data;
    // specta 把 Rust 的 `Vec<T>` / `i64` 生成为可选。缺了就当空/零，
    // 而不是让组件里到处写 `?.` 和 `??`。
    const spread = query.data.spread ?? [];
    const avg = typeof map.avg === 'number' ? map.avg : null;
    const raters = typeof map.raters === 'number' ? map.raters : 0;
    const hasRating = avg !== null && raters > 0;

    // 实测：整个 perf 可以没有，里面每一项也可以没有。
    const platformText = perf?.platforms?.length
        ? perf.platforms.join(' · ')
        : null;
    const sizeText =
        typeof perf?.size === 'number' && perf.size > 0
            ? `${(perf.size / 1024 / 1024).toFixed(1)} MB`
            : null;
    const loadText =
        typeof perf?.load === 'number' && perf.load > 0
            ? `${Math.round(perf.load)} ms`
            : null;

    return (
        <div className="flex min-w-0 flex-col gap-4">
            {/* 均分 + 星级 + 人数 + 名次 */}
            <div className="bg-card rounded-lg border p-3">
                <div className="flex min-w-0 items-center gap-3">
                    <div className="min-w-0">
                        <div className="flex items-baseline gap-1">
                            <span className="text-2xl leading-none font-semibold tabular-nums">
                                {hasRating ? avg.toFixed(1) : '—'}
                            </span>
                            <span className="text-muted-foreground text-xs">
                                / 10
                            </span>
                        </div>
                        <div className="text-muted-foreground mt-1 text-[11px] leading-tight">
                            {hasRating
                                ? t('dialog.world.vrcc.raters', {
                                      count: raters
                                  })
                                : t('dialog.world.vrcc.no_rating')}
                        </div>
                    </div>
                    <span className="ml-auto shrink-0">
                        <WorldVrccRatingStars score={hasRating ? avg : 0} />
                    </span>
                </div>

                {/* 名次只在这一页出现 */}
                {typeof map.rank === 'number' && map.rank > 0 ? (
                    <div className="text-muted-foreground mt-2 border-t pt-2 text-[11px]">
                        {t('dialog.world.vrcc.rank', { rank: map.rank })}
                    </div>
                ) : null}
            </div>

            <VrccScoreSpread spread={spread} />

            {/* 去过 / 上限 / 实测 */}
            <div className="flex flex-col gap-1.5">
                <VrccFactRow
                    label={t('dialog.world.vrcc.went')}
                    value={
                        typeof went === 'number' && went > 0
                            ? t('dialog.world.vrcc.went_count', { count: went })
                            : null
                    }
                />
                <VrccFactRow
                    label={t('dialog.world.vrcc.cap')}
                    value={cap && cap > 0 ? String(cap) : null}
                />
                <VrccFactRow
                    label={t('dialog.world.vrcc.perf_platforms')}
                    value={platformText}
                />
                <VrccFactRow
                    label={t('dialog.world.vrcc.perf_size')}
                    value={sizeText}
                />
                <VrccFactRow
                    label={t('dialog.world.vrcc.perf_load')}
                    value={loadText}
                />
            </div>

            {/* 热评：只有一条，公开接口就这么多 */}
            {map.hot?.text ? (
                <div className="bg-card rounded-lg border p-3">
                    <div className="text-muted-foreground mb-1.5 text-xs font-medium">
                        {t('dialog.world.vrcc.hot')}
                    </div>
                    <div className="text-sm leading-relaxed break-words whitespace-pre-wrap">
                        {map.hot.text}
                    </div>
                    <div className="text-muted-foreground mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px]">
                        {map.hot.by ? <span>{map.hot.by}</span> : null}
                        {map.hot.went ? (
                            <span className="bg-muted rounded px-1">
                                {t('dialog.world.vrcc.hot_went')}
                            </span>
                        ) : null}
                    </div>
                </div>
            ) : null}

            {/* 完整短评要令牌，下一批才有 */}
            <div className="text-muted-foreground text-[11px] leading-relaxed">
                {t('dialog.world.vrcc.words_hint')}
            </div>

            {/* 去网页看完整页面 */}
            {map.page ? (
                <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    className="self-start"
                    onClick={() => {
                        // 交给系统浏览器，不在应用内嵌网页
                        void import('@/services/shellIntegrationService').then(
                            (mod) => mod.openExternalLink(map.page as string)
                        );
                    }}
                >
                    <ExternalLinkIcon data-icon="inline-start" />
                    {t('dialog.world.vrcc.open_page')}
                </Button>
            ) : null}

            <div className="text-muted-foreground/70 text-[10px] leading-tight">
                {t('dialog.world.vrcc.source')}
            </div>
        </div>
    );
}

/**
 * 星星（大号版本）。
 *
 * 概览那个是 12px 的小星；这一页是主视图，星要大一号。颜色同样走
 * `text-primary`，跟随主题。**不把 StarRating 从 WorldVrccRating 里
 * 抽出来复用**：那边是按 12px 调的，这里的尺寸/间距需求不同，
 * 硬抽会让两边都长出可选参数。
 */
function WorldVrccRatingStars({ score }: { score: number }) {
    const filled = Math.max(0, Math.min(5, score / 2));
    return (
        <span
            className="inline-flex items-center gap-0.5"
            role="img"
            aria-label={score.toFixed(1)}
        >
            {[0, 1, 2, 3, 4].map((index) => {
                const ratio = Math.max(0, Math.min(1, filled - index));
                return (
                    <span
                        key={index}
                        className={cn(
                            'relative inline-block size-4 leading-none'
                        )}
                    >
                        <StarGlyphLarge className="text-muted-foreground/35 absolute inset-0" />
                        {ratio > 0 ? (
                            <span
                                className="absolute inset-0 overflow-hidden"
                                style={{ width: `${ratio * 100}%` }}
                            >
                                <StarGlyphLarge className="text-primary size-4" />
                            </span>
                        ) : null}
                    </span>
                );
            })}
        </span>
    );
}

function StarGlyphLarge({ className }: { className?: string }) {
    return (
        <svg
            viewBox="0 0 24 24"
            aria-hidden="true"
            className={cn('size-4', className)}
            fill="currentColor"
        >
            <path d="M12 3.4l2.6 5.5 6 .8-4.4 4.2 1.1 6L12 17l-5.3 2.9 1.1-6-4.4-4.2 6-.8z" />
        </svg>
    );
}
