import { useQuery } from '@tanstack/react-query';
import { ExternalLinkIcon } from 'lucide-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { formatRelativeTime } from '@/lib/dateTime';
import { cn } from '@/lib/utils';
import type { VrccMyMapPage } from '@/platform/tauri/bindings';
import {
    vrccMyMapPageQueryOptions,
    vrccPublicMapQueryOptions
} from '@/repositories/vrccMapRepository';
import { Button } from '@/ui/shadcn/button';

import {
    VrccMetricRow,
    VrccPerfGrid,
    VrccScoreSpread,
    VrccSection
} from './WorldVrccPanelParts';

/** 一条短评。从绑定类型里派生，免得两处各写一份形状。 */
type VrccMapRate = NonNullable<VrccMyMapPage['rates']>[number];

/**
 * `CommentCard` 吃的东西：完整短评，或者公开接口给的那一条热评。
 *
 * 两者形状不同（热评没有 `who`，署名在 `by`；它有自己的 `went` 字段），
 * 但卡片要表达的是同一件事，所以在这里放宽成一个联合形状，而不是写两个
 * 几乎一样的组件。
 */
type CommentLike = Partial<VrccMapRate> & {
    by?: string | null;
    went?: boolean | null;
};

const SCORE_MAX = 10;

/**
 * 「星遇同频」标签页。
 *
 * ## 页面结构
 *
 * 照官方地图页的分段走（指标行 / 介绍 / 打分 / 短评 / 配置），因为我们自己
 * 一开始把它排成一坨竖列表单，读起来完全没有「一张地图的资料页」的感觉。
 * **只照结构，不照搬任何样式值**：官网前端是保留所有权利的资产，配色与尺寸
 * 全部取自本项目的 design token。
 *
 * ## 数据分两半
 *
 * - 公开的 `/maps/{world}/page`：均分、分布、去过、上限、实测、一条热评。
 *   不需要令牌，所以**没连星遇同频也有一页东西可看**。
 * - 带令牌的 `/me/maps/{world}/page`：完整短评、我的评分、地图介绍。
 *   未连接时失败——这是预期，公开那一半照常显示。
 *
 * 两半各有自己的查询与缓存键，失败也各自降级，互不连坐。
 */
export function WorldVrccPanel({ worldId }: { worldId: string }) {
    const { t } = useTranslation();
    // 排序是这一页唯一的交互状态。默认「最热」，与官方一致。
    const [sort, setSort] = useState<'hot' | 'new'>('hot');

    const publicQuery = useQuery({
        ...vrccPublicMapQueryOptions(worldId),
        enabled: Boolean(worldId)
    });

    const signedIn = useQuery({
        ...vrccMyMapPageQueryOptions(worldId, sort),
        enabled: Boolean(worldId)
    });

    if (publicQuery.isPending) {
        return (
            <div className="text-muted-foreground flex h-40 items-center justify-center text-sm">
                {t('common.loading', '加载中…')}
            </div>
        );
    }

    // 请求失败是「我们读不到」，与「这张图不在榜上」不同。分开说。
    if (publicQuery.isError) {
        return (
            <div className="text-muted-foreground flex h-40 items-center justify-center text-sm">
                {t('dialog.world.vrcc.load_failed')}
            </div>
        );
    }

    // 后端把 404 转成 null：这张图不在榜上（含未过审）。
    if (publicQuery.data === null) {
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

    if (!publicQuery.data) {
        return null;
    }

    const { map, went, cap, perf } = publicQuery.data;
    const spread = publicQuery.data.spread ?? [];
    const avg = typeof map.avg === 'number' ? map.avg : null;
    const raters = typeof map.raters === 'number' ? map.raters : 0;
    const hasRating = avg !== null && raters > 0;

    // 顶部指标行：配置档位 / 去过 / 平台 / 房间上限。缺的项直接不出现。
    const weightLabel = perf?.weight
        ? t(`dialog.world.vrcc.weight_${perf.weight}`)
        : null;
    const platformText = perf?.platforms?.length
        ? perf.platforms.join(' · ')
        : null;
    const metrics = [
        weightLabel
            ? { label: t('dialog.world.vrcc.perf_weight'), value: weightLabel }
            : null,
        typeof went === 'number' && went > 0
            ? {
                  label: t('dialog.world.vrcc.went_label'),
                  value: String(went)
              }
            : null,
        platformText
            ? {
                  label: t('dialog.world.vrcc.perf_platforms'),
                  value: platformText
              }
            : null,
        cap && cap > 0
            ? { label: t('dialog.world.vrcc.cap_label'), value: String(cap) }
            : null
    ].filter((item): item is { label: string; value: string } => item !== null);

    // 配置四宫格：显存 / 内存 / 加载 / 下载。
    const perfCells = [
        typeof perf?.vram === 'number' && perf.vram > 0
            ? {
                  label: t('dialog.world.vrcc.perf_vram'),
                  value: `${Math.round(perf.vram)}M`
              }
            : null,
        typeof perf?.mem === 'number' && perf.mem > 0
            ? {
                  label: t('dialog.world.vrcc.perf_mem'),
                  value: `${(perf.mem / 1024).toFixed(1)}G`
              }
            : null,
        typeof perf?.load === 'number' && perf.load > 0
            ? {
                  label: t('dialog.world.vrcc.perf_load'),
                  value: `${Math.round(perf.load / 1000)}${t('dialog.world.vrcc.unit_sec')}`
              }
            : null,
        typeof perf?.size === 'number' && perf.size > 0
            ? {
                  label: t('dialog.world.vrcc.perf_size'),
                  value: `${Math.round(perf.size / 1024 / 1024)}M`
              }
            : null
    ].filter((item): item is { label: string; value: string } => item !== null);

    const mine = signedIn.data?.mine?.rate ?? null;
    const intro = signedIn.data?.intro?.text ?? null;
    const rates = (signedIn.data?.rates ?? []).filter(
        (rate) => rate && !rate.folded
    );

    return (
        <div className="flex min-w-0 flex-col gap-5 pb-2">
            {/* ① 指标行 */}
            <VrccMetricRow items={metrics} />

            {/* ② 地图介绍 */}
            <VrccSection title={t('dialog.world.vrcc.intro')}>
                {intro ? (
                    <p className="text-sm leading-relaxed break-words whitespace-pre-wrap">
                        {intro}
                    </p>
                ) : (
                    <p className="text-muted-foreground text-xs">
                        {signedIn.data
                            ? t('dialog.world.vrcc.no_intro')
                            : t('dialog.world.vrcc.words_hint')}
                    </p>
                )}
            </VrccSection>

            {/* ③ 打个分吧：主分数 + 大号星星 */}
            <VrccSection title={t('dialog.world.vrcc.rate_section')}>
                <div className="flex min-w-0 flex-col gap-2">
                    <div className="flex min-w-0 items-center gap-3">
                        <span className="text-3xl leading-none font-semibold tabular-nums">
                            {hasRating ? avg.toFixed(1) : '—'}
                        </span>
                        <div className="flex min-w-0 flex-col gap-1">
                            <StarRow score={hasRating ? avg : 0} />
                            <span className="text-muted-foreground text-[11px] leading-none">
                                {hasRating
                                    ? t('dialog.world.vrcc.raters', {
                                          count: raters
                                      })
                                    : t('dialog.world.vrcc.no_rating')}
                            </span>
                        </div>
                        {/* 名次只在这一页出现，概览那一块刻意不显示 */}
                        {typeof map.rank === 'number' && map.rank > 0 ? (
                            <span className="text-muted-foreground ml-auto shrink-0 text-[11px] tabular-nums">
                                {t('dialog.world.vrcc.rank', {
                                    rank: map.rank
                                })}
                            </span>
                        ) : null}
                    </div>

                    {/* 我自己的分：只读展示。写入要 maps.rate，属于下一批 */}
                    {mine ? (
                        <MineRating rate={mine} />
                    ) : signedIn.data ? (
                        <span className="text-muted-foreground text-xs">
                            {t('dialog.world.vrcc.not_rated_by_me')}
                        </span>
                    ) : null}
                </div>
            </VrccSection>

            <VrccScoreSpread spread={spread} />

            {/* ④ 短评 */}
            <VrccSection
                title={t('dialog.world.vrcc.comments_title')}
                aside={
                    signedIn.data ? (
                        <SortToggle value={sort} onChange={setSort} />
                    ) : null
                }
            >
                {!signedIn.data ? (
                    <p className="text-muted-foreground text-xs leading-relaxed">
                        {signedIn.isPending
                            ? t('common.loading', '加载中…')
                            : t('dialog.world.vrcc.words_hint')}
                    </p>
                ) : rates.length ? (
                    <div className="flex flex-col gap-2">
                        {rates.map((rate, index) => (
                            <CommentCard
                                key={
                                    rate.id ?? `${rate.who?.n ?? 'x'}-${index}`
                                }
                                rate={rate}
                            />
                        ))}
                    </div>
                ) : (
                    <p className="text-muted-foreground text-xs">
                        {t('dialog.world.vrcc.no_words_yet')}
                    </p>
                )}
            </VrccSection>

            {/* ⑤ 热评：公开接口唯一给的一条，登录后没有短评列表时用它兜底 */}
            {!signedIn.data && map.hot?.text ? (
                <VrccSection title={t('dialog.world.vrcc.hot')}>
                    <CommentCard
                        rate={{
                            text: map.hot.text,
                            by: map.hot.by,
                            went: map.hot.went,
                            up: map.hot.up
                        }}
                    />
                </VrccSection>
            ) : null}

            {/* ⑥ 配置要求 */}
            {perfCells.length ? (
                <VrccSection title={t('dialog.world.vrcc.perf_section')}>
                    <VrccPerfGrid items={perfCells} />
                </VrccSection>
            ) : null}

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

/** 排序切换：最热 / 最新。 */
function SortToggle({
    value,
    onChange
}: {
    value: 'hot' | 'new';
    onChange: (next: 'hot' | 'new') => void;
}) {
    const { t } = useTranslation();
    const options: Array<{ key: 'hot' | 'new'; label: string }> = [
        { key: 'hot', label: t('dialog.world.vrcc.sort_hot') },
        { key: 'new', label: t('dialog.world.vrcc.sort_new') }
    ];
    return (
        <div className="bg-muted/60 flex items-center gap-0.5 rounded-md p-0.5">
            {options.map((option) => (
                <button
                    key={option.key}
                    type="button"
                    onClick={() => onChange(option.key)}
                    aria-pressed={value === option.key}
                    className={cn(
                        'rounded px-1.5 py-0.5 text-[10px] leading-none transition-colors',
                        value === option.key
                            ? 'bg-background text-foreground font-medium'
                            : 'text-muted-foreground hover:text-foreground'
                    )}
                >
                    {option.label}
                </button>
            ))}
        </div>
    );
}

/** 我自己的评分块：分数 + 审核状态 + 我写的短评。 */
function MineRating({ rate }: { rate: VrccMapRate }) {
    const { t } = useTranslation();
    const score = typeof rate.score === 'number' ? rate.score : null;
    return (
        <div className="bg-muted/40 flex min-w-0 flex-col gap-1 rounded-md border px-2 py-1.5">
            <div className="flex min-w-0 items-center gap-2 text-[11px]">
                <span className="text-muted-foreground">
                    {t('dialog.world.vrcc.my_rating')}
                </span>
                {score !== null ? (
                    <span className="text-foreground font-semibold tabular-nums">
                        {score} / {SCORE_MAX}
                    </span>
                ) : null}
                {rate.check && rate.check !== 'ok' ? (
                    <span className="text-muted-foreground bg-background/60 ml-auto rounded px-1 text-[10px]">
                        {t(
                            rate.check === 'wait'
                                ? 'dialog.world.vrcc.check_wait'
                                : 'dialog.world.vrcc.check_hidden'
                        )}
                    </span>
                ) : null}
            </div>
            <span className="text-sm leading-relaxed break-words whitespace-pre-wrap">
                {rate.text || t('dialog.world.vrcc.no_words')}
            </span>
        </div>
    );
}

/**
 * 一条短评。
 *
 * 结构照官方那一页：**顶部署名行**（谁写的、留下的分、去过标记），
 * **中间正文**，**底部元信息行**（时间、赞踩）。三个区域各有自己的字号层级。
 *
 * `by` 只在热评（公开接口那条）里有——它是服务器拼好的匿名署名；完整列表
 * 里用的是 `who`，两者形状不同，所以都要接受。
 */
function CommentCard({ rate }: { rate: CommentLike }) {
    const { t } = useTranslation();
    const who = rate.who;
    // 服务器给的署名可能只有匿名的「某位<星位>」，也可能带 VRChat 名字。
    // 有 VRChat 名字就显示它，否则显示 name，再退到 by（热评那条）。
    const name = who?.vrc || who?.name || rate.by || null;
    const score = typeof rate.score === 'number' ? rate.score : null;
    const up = typeof rate.up === 'number' ? rate.up : 0;
    const down = typeof rate.down === 'number' ? rate.down : 0;
    const at = rate.at;
    const hasText = Boolean(rate.text && rate.text.trim());

    return (
        <div className="bg-card flex min-w-0 flex-col gap-1.5 rounded-lg border p-2.5">
            <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                {name ? (
                    <span className="text-foreground truncate text-xs font-medium">
                        {name}
                    </span>
                ) : null}
                {score !== null ? <StarRow score={score} small /> : null}
                {rate.went ? (
                    <span className="bg-muted text-muted-foreground rounded px-1 text-[10px] leading-tight">
                        {t('dialog.world.vrcc.went_badge')}
                    </span>
                ) : null}
                {rate.mine ? (
                    <span className="bg-primary/15 text-primary rounded px-1 text-[10px] leading-tight">
                        {t('dialog.world.vrcc.mine')}
                    </span>
                ) : null}
            </div>

            {hasText ? (
                <p className="text-sm leading-relaxed break-words whitespace-pre-wrap">
                    {rate.text}
                </p>
            ) : null}

            <div className="text-muted-foreground flex min-w-0 items-center gap-2 text-[10px]">
                {at ? (
                    <span>{formatRelativeTime(at, { style: 'short' })}</span>
                ) : null}
                <span className="ml-auto flex shrink-0 items-center gap-2">
                    {up > 0 ? <span>{`👍 ${up}`}</span> : null}
                    {down > 0 ? <span>{`👎 ${down}`}</span> : null}
                </span>
            </div>
        </div>
    );
}

/** 一排星星。分数是 1–10，按 `score / 2` 折成 0–5 星。 */
function StarRow({ score, small }: { score: number; small?: boolean }) {
    const filled = Math.max(0, Math.min(5, score / 2));
    const size = small ? 'size-3' : 'size-5';
    return (
        <span
            className={cn('inline-flex shrink-0 items-center gap-0.5')}
            role="img"
            aria-label={score.toFixed(1)}
        >
            {[0, 1, 2, 3, 4].map((index) => {
                const ratio = Math.max(0, Math.min(1, filled - index));
                return (
                    <span key={index} className={cn('relative', size)}>
                        <StarGlyph
                            className={cn(
                                'text-muted-foreground/30 absolute inset-0',
                                size
                            )}
                        />
                        {ratio > 0 ? (
                            <span
                                className="absolute inset-0 overflow-hidden"
                                style={{ width: `${ratio * 100}%` }}
                            >
                                <StarGlyph
                                    className={cn('text-primary', size)}
                                />
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
            className={cn(className)}
            fill="currentColor"
        >
            <path d="M12 3.4l2.6 5.5 6 .8-4.4 4.2 1.1 6L12 17l-5.3 2.9 1.1-6-4.4-4.2 6-.8z" />
        </svg>
    );
}
