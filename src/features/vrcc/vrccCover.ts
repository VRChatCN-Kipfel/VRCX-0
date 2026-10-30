/**
 * 星遇同频站点与封面地址。
 *
 * 官方 `boards/maps` 的 `img` **现在就是完整绝对网址**，形如
 * `https://vrcc.cc/maps/<worldId>.jpg`（实测确认，不是早前记录的
 * `/api/match?xdimg=…` 那种内部接口）。它带 `Cache-Control: max-age=14400`，
 * 浏览器与 Cloudflare 会缓存，我们不需要自己存。
 *
 * `imgUrl`（1.11.0 起）是同一张封面的另一个字段，但 `boards/maps` 至今
 * **不返回**它，只有 `me/board`、地图页、`me/maps/{world}` 才有。
 *
 * 这一层只负责把两者统一成「一个可以直接放进 `<img src>` 的字符串」，
 * 不判断图片存不存在——那是 `onError` 的事。
 */

export const VRCC_SITE_ORIGIN = 'https://vrcc.cc';

/**
 * 把地图封面字段归一成可直接加载的绝对地址。
 *
 * 优先级与后端 `VrccMyMap::image_url()` 保持一致：
 * 1. `imgUrl`，但只接受 `https://` 开头的（官方也可能给你一个相对路径）
 * 2. `img` 已经是绝对地址就原样用
 * 3. 否则当作站点相对路径拼到 [`VRCC_SITE_ORIGIN`] 后面
 *
 * 都给不出时返回 `null`，调用方据此不渲染图片，而不是渲染一个空 `src`
 * （空 `src` 会让浏览器去请求当前页面本身）。
 */
export function vrccCoverUrl(
    imgUrl: string | null | undefined,
    img: string | null | undefined
): string | null {
    const absolute = (imgUrl ?? '').trim();
    if (absolute.startsWith('https://')) {
        return absolute;
    }

    // `imgUrl` 也可能是个相对路径（官方没有保证它一定是完整网址），
    // 这时把它当作候选路径之一，别直接丢掉。
    const path = (img ?? '').trim() || absolute;
    if (!path) {
        return null;
    }
    if (path.startsWith('https://') || path.startsWith('http://')) {
        return path;
    }
    return `${VRCC_SITE_ORIGIN}${path.startsWith('/') ? path : `/${path}`}`;
}
