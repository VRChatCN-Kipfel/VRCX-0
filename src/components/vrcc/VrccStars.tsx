import { cn } from '@/lib/utils';

/** 星星的尺寸档位。`xs` 给榜单行，`sm` 给短评署名，`md` 给主评分区。 */
const SIZE_CLASS = {
    xs: 'size-2.5',
    sm: 'size-3',
    md: 'size-5'
} as const;

export type VrccStarSize = keyof typeof SIZE_CLASS;

/**
 * 星级显示（只读）。
 *
 * ## 分数是 1–10，星是 5 颗
 *
 * 所以 `score / 2` 折成 0–5，**不足一颗的按比例部分填充**——这样 `8.4` 和
 * `8.2` 画出来都是四颗多，但肉眼能看出 8.4 比 8.2 多一点。
 *
 * ## 为什么颜色写 `text-primary`
 *
 * `--primary` 由 `--vrcx-theme-primary` 一路派生，玩家换强调色、装社区主题时
 * 星星会跟着变。**不写死任何色值**。
 *
 * ## 星星不能替代数字
 *
 * 五档星表达不了 1–10 的精度：`8.4` 和 `8.2` 的星几乎一样。所以界面里
 * **星和数字要同时出现**——星负责扫视，数字负责比较。
 */
export function VrccStars({
    score,
    size = 'sm',
    className
}: {
    /** 1–10 的分数。传 `null` 时渲染五颗空星。 */
    score: number | null | undefined;
    size?: VrccStarSize;
    className?: string;
}) {
    const sizeClass = SIZE_CLASS[size];
    const safe =
        typeof score === 'number' && Number.isFinite(score) ? score : 0;
    const filled = Math.max(0, Math.min(5, safe / 2));

    return (
        <span
            className={cn(
                'inline-flex shrink-0 items-center gap-px',
                className
            )}
            role="img"
            // 读屏用一位小数，和界面上的数字一致
            aria-label={safe.toFixed(1)}
        >
            {[0, 1, 2, 3, 4].map((index) => {
                const ratio = Math.max(0, Math.min(1, filled - index));
                return (
                    <span
                        key={index}
                        className={cn('relative inline-block', sizeClass)}
                    >
                        <StarGlyph
                            className={cn(
                                'text-muted-foreground/30 absolute inset-0',
                                sizeClass
                            )}
                        />
                        {ratio > 0 ? (
                            <span
                                className="absolute inset-0 overflow-hidden"
                                // 宽度是数据驱动的，进不了 Tailwind 类
                                style={{ width: `${ratio * 100}%` }}
                                aria-hidden="true"
                            >
                                <StarGlyph
                                    className={cn('text-primary', sizeClass)}
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
