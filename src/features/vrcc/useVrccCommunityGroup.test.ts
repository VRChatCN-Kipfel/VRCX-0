// @vitest-environment jsdom

import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const getCachedQueryData = vi.fn();
const getGroupProfile = vi.fn();

vi.mock('@/lib/entityQueryCache', async (importOriginal) => {
    const actual =
        await importOriginal<typeof import('@/lib/entityQueryCache')>();
    return {
        ...actual,
        getCachedQueryData: (...args: unknown[]) => getCachedQueryData(...args)
    };
});

vi.mock('@/repositories/groupProfileRepository', () => ({
    default: {
        getGroupProfile: (...args: unknown[]) => getGroupProfile(...args)
    }
}));

import { queryKeys } from '@/lib/entityQueryCache';
import { VRCC_COMMUNITY_GROUP_ID } from '@/shared/constants/vrccCommunity';
import { DEFAULT_VRCHAT_API_ENDPOINT } from '@/shared/vrchatEndpoint';

import { useVrccCommunityGroup } from './useVrccCommunityGroup';

beforeEach(() => {
    getCachedQueryData.mockReset();
    getGroupProfile.mockReset();
});

afterEach(() => {
    vi.clearAllMocks();
});

describe('useVrccCommunityGroup', () => {
    it('缓存命中时直接用，一个请求都不发', async () => {
        getCachedQueryData.mockReturnValue({ membershipStatus: 'member' });

        const { result } = renderHook(() => useVrccCommunityGroup());

        await waitFor(() => {
            expect(result.current.joined).toBe(true);
        });
        // 这是这个 hook 存在的理由：命中缓存就不该有网络请求。
        expect(getGroupProfile).not.toHaveBeenCalled();
    });

    it('缓存用的是 includeRoles=true 且带 endpoint，和群组对话框同一个键', async () => {
        getCachedQueryData.mockReturnValue(undefined);
        getGroupProfile.mockResolvedValue({ membershipStatus: 'inactive' });

        renderHook(() => useVrccCommunityGroup());

        await waitFor(() => {
            expect(getGroupProfile).toHaveBeenCalled();
        });
        // 期望值直接由生产用的那个函数算出来，不写死数组。
        //
        // 写死过一次，结果漏掉了 endpoint 那一段：`getGroupProfile` 自己传的是
        // `DEFAULT_VRCHAT_API_ENDPOINT`，键因此比 ['group', id, true] 多一层，
        // 两边长度都不同——缓存永远命中不了，「零请求」是假的。把字面量抄进
        // 断言等于让测试跟着实现一起错。
        expect(getCachedQueryData).toHaveBeenCalledWith(
            queryKeys.group(
                VRCC_COMMUNITY_GROUP_ID,
                true,
                DEFAULT_VRCHAT_API_ENDPOINT
            )
        );
        expect(getGroupProfile).toHaveBeenCalledWith(
            expect.objectContaining({
                groupId: VRCC_COMMUNITY_GROUP_ID,
                includeRoles: true,
                dialog: true
            })
        );
    });

    it('membershipStatus 为 member 才算已加入', async () => {
        getCachedQueryData.mockReturnValue({ membershipStatus: ' member ' });

        const { result } = renderHook(() => useVrccCommunityGroup());

        await waitFor(() => {
            expect(result.current.joined).toBe(true);
        });
    });

    it('inactive（不在群）不算已加入', async () => {
        getCachedQueryData.mockReturnValue({ membershipStatus: 'inactive' });

        const { result } = renderHook(() => useVrccCommunityGroup());

        await waitFor(() => {
            expect(result.current.joined).toBe(false);
        });
    });

    it('userblocked（被拉黑）不算已加入', async () => {
        getCachedQueryData.mockReturnValue({ membershipStatus: 'userblocked' });

        const { result } = renderHook(() => useVrccCommunityGroup());

        await waitFor(() => {
            expect(result.current.joined).toBe(false);
        });
    });

    it('拉取失败时不误报已加入', async () => {
        getCachedQueryData.mockReturnValue(undefined);
        getGroupProfile.mockRejectedValue(new Error('offline'));

        const { result } = renderHook(() => useVrccCommunityGroup());

        await waitFor(() => {
            expect(result.current.loading).toBe(false);
        });
        // 一次网络失败不能变成「你已经加入了」——那是假消息。
        expect(result.current.joined).toBeNull();
    });
});
