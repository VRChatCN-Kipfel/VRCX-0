import { useCallback, useEffect, useRef, useState } from 'react';

import { copyTextToClipboard } from '@/services/clipboardService';
import { openUserDialog, openWorldDialog } from '@/services/dialogService';
import i18n from '@/services/i18nService';
import { openExternalLink } from '@/services/shellIntegrationService';
import { toast } from '@/services/toastService';

import {
    vrccApi,
    VrccError,
    type VrccBoard,
    type VrccConnection,
    type VrccDeviceAuthPrompt,
    type VrccMutuals,
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

export type VrccTab = 'maps' | 'notifications' | 'mutuals' | 'me';

type State = {
    tab: VrccTab;
    connection: VrccConnection | null;
    usage: VrccUsage | null;
    boards: VrccBoard | null;
    recent: VrccRecentNotifications | null;
    /** 当前所在地图在榜上的状态；null 表示不显示（未连接、无地图、或被隐藏）。 */
    myMap: VrccMyMap | null;
    /** 「互相连上」的星友；null 表示还没拉过或没权限。 */
    mutuals: VrccMutuals | null;
    /**
     * 「互相连上」这一项失败的原因。
     *
     * 单独放一个字段而不是共用全局 rror：它以 dev- 标识必然失败（敏感
     * 权限要审核），那是**某个页签的功能没开通**，不是整个页面坏了。塞进
     * 全局错误会让玩家以为连接出了问题。
     */
    mutualsError: string | null;
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
    mutuals: null,
    mutualsError: null,
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

    /**
     * 统一上报一个错误。
     *
     * 三条路一起走，缺一不可：
     * - `console.error` 留原始对象，便于在 DevTools 里看堆栈和错误码
     * - 应用内 toast 给即时反馈（是窗口内的 toast，不是系统级 Windows 通知）
     * - 页内 `error` 横幅留住它，玩家切页签回来还能看到
     *
     * 只做其中一两样会出现「操作失败了但没有任何提示」，这是最难受的体验。
     *
     * `vrccApi` 保证抛出的都是 `VrccError`（内部已把后端错误归一化并分类），
     * 所以这里直接用；万一有非 `VrccError` 漏出来，按 `other` 兜住。
     */
    const reportError = useCallback(
        (error: unknown, where: string) => {
            console.error(`[vrcc] ${where} failed:`, error);
            const normalized =
                error instanceof VrccError
                    ? error
                    : new VrccError(
                          error instanceof Error
                              ? error.message
                              : String(error),
                          'other'
                      );
            toast.add({
                type: 'error',
                title: normalized.message || i18n.t('vrcc.error.unknown')
            });
            patch({ error: normalized });
        },
        // 这里刻意只依赖 `patch`（空依赖的 useCallback，永远稳定）。
        //
        // 翻译不通过 `useTranslation()` 拿，而是直接用全局 `i18n` 实例——
        // 和 `toastService` 的做法一致。原因：`useTranslation()` 返回的 `t`
        // 每次渲染可能是新引用，放进依赖会让 `reportError` 换引用 →
        // 依赖它的 `load` 换引用 → 调 `load` 的 effect 重跑 → `load` 里
        // `patch` → 再渲染 → 无限循环。实测在 jsdom 里几秒就把堆打爆。
        //
        // 而且 `t` 的引用变化并不代表文案变了：`i18n.t` 读全局实例，
        // 切语言时自己会返回新文案。
        [patch]
    );

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

    /**
     * 「互相连上」的星友列表。
     *
     * 需要 `mutuals` 敏感权限。`dev-` 标识拿到的是 `403 scope`——这是
     * **预期内**的结果（敏感权限要审核），不是故障。
     *
     * 所以失败时只把原因记到 `mutualsError` 供「连上」页签自己显示，
     * 不走全局 `reportError`：那会把一条「某个可选功能没开通」渲染成
     * 整页红色横幅，让玩家以为连接坏了。
     */
    const refreshMutuals = useCallback(async () => {
        try {
            const mutuals = await vrccApi.mutuals();
            patch({ mutuals, mutualsError: null });
        } catch (error) {
            if (error instanceof VrccError && error.kind === 'missingScope') {
                // 权限没开通：静默记录，界面在自己的页签里说明。
                patch({ mutuals: null, mutualsError: error.message });
                return;
            }
            patch({ mutuals: null });
            throw error;
        }
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

    /**
     * 初次加载。
     *
     * 分两段：本地状态（连接、用量、所在地图）先出来，页面立刻可用；
     * 网络内容（地图榜、提醒、连上）各自异步加载。
     *
     * 原来是一路 `await` 串行，其中 `refreshBoards` 在没网时要等 12 秒超时，
     * 期间 `loading` 一直是 true，整页卡在转圈上——一个可选区块拖住了整页。
     * 网络请求失败该表现为那一块空着 + 一条错误提示，而不是「页面没加载完」。
     */
    const load = useCallback(async () => {
        patch({ loading: true, error: null });
        let connected = false;
        try {
            // 只读本地状态：这两个不会因为网络卡住。
            const connection = await refreshConnection();
            connected = connection.connected;
        } catch (error) {
            reportError(error, 'load');
        } finally {
            // 骨架先落地：即使后面的请求全都失败，页面也是可用的。
            patch({ loading: false });
        }

        // 以下全都可能发网络请求，一律不阻塞首屏。
        void refreshService().catch((error) => {
            reportError(error, 'refreshService');
        });
        void refreshBoards().catch((error) => {
            reportError(error, 'refreshBoards');
        });
        // 所在地图要已连接才有意义，且它内部会查接口——同样不能挡首屏。
        void refreshMyMap().catch((error) => {
            reportError(error, 'refreshMyMap');
        });
        if (connected) {
            void refreshRecent(false).catch((error) => {
                reportError(error, 'refreshRecent');
            });
            // 「互相连上」要敏感权限，`dev-` 一定拿不到；单独兜住，不影响别的。
            void refreshMutuals().catch((error) => {
                reportError(error, 'refreshMutuals');
            });
        }
    }, [
        patch,
        refreshBoards,
        refreshConnection,
        refreshMutuals,
        refreshMyMap,
        refreshRecent,
        refreshService,
        reportError
    ]);

    /**
     * 初次加载：只在挂载时跑一次。
     *
     * **不能把 `load` 放进依赖数组。** `load` 会 `patch({ loading: true })`，
     * 这触发重新渲染 → 生成新的 `load` → effect 重跑 → 再 patch……
     * 依赖里放一个"自己会改状态"的回调，就是无限循环。
     *
     * 用 ref 记住已经跑过，依赖数组留空。需要手动重载时走 `reload()`。
     *
     * **必须在这个 effect 里把 `mounted.current` 置回 true。** 严格模式下
     * effect 会 挂载 → 清理 → 再挂载：清理时把它设成 false，而第二次挂载被
     * `didLoad` 挡在前面直接 return，这个 ref 就永远停在 false。`patch` 的前置
     * 判断是 `if (mounted.current)`，于是**请求照发、数据照回，但每次 setState
     * 都被丢掉**——界面永远停在「加载中」，而后台日志里能看到完整的成功响应。
     */
    const didLoad = useRef(false);
    useEffect(() => {
        // 先恢复挂载标记：清理函数会把它清掉，重挂载时（严格模式或
        // 组件真的被卸载再挂回）需要重新打开，否则状态更新全部静默。
        mounted.current = true;
        if (didLoad.current) return;
        didLoad.current = true;
        void load();
        return () => {
            mounted.current = false;
            if (pollTimer.current) clearTimeout(pollTimer.current);
            if (authTimer.current) clearTimeout(authTimer.current);
        };
        // `load` 故意不放进依赖：它自己会 `patch`，进依赖就是无限循环。
        // 上面的 `didLoad` ref 已经保证只跑一次，所以这个空依赖是正确的。
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

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
                    if (!cancelled) reportError(error, 'poll');
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
    }, [state.connection?.connected, patch, refreshRecent, reportError]);

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
            if (!state.connection?.connected) return;
            if (tab === 'notifications') {
                refreshRecent(true).catch(() => {
                    // 标记已读失败不该打断看列表；列表内容已经在了。
                });
            }
            if (tab === 'mutuals') {
                // 打开时拉一次：列表可能在上次加载后变了。
                refreshMutuals().catch((error) => {
                    reportError(error, 'refreshMutuals');
                });
            }
        },
        [
            patch,
            refreshMutuals,
            refreshRecent,
            reportError,
            state.connection?.connected
        ]
    );

    /** 开始设备授权。 */
    const beginDeviceAuth = useCallback(async () => {
        patch({ deviceAuthBusy: true, deviceAuthMessage: null, error: null });
        try {
            const prompt = await vrccApi.beginDeviceAuth(null);
            patch({ deviceAuth: prompt, deviceAuthBusy: false });

            // 先把授权码放进剪贴板：玩家的下一步要么在网页上粘贴，
            // 要么在手机上输——两种都要这个码，先给到手最省事。
            // 很短的码（`ABCD-2345`）手工抄写极易错位。
            void copyTextToClipboard(prompt.userCode, {
                successMessage: i18n.t('vrcc.me.code_copied', {
                    code: prompt.userCode
                })
            });

            // 再用系统浏览器打开同意页。`window.open` 在 Tauri 里会被拦掉，
            // 必须走 `app__open_link`。
            const target =
                prompt.verificationUriComplete ?? prompt.verificationUri;
            if (target) {
                void openExternalLink(target).catch((error: unknown) => {
                    // 打不开浏览器不该让整个授权流程失败：码已经复制好了，
                    // 玩家还能自己打开网页。给一条提示即可。
                    console.error('[vrcc] openExternalLink failed:', error);
                    toast.add({
                        type: 'error',
                        title: i18n.t('vrcc.me.open_browser_failed')
                    });
                });
            }
        } catch (error) {
            patch({ deviceAuthBusy: false });
            reportError(error, 'beginDeviceAuth');
        }
    }, [patch, reportError]);

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
                    patch({ deviceAuth: null });
                    reportError(error, 'pollDeviceAuth');
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
    }, [state.deviceAuth, load, patch, reportError]);

    const disconnect = useCallback(async () => {
        patch({ error: null });
        try {
            await vrccApi.disconnect();
            patch({ deviceAuth: null, recent: null });
            await refreshConnection();
        } catch (error) {
            reportError(error, 'disconnect');
        }
    }, [patch, refreshConnection, reportError]);

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

    /**
     * 打开 VRCX-0 自己的玩家详情窗。
     *
     * 官方合规要求的落点就在这里：`me/mutuals` 里对方的 `vrcUser` 只用来打开
     * 我们自己的玩家页，**加好友由玩家在那个界面里自己点**。
     * 官方原文两处都写明「严禁由程序自动代替玩家向 VRChat 发送好友请求」，
     * 所以这里只到"打开详情页"为止，没有、也不会有自动发送好友请求的分支。
     */
    const openUser = useCallback((userId: string) => {
        if (!userId.startsWith('usr_')) return;
        openUserDialog({ userId });
    }, []);

    return {
        ...state,
        setTab,
        reload: load,
        refreshBoards,
        refreshMyMap,
        refreshMutuals,
        beginDeviceAuth,
        disconnect,
        openWorld,
        openUser
    };
}
