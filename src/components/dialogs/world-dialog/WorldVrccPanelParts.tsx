import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';

/**
 * 分数分布条：9–10、7–8、5–6、3–4、1–2 各占百分之几。
 *
 * 服务端给的是五个段各自的比例，加起来不一定正好 100（四舍五入）。
 * 这里**不做归一**：直接按服务端给的宽度画，加起来少于 100 时右边留空。
 * 归一化会让人以为「数据里就是这样」，而那其实是我们改过的。
 *
 * 用横向条形而不是饼图/柱状图：段是**有序**的（分数从高到低），
 * 条形天然表达顺序，饼图会把它变成一个环、顺序反而难读。
 *
 * 每个字段都可选（specta 对 Rust 的 `i64` 就是这样生成的），所以入参类型
 * 照实放宽，缺字段的段直接丢掉，而不是让整个分布条消失。
 */
export function VrccScoreSpread({
    spread
}: {
    spread: Array<{ from?: number; to?: number; pct?: number }>;
}) {
    const { t } = useTranslation();
    const bands = spread
        .filter(
            (band): band is { from: number; to: number; pct: number } =>
                typeof band.from === 'number' &&
                typeof band.to === 'number' &&
                typeof band.pct === 'number' &&
                band.pct > 0
        )
        // 从高分到低分排列。服务端本来就是这个顺序，这里再排一次是防御：
        // 顺序变了会让「左边是高分」这个前提悄悄失效。
        .sort((a, b) => b.from - a.from);

    if (!bands.length) {
        return null;
    }

    const ordered = bands;

    return (
        <div className="flex flex-col gap-1.5">
            <div className="text-muted-foreground text-xs font-medium">
                {t('dialog.world.vrcc.spread')}
            </div>
            <div className="flex flex-col gap-1">
                {ordered.map((band) => (
                    <div
                        key={`${band.from}-${band.to}`}
                        className="flex items-center gap-2 text-[11px]"
                    >
                        <span className="text-muted-foreground w-9 shrink-0 text-right tabular-nums">
                            {band.from}–{band.to}
                        </span>
                        <span className="bg-muted h-1.5 min-w-0 flex-1 overflow-hidden rounded-full">
                            <span
                                className="bg-primary block h-full rounded-full"
                                // 宽度是数据驱动的，进不了 Tailwind 类
                                style={{ width: `${Math.min(100, band.pct)}%` }}
                            />
                        </span>
                        <span className="text-muted-foreground w-8 shrink-0 tabular-nums">
                            {band.pct}%
                        </span>
                    </div>
                ))}
            </div>
        </div>
    );
}

/**
 * 一行的「标签 + 值」，用于去过人数、房间上限这类简单事实。
 *
 * 值为空（`null` / 空串 / 0 人上限）时**整行不渲染**，而不是显示 `—`：
 * 这一块是补充信息，一堆破折号比少两行更难看，也不提供信息。
 */
export function VrccFactRow({
    label,
    value,
    className
}: {
    label: string;
    value: string | null;
    className?: string;
}) {
    if (!value) {
        return null;
    }
    return (
        <div
            className={cn(
                'flex min-w-0 items-baseline gap-2 text-xs',
                className
            )}
        >
            <span className="text-muted-foreground shrink-0">{label}</span>
            <span className="text-foreground min-w-0 truncate font-medium tabular-nums">
                {value}
            </span>
        </div>
    );
}
