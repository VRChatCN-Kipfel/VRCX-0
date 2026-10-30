import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';

/**
 * 半星打分控件：星星画五颗，每个可点两位，共 10 档。
 *
 * ## 为什么是半星而不是整星
 *
 * 官方的评分是 **1–10 的整数**。整星五档只能表达 2/4/6/8/10，等于自己丢掉
 * 一半精度。每颗星切两半——左半奇数、右半偶数——正好覆盖 1–10，而星星的
 * 视觉语言不用改。
 *
 * ## 为什么不做拖动/悬停即提交
 *
 * 官方《使用要求》写明打分必须「由玩家本人每次主动点击触发」。滑块式拖动会
 * 在拖动过程中产生一串提交，属于隐式批量操作。所以这里只有「点选」，
 * 提交按钮单独存在。
 *
 * ## 键盘可达
 *
 * 每颗星是一个 `<button role="slider">`？不是——那是错误语义。这里用十个
 * 独立的 radio 语义按钮确实更啰嗦，所以采用的折中：整组是一个
 * `role="radiogroup"`，每个可选档位是一个 `role="radio"` 按钮，左右方向键
 * 逐档移动。这样读屏能报出当前是「第 N 档，共 10 档」。
 */
export function StarRatingInput({
    value,
    onChange,
    disabled,
    max = 10
}: {
    /** 当前选中的分数（1–10）。`null` 表示还没选。 */
    value: number | null;
    onChange: (score: number) => void;
    disabled?: boolean;
    max?: number;
}) {
    const { t } = useTranslation();
    const [focused, setFocused] = useState<number | null>(null);

    // 悬停时预览「点下去会是多少」，松手回到当前值。
    const shown = focused ?? value ?? 0;

    return (
        <div
            role="radiogroup"
            aria-label={t('dialog.world.vrcc.rate_input_label')}
            aria-disabled={disabled || undefined}
            // 组本身不进 tab 序列（`-1`），键盘用户靠里面的 radio 逐个走。
            // 但 `radiogroup` 这个 role 要求元素技术上可聚焦，所以不能省。
            tabIndex={-1}
            className="inline-flex items-center gap-0.5"
            onMouseLeave={() => setFocused(null)}
        >
            {[0, 1, 2, 3, 4].map((index) => {
                const left = index * 2 + 1;
                const right = index * 2 + 2;
                const ratio = Math.max(0, Math.min(1, shown / 2 - index));
                return (
                    <span
                        key={index}
                        className="relative size-8"
                        onMouseMove={(event) => {
                            if (disabled) {
                                return;
                            }
                            // 用相对宽度判断落在左半还是右半，而不是读 offsetX：
                            // 星星带内边距，offsetX 的基准会随样式变化漂移。
                            const rect =
                                event.currentTarget.getBoundingClientRect();
                            const isLeft =
                                event.clientX - rect.left < rect.width / 2;
                            setFocused(isLeft ? left : right);
                        }}
                    >
                        {/* 底：空心星 */}
                        <StarGlyph className="text-muted-foreground/30 absolute inset-0 size-8" />
                        {/* 上：实心星，按比例裁切 */}
                        {ratio > 0 ? (
                            <span
                                className="absolute inset-0 overflow-hidden"
                                style={{ width: `${ratio * 100}%` }}
                                aria-hidden="true"
                            >
                                <StarGlyph className="text-primary size-8" />
                            </span>
                        ) : null}

                        {/* 两个点击热区，左右各半。视觉上是透明的，只负责命中。 */}
                        <button
                            type="button"
                            role="radio"
                            aria-checked={value === left}
                            aria-label={t('dialog.world.vrcc.rate_value', {
                                score: left,
                                max
                            })}
                            disabled={disabled}
                            onClick={() => onChange(left)}
                            onFocus={() => setFocused(left)}
                            onBlur={() => setFocused(null)}
                            onKeyDown={(event) => {
                                // 左右方向键逐档移动；跳过半颗星时保留奇偶，
                                // 让「按一下 = 动半颗」的手感成立。
                                if (
                                    event.key !== 'ArrowLeft' &&
                                    event.key !== 'ArrowRight'
                                ) {
                                    return;
                                }
                                event.preventDefault();
                                const step =
                                    event.key === 'ArrowRight' ? 1 : -1;
                                const next = Math.min(
                                    max,
                                    Math.max(1, (value ?? 0) + step)
                                );
                                onChange(next);
                            }}
                            className={cn(
                                'absolute top-0 left-0 h-full w-1/2 cursor-pointer rounded-sm',
                                'focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-none',
                                'disabled:cursor-not-allowed'
                            )}
                        />
                        <button
                            type="button"
                            role="radio"
                            aria-checked={value === right}
                            aria-label={t('dialog.world.vrcc.rate_value', {
                                score: right,
                                max
                            })}
                            disabled={disabled}
                            onClick={() => onChange(right)}
                            onFocus={() => setFocused(right)}
                            onBlur={() => setFocused(null)}
                            onKeyDown={(event) => {
                                if (
                                    event.key !== 'ArrowLeft' &&
                                    event.key !== 'ArrowRight'
                                ) {
                                    return;
                                }
                                event.preventDefault();
                                const step =
                                    event.key === 'ArrowRight' ? 1 : -1;
                                const next = Math.min(
                                    max,
                                    Math.max(1, (value ?? 0) + step)
                                );
                                onChange(next);
                            }}
                            className={cn(
                                'absolute top-0 right-0 h-full w-1/2 cursor-pointer rounded-sm',
                                'focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-none',
                                'disabled:cursor-not-allowed'
                            )}
                        />
                    </span>
                );
            })}
            <span className="text-muted-foreground ml-2 shrink-0 text-xs tabular-nums">
                {shown > 0
                    ? t('dialog.world.vrcc.rate_value', { score: shown, max })
                    : t('dialog.world.vrcc.rate_none')}
            </span>
        </div>
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
