import { useQuery } from '@tanstack/react-query';
import { ExternalLinkIcon } from 'lucide-react';
import { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { formatRelativeTime } from '@/lib/dateTime';
import { queryKeys } from '@/lib/entityQueryCache';
import { cn } from '@/lib/utils';
import type { VrccMyMapPage } from '@/platform/tauri/bindings';
import {
    fetchVrccScopeGrants,
    voteVrccMapContent,
    vrccMyMapPageQueryOptions,
    vrccPublicMapQueryOptions
} from '@/repositories/vrccMapRepository';
import { Button } from '@/ui/shadcn/button';

import { VrccRatingEditor } from './VrccRatingEditor';
import {
    VrccCollapse,
    VrccMetricRow,
    VrccPerfGrid,
    VrccScoreSpread,
    VrccSection
} from './WorldVrccPanelParts';
import type { VrccMetric } from './WorldVrccPanelParts';

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
    // 排序是这一页的两个交互状态之一。默认「最热」，与官方一致。
    const [sort, setSort] = useState<'hot' | 'new'>('hot');
    // 配置四宫格默认收起：它是「想深究再看」的信息，不该占住首屏。
    const [perfOpen, setPerfOpen] = useState(false);
    const perfRef = useRef<HTMLDivElement | null>(null);

    // 展开后把它滚进视野。`block: 'nearest'` 在已经看得见时**不滚动**，
    // 只在放不下时移动最小距离——这样不会出现「先展开、再突然跳下去」。
    // 折叠时不动，避免把玩家刚点的那一项甩出视野。
    const togglePerf = useCallback(() => {
        setPerfOpen((previous) => {
            const next = !previous;
            if (next) {
                requestAnimationFrame(() => {
                    perfRef.current?.scrollIntoView({
                        block: 'nearest',
                        behavior: 'smooth'
                    });
                });
            }
            return next;
        });
    }, []);

    const publicQuery = useQuery({
        ...vrccPublicMapQueryOptions(worldId),
        enabled: Boolean(worldId)
    });

    const signedIn = useQuery({
        ...vrccMyMapPageQueryOptions(worldId, sort),
        enabled: Boolean(worldId)
    });

    // 令牌里有没有 `maps.rate`。**必须在任何提前 return 之前调用**：
    // 这个 Hook 写在下面（数据就绪之后）会让 Hook 顺序随渲染路径变化。
    const scopeQuery = useQuery({
        queryKey: queryKeys.vrccScope('maps.rate'),
        queryFn: () => fetchVrccScopeGrants('maps.rate'),
        staleTime: 60 * 1000,
        retry: 0,
        refetchOnWindowFocus: false
    });

    // 投票是另一项权限（maps.write），和打分（maps.rate）分开判定：
    // 两个敏感权限都可以单独缺失，混成一个布尔会让「能打分不能投票」的
    // 玩家看到一个点不动的赞。**同样必须在提前 return 之前调用。**
    const writeScopeQuery = useQuery({
        queryKey: queryKeys.vrccScope('maps.write'),
        queryFn: () => fetchVrccScopeGrants('maps.write'),
        staleTime: 60 * 1000,
        retry: 0,
        refetchOnWindowFocus: false
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

    // 「配置」是唯一可点的一项：它有下钻内容（四宫格）。其余三项没有
    // 下钻，保持纯文本——都做成可点会让人去点「去过 69」然后什么都不发生。
    const metricCandidates: Array<VrccMetric | null> = [
        weightLabel
            ? {
                  label: t('dialog.world.vrcc.perf_weight'),
                  value: weightLabel,
                  onToggle: perfCells.length ? togglePerf : undefined,
                  expanded: perfOpen,
                  toggleLabel: t('dialog.world.vrcc.perf_toggle')
              }
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
    ];
    const metrics = metricCandidates.filter(
        (item): item is VrccMetric => item !== null
    );

    const mine = signedIn.data?.mine?.rate ?? null;
    const intro = signedIn.data?.intro?.text ?? null;
    const rates = (signedIn.data?.rates ?? []).filter(
        (rate) => rate && !rate.folded
    );
    // 能不能打分，取决于**令牌里有没有 `maps.rate`**，而不是能不能连上。
    // 已经连上、但令牌是加权限之前发的玩家，正是这里的分水岭：他们有 `maps`
    // 却没有 `maps.rate`，服务器会回 `403 scope`。给这类玩家看一个点不动的
    // 打分器，不如直接告诉他「重新连接一次即可」。
    const canRate = scopeQuery.data === true;
    const canVote = writeScopeQuery.data === true;

    return (
        // `h-full` + 内部 flex：让这一页永远不超过标签页容器的高度。
        // 前面的分节各占自己该占的，剩下的全部给短评区，由它自己滚动。
        // 标签页容器本身是 `overflow-auto`（别的页签都靠它滚），我们不改它，
        // 只是让这一页正好塞得进去。
        <div className="flex h-full min-w-0 flex-col gap-5 overflow-hidden pb-2">
            {/* ① 指标行。点「配置」会在它正下方展开四宫格。 */}
            <div className="flex min-w-0 shrink-0 flex-col gap-2">
                <VrccMetricRow items={metrics} />
                <div ref={perfRef}>
                    <VrccCollapse open={perfOpen}>
                        <div className="pb-1">
                            <VrccPerfGrid items={perfCells} />
                        </div>
                    </VrccCollapse>
                </div>
            </div>

            {/* ② 地图介绍 */}
            <VrccSection
                title={t('dialog.world.vrcc.intro')}
                className="shrink-0"
            >
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
            <VrccSection
                title={t('dialog.world.vrcc.rate_section')}
                className="shrink-0"
            >
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

                    {/* 我自己的分：现在可以改。
                        写操作需要 maps.rate，所以只有连上之后才给控件；
                        未连接时连输入框都不出现——摆一个点不动的打分器
                        比没有更糟。 */}
                    {canRate ? (
                        <VrccRatingEditor
                            worldId={worldId}
                            currentScore={
                                mine && typeof mine.score === 'number'
                                    ? mine.score
                                    : null
                            }
                            currentText={mine?.text ?? null}
                            check={mine?.check ?? null}
                            onDone={() => {
                                void signedIn.refetch();
                            }}
                        />
                    ) : mine ? (
                        <MineRating rate={mine} />
                    ) : signedIn.data && scopeQuery.data === false ? (
                        // 连上了，但令牌里没有 maps.rate。这种情况几乎都是
                        // 「令牌是加权限之前签发的」，重连一次就好——直接说清楚，
                        // 比给一个点了必然 403 的控件有用。
                        <span className="text-muted-foreground text-xs">
                            {t('dialog.world.vrcc.rate_needs_scope')}
                        </span>
                    ) : null}
                </div>
            </VrccSection>

            <div className="shrink-0">
                <VrccScoreSpread spread={spread} />
            </div>

            {/* ④ 短评。这一节吃掉剩余高度，列表在它内部滚动——
                页面本身不需要滚动条。 */}
            <VrccSection
                title={t('dialog.world.vrcc.comments_title')}
                aside={
                    signedIn.data ? (
                        <SortToggle value={sort} onChange={setSort} />
                    ) : null
                }
                className="min-h-24 flex-1"
            >
                {!signedIn.data ? (
                    <p className="text-muted-foreground text-xs leading-relaxed">
                        {signedIn.isPending
                            ? t('common.loading', '加载中…')
                            : t('dialog.world.vrcc.words_hint')}
                    </p>
                ) : rates.length ? (
                    <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pr-1">
                        {rates.map((rate, index) => (
                            <CommentCard
                                key={
                                    rate.id ?? `${rate.who?.n ?? 'x'}-${index}`
                                }
                                rate={rate}
                                canVote={canVote}
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
                <VrccSection
                    title={t('dialog.world.vrcc.hot')}
                    className="shrink-0"
                >
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

/**
 * 赞 / 踩按钮。
 *
 * ## 三件事必须同时成立才可点
 *
 * 1. 有 `maps.write`（否则服务器 `403 scope`）；
 * 2. 这条**不是我自己写的**（官方禁止投自己的票）；
 * 3. 这条有编号（公开接口那条热评没有 `id`，投不了）。
 *
 * 任一不满足就只显示计数、不做成按钮：一个点了必然失败的按钮比纯数字更糟。
 *
 * ## 点击语义
 *
 * 点已赞的赞 = 取消（官方 `v: 0`）。这个换算写在契约层的
 * `VrccVote::toggled_from` 里，前后端共用同一份判断。
 *
 * ## 为什么本地存一份计数
 *
 * 投票接口返回的是**更新之后**的 up/down 与我的票，直接拿来刷新这一行，
 * 不必重查整个列表。若失败则回滚到原值——不做乐观更新，只在成功后写。
 */
function VoteButtons({
    rate,
    canVote
}: {
    rate: CommentLike;
    canVote?: boolean;
}) {
    const { t } = useTranslation();
    const id = typeof rate.id === 'number' ? rate.id : null;
    const isMine = rate.mine === true;

    const [up, setUp] = useState(typeof rate.up === 'number' ? rate.up : 0);
    const [down, setDown] = useState(
        typeof rate.down === 'number' ? rate.down : 0
    );
    const [myVote, setMyVote] = useState<'none' | 'up' | 'down'>(
        rate.vote === 1 ? 'up' : rate.vote === -1 ? 'down' : 'none'
    );
    const [busy, setBusy] = useState(false);

    const interactive = Boolean(canVote) && !isMine && id !== null;

    async function cast(tapped: 'up' | 'down') {
        if (!interactive || id === null || busy) {
            return;
        }
        // 点已选的那个 = 取消
        const next = myVote === tapped ? 'none' : tapped;
        setBusy(true);
        try {
            const result = await voteVrccMapContent(id, next);
            // 以服务器返回的为准
            if (typeof result.up === 'number') {
                setUp(result.up);
            }
            if (typeof result.down === 'number') {
                setDown(result.down);
            }
            // 注意这个字段叫 `mine`——它来自官方的 `mine`，含义是**我投的票**
            // （整数），不是「这条是不是我写的」。后者在 `CommentLike.mine`
            // （布尔）。两者同名不同义，读代码时别弄混。
            setMyVote(
                result.mine === 1 ? 'up' : result.mine === -1 ? 'down' : 'none'
            );
        } catch {
            // 写操作不重试，也不谎报成功：计数保持原样，玩家可以再点一次。
        } finally {
            setBusy(false);
        }
    }

    const label = (kind: 'up' | 'down') =>
        kind === 'up'
            ? t('dialog.world.vrcc.vote_up')
            : t('dialog.world.vrcc.vote_down');

    // 只读形态：连 0 都不显示，避免一排 `0` 噪音
    if (!interactive) {
        const showUp = up > 0;
        const showDown = down > 0;
        if (!showUp && !showDown) {
            return null;
        }
        return (
            <span
                className="text-muted-foreground flex items-center gap-2"
                // 不可投票时说清为什么，免得玩家以为界面坏了
                title={
                    isMine
                        ? t('dialog.world.vrcc.vote_own_hint')
                        : !canVote
                          ? t('dialog.world.vrcc.vote_needs_scope')
                          : undefined
                }
            >
                {showUp ? <span>{`👍 ${up}`}</span> : null}
                {showDown ? <span>{`👎 ${down}`}</span> : null}
            </span>
        );
    }

    return (
        <span className="flex items-center gap-0.5">
            <VoteButton
                active={myVote === 'up'}
                disabled={busy}
                count={up}
                icon="👍"
                label={label('up')}
                onClick={() => void cast('up')}
            />
            <VoteButton
                active={myVote === 'down'}
                disabled={busy}
                count={down}
                icon="👎"
                label={label('down')}
                onClick={() => void cast('down')}
            />
        </span>
    );
}

function VoteButton({
    active,
    disabled,
    count,
    icon,
    label,
    onClick
}: {
    active: boolean;
    disabled: boolean;
    count: number;
    icon: string;
    label: string;
    onClick: () => void;
}) {
    return (
        <button
            type="button"
            onClick={onClick}
            disabled={disabled}
            aria-pressed={active}
            aria-label={label}
            className={cn(
                'flex items-center gap-0.5 rounded px-1 py-0.5 transition-colors',
                'hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50',
                'focus-visible:ring-ring focus-visible:ring-1 focus-visible:outline-none',
                active && 'bg-primary/15 text-primary'
            )}
        >
            <span aria-hidden="true">{icon}</span>
            {/* 票数为 0 时不显示数字，让按钮更紧凑 */}
            {count > 0 ? <span className="tabular-nums">{count}</span> : null}
        </button>
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
function CommentCard({
    rate,
    canVote
}: {
    rate: CommentLike;
    canVote?: boolean;
}) {
    const { t } = useTranslation();
    const who = rate.who;
    // 服务器给的署名可能只有匿名的「某位<星位>」，也可能带 VRChat 名字。
    // 有 VRChat 名字就显示它，否则显示 name，再退到 by（热评那条）。
    const name = who?.vrc || who?.name || rate.by || null;
    const score = typeof rate.score === 'number' ? rate.score : null;
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
                <span className="ml-auto flex shrink-0 items-center gap-1">
                    <VoteButtons rate={rate} canVote={canVote} />
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
