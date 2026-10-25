import { useCallback, useEffect, useRef, useState } from 'react';

import { openWorldDialog } from '@/services/dialogService';
import { toast } from '@/services/toastService';

import {
    vrccApi,
    VrccError,
    type VrccBoard,
    type VrccConnection,
    type VrccDeviceAuthPrompt,
    type VrccMyMap,
    type VrccRecentNotifications,
    type VrccStatus,
    type VrccUsage
} from './vrccApi';

/** 提醒轮询的基准间隔。官方建议 60 秒。 */
const POLL_BASE_MS = 60_000;
/** 抖动上限：避免所有客户端在同一秒撞车（官方脚本也这么做）。 */
const POLL_JITTER_MS = 20_000;

function nextPollDelay(): number {
    return POLL_BASE_MS + Math.random() * POLL_JITTER_MS;
}

export type VrccTab = 'maps' | 'notifications' | 'me';

type State = {
    tab: VrccTab;
    connection: VrccConnection | null;
    usage: VrccUsage | null;
    boards: VrccBoard | null;
    recent: VrccRecentNotifications | null;
    /** 当前所在地图在榜上的状态；null 表示不显示（未连接、无地图、或被隐藏）。 */
    myMap: VrccMyMap | null;
    service: VrccStatus | null;
    loading: boolean;
    error: VrccError | null;
    /** 设备授权进行中。 */
    deviceAuth: VrccDeviceAuthPrompt | null;
    deviceAuthBusy: boolean;
    deviceAuthMessage: string | null;
};

const initialState: State = {
    tab: 'maps',
    connection: null,
    usage: null,
    boards: null,
    recent: null,
    myMap: null,
    service: null,
    loading: true,
    error: null,
    deviceAuth: null,
    deviceAuthBusy: false,
    deviceAuthMessage: null
};

/**
 * 星遇同频页面的控制器。
 *
 * 轮询在未连接时也跑，因为地图榜是开放数据、不需要令牌。个人接口只在
 * 已连接时轮询，且间隔带抖动。
 */
export function useVrccController() {
    const [state, setState] = useState<State>(initialState);
    const pollTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const authTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const mounted = useRef(true);

    const patch = useCallback((next: Partial<State>) => {
        if (mounted.current) {
            setState((current) => ({ ...current, ...next }));
        }
    }, []);

    /** 刷新连接状态与本地用量。 */
    const refreshConnection = useCallback(async () => {
        const [connection, usage] = await Promise.all([
            vrccApi.connection(),
            vrccApi.usage()
        ]);
        patch({ connection, usage });
        return connection;
    }, [patch]);

    /** 地图榜：官方说明服务器最多 5 分钟更新一次，问再勤也是同一份。 */
    const refreshBoards = useCallback(async () => {
        const boards = await vrccApi.boards(20);
        patch({ boards });
    }, [patch]);

    /**
     * 最近的提醒。
     *
     * `mark` 只在玩家真正打开「提醒」页时为 true —— 那会把已读同步到账号，
     * 网页和手机 App 上的红点跟着消。后台轮询绝不传 true。
     */
    const refreshRecent = useCallback(
        async (mark = false) => {
            const recent = await vrccApi.recentNotifications(mark);
            patch({ recent });
        },
        [patch]
    );

    /**
     * 当前所在地图在榜上的状态。
     *
     * 位置由后端自己取，前端不参与——地图 ID 只有后端知道。
     * 官方要求：地图被下架或不对当前玩家开放时客户端不显示打分面板；
     * 后端已经把那种情况转成 null 返回。
     */
    const refreshMyMap = useCallback(async () => {
        const myMap = await vrccApi.myMap();
        patch({ myMap });
    }, [patch]);

    const refreshService = useCallback(async () => {
        try {
            const service = await vrccApi.status();
            patch({ service });
        } catch {
            // 探活失败本身不是页面错误，静默处理。
            patch({ service: { at: null, state: 'down' } });
        }
    }, [patch]);

    /** 初次加载。 */
    const load = useCallback(async () => {
        patch({ loading: true, error: null });
        try {
            const connection = await refreshConnection();
            await refreshService();
            await refreshBoards();
            if (connection.connected) {
                await refreshRecent();
            }
            // 当前所在地图从 VRCX 自己的游戏日志拿，不额外请求 VRChat。
            await refreshMyMap();
            patch({ loading: false });
        } catch (error) {
            patch({ loading: false, error: error as VrccError });
        }
    }, [
        patch,
        refreshBoards,
        refreshConnection,
        refreshMyMap,
        refreshRecent,
        refreshService
    ]);

    useEffect(() => {
        mounted.current = true;
        void load();
        return () => {
            mounted.current = false;
            if (pollTimer.current) clearTimeout(pollTimer.current);
            if (authTimer.current) clearTimeout(authTimer.current);
        };
    }, [load]);

    /**
     * 提醒轮询。只在已连接时跑，间隔带抖动。
     *
     * 代际校验：每次轮询前确认组件还挂着，且仍是这个连接——断开或重新
     * 授权后旧的回答不该覆盖新的状态。
     */
    useEffect(() => {
        if (!state.connection?.connected) {
            return;
        }
        let cancelled = false;

        const tick = async () => {
            try {
                const notifications = await vrccApi.pollNotifications();
                if (cancelled) return;
                if (notifications.items?.length) {
                    // 新提醒到达时弹一条 toast：官方插件会把提醒弹在 VRCX 里，
                    // 这里用 VRCX-0 自己的 toast 做同样的事。
                    for (const item of notifications.items) {
                        toast.add({
                            type: 'info',
                            title: item.title ?? '星遇同频',
                            description: item.body ?? undefined
                        });
                    }
                    // 后台刷新**不**标记已读：玩家还没看到，替他标记是错的。
                    await refreshRecent(false);
                }
            } catch (error) {
                if (
                    error instanceof VrccError &&
                    error.kind === 'needsReauth'
                ) {
                    // 令牌失效：停下轮询，让玩家看到「重新连接」。
                    if (!cancelled) patch({ error });
                    return;
                }
            }
            if (!cancelled) {
                pollTimer.current = setTimeout(tick, nextPollDelay());
            }
        };

        pollTimer.current = setTimeout(tick, nextPollDelay());

        return () => {
            cancelled = true;
            if (pollTimer.current) clearTimeout(pollTimer.current);
        };
    }, [state.connection?.connected, patch, refreshRecent]);

    /**
     * 切页签。
     *
     * 切到「提醒」时用 `mark=true` 重拉一次：这是玩家真正在看这个列表的时刻，
     * 服务器据此把已读同步到账号，网页和手机 App 上的红点会跟着消。
     * 官方的做法也是「打开最近的提醒」时才 mark。
     */
    const setTab = useCallback(
        (tab: VrccTab) => {
            patch({ tab });
            if (tab === 'notifications' && state.connection?.connected) {
                refreshRecent(true).catch(() => {
                    // 标记已读失败不该打断看列表；列表内容已经在了。
                });
            }
        },
        [patch, refreshRecent, state.connection?.connected]
    );

    /** 开始设备授权。 */
    const beginDeviceAuth = useCallback(async () => {
        patch({ deviceAuthBusy: true, deviceAuthMessage: null, error: null });
        try {
            const prompt = await vrccApi.beginDeviceAuth(null);
            patch({ deviceAuth: prompt, deviceAuthBusy: false });
            // 优先把带码的链接丢给系统浏览器，玩家不用手输。
            const target =
                prompt.verificationUriComplete ?? prompt.verificationUri;
            if (target) {
                window.open(target, '_blank', 'noopener,noreferrer');
            }
        } catch (error) {
            patch({
                deviceAuthBusy: false,
                error: error as VrccError
            });
        }
    }, [patch]);

    /**
     * 轮询设备授权。按后端给的 interval 反复问，直到成功、终态或超时。
     */
    useEffect(() => {
        const prompt = state.deviceAuth;
        if (!prompt) return;
        let cancelled = false;

        const tick = async () => {
            try {
                const step = await vrccApi.pollDeviceAuth(prompt);
                if (cancelled) return;
                if (step.state === 'connected') {
                    patch({
                        deviceAuth: null,
                        deviceAuthMessage: null
                    });
                    await load();
                    return;
                }
                if (step.state === 'terminal') {
                    patch({
                        deviceAuth: null,
                        deviceAuthMessage: step.message
                    });
                    return;
                }
                if (step.state === 'pending') {
                    authTimer.current = setTimeout(
                        tick,
                        Math.max(5, step.interval_secs) * 1000
                    );
                }
            } catch (error) {
                if (!cancelled) {
                    patch({
                        deviceAuth: null,
                        error: error as VrccError
                    });
                }
            }
        };

        authTimer.current = setTimeout(
            tick,
            Math.max(5, prompt.intervalSecs) * 1000
        );

        return () => {
            cancelled = true;
            if (authTimer.current) clearTimeout(authTimer.current);
        };
    }, [state.deviceAuth, load, patch]);

    const disconnect = useCallback(async () => {
        patch({ error: null });
        try {
            await vrccApi.disconnect();
            patch({ deviceAuth: null, recent: null });
            await refreshConnection();
        } catch (error) {
            patch({ error: error as VrccError });
        }
    }, [patch, refreshConnection]);

    /**
     * 打开 VRCX-0 自己的世界详情窗。
     *
     * 对齐官方插件「加好友/看地图是打开 VRCX 自己的窗口，由你自己点」的做法：
     * 我们只负责把窗口叫出来，真正的操作交给 VRCX 原生界面。
     */
    const openWorld = useCallback((worldId: string) => {
        if (!worldId) return;
        openWorldDialog({ worldId });
    }, []);

    return {
        ...state,
        setTab,
        reload: load,
        refreshBoards,
        refreshMyMap,
        beginDeviceAuth,
        disconnect,
        openWorld
    };
}
