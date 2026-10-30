// @vitest-environment jsdom

import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    connection: vi.fn(),
    usage: vi.fn(),
    boards: vi.fn(),
    status: vi.fn(),
    announcements: vi.fn(),
    unread: vi.fn(),
    notes: vi.fn(),
    recentNotifications: vi.fn(),
    pollNotifications: vi.fn(),
    myMap: vi.fn(),
    mutuals: vi.fn(),
    crossings: vi.fn(),
    crossingCards: vi.fn(),
    today: vi.fn(),
    beginDeviceAuth: vi.fn(),
    deviceAuthPoll: vi.fn(),
    disconnect: vi.fn(),
    // A 批新增的读接口。`vi.fn()` 默认返回 `undefined`，调用方拿到的是
    // 「没有数据」，和真实的「旧服务器回 404」等价，不会在初始化时炸掉。
    maps: vi.fn(),
    stickers: vi.fn(),
    myMaps: vi.fn(),
    mapPage: vi.fn(),
    myBoard: vi.fn(),
    toastAdd: vi.fn(),
    openWorldDialog: vi.fn(),
    openUserDialog: vi.fn(),
    openGroupDialog: vi.fn(),
    subscribeNotices: vi.fn()
}));

vi.mock('@/services/toastService', () => ({
    toast: { add: mocks.toastAdd }
}));

vi.mock('@/services/dialogService', () => ({
    openWorldDialog: mocks.openWorldDialog,
    openUserDialog: mocks.openUserDialog,
    openGroupDialog: mocks.openGroupDialog
}));

// 提醒改由后端轮询、前端订阅事件。测试里给一个能立刻退订的假订阅：
// 不关心事件何时到，只关心订阅/退订的成对行为。
vi.mock('@/platform/tauri/client', () => ({
    tauriClient: {
        events: {
            subscribe: mocks.subscribeNotices
        }
    }
}));

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccConnection: mocks.connection,
        appVrccUsage: mocks.usage,
        appVrccBoards: mocks.boards,
        appVrccStatus: mocks.status,
        appVrccAnnouncements: mocks.announcements,
        appVrccUnread: mocks.unread,
        appVrccNotes: mocks.notes,
        appVrccRecentNotifications: mocks.recentNotifications,
        appVrccPollNotifications: mocks.pollNotifications,
        appVrccMyMap: mocks.myMap,
        appVrccMutuals: mocks.mutuals,
        appVrccCrossings: mocks.crossings,
        appVrccCrossingCards: mocks.crossingCards,
        appVrccToday: mocks.today,
        appVrccDeviceAuthBegin: mocks.beginDeviceAuth,
        appVrccDeviceAuthPoll: mocks.deviceAuthPoll,
        appVrccDisconnect: mocks.disconnect,
        appVrccMaps: mocks.maps,
        appVrccStickers: mocks.stickers,
        appVrccMyMaps: mocks.myMaps,
        appVrccMapPage: mocks.mapPage,
        appVrccMyBoard: mocks.myBoard
    }
}));

// 必须部分 mock：`i18nService` 在模块初始化时要用到真实的
// `initReactI18next`，整体替换会让它在 import 阶段就失败。
vi.mock('react-i18next', async (importOriginal) => {
    const actual = await importOriginal<typeof import('react-i18next')>();
    return {
        ...actual,
        useTranslation: () => ({ t: (key: string) => key })
    };
});

import { useVrccController } from './useVrccController';

const connected = {
    connected: true,
    expired: false,
    clientIsDev: true,
    star: 77,
    vrcName: 'VRC',
    scope: 'profile maps',
    client: 'dev-test',
    lastPollAt: null,
    nextPollAt: null
};

const usage = {
    personalUsedToday: 7,
    personalDailyLimit: 1000,
    unreadUsedToday: 40,
    unreadDailyLimit: 1500,
    windowUsed: 2,
    windowLimit: 120,
    backoffRemainingMs: 0,
    resetSecs: 3600
};

beforeEach(() => {
    vi.clearAllMocks();
    mocks.connection.mockResolvedValue(connected);
    mocks.usage.mockResolvedValue(usage);
    mocks.boards.mockImplementation((_limit: number, sort: string | null) =>
        Promise.resolve({
            board: 'maps',
            sort: sort ?? 'top',
            min: 1,
            maps: [],
            at: 1
        })
    );
    mocks.status.mockResolvedValue({ at: 1, state: 'ok' });
    mocks.announcements.mockResolvedValue({ stop: null, posts: [] });
    mocks.unread.mockResolvedValue({ inbox: 1, cross: 0, chats: 2, notes: 0 });
    mocks.notes.mockResolvedValue({ notes: [] });
    mocks.recentNotifications.mockResolvedValue({ items: [], read: 0 });
    mocks.pollNotifications.mockResolvedValue({
        last: 1,
        items: [],
        every: 60,
        dot: 0
    });
    mocks.myMap.mockResolvedValue(null);
    mocks.mutuals.mockResolvedValue({ stars: [], n: 0 });
    mocks.crossings.mockResolvedValue({
        stars: [],
        n: 0,
        why: null,
        hidden: false,
        me: null
    });
    mocks.crossingCards.mockResolvedValue({
        cards: [],
        seen: null,
        left: null,
        max: null
    });
    // 订阅立即返回一个退订函数，模拟 Tauri 正常挂上监听。
    mocks.subscribeNotices.mockResolvedValue(() => {});
});

afterEach(() => {
    cleanup();
});

describe('useVrccController', () => {
    it('加载后填入连接状态与真实用量', async () => {
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        expect(result.current.connection?.connected).toBe(true);
        // 用量必须是后端给的真值：写死的 0 会被这条抓住。
        expect(result.current.usage?.personalUsedToday).toBe(7);
        expect(result.current.usage?.personalDailyLimit).toBe(1000);
    });

    it('申请连接码后进入 5 秒冷却，冷却期内不再发请求', async () => {
        vi.useFakeTimers({ shouldAdvanceTime: true });
        mocks.beginDeviceAuth.mockResolvedValue({
            userCode: 'ABCD-2345',
            deviceCode: 'dc',
            verificationUri: 'https://vrcc.cc/device',
            verificationUriComplete: 'https://vrcc.cc/device?code=ABCD-2345',
            interval: 5,
            expiresIn: 600
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        expect(result.current.deviceAuthCooldown).toBe(0);

        await act(async () => {
            await result.current.beginDeviceAuth();
        });
        expect(mocks.beginDeviceAuth).toHaveBeenCalledTimes(1);
        // 请求一发出就开始计时，不等它回来。
        expect(result.current.deviceAuthCooldown).toBeGreaterThan(0);

        // 冷却期内再点：连请求都不该发出去。
        await act(async () => {
            await result.current.beginDeviceAuth();
        });
        expect(mocks.beginDeviceAuth).toHaveBeenCalledTimes(1);

        // 走完 5 秒后按钮恢复可用。
        await act(async () => {
            vi.advanceTimersByTime(5_000);
        });
        await waitFor(() => expect(result.current.deviceAuthCooldown).toBe(0));
        vi.useRealTimers();
    });

    it('打开「提醒」页签时用 mark=true 同步账号已读', async () => {
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        mocks.recentNotifications.mockClear();

        act(() => result.current.setTab('notifications'));

        await waitFor(() =>
            expect(mocks.recentNotifications).toHaveBeenCalledWith(true)
        );
        // 只有这一次是 true——后台轮询不会传 true。
        const trues = mocks.recentNotifications.mock.calls.filter(
            (call) => call[0] === true
        );
        expect(trues).toHaveLength(1);
    });

    it('初次加载的提醒拉取不带 mark', async () => {
        renderHook(() => useVrccController());
        await waitFor(() =>
            expect(mocks.recentNotifications).toHaveBeenCalled()
        );
        // 加载时只是为了拿数据，不该替玩家标记已读。
        expect(mocks.recentNotifications).toHaveBeenCalledWith(false);
    });

    it('已连接时订阅后端提醒事件，卸载时退订', async () => {
        const unsubscribe = vi.fn();
        mocks.subscribeNotices.mockResolvedValue(unsubscribe);

        const { unmount } = renderHook(() => useVrccController());

        await waitFor(() =>
            expect(mocks.subscribeNotices).toHaveBeenCalledWith(
                'vrccNoticesChanged',
                expect.any(Function)
            )
        );
        expect(unsubscribe).not.toHaveBeenCalled();

        unmount();
        // 必须退订：否则页面切走后监听还挂着，事件到达会打到已卸戴的组件上。
        await waitFor(() => expect(unsubscribe).toHaveBeenCalled());
    });

    it('后端推来「提醒有变化」时刷新列表，且不标记已读', async () => {
        const handlers: Record<string, () => void> = {};
        mocks.subscribeNotices.mockImplementation(
            (event: string, handler: () => void) => {
                handlers[event] = handler;
                return Promise.resolve(() => {});
            }
        );

        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        await waitFor(() => expect(handlers.vrccNoticesChanged).toBeDefined());

        mocks.recentNotifications.mockClear();
        mocks.unread.mockClear();
        act(() => handlers.vrccNoticesChanged());

        // 后台发现的提醒不能替玩家标记已读。
        await waitFor(() =>
            expect(mocks.recentNotifications).toHaveBeenCalledWith(false)
        );
        // 未读数读后台刚问过的缓存，不再花一次 me/unread。
        expect(mocks.unread).toHaveBeenCalledWith(false);
    });

    it('后端说连接失效时重读连接状态', async () => {
        const handlers: Record<string, () => void> = {};
        mocks.subscribeNotices.mockImplementation(
            (event: string, handler: () => void) => {
                handlers[event] = handler;
                return Promise.resolve(() => {});
            }
        );

        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        await waitFor(() =>
            expect(handlers.vrccConnectionChanged).toBeDefined()
        );

        mocks.connection.mockResolvedValue({
            ...connected,
            connected: false,
            expired: true
        });
        act(() => handlers.vrccConnectionChanged());

        await waitFor(() =>
            expect(result.current.connection?.connected).toBe(false)
        );
        expect(result.current.connection?.expired).toBe(true);
    });

    it('请求报令牌失效时也重读连接状态，界面不再挂着「已连接」', async () => {
        const consoleSpy = vi
            .spyOn(console, 'error')
            .mockImplementation(() => {});
        mocks.myMap.mockRejectedValue(
            Object.assign(new Error('星遇同频接口返回 HTTP 404（gone）'), {
                vrcc: { kind: 'needsReauth', need: null }
            })
        );
        mocks.connection.mockResolvedValueOnce(connected).mockResolvedValue({
            ...connected,
            connected: false,
            expired: true
        });

        const { result } = renderHook(() => useVrccController());

        await waitFor(() =>
            expect(result.current.connection?.expired).toBe(true)
        );
        expect(result.current.error?.kind).toBe('needsReauth');
        consoleSpy.mockRestore();
    });

    it('crossings 缺权限时只记在页签里，不弹 toast、不拖垮整页', async () => {
        // 与 mutuals 同一个道理：`dev-` 标识拿不到 `crossings` 敏感权限，
        // 线上实返 `403 scope`。这是预期结果，不是故障——只在「星轨相交」
        // 页签里说明，不该渲染成整页红色横幅。
        mocks.crossings.mockRejectedValue(
            Object.assign(new Error('HTTP 403 scope'), {
                vrcc: { kind: 'missingScope', need: 'crossings' }
            })
        );
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        act(() => result.current.setTab('crossings'));
        await waitFor(() =>
            expect(result.current.crossingsError).not.toBeNull()
        );

        expect(result.current.crossingsError).toContain('403');
        expect(result.current.crossings).toBeNull();
        expect(mocks.toastAdd).not.toHaveBeenCalled();
        // 地图榜与连接状态照旧可用。
        expect(result.current.boards).not.toBeNull();
        expect(result.current.connection?.connected).toBe(true);
    });

    it('crossings 把 hidden 带出来，交给界面隐藏整个模块', async () => {
        // `hide` 意味着玩家不该看到这个模块（16-17 岁星池，或玩家在隐私
        // 设置里关了星轨相交）。官方要求客户端不显示，所以控制器必须把
        // 后端算好的 `hidden` 留在 state 里，不能吞掉——吞掉就会变成一个
        // 普通空列表，等于向不该看到的人确认「这里有星轨相交」这个事实。
        mocks.crossings.mockResolvedValue({
            stars: [],
            n: 0,
            why: 'hide',
            hidden: true,
            me: null
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        act(() => result.current.setTab('crossings'));
        await waitFor(() =>
            expect(result.current.crossings?.hidden).toBe(true)
        );
        expect(result.current.crossings?.why).toBe('hide');
        expect(result.current.crossings?.stars).toHaveLength(0);
    });

    it('首屏不拉星轨相交，切到这一页才拉、并且带 mark=true', async () => {
        // 两件事一起钉住：
        // 1. 首屏一定停在地图榜，没人看得见星轨相交。它算两份每天额度
        //    （列表一份、星卡一份），提前拉就是每次开页白花 2 次。
        // 2. 玩家真切到这一页 = 看过了，要带 mark=true 把红点消掉。
        //    这条链路错了，红点永远消不掉。
        mocks.crossings.mockResolvedValue({
            stars: [],
            n: 0,
            why: null,
            hidden: false,
            me: null
        });
        mocks.crossingCards.mockResolvedValue({
            cards: [],
            seen: null,
            left: 3,
            max: 5
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        // 首屏：一次都没问过。
        expect(mocks.crossings).not.toHaveBeenCalled();
        expect(mocks.crossingCards).not.toHaveBeenCalled();

        // 玩家切到「星轨相交」：带 mark=true 拉。
        act(() => result.current.setTab('crossings'));
        await waitFor(() => expect(mocks.crossings).toHaveBeenCalledWith(true));
        expect(mocks.crossingCards).toHaveBeenCalledWith(true);
        // 星卡的额度读数被记下来了。
        await waitFor(() => expect(result.current.crossingCards?.left).toBe(3));
    });

    it('手动刷新星轨相交不替玩家把记录标成已读', async () => {
        // 刷新按钮只是「再看看有没有新的」，不是「我看过了」。顺手传
        // mark=true 会把玩家还没看完的记录标成已读，而且不可撤销、
        // 不会报错。
        mocks.crossings.mockResolvedValue({
            stars: [],
            n: 0,
            why: null,
            hidden: false,
            me: null
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        mocks.crossings.mockClear();
        await act(async () => {
            await result.current.refreshCrossings(false);
        });
        expect(mocks.crossings).toHaveBeenCalledWith(false);
    });

    it('断开连接后清掉星轨相交，不把上个账号的记录留在界面上', async () => {
        // 擦肩记录是**个人数据**。不清的话，断开后页签里还留着别人的
        // 记录和额度，看起来像是连着的。
        mocks.crossings.mockResolvedValue({
            stars: [{ n: 77, name: '拾光旅人', linked: false, mutual: false }],
            n: 1,
            why: null,
            hidden: false,
            me: null
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        // 首屏不拉，先切到那一页把数据填上。
        act(() => result.current.setTab('crossings'));
        await waitFor(() =>
            expect(result.current.crossings?.stars).toHaveLength(1)
        );

        mocks.disconnect.mockResolvedValue(undefined);
        await act(async () => {
            await result.current.disconnect();
        });

        expect(result.current.crossings).toBeNull();
        expect(result.current.crossingCards).toBeNull();
    });

    it('mutuals 缺权限时只记在页签里，不弹 toast、不拖垮整页', async () => {
        // `dev-` 标识拿不到 `mutuals` 敏感权限，线上实返 `403 scope`。
        // 这是**预期内**的结果，不是故障：界面只在「连上」页签里说明，
        // 不该走全局错误路径（那会渲染成整页红色横幅，看着像连接坏了）。
        mocks.mutuals.mockRejectedValue(
            Object.assign(new Error('HTTP 403 scope'), {
                vrcc: { kind: 'missingScope', need: 'mutuals' }
            })
        );
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        await waitFor(() => expect(result.current.mutualsError).not.toBeNull());

        // 失败原因被记下来了，供页签自己显示。
        expect(result.current.mutualsError).toContain('403');
        expect(result.current.mutuals).toBeNull();
        // 关键：**没有**弹 toast。这条断言以前不存在，用例名与行为不符。
        expect(mocks.toastAdd).not.toHaveBeenCalled();
        // 地图榜和连接状态照旧可用。
        expect(result.current.boards).not.toBeNull();
        expect(result.current.connection?.connected).toBe(true);
    });

    it('mutuals 遇到真故障时仍然弹 toast', async () => {
        // 与上一条对照：非缺权限的失败（网络、5xx）是真故障，要有反馈。
        const consoleSpy = vi
            .spyOn(console, 'error')
            .mockImplementation(() => {});
        mocks.mutuals.mockRejectedValue(
            Object.assign(new Error('network unreachable'), {
                vrcc: { kind: 'transport', need: null }
            })
        );
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        await waitFor(() => expect(mocks.toastAdd).toHaveBeenCalled());

        expect(result.current.mutualsError).toBeNull();
        consoleSpy.mockRestore();
    });

    it('连上后拉一次未读数（发请求）', async () => {
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.unread).not.toBeNull());
        expect(mocks.unread).toHaveBeenCalledWith(true);
        expect(result.current.unread?.chats).toBe(2);
    });

    it('服务器还没有新接口时安静地不显示', async () => {
        mocks.unread.mockResolvedValue(null);
        mocks.announcements.mockResolvedValue(null);
        mocks.notes.mockResolvedValue(null);

        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        await waitFor(() => expect(mocks.unread).toHaveBeenCalled());

        act(() => result.current.setNotificationsView('notes'));
        await waitFor(() => expect(result.current.notesSupported).toBe(false));
        expect(result.current.unread).toBeNull();
        expect(result.current.announcements).toBeNull();
        expect(result.current.error).toBeNull();
        expect(mocks.toastAdd).not.toHaveBeenCalled();
    });

    it('切到「通知」才拉通知中心', async () => {
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        expect(mocks.notes).not.toHaveBeenCalled();

        act(() => result.current.setNotificationsView('notes'));
        await waitFor(() => expect(mocks.notes).toHaveBeenCalledTimes(1));
        expect(result.current.notificationsView).toBe('notes');
        await waitFor(() => expect(result.current.notesSupported).toBe(true));
    });

    it('切换地图榜排序', async () => {
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.boards).not.toBeNull());
        expect(mocks.boards).toHaveBeenCalledWith(20, 'top');

        act(() => result.current.setBoardSort('week'));
        await waitFor(() => expect(result.current.boardSort).toBe('week'));
        expect(mocks.boards).toHaveBeenCalledWith(20, 'week');
        expect(result.current.boardSortSupported).toBe(true);

        // 一分钟内切回总榜，直接用手上的，不再请求。
        mocks.boards.mockClear();
        act(() => result.current.setBoardSort('top'));
        await waitFor(() => expect(result.current.boardSort).toBe('top'));
        expect(mocks.boards).not.toHaveBeenCalled();
    });

    it('服务器不认 sort 时退回总榜并藏起切换', async () => {
        // 旧服务器：不管要什么都回总榜，回答里没有 sort。
        mocks.boards.mockResolvedValue({
            board: 'maps',
            min: 1,
            maps: [],
            at: 1
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.boards).not.toBeNull());

        act(() => result.current.setBoardSort('week'));
        await waitFor(() =>
            expect(result.current.boardSortSupported).toBe(false)
        );
        expect(result.current.boardSort).toBe('top');
    });

    it('地图被隐藏时后端返回 null，页面不显示卡片', async () => {
        mocks.myMap.mockResolvedValue(null);
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        expect(result.current.myMap).toBeNull();
    });

    it('地图可见时填充卡片数据', async () => {
        mocks.myMap.mockResolvedValue({
            world: 'wrld_x',
            name: '茶屋',
            board: true,
            pending: false,
            off: false,
            avg: 9.2,
            raters: 18,
            mine: 9,
            words: null,
            wordsOk: null,
            img: null,
            imgUrl: null
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        expect(result.current.myMap?.avg).toBe(9.2);
    });

    it('openUser 只接受 usr_ 前缀', () => {
        const { result } = renderHook(() => useVrccController());

        act(() => result.current.openUser('not-a-user-id'));
        expect(mocks.openUserDialog).not.toHaveBeenCalled();

        act(() => result.current.openUser('usr_0123456789abcdef'));
        expect(mocks.openUserDialog).toHaveBeenCalledWith({
            userId: 'usr_0123456789abcdef'
        });
    });

    it('openWorld 空 id 不打开对话框', () => {
        const { result } = renderHook(() => useVrccController());

        act(() => result.current.openWorld(''));
        expect(mocks.openWorldDialog).not.toHaveBeenCalled();

        act(() => result.current.openWorld('wrld_abc'));
        expect(mocks.openWorldDialog).toHaveBeenCalledWith({
            worldId: 'wrld_abc'
        });
    });

    it('加载失败时同时写 console 与 toast', async () => {
        const consoleSpy = vi
            .spyOn(console, 'error')
            .mockImplementation(() => {});
        mocks.boards.mockRejectedValue(new Error('boom'));

        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        expect(consoleSpy).toHaveBeenCalled();
        expect(mocks.toastAdd).toHaveBeenCalled();
        const toastCall = mocks.toastAdd.mock.calls[0][0];
        expect(toastCall.type).toBe('error');
        // 错误也留在页面上，切页签回来还能看到。
        expect(result.current.error).not.toBeNull();

        consoleSpy.mockRestore();
    });
});
