/**
 * 星遇同频的星群（官方 `stars[].group`）。
 *
 * 官方给了 5 个代号：`G` 漫游、`W` 游戏、`Z` 创作、`L` 聊天、`A` 万象。
 * 其中前四个有专属颜色，**万象（`A`）没有**——官方脚本自己的颜色表也只列了
 * 这四个，万象用它自己的文字色。所以这里不给 `A` 造一个颜色，而是让调用方
 * 在查不到时退回前景色，行为和官方一致。
 *
 * 色值取自官方脚本的颜色表，两边看到的是同一组颜色。
 */
export const VRCC_STAR_GROUP_COLORS: Readonly<Record<string, string>> =
    Object.freeze({
        G: '#4db4ff',
        W: '#ffb547',
        Z: '#d69bff',
        L: '#5fe0b4'
    });

/**
 * 查一个星群代号的颜色。
 *
 * 查不到时返回 `null`，由调用方决定退回什么（本仓库用 `currentColor`）。
 * 返回 `null` 而不是一个兜底色，是为了让「有星群但没专属色」和「根本不知道
 * 星群」在调用点上能被区分开。
 */
export function vrccStarGroupColor(
    group: string | null | undefined
): string | null {
    const key = (group ?? '').trim().toUpperCase();
    if (!key) {
        return null;
    }
    return VRCC_STAR_GROUP_COLORS[key] ?? null;
}
