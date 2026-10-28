import { Link2, Radar, Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/ui/shadcn/button';

import type { VrccCrossingCards, VrccCrossings } from '../vrccApi';

/**
 * 星轨相交：在同一个 VRChat 房间偶遇过的星遇同频玩家。
 *
 * 与「互相连上」的区别（两者都在同一层页签里）：
 * - 互相连上：**双向已连线**的星友，是结果；
 * - 星轨相交：只是**同房间擦肩**，大多数还没连线，是线索。
 *
 * 所以这里每条要显示 `linked`（我已连 TA）和 `mutual`（已双向）两个
 * 状态，而不是只显示名单。
 *
 * **为什么单独处理 `why`**：官方文档规定 `hide` 时「客户端不应显示此
 * 模块」——16–17 岁星池没有星轨相交，玩家在隐私设置里也能关掉。把
 * `hide` 当成普通空列表处理，等于向不该看到的人确认「这里有星轨相交」
 * 这个事实本身。其余几种原因（未验证、机器人好友排队等）**要显示**
 * 并说明，因为那是「功能在但暂时用不了」，玩家需要知道下一步做什么。
 *
 * **合规边界**：与 mutuals 一样，只提供「打开 VRCX-0 自己的玩家详情页」。
 * 星轨相交的条目大多没有 `vrcUser`（对方未公开已验证），这里也不提供
 * 任何代发好友请求的入口。
 */
export function VrccCrossingsPanel({
    connected,
    crossings,
    cards,
    crossingsError,
    crossingsWhy,
    onOpenLink,
    onRefresh
}: {
    connected: boolean;
    crossings: VrccCrossings | null;
    /** 完整星卡，用来显示「今天还能连几颗」。旧服务器上为 null。 */
    cards: VrccCrossingCards | null;
    /** 这项功能没开通的原因（`dev-` 标识必然要不到 crossings）。 */
    crossingsError?: string | null;
    /** 服务端说的 `why`；`hide` 时整个模块不显示。 */
    crossingsWhy?: string | null;
    onOpenLink: (url: string) => void;
    onRefresh: () => void;
}) {
    const { t } = useTranslation();

    if (!connected) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 text-sm">
                <Radar className="size-6" />
                {t('vrcc.crossings.need_connect')}
            </div>
        );
    }

    // `hide`：官方要求整个模块不显示。这里返回 null，连「不可用」都不说。
    if (crossingsWhy === 'hide') {
        return null;
    }

    const stars = crossings?.stars ?? [];
    const left = cards?.left;
    const max = cards?.max;

    // 有 why 且不是 hide：功能在，但暂时用不了，要说明下一步。
    if (crossingsWhy && stars.length === 0) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 px-6 text-center text-sm">
                <Radar className="size-6" />
                <div className="leading-relaxed">
                    {t(`vrcc.crossings.why.${crossingsWhy}`, {
                        defaultValue: t('vrcc.crossings.why.unknown')
                    })}
                </div>
            </div>
        );
    }

    if (stars.length === 0) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 px-6 text-center text-sm">
                <Radar className="size-6" />
                <div className="leading-relaxed">
                    {crossingsError
                        ? t('vrcc.crossings.unavailable')
                        : t('vrcc.crossings.empty')}
                </div>
                {crossingsError && (
                    <div className="text-[11px] leading-relaxed">
                        {t('vrcc.crossings.needs_scope')}
                    </div>
                )}
            </div>
        );
    }

    return (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
            {/* 顶部：今天还能连几颗 + 手动刷新。
                星卡（cards）只有 1.13.0 起才有，旧服务器上这一行不显示。 */}
            <div className="flex shrink-0 items-center justify-between px-1">
                <div className="text-muted-foreground flex items-center gap-1.5 text-xs">
                    <Sparkles className="size-3.5" />
                    {left != null && max != null
                        ? t('vrcc.crossings.quota', { left, max })
                        : t('vrcc.crossings.count', { count: stars.length })}
                </div>
                <Button
                    variant="ghost"
                    size="sm"
                    onClick={onRefresh}
                    title={t('vrcc.crossings.refresh')}
                >
                    {t('vrcc.crossings.refresh')}
                </Button>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border">
                <ul className="divide-y">
                    {stars.map((star) => {
                        // 星轨相交的契约里**没有** vrcUser：官方只在互相连上
                        // 的端点上给这个 ID。所以这里不提供「打开玩家详情」，
                        // 只把玩家送到网页那一页自己看完整星卡。
                        // 新产生的记录（上次查看之后）加一点提示。
                        const isFresh = star.fresh === true;
                        return (
                            <li
                                key={star.n}
                                className="hover:bg-accent/40 flex items-start gap-3 px-3 py-2"
                            >
                                <span className="text-muted-foreground w-8 shrink-0 pt-0.5 text-right text-xs tabular-nums">
                                    #{star.n}
                                </span>
                                <div className="min-w-0 flex-1">
                                    <div className="flex items-center gap-1.5">
                                        <span className="truncate text-sm font-medium">
                                            {star.name ??
                                                t('vrcc.crossings.unnamed')}
                                        </span>
                                        {isFresh && (
                                            <span className="bg-primary/15 text-primary shrink-0 rounded-full px-1.5 text-[10px] leading-4">
                                                {t('vrcc.crossings.fresh')}
                                            </span>
                                        )}
                                    </div>
                                    {/* 一句话描述；没有就退回匿名星卡介绍。 */}
                                    {(star.line || star.intro) && (
                                        <div className="text-muted-foreground mt-0.5 line-clamp-2 text-xs leading-relaxed">
                                            {star.line ?? star.intro}
                                        </div>
                                    )}
                                    {/* vrc 只在双向连上后才有，正是要注意的地方：
                                        没连上时这里显示的是「未连线」而不是空白。 */}
                                    <div className="text-muted-foreground mt-0.5 flex items-center gap-2 text-[11px]">
                                        {star.mutual ? (
                                            <span className="flex items-center gap-0.5">
                                                <Link2 className="size-3" />
                                                {star.vrc ??
                                                    t('vrcc.crossings.mutual')}
                                            </span>
                                        ) : star.linked ? (
                                            <span className="flex items-center gap-0.5">
                                                <Link2 className="size-3" />
                                                {t('vrcc.crossings.linked')}
                                            </span>
                                        ) : (
                                            <span>
                                                {t('vrcc.crossings.not_linked')}
                                            </span>
                                        )}
                                    </div>
                                </div>
                                <div className="flex shrink-0 flex-col gap-1">
                                    {/* 网页上「星轨相交」那一页：可以看完整星卡、
                                        也可以从那里连线。客户端不代做连线，
                                        只把玩家送到网页上自己决定。 */}
                                    <Button
                                        variant="ghost"
                                        size="sm"
                                        onClick={() =>
                                            onOpenLink(
                                                `https://vrcc.cc/match/?tab=inbox&cross=${star.n}`
                                            )
                                        }
                                        title={t('vrcc.crossings.open_page')}
                                    >
                                        <Radar className="size-3.5" />
                                    </Button>
                                </div>
                            </li>
                        );
                    })}
                </ul>
            </div>
        </div>
    );
}
