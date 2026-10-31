import { create } from 'zustand';

/**
 * 星遇同频的未读合计，供侧边栏那个条目使用。
 *
 * 数据原本只在 `useVrccController` 的 state 里，而 controller 只在星遇同频
 * 页面挂载时才跑。侧边栏是常驻的，所以这里存一份算好的数字：页面关闭后
 * 侧边栏仍显示上次拉到的未读数，直到下一次重新拉取。
 *
 * 只存数字不存原始响应：`me/unread` 的字段会随服务端版本变化，侧边栏不该
 * 关心这些细节，也就不该跟着它们改。
 */
type VrccUnreadStore = {
    /** 几处红点的合计。0 表示没有未读。 */
    unreadTotal: number;
    /** 是否连上了星遇同频。没连上时侧边栏不显示角标。 */
    connected: boolean;
    setUnreadTotal(unreadTotal: number): void;
    setConnected(connected: boolean): void;
    /** 断开连接或退出登录时把角标清掉，避免留一个过期的数字。 */
    reset(): void;
};

export const useVrccUnreadStore = create<VrccUnreadStore>((set) => ({
    unreadTotal: 0,
    connected: false,
    setUnreadTotal: (unreadTotal) => set({ unreadTotal }),
    setConnected: (connected) =>
        set((state) => (state.connected === connected ? state : { connected })),
    reset: () => set({ unreadTotal: 0, connected: false })
}));
