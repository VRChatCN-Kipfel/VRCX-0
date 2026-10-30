import { useCallback, useEffect, useRef, useState } from 'react';

import { tauriClient } from '@/platform/tauri/client';
import { copyTextToClipboard } from '@/services/clipboardService';
import { openUserDialog, openWorldDialog } from '@/services/dialogService';
import i18n from '@/services/i18nService';
import { openExternalLink } from '@/services/shellIntegrationService';
import { toast } from '@/services/toastService';
import { useRuntimeStore } from '@/state/runtimeStore';

import {
    vrccApi,
    VrccError,
    type VrccAnnouncements,
    type VrccBoard,
    type VrccBoardSort,
    type VrccConnection,
    type VrccCrossingCards,
    type VrccCrossings,
    type VrccDeviceAuthPrompt,
    type VrccMutuals,
    type VrccMyMap,
    type VrccNotes,
    type VrccRecentNotifications,
    type VrccStatus,
    type VrccUnread,
    type VrccUsage
} from './vrccApi';

/**
 * 后端提醒轮询发出的事件名，与 Rust 侧
 * `runtime-host-desktop::vrcc::VRCC_NOTICES_CHANGED_EVENT` 必须一致。
 */
const VRCC_NOTICES_CHANGED_EVENT = 'vrccNoticesChanged';

/**
 * 令牌被服务器判了失效（`404 gone`）、后端已经清掉它时发出的事件名，
 * 与 Rust 侧 `VRCC_CONNECTION_CHANGED_EVENT` 必须一致。
 */
const VRCC_CONNECTION_CHANGED_EVENT = 'vrccConnectionChanged';

/**
 * 同一种排序的地图榜多久内不重新请求。
 *
 * 服务器最多 5 分钟更新一次；后端对同一种排序还有 60 秒的最小间隔（不到
 * 60 秒再问会等着）。切回刚看过的排序直接用手上的，不让玩家干等。
 */
const BOARD_REUSE_MS = 60_000;

/**
 * 两次「申请连接码」之间至少隔多久（秒）。
 *
 * 与后端 `device_code` 的 `min_gap` 一致。后端那一层是**发请求前等 5 秒**，
 * 而这里挡的是**界面上的连点**：那份额度按网络出口 IP 每天只有 30 个，
 * 同一 NAT 后所有人共用，点一次就真的消耗服务端一次计数。让按钮在冷却期间
 * 不可点，玩家就不会用连点去赌「也许这次能过」。
 */
const DEVICE_AUTH_COOLDOWN_SECS = 5;

export type VrccTab = 'maps' | 'notifications' | 'mutuals' | 'crossings' | 'me';

/** 「提醒」页签里看哪一个：最近的提醒，还是通知中心。 */
export type VrccNotificationsView = 'recent' | 'notes';

type State = {
    tab: VrccTab;
    connection: VrccConnection | null;
    usage: VrccUsage | null;
    boards: VrccBoard | null;
    /** 正在看的地图榜排序。 */
    boardSort: VrccBoardSort;
    /**
     * 服务器认不认 `sort`（1.13.0 起）。要了本周榜却拿回没有 `sort` 的总榜时
     * 为 false，界面藏起切换。
     */
    boardSortSupported: boolean;
    recent: VrccRecentNotifications | null;
    /** 几处未读数；null 表示还没拿到，或服务器还没有 `me/unread`。 */
    unread: VrccUnread | null;
    notificationsView: VrccNotificationsView;
    /** 通知中心；null 表示还没拉过，或服务器还没有 `me/notes`。 */
    notes: VrccNotes | null;
    /** 服务器有没有通知中心；还不知道时为 null。 */
    notesSupported: boolean | null;
    /** 公告和停站；null 表示没拉到，或服务器还没有这个接口。 */
    announcements: VrccAnnouncements | null;
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
    /**
     * 「星轨相交」列表；null 表示还没拉过或没权限。
     *
     * 与 mutuals 的区别：mutuals 是**双向已连线**的星友，crossings 只是
     * **同房间擦肩**的人，大多数还没连线。
     */
    crossings: VrccCrossings | null;
    /** 星轨相交的完整星卡（1.13.0 起）；null 表示没拉过或不支持。 */
    crossingCards: VrccCrossingCards | null;
    /**
     * 「星轨相交」这一项失败的原因。
     *
     * 与 mutualsError 同理单独放：`dev-` 标识必然拿不到 `crossings`
     * 敏感权限，那是**某个功能没开通**，不是整个页面坏了。
     *
     * 不可用的**原因分类**不在这里：它在 `crossings.why` / `crossings.hidden`
     * 里跟着响应一起存下来，只有一份，不会和它各说各话。
     */
    crossingsError: string | null;
    service: VrccStatus | null;
    loading: boolean;
    error: VrccError | null;
    /** 设备授权进行中。 */
    deviceAuth: VrccDeviceAuthPrompt | null;
    deviceAuthBusy: boolean;
    deviceAuthMessage: string | null;
    /**
     * 距离下一次允许点「申请连接码」还有多少秒；`0` 表示现在就能点。
     *
     * 这个额度是**按网络出口 IP** 算的（每天 30 个，同一 NAT 后所有人共用），
     * 点一次就真的消耗服务端一次计数，所以不能靠连续点击去碰运气。
     */
    deviceAuthCooldown: number;
};

const initialState: State = {
    tab: 'maps',
    connection: null,
    usage: null,
    boards: null,
    boardSort: 'top',
    boardSortSupported: true,
    recent: null,
    unread: null,
    notificationsView: 'recent',
    notes: null,
    notesSupported: null,
    announcements: null,
    myMap: null,
    mutuals: null,
    mutualsError: null,
    crossings: null,
    crossingCards: null,
    crossingsError: null,
    service: null,
    loading: true,
    error: null,
    deviceAuth: null,
    deviceAuthBusy: false,
    deviceAuthMessage: null,
    deviceAuthCooldown: 0
};

/**
 * 星遇同频页面的控制器。
 *
 * 轮询在未连接时也跑，因为地图榜是开放数据、不需要令牌。个人接口只在
 * 已连接时轮询，且间隔带抖动。
 */
export function useVrccController() {
    const [state, setState] = useState<State>(initialState);
    const authTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const mounted = useRef(true);
    /** 设备码冷却的倒计时句柄。 */
    const cooldownTimer = useRef<ReturnType<typeof setInterval> | null>(null);
    /** 各种排序最近拿到的地图榜，切回来时直接用。 */
    const boardCache = useRef<
        Partial<Record<VrccBoardSort, { board: VrccBoard; at: number }>>
    >({});
    /** 正在看的排序；重新加载时照它拉，而不是跳回总榜。 */
    const boardSortRef = useRef<VrccBoardSort>('top');
    /**
     * 个人数据（所在地图、互相连上）的请求代次。
     *
     * 断开连接时把它推进一位，让**断开前已经发出、还没回来**的请求作废——
     * 否则它们晚到一步，会把刚清掉的状态又写回去，界面就显示上一个账号的
     * 评分了。同一个接口的并发刷新也靠它区分先后。
     */
    const personalEpoch = useRef(0);

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
            if (normalized.kind === 'needsReauth') {
                // 令牌失效（`404 gone`）时后端已经把它清掉了：重读连接状态，
                // 界面改显示「连接已失效，请重新连接」，而不是一直挂着「已连接」。
                void vrccApi
                    .connection()
                    .then((connection) => patch({ connection }))
                    .catch(() => {});
            }
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

    /**
     * 地图榜：官方说明服务器最多 5 分钟更新一次，问再勤也是同一份。
     *
     * `sort` 是本周榜或新上榜、回来的却没有 `sort` 时，服务器还不认这个参数，
     * 回的是总榜：照总榜显示，并藏起切换。
     */
    const refreshBoards = useCallback(
        async (sort: VrccBoardSort = 'top', reuse = false) => {
            const cached = boardCache.current[sort];
            if (reuse && cached && Date.now() - cached.at < BOARD_REUSE_MS) {
                boardSortRef.current = sort;
                patch({ boards: cached.board, boardSort: sort });
                return;
            }
            const boards = await vrccApi.boards(20, sort);
            if (sort !== 'top' && boards.sort == null) {
                boardCache.current.top = { board: boards, at: Date.now() };
                boardSortRef.current = 'top';
                patch({ boards, boardSort: 'top', boardSortSupported: false });
                return;
            }
            boardCache.current[sort] = { board: boards, at: Date.now() };
            boardSortRef.current = sort;
            patch({ boards, boardSort: sort });
        },
        [patch]
    );

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
        // 记下发起时的代次：断开连接会推进它，这样晚到的响应就不会把
        // 已经清空的 `myMap` 又写回去（那会让界面显示上一个账号的评分）。
        const epoch = personalEpoch.current;
        const myMap = await vrccApi.myMap();
        if (epoch !== personalEpoch.current) return;
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
        const epoch = personalEpoch.current;
        try {
            const mutuals = await vrccApi.mutuals();
            if (epoch !== personalEpoch.current) return;
            patch({ mutuals, mutualsError: null });
        } catch (error) {
            if (epoch !== personalEpoch.current) return;
            if (error instanceof VrccError && error.kind === 'missingScope') {
                // 权限没开通：静默记录，界面在自己的页签里说明。
                patch({ mutuals: null, mutualsError: error.message });
                return;
            }
            patch({ mutuals: null });
            throw error;
        }
    }, [patch]);

    /**
     * 几处未读数（`me/unread`）。
     *
     * `refresh` 为 false 时只读后台轮询的缓存：收到「有变化」的事件时后台刚问过，
     * 不必再花一次（每天 1500 次）。
     */
    const refreshUnread = useCallback(
        async (refresh = true) => {
            const unread = await vrccApi.unread(refresh);
            patch({ unread });
        },
        [patch]
    );

    /** 通知中心（`me/notes`）。只在玩家切到「通知」时拉，读了不标已读。 */
    const refreshNotes = useCallback(async () => {
        const notes = await vrccApi.notes();
        patch({ notes, notesSupported: notes !== null });
    }, [patch]);

    /**
     * 公告和停站。拿不到不算页面错误：它只是附加信息，停站时个人接口
     * 自己也会报「停站维护中」。
     */
    const refreshAnnouncements = useCallback(async () => {
        try {
            const announcements = await vrccApi.announcements();
            patch({ announcements });
        } catch (error) {
            console.error('[vrcc] refreshAnnouncements failed:', error);
        }
    }, [patch]);

    /**
     * 「星轨相交」列表与完整星卡。
     *
     * 需要 `crossings` 敏感权限。`dev-` 标识拿到的是 `403 scope`——预期内
     * 的结果（敏感权限要审核），不是故障，所以只记到 `crossingsError`
     * 供该页签自己显示，不走全局 `reportError`。
     *
     * `mark` 只在玩家真的打开这一页时为 true：它会把已读同步到账号，
     * 网页和手机上的红点跟着消。别的场景一律 false。
     *
     * 星卡的额度（`left` / `max`）是**按天**算的，读一次算第二份读的
     * 一次，所以这里把它和列表绑在同一个 `mark` 上：后台为了算数字时
     * 两个都问、两个都花额度，是最贵的用法，也没有任何界面需要它。
     */
    const refreshCrossings = useCallback(
        async (mark = false) => {
            const epoch = personalEpoch.current;
            try {
                // 星卡是 1.13.0 才有的接口；旧服务器会给 404。那只影响
                // 「今天还能连几颗」这一小块，不该让整个列表失败。
                const [crossings, cards] = await Promise.all([
                    vrccApi.crossings(mark),
                    vrccApi.crossingCards(mark).catch(() => null)
                ]);
                // 断开连接会推进代次：断开前发出、断开后才回来的响应
                // 不能再把状态写回去，否则界面会显示上一个账号的擦肩记录。
                if (epoch !== personalEpoch.current) return null;
                patch({
                    crossings,
                    crossingCards: cards,
                    crossingsError: null
                });
                return crossings;
            } catch (error) {
                if (epoch !== personalEpoch.current) return null;
                if (
                    error instanceof VrccError &&
                    error.kind === 'missingScope'
                ) {
                    // 权限没开通：静默记录，界面在自己的页签里说明。
                    patch({
                        crossings: null,
                        crossingCards: null,
                        crossingsError: error.message
                    });
                    return null;
                }
                patch({ crossings: null, crossingCards: null });
                throw error;
            }
        },
        [patch]
    );

    const refreshService = useCallback(async () => {
        try {
            const service = await vrccApi.status();
            patch({ service });
        } catch (error) {
            // 探活失败本身不是页面错误，静默处理。
            //
            // 但**不能一律标 down**：`status()` 也要过 `acquire()`，别的接口
            // 刚吃过一次 429 时，在全局退避期间这个请求根本不会发出去，
            // 本地直接返回 `throttled`。那时服务是好的，标成「服务不可用」
            // 是在冤枉它。只有真的网络失败或拿到了非 ok 的回答才算 down。
            if (error instanceof VrccError && error.kind === 'throttled') {
                return;
            }
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
        void refreshAnnouncements();
        void refreshBoards(boardSortRef.current).catch((error) => {
            reportError(error, 'refreshBoards');
        });
        if (connected) {
            // 所在地图要已连接才有意义：未连接时后端直接报 TokenMissing，
            // 会被判成 needsReauth，于是每开一次页面就弹一次「去重新连接」，
            // 而其实玩家只是还没连。
            void refreshMyMap().catch((error) => {
                reportError(error, 'refreshMyMap');
            });
            void refreshRecent(false).catch((error) => {
                reportError(error, 'refreshRecent');
            });
            // 「互相连上」要敏感权限，`dev-` 一定拿不到；单独兜住，不影响别的。
            void refreshMutuals().catch((error) => {
                reportError(error, 'refreshMutuals');
            });
            // 「星轨相交」**不在这里拉**：首屏一定是地图榜那个页签，没人看得见
            // 它。它又算两份每天额度（列表一份、星卡一份），提前拉等于每次开页
            // 都白花 2 次。等玩家真切到那一页时 `setTab` 会拉，连 mark 一起做。
            void refreshUnread(true).catch((error) => {
                reportError(error, 'refreshUnread');
            });
        }
    }, [
        patch,
        refreshAnnouncements,
        refreshBoards,
        refreshConnection,
        refreshMutuals,
        refreshMyMap,
        refreshRecent,
        refreshService,
        refreshUnread,
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
            if (authTimer.current) clearTimeout(authTimer.current);
            if (cooldownTimer.current) clearInterval(cooldownTimer.current);
        };
        // `load` 故意不放进依赖：它自己会 `patch`，进依赖就是无限循环。
        // 上面的 `didLoad` ref 已经保证只跑一次，所以这个空依赖是正确的。
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    /**
     * 玩家换世界时刷新「所在地图」卡片。
     *
     * 官方 README「场景三」要求在「打开面板或切地图时」请求所在地图。
     * 原来只在 `load()` 里拉一次，玩家在页面开着时换图，卡片会一直显示旧地图。
     *
     * 用 ref 记住上一次的世界：首次只是记下来，不重复请求——`load()` 已经
     * 拉过一次，而后端对 `me/maps/{world}` 有 30 秒最小间隔，多问一次会白等。
     */
    const currentWorldId = useRuntimeStore(
        (store) => store.gameState.currentWorldId ?? null
    );
    const lastWorldRef = useRef<string | null>(null);
    useEffect(() => {
        const previous = lastWorldRef.current;
        lastWorldRef.current = currentWorldId;
        // 首次（或没换图）不请求。
        if (previous === null || previous === currentWorldId) return;
        if (!state.connection?.connected) return;
        void refreshMyMap().catch((error) => {
            reportError(error, 'refreshMyMap');
        });
    }, [
        currentWorldId,
        state.connection?.connected,
        refreshMyMap,
        reportError
    ]);

    /**
     * 订阅后端推来的「提醒有变化」事件。
     *
     * **轮询在 Rust 侧**（`DesktopVrccRuntime::spawn_background_poll`）。
     * 原来这里自己 `setTimeout` 轮询，有两个问题：
     *
     * 1. 窗口隐藏到托盘后 Chromium 会节流隐藏页面的定时器，60 秒可能被拉到
     *    几分钟——而"后台也能收到提醒"正是这个功能的主要场景。
     * 2. 和 Rust 侧轮询并存会每个周期发两次请求，`mark` 语义也会打架。
     *
     * 现在后端发事件、前端只负责刷新状态。事件本身不带提醒正文（不把玩家
     * 内容经手额外的事件层），所以收到后要自己拉一次列表。
     *
     * 另一个事件是「连接失效」：后端收到 `404 gone` 后已经清掉令牌，这里
     * 重读连接状态，界面改显示「请重新连接」。连接一断，这个 effect 就退订。
     */
    useEffect(() => {
        if (!state.connection?.connected) {
            return;
        }
        let disposed = false;
        const unsubscribers: Array<() => void> = [];

        const subscribe = (event: string, handler: () => void) => {
            tauriClient.events
                .subscribe(event, () => {
                    if (!disposed) handler();
                })
                .then((off) => {
                    if (disposed) {
                        // 订阅回来时组件已经卸载：立刻退订，别留下悬空监听。
                        off();
                        return;
                    }
                    unsubscribers.push(off);
                    console.info(`[vrcc] ${event} event subscribed`);
                })
                .catch((error) => {
                    console.error(
                        `[vrcc] subscribe ${event} event failed:`,
                        error
                    );
                });
        };

        subscribe(VRCC_NOTICES_CHANGED_EVENT, () => {
            // 收到后端信号说明有新提醒或未读数变了，这里拉一次列表。
            // 这行日志用于区分「事件没到」和「事件到了但刷新失败」，
            // 是排查「后台弹了通知但界面没变」的第一现场。
            console.info('[vrcc] notices changed event received');
            // 后台发现的提醒不标记已读：玩家还没看到，替他标记是错的。
            refreshRecent(false).catch((error) => {
                if (disposed) return;
                if (
                    error instanceof VrccError &&
                    error.kind === 'needsReauth'
                ) {
                    reportError(error, 'poll');
                }
            });
            // 后台刚问过未读数，读它留下的就行。
            refreshUnread(false).catch((error) => {
                console.error('[vrcc] refreshUnread failed:', error);
            });
        });
        subscribe(VRCC_CONNECTION_CHANGED_EVENT, () => {
            console.info('[vrcc] connection changed event received');
            refreshConnection().catch((error) => {
                if (!disposed) reportError(error, 'refreshConnection');
            });
        });

        return () => {
            disposed = true;
            for (const off of unsubscribers) off();
            console.info('[vrcc] events unsubscribed');
        };
    }, [
        state.connection?.connected,
        refreshConnection,
        refreshRecent,
        refreshUnread,
        reportError
    ]);

    /**
     * 切页签。
     *
     * 切到「提醒」时用 `mark=true` 重拉一次：这是玩家真正在看这个列表的时刻，
     * 服务器据此把已读同步到账号，网页和手机 App 上的红点会跟着消。
     * 官方的做法也是「打开最近的提醒」时才 mark。
     *
     * 「星轨相交」同理，而且这里**只有**这一条路会传 `mark=true`：
     * `load()` 与刷新按钮都传 false。漏掉这一条，玩家永远消不掉红点；
     * 反过来在别处误传 true，等于替玩家把没看过的记录标成看过。
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
            if (tab === 'crossings') {
                // 打开这一页 = 看过了。权限没开通时 `refreshCrossings`
                // 自己把原因记进 `crossingsError`，不会抛出来。
                refreshCrossings(true).catch((error) => {
                    reportError(error, 'refreshCrossings');
                });
            }
        },
        [
            patch,
            refreshCrossings,
            refreshMutuals,
            refreshRecent,
            reportError,
            state.connection?.connected
        ]
    );

    /** 切换地图榜的排序。刚看过的直接用手上的，不再请求。 */
    const setBoardSort = useCallback(
        (sort: VrccBoardSort) => {
            refreshBoards(sort, true).catch((error) => {
                reportError(error, 'refreshBoards');
            });
        },
        [refreshBoards, reportError]
    );

    /**
     * 「提醒」页签里切换「最近的提醒 / 通知中心」。
     *
     * 通知中心每次切过来都拉一次：它不标已读、玩家在网页上看过会变，而它算
     * 每天 1000 次的第二份读，只在玩家真的要看时才花。
     */
    const setNotificationsView = useCallback(
        (view: VrccNotificationsView) => {
            patch({ notificationsView: view });
            if (view === 'notes' && state.connection?.connected) {
                refreshNotes().catch((error) => {
                    reportError(error, 'refreshNotes');
                });
            }
        },
        [patch, refreshNotes, reportError, state.connection?.connected]
    );

    /**
     * 启动（或重启）设备码冷却倒计时。
     *
     * 用 `setInterval` 而不是一次性 `setTimeout`：界面要显示「还有几秒」，
     * 每秒都要动一次。到 0 就自己停掉，不留下空转的定时器。
     */
    const startDeviceAuthCooldown = useCallback(() => {
        if (cooldownTimer.current != null) {
            clearInterval(cooldownTimer.current);
            cooldownTimer.current = null;
        }
        let left = DEVICE_AUTH_COOLDOWN_SECS;
        patch({ deviceAuthCooldown: left });
        cooldownTimer.current = setInterval(() => {
            left -= 1;
            if (!mounted.current) {
                if (cooldownTimer.current != null) {
                    clearInterval(cooldownTimer.current);
                    cooldownTimer.current = null;
                }
                return;
            }
            patch({ deviceAuthCooldown: Math.max(0, left) });
            if (left <= 0 && cooldownTimer.current != null) {
                clearInterval(cooldownTimer.current);
                cooldownTimer.current = null;
            }
        }, 1000);
    }, [patch]);

    /**
     * 开始设备授权。
     *
     * 冷却期内直接返回，**连请求都不发**：那份额度是按网络出口 IP 算的，
     * 每次调用都真实消耗服务端一个计数，不能用连点去试探。
     */
    const beginDeviceAuth = useCallback(async () => {
        if (state.deviceAuthCooldown > 0) {
            return;
        }
        // 请求一发出就开始冷却，不等它回来：`too_many` 时没有响应，若等结果
        // 再计时，失败的那一次反而让玩家立刻又能点。
        startDeviceAuthCooldown();
        patch({ deviceAuthBusy: true, deviceAuthMessage: null, error: null });
        try {
            const prompt = await vrccApi.beginDeviceAuth();
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
    }, [patch, reportError, startDeviceAuthCooldown, state.deviceAuthCooldown]);

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
        // 先推进代次：断开前发出的个人数据请求从这里开始就作废了，
        // 它们的响应不会再写回状态。
        personalEpoch.current += 1;
        try {
            await vrccApi.disconnect();
            patch({
                deviceAuth: null,
                recent: null,
                unread: null,
                notes: null,
                // 「所在地图」「互相连上」「星轨相交」都是**上一个账号**的
                // 个人数据。不清的话，`VrccPage` 只看 `myMap` 是否存在就渲染
                // 卡片，断开后还会继续显示别人的评分和擦肩记录。
                myMap: null,
                mutuals: null,
                mutualsError: null,
                crossings: null,
                crossingCards: null,
                crossingsError: null
            });
        } catch (error) {
            // 可重试的失败（网络不通、正在退避）**后端会刻意保留令牌**，
            // 等玩家再点一次。这时界面必须如实显示「还连着」，不能骗人。
            reportError(error, 'disconnect');
        } finally {
            // 无论成败都要重读连接状态。
            //
            // 原来这句在 `try` 里 `await` 之后：一旦 `disconnect` 抛错就跳过，
            // 于是后端已经断开、页面却还挂着「已连接」；反过来后端刻意保留了
            // 令牌、页面又可能显示成已断开。放 `finally` 保证界面跟后端一致。
            await refreshConnection();
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
        refreshBoards: () => refreshBoards(boardSortRef.current),
        setBoardSort,
        setNotificationsView,
        refreshMyMap,
        refreshMutuals,
        refreshCrossings,
        beginDeviceAuth,
        disconnect,
        openWorld,
        openUser
    };
}
