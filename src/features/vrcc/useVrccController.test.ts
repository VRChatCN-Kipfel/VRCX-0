// @vitest-environment jsdom

import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    connection: vi.fn(),
    usage: vi.fn(),
    boards: vi.fn(),
    status: vi.fn(),
    recentNotifications: vi.fn(),
    pollNotifications: vi.fn(),
    myMap: vi.fn(),
    mutuals: vi.fn(),
    today: vi.fn(),
    beginDeviceAuth: vi.fn(),
    deviceAuthPoll: vi.fn(),
    disconnect: vi.fn(),
    classifyError: vi.fn(),
    toastAdd: vi.fn(),
    openWorldDialog: vi.fn(),
    openUserDialog: vi.fn()
}));

vi.mock('@/services/toastService', () => ({
    toast: { add: mocks.toastAdd }
}));

vi.mock('@/services/dialogService', () => ({
    openWorldDialog: mocks.openWorldDialog,
    openUserDialog: mocks.openUserDialog
}));

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccConnection: mocks.connection,
        appVrccUsage: mocks.usage,
        appVrccBoards: mocks.boards,
        appVrccStatus: mocks.status,
        appVrccRecentNotifications: mocks.recentNotifications,
        appVrccPollNotifications: mocks.pollNotifications,
        appVrccMyMap: mocks.myMap,
        appVrccMutuals: mocks.mutuals,
        appVrccToday: mocks.today,
        appVrccDeviceAuthBegin: mocks.beginDeviceAuth,
        appVrccDeviceAuthPoll: mocks.deviceAuthPoll,
        appVrccDisconnect: mocks.disconnect,
        appVrccClassifyError: mocks.classifyError
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
    star: 77,
    vrcName: 'VRC',
    scope: 'profile maps',
    client: 'dev-test',
    lastPollAt: null,
    nextPollAt: null
};

const usage = {
    personalUsedToday: 7,
    personalDailyLimit: 300,
    deviceCodeUsedToday: 1,
    deviceCodeDailyLimit: 30,
    windowUsed: 2,
    windowLimit: 120,
    backoffRemainingMs: 0,
    date: '2026-10-26'
};

beforeEach(() => {
    vi.clearAllMocks();
    mocks.connection.mockResolvedValue(connected);
    mocks.usage.mockResolvedValue(usage);
    mocks.boards.mockResolvedValue({ board: 'maps', min: 5, maps: [], at: 1 });
    mocks.status.mockResolvedValue({ at: 1, state: 'ok' });
    mocks.recentNotifications.mockResolvedValue({ items: [], read: 0 });
    mocks.pollNotifications.mockResolvedValue({
        last: 1,
        items: [],
        every: 60,
        dot: 0
    });
    mocks.myMap.mockResolvedValue(null);
    mocks.mutuals.mockResolvedValue({ stars: [], n: 0 });
    mocks.classifyError.mockResolvedValue('other');
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
        expect(result.current.usage?.personalDailyLimit).toBe(300);
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
            can: true,
            need: null,
            words: null,
            wordsOk: null,
            img: null
        });
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));
        expect(result.current.myMap?.avg).toBe(9.2);
    });

    it('mutuals 失败不拖垮整页（敏感权限，dev- 必然失败）', async () => {
        mocks.mutuals.mockRejectedValue(new Error('HTTP 403 scope_unreviewed'));
        const { result } = renderHook(() => useVrccController());
        await waitFor(() => expect(result.current.loading).toBe(false));

        // 地图榜和连接状态仍然可用。
        expect(result.current.boards).not.toBeNull();
        expect(result.current.connection?.connected).toBe(true);
        // 错误有反馈，但不是致命错误。
        expect(mocks.toastAdd).toHaveBeenCalled();
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
