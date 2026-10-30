import { useQuery } from '@tanstack/react-query';
import { ExternalLinkIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';
import type { VrccMyMapPage } from '@/platform/tauri/bindings';
import {
    vrccMyMapPageQueryOptions,
    vrccPublicMapQueryOptions
} from '@/repositories/vrccMapRepository';
import { Button } from '@/ui/shadcn/button';

import { VrccFactRow, VrccScoreSpread } from './WorldVrccPanelParts';

/** 一条短评。从绑定类型里派生，免得两处各写一份形状。 */
type VrccMapRate = NonNullable<VrccMyMapPage['rates']>[number];

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

    // 登录版：完整短评列表 + 我的评分。未连接时这个查询会失败，
    // 但**不把页面变成错误页**——公开的那一半照常显示，只是没有短评列表。
    const signedIn = useQuery({
        ...vrccMyMapPageQueryOptions(worldId),
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

            {/* 我的评分 + 完整短评列表（需要连接）。
                未连接或读失败时退回一行说明，不把整页变成错误。 */}
            {signedIn.data ? (
                <VrccSignedInSection page={signedIn.data} />
            ) : (
                <div className="text-muted-foreground text-[11px] leading-relaxed">
                    {signedIn.isPending
                        ? t('common.loading', '加载中…')
                        : t('dialog.world.vrcc.words_hint')}
                </div>
            )}

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

/**
 * 连接之后才有的一半：我的评分 + 完整短评列表。
 *
 * ## 为什么单独拆出来
 *
 * 它由另一个查询驱动（`/me/maps/{world}/page`），失败的原因和公开那一半
 * 完全不同：公开版失败是网络问题，这里失败多半是**没连接**。混在一个组件里
 * 会让「未连接」表现为「整页的加载失败」，那对玩家是误导。
 *
 * ## 显示什么
 *
 * 自己的那一条置顶并标明「我的评分」，下面按服务器给的顺序列出所有人的
 * 短评。**不自己拼匿名格式**：`who` 的完整程度由服务器按令牌权限决定，
 * 我们照原样显示，拼了反而会在权限变化时显示错的东西。
 */
function VrccSignedInSection({ page }: { page: VrccMyMapPage }) {
    const { t } = useTranslation();
    const mine = page.mine?.rate ?? null;
    const rates = (page.rates ?? []).filter((rate) => rate && !rate.folded);

    return (
        <div className="flex flex-col gap-3">
            {/* 我的评分 */}
            <div className="bg-card rounded-lg border p-3">
                <div className="text-muted-foreground mb-1.5 text-xs font-medium">
                    {t('dialog.world.vrcc.my_rating')}
                </div>
                {mine && typeof mine.score === 'number' ? (
                    <div className="flex flex-col gap-1">
                        <div className="flex items-center gap-2">
                            <span className="text-lg leading-none font-semibold tabular-nums">
                                {mine.score}
                            </span>
                            <span className="text-muted-foreground text-xs">
                                / 10
                            </span>
                            {mine.check && mine.check !== 'ok' ? (
                                <span className="bg-muted text-muted-foreground rounded px-1 text-[10px]">
                                    {t(
                                        mine.check === 'wait'
                                            ? 'dialog.world.vrcc.check_wait'
                                            : 'dialog.world.vrcc.check_hidden'
                                    )}
                                </span>
                            ) : null}
                        </div>
                        {mine.text ? (
                            <div className="text-sm leading-relaxed break-words whitespace-pre-wrap">
                                {mine.text}
                            </div>
                        ) : (
                            <div className="text-muted-foreground text-[11px]">
                                {t('dialog.world.vrcc.no_words')}
                            </div>
                        )}
                    </div>
                ) : (
                    <div className="text-muted-foreground text-[11px]">
                        {t('dialog.world.vrcc.not_rated_by_me')}
                    </div>
                )}
            </div>

            {/* 所有人的短评 */}
            <div className="flex flex-col gap-2">
                <div className="text-muted-foreground text-xs font-medium">
                    {t('dialog.world.vrcc.all_words', { count: rates.length })}
                </div>
                {rates.length ? (
                    rates.map((rate, index) => (
                        <VrccMapRateRow
                            key={rate.id ?? `${rate.who?.n ?? 'x'}-${index}`}
                            rate={rate}
                        />
                    ))
                ) : (
                    <div className="text-muted-foreground text-[11px]">
                        {t('dialog.world.vrcc.no_words_yet')}
                    </div>
                )}
            </div>
        </div>
    );
}

/** 一条短评：分数、正文、署名、时间。 */
function VrccMapRateRow({ rate }: { rate: VrccMapRate }) {
    const { t } = useTranslation();
    const who = rate.who;
    // 服务器给的署名可能只有匿名的「某位<星位>」，也可能带 VRChat 名字。
    // 有 VRChat 名字就显示它，否则显示 name，两者都没有才退回占位。
    const name = who?.vrc || who?.name || null;
    const score = typeof rate.score === 'number' ? rate.score : null;
    const hasText = Boolean(rate.text && rate.text.trim());

    return (
        <div className="bg-card rounded-lg border p-2.5">
            <div className="flex min-w-0 items-center gap-2 text-[11px]">
                {name ? (
                    <span className="text-foreground truncate font-medium">
                        {name}
                    </span>
                ) : null}
                {rate.mine ? (
                    <span className="bg-primary/15 text-primary shrink-0 rounded px-1 text-[10px]">
                        {t('dialog.world.vrcc.mine')}
                    </span>
                ) : null}
                {score !== null ? (
                    <span className="text-muted-foreground ml-auto shrink-0 tabular-nums">
                        {score} / 10
                    </span>
                ) : null}
            </div>
            {hasText ? (
                <div className="mt-1 text-sm leading-relaxed break-words whitespace-pre-wrap">
                    {rate.text}
                </div>
            ) : null}
        </div>
    );
}
