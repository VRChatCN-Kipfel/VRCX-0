import { useState } from 'react';

/**
 * 地图封面缩略图。
 *
 * **为什么不直接用 `<img>`**：封面地址来自星遇同频网页端那套内部接口
 * （`/api/match?xdimg=…`），官方从未承诺它稳定，随时可能 404。图裂了在列表里
 * 会留下一个破图标、甚至把行撑高，比没有图更难看。
 *
 * 所以这里的处理是：**加载失败就彻底不渲染**（`onError` 把 state 置空），
 * 行退回原来的纯文字排版。这也是为什么它必须是个组件——列表里的每一行都要
 * 各自记住自己裂了没有。
 */
export function VrccCover({
    src,
    className
}: {
    /** 已经过 [`vrccCoverUrl`] 归一化的绝对地址；`null` 表示没有封面。 */
    src: string | null;
    className?: string;
}) {
    const [failed, setFailed] = useState(false);

    if (!src || failed) {
        return null;
    }

    return (
        <img
            src={src}
            alt=""
            // 装饰性图片：它在列表里的作用是让玩家一眼认出地图，
            // 相邻的文字已经说了是哪张图，所以不进无障碍树。
            aria-hidden
            loading="lazy"
            decoding="async"
            className={className}
            onError={() => setFailed(true)}
        />
    );
}
