// @vitest-environment jsdom

import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const appWorldSummariesGet = vi.fn();
const appWorldGet = vi.fn();

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appWorldSummariesGet: (...args: unknown[]) =>
            appWorldSummariesGet(...args),
        appWorldGet: (...args: unknown[]) => appWorldGet(...args)
    }
}));

import { useVrccWorldCovers } from '@/features/vrcc/useVrccWorldCovers';

import {
    prefetchVrccWorldCovers,
    readVrccWorldCovers
} from './vrccMapRepository';

beforeEach(() => {
    appWorldSummariesGet.mockReset();
    appWorldGet.mockReset();
    appWorldSummariesGet.mockResolvedValue({});
    appWorldGet.mockResolvedValue({});
});

afterEach(() => {
    cleanup();
});

describe('readVrccWorldCovers', () => {
    it('优先用缩略图，没有才用原图', async () => {
        appWorldSummariesGet.mockResolvedValue({
            wrld_a: { thumbnailImageUrl: 'thumb-a', imageUrl: 'full-a' },
            wrld_b: { thumbnailImageUrl: '', imageUrl: 'full-b' },
            wrld_c: { thumbnailImageUrl: '', imageUrl: '' }
        });

        const covers = await readVrccWorldCovers([
            'wrld_a',
            'wrld_b',
            'wrld_c'
        ]);

        expect(covers).toEqual({ wrld_a: 'thumb-a', wrld_b: 'full-b' });
    });

    it('读缓存失败时返回空表，不让榜单显示不出来', async () => {
        appWorldSummariesGet.mockRejectedValue(new Error('db locked'));
        await expect(readVrccWorldCovers(['wrld_a'])).resolves.toEqual({});
    });

    it('空输入不发请求', async () => {
        await readVrccWorldCovers([]);
        expect(appWorldSummariesGet).not.toHaveBeenCalled();
    });
});

describe('prefetchVrccWorldCovers', () => {
    it('跳过已缓存的、非 wrld_ 的和重复的', async () => {
        const stored = await prefetchVrccWorldCovers(
            ['wrld_a', 'wrld_a', 'wrld_b', 'not-a-world', ''],
            { known: { wrld_a: 'thumb-a' } }
        );

        // 只剩 wrld_b 需要拉
        expect(stored).toBe(1);
        expect(appWorldGet).toHaveBeenCalledTimes(1);
        expect(appWorldGet).toHaveBeenCalledWith(
            expect.objectContaining({ worldId: 'wrld_b' })
        );
    });

    it('单个世界失败不影响其它的', async () => {
        appWorldGet.mockImplementation((input: { worldId: string }) =>
            input.worldId === 'wrld_bad'
                ? Promise.reject(new Error('gone'))
                : Promise.resolve({})
        );

        const stored = await prefetchVrccWorldCovers([
            'wrld_bad',
            'wrld_ok',
            'wrld_ok2'
        ]);

        expect(stored).toBe(2);
        expect(appWorldGet).toHaveBeenCalledTimes(3);
    });

    it('并发不超过上限', async () => {
        let inFlight = 0;
        let peak = 0;
        appWorldGet.mockImplementation(async () => {
            inFlight += 1;
            peak = Math.max(peak, inFlight);
            await new Promise((resolve) => setTimeout(resolve, 1));
            inFlight -= 1;
            return {};
        });

        await prefetchVrccWorldCovers(
            ['wrld_1', 'wrld_2', 'wrld_3', 'wrld_4', 'wrld_5', 'wrld_6'],
            { concurrency: 2 }
        );

        expect(peak).toBeLessThanOrEqual(2);
    });

    it('取消信号能提前收手', async () => {
        const controller = new AbortController();
        appWorldGet.mockImplementation(async () => {
            controller.abort();
            return {};
        });

        const stored = await prefetchVrccWorldCovers(
            ['wrld_1', 'wrld_2', 'wrld_3', 'wrld_4'],
            { concurrency: 1, signal: controller.signal }
        );

        // 第一个跑完之后发现已取消，剩下的不再发
        expect(stored).toBe(1);
        expect(appWorldGet).toHaveBeenCalledTimes(1);
    });
});

describe('useVrccWorldCovers', () => {
    it('缓存命中的世界立刻出图', async () => {
        appWorldSummariesGet.mockResolvedValue({
            wrld_a: { thumbnailImageUrl: 'thumb-a', imageUrl: 'full-a' }
        });

        const { result } = renderHook(() =>
            useVrccWorldCovers(['wrld_a'], new Set(['wrld_a']))
        );

        await waitFor(() => expect(result.current.wrld_a).toBe('thumb-a'));
    });

    it('不可视的世界优先拉，可视的排在后面', async () => {
        appWorldSummariesGet.mockResolvedValue({});
        const order: string[] = [];
        appWorldGet.mockImplementation(async (input: { worldId: string }) => {
            order.push(input.worldId);
            return {};
        });

        const { result } = renderHook(() =>
            useVrccWorldCovers(
                ['wrld_visible', 'wrld_offscreen'],
                new Set(['wrld_visible'])
            )
        );

        await waitFor(() => expect(order.length).toBe(2), { timeout: 3000 });
        // 不可视的先拉：等玩家滚下去时它已经有原图了
        expect(order[0]).toBe('wrld_offscreen');
        expect(result.current).toBeTruthy();
    });
});
