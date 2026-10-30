import { ExternalLink, Radar, Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/ui/shadcn/button';

import type { VrccCrossingCards, VrccCrossings } from '../vrccApi';

const SITE = 'https://vrcc.cc';

/**
 * `why` 的白名单。
 *
 * 不做 `t(`vrcc.crossings.why.${why}`)` 拼键名：`why` 是服务器给的字符串，
 * 拼出来可以是 `constructor`、`__proto__` 这种本来就存在的对象属性，
 * 拿到的不是文案而是对象/`undefined`，界面直接崩。白名单里没有的一律
 * 走 `unknown`。
 */
const WHY_KEYS = [
    'verify',
    'wait',
    'queued',
    'sent',
    'gone',
    'friend'
] as const;

type WhyKey = (typeof WHY_KEYS)[number];

function whyKey(why: string): WhyKey | null {
    return (WHY_KEYS as readonly string[]).includes(why)
        ? (why as WhyKey)
        : null;
}

/**
 * 星轨相交：在同一个 VRChat 房间偶遇过的星遇同频玩家。
 *
 * 与「互相连上」的区别（两者是同一层的两个页签）：
 * - 互相连上：**双向已连线**的星友，是结果；
 * - 星轨相交：只是**同房间擦肩**，大多数还没连线，是线索。
 *
 * 所以每条显示 `mutual` / `linked` / 都没有 三种状态，而不是只给名单。
 *
 * **`hidden` 一定要先处理**：官方文档规定 `hide` 时「客户端不应显示此
 * 模块」——16–17 岁星池没有星轨相交，玩家在隐私设置里也能关掉。把 `hide`
 * 当成普通空列表，等于向不该看到的人确认「这里有星轨相交」这个事实本身。
 * 其余几种原因（未验证、机器人好友排队等）**要显示**并说明下一步，因为
 * 那是「功能在但暂时用不了」。
 *
 * **合规边界**：星轨相交的条目**没有** `vrcUser`（官方只在 `mutuals` 端点
 * 给这个 ID），所以这里不提供「打开玩家详情」，只把玩家送到网页那一页
 * 自己看完整星卡、自己决定连不连。程序不代做任何连线动作，也不代发好友请求。
 */
export function VrccCrossingsPanel({
    connected,
    crossings,
    cards,
    crossingsError,
    onOpenLink,
    onRefresh
}: {
    connected: boolean;
    crossings: VrccCrossings | null;
    /** 完整星卡，只用来显示「今天还能连几颗」。旧服务器上为 null。 */
    cards: VrccCrossingCards | null;
    /** 这项功能没开通的原因（`dev-` 标识必然要不到 crossings）。 */
    crossingsError?: string | null;
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

    // `hide`：官方要求整个模块不显示。返回 null，连「不可用」都不说。
    //
    // 判据是后端算好的 `hidden` 字段，不是这里的 `why === 'hide'` 字符串
    // 比较——那样每个消费方都要各判一次，漏一处就是一次信息泄漏。
    if (crossings?.hidden) {
        return null;
    }

    const stars = crossings?.stars ?? [];
    const why = crossings?.why ?? null;
    const left = cards?.left;
    const max = cards?.max;

    // 有 why：功能在，但暂时用不了，要说明下一步做什么。
    if (why && stars.length === 0) {
        const key = whyKey(why);
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 px-6 text-center text-sm">
                <Radar className="size-6" />
                <div className="leading-relaxed">
                    {key
                        ? t(`vrcc.crossings.why.${key}`)
                        : t('vrcc.crossings.why.unknown')}
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
                {/* 只有真的因为缺权限失败时才提示去开权限。已经有权限、
                    只是确实还没擦肩的玩家，看到「重新连接一次」只会困惑。 */}
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
                额度只有星卡接口（1.13.0 起）才给，旧服务器上退回条数。 */}
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
                    {stars.map((star) => (
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
                                    {star.fresh && (
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
                                {/* `vrc` 只在双向连上后才有。没连上时显示
                                    的是「未连线」，不是空白——空白会被读成
                                    「对方没有名字」。 */}
                                <div className="text-muted-foreground mt-0.5 flex items-center gap-2 text-[11px]">
                                    {star.mutual ? (
                                        <span className="flex items-center gap-0.5">
                                            <ExternalLink className="size-3" />
                                            {star.vrc ??
                                                t('vrcc.crossings.mutual')}
                                        </span>
                                    ) : star.linked ? (
                                        <span>
                                            {t('vrcc.crossings.linked')}
                                        </span>
                                    ) : (
                                        <span>
                                            {t('vrcc.crossings.not_linked')}
                                        </span>
                                    )}
                                </div>
                            </div>
                            {/* 网页上「星轨相交」那一页：可以看完整星卡，
                                也可以从那里连线。客户端不代做连线。 */}
                            <Button
                                variant="ghost"
                                size="icon"
                                className="shrink-0"
                                onClick={() =>
                                    onOpenLink(
                                        `${SITE}/match/?tab=inbox&cross=${star.n}`
                                    )
                                }
                                aria-label={t('vrcc.crossings.open_page')}
                                title={t('vrcc.crossings.open_page')}
                            >
                                <Radar className="size-3.5" />
                            </Button>
                        </li>
                    ))}
                </ul>
            </div>
        </div>
    );
}
