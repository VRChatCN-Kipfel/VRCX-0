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

/**
 * 分节标题 + 分割线。
 *
 * 官方那一页是用「小标题 + 一条线」把页面切成若干段（介绍 / 打分 / 短评 /
 * 配置），这是它读起来不像一坨的关键。我们照这个**结构**走，配色用本项目的
 * 令牌。
 *
 * `aside` 放右侧的次要内容（如「10 条」、排序切换）。
 */
export function VrccSection({
    title,
    aside,
    children,
    className
}: {
    title: string;
    aside?: React.ReactNode;
    children: React.ReactNode;
    className?: string;
}) {
    return (
        <section className={cn('flex min-w-0 flex-col gap-2', className)}>
            {/* 标题栏不参与伸缩：它是这一节的固定头部。 */}
            <div className="flex min-w-0 shrink-0 items-center gap-2 border-b pb-1.5">
                <h4 className="text-muted-foreground shrink-0 text-xs font-medium">
                    {title}
                </h4>
                {aside ? <div className="ml-auto shrink-0">{aside}</div> : null}
            </div>
            {/* 内容区自己成为一个 flex 列：这样调用方给 `className` 加
                `min-h-0 flex-1` 时，里面的滚动容器才能真正拿到高度。
                没有这一层，`flex:1` 只会作用到 section，子元素仍然是
                auto 高，`overflow-y-auto` 永远不会触发。 */}
            <div className="flex min-h-0 min-w-0 flex-col">{children}</div>
        </section>
    );
}

/**
 * 一项指标。
 *
 * `onToggle` 存在时这一项是**可展开的**：渲染成 `button`，值后面挂一个
 * 箭头，`expanded` 控制箭头方向。其余项保持纯文本——只有嵌了一层细节的那
 * 一项才该看起来能点，否则玩家会去点「去过 69」而什么也不发生。
 */
export type VrccMetric = {
    label: string;
    value: string;
    /** 有值表示这一项可以点开/收起。 */
    onToggle?: () => void;
    /** 当前是否已展开（只在该项可点时有意义）。 */
    expanded?: boolean;
    /** 给读屏用：展开控件的完整描述。 */
    toggleLabel?: string;
};

/**
 * 一排指标：值大、标签小，横向平铺。
 *
 * 用于顶部那一行（配置档位 / 去过人数 / 平台 / 房间上限）。官方是四项等宽，
 * 我们跟随同一结构；缺项的会被调用方过滤掉，不留空位。
 */
export function VrccMetricRow({ items }: { items: VrccMetric[] }) {
    if (!items.length) {
        return null;
    }
    return (
        <div className="flex min-w-0 flex-wrap items-stretch gap-x-5 gap-y-2">
            {items.map((item) => {
                const value = (
                    <span className="text-foreground flex items-center gap-0.5 text-sm font-semibold tabular-nums">
                        {item.value}
                        {item.onToggle ? (
                            <Chevron expanded={item.expanded} />
                        ) : null}
                    </span>
                );
                const label = (
                    <span className="text-muted-foreground text-[10px] leading-none">
                        {item.label}
                    </span>
                );

                if (!item.onToggle) {
                    return (
                        <div
                            key={item.label}
                            className="flex min-w-0 flex-col items-center gap-0.5"
                        >
                            {value}
                            {label}
                        </div>
                    );
                }

                return (
                    <button
                        key={item.label}
                        type="button"
                        onClick={item.onToggle}
                        aria-expanded={item.expanded ?? false}
                        aria-label={item.toggleLabel}
                        className={cn(
                            'hover:bg-muted/50 -mx-1 flex min-w-0 flex-col items-center gap-0.5 rounded px-1 py-0.5 transition-colors'
                        )}
                    >
                        {value}
                        {label}
                    </button>
                );
            })}
        </div>
    );
}

/** 展开指示箭头。展开时转 180°。 */
function Chevron({ expanded }: { expanded?: boolean }) {
    return (
        <svg
            viewBox="0 0 24 24"
            aria-hidden="true"
            className={cn(
                'text-muted-foreground size-3 shrink-0 transition-transform duration-200',
                expanded && 'rotate-180'
            )}
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
        >
            <path d="M6 9l6 6 6-6" />
        </svg>
    );
}

/**
 * 可折叠区：`grid-template-rows` 从 `0fr` 到 `1fr` 做高度动画。
 *
 * ## 为什么不是 `max-height`
 *
 * `max-height` 要猜一个足够大的值，内容少时前半段看不见东西、内容多时又会被
 * 截断。`0fr → 1fr` 是**按内容真实高度**算的，内容变了不用重新测量。
 *
 * ## 为什么外面那层要 `overflow: hidden`
 *
 * `grid` 的行高从 0 长到 auto 的过程中，子元素已经在渲染，没有这层就会溢出
 * 到下面的分节上，看着像内容「穿」出去。
 *
 * ## 与滚动容器的关系
 *
 * 这个区在详情对话框的左栏里，那一栏是 `overflow-y-auto`。展开会让内容变高，
 * 可能触发滚动条出现，进而挤掉几像素宽度、横向抖一下。所以调用方在展开后
 * 会把这一块滚进视野——`scrollIntoView({ block: 'nearest' })` 在内容已经
 * 放得下时不滚动，放不下时才动最小距离，避免「先展开再突然跳下去」。
 */
export function VrccCollapse({
    open,
    children
}: {
    open: boolean;
    children: React.ReactNode;
}) {
    return (
        <div
            className={cn(
                'grid transition-[grid-template-rows] duration-200 ease-out',
                open ? 'grid-rows-[1fr]' : 'grid-rows-[0fr]'
            )}
        >
            {/* 收起时用 aria-hidden + opacity 一起藏：只靠高度为 0 的话，
                里面的按钮仍在 tab 序列里，键盘会跳到看不见的东西上。 */}
            <div
                className={cn(
                    'min-h-0 overflow-hidden transition-opacity duration-200',
                    open ? 'opacity-100' : 'opacity-0'
                )}
                aria-hidden={!open}
                // 收起的区不可交互。`inert` 是标准做法；不支持时下面还有
                // `pointer-events` 兜底。
                {...(!open ? { inert: true } : {})}
            >
                {children}
            </div>
        </div>
    );
}

/**
 * 配置四宫格：显存 / 内存 / 加载 / 下载。
 *
 * 这里是**格子**而不是指标行：四个数都是容量或耗时，放在等宽方框里能一眼
 * 横向比较，铺成一行反而读不出「这是一组配置」。官方也是这么排的。
 */
export function VrccPerfGrid({
    items
}: {
    items: Array<{ label: string; value: string }>;
}) {
    if (!items.length) {
        return null;
    }
    return (
        <div className="grid grid-cols-4 gap-1.5">
            {items.map((item) => (
                <div
                    key={item.label}
                    className="bg-muted/40 flex min-w-0 flex-col items-center gap-1 rounded-md border px-1 py-2"
                >
                    <span className="text-foreground text-xs font-semibold tabular-nums">
                        {item.value}
                    </span>
                    <span className="text-muted-foreground text-[10px] leading-none">
                        {item.label}
                    </span>
                </div>
            ))}
        </div>
    );
}
