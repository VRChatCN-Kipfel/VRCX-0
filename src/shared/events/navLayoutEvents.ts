export const NAV_LAYOUT_UPDATED_EVENT = 'vrcx:nav-layout-updated';
export const NAV_CUSTOMIZE_REQUESTED_EVENT = 'vrcx:nav-customize-requested';
export const NAV_SHORTCUT_REQUESTED_EVENT = 'vrcx:nav-shortcut-requested';
export const NAV_SHORTCUT_POSITION_LIMIT = 9;

/**
 * 请求跳到某个路由。
 *
 * 后台服务（通知激活、深链）手里没有 router 上下文，但它们需要让界面跳页。
 * 这个事件就是那条通道：服务只管发，`AppNavMenu` 收到后调 `navigate`。
 *
 * 直接用 `window.location` 会触发整页重载（等于重启前端），走事件不会。
 */
export const NAV_NAVIGATE_REQUESTED_EVENT = 'vrcx:nav-navigate-requested';

export function publishNavLayoutUpdated(): void {
    if (typeof window !== 'undefined') {
        window.dispatchEvent(new CustomEvent(NAV_LAYOUT_UPDATED_EVENT));
    }
}

export function publishNavCustomizeRequested(): void {
    if (typeof window !== 'undefined') {
        window.dispatchEvent(new CustomEvent(NAV_CUSTOMIZE_REQUESTED_EVENT));
    }
}

export function publishNavShortcutRequested(position: number): void {
    if (
        typeof window === 'undefined' ||
        !Number.isInteger(position) ||
        position < 1 ||
        position > NAV_SHORTCUT_POSITION_LIMIT
    ) {
        return;
    }
    window.dispatchEvent(
        new CustomEvent(NAV_SHORTCUT_REQUESTED_EVENT, { detail: position })
    );
}

export function publishNavNavigateRequested(to: string): void {
    if (typeof window === 'undefined' || !to.startsWith('/')) {
        return;
    }
    window.dispatchEvent(
        new CustomEvent(NAV_NAVIGATE_REQUESTED_EVENT, { detail: to })
    );
}
