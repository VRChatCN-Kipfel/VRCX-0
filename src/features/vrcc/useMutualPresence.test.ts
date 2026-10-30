// @vitest-environment jsdom

import { renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { useFriendRosterStore } from '@/state/friendRosterStore';

import { useMutualPresence } from './useMutualPresence';

function onlinePresence() {
    return { kind: 'online' as const };
}

function offlinePresence() {
    return { kind: 'offline' as const };
}

function seedRoster(
    friends: Record<string, { kind: 'online' | 'offline' }>,
    loadStatus: 'idle' | 'ready' = 'ready'
) {
    useFriendRosterStore.setState({
        loadStatus,
        friendsById: Object.fromEntries(
            Object.entries(friends).map(([userId, presence]) => [
                userId,
                { $presence: presence }
            ])
        )
    } as never);
}

beforeEach(() => {
    useFriendRosterStore.setState({
        loadStatus: 'idle',
        friendsById: {}
    } as never);
});

describe('useMutualPresence', () => {
    it('在线的好友判为 online', () => {
        seedRoster({ usr_a: onlinePresence() });
        const { result } = renderHook(() => useMutualPresence());
        expect(result.current.of('usr_a')).toBe('online');
    });

    it('在好友表里但离线判为 friend', () => {
        seedRoster({ usr_a: offlinePresence() });
        const { result } = renderHook(() => useMutualPresence());
        expect(result.current.of('usr_a')).toBe('friend');
    });

    it('不在好友表里判为 unknown', () => {
        seedRoster({ usr_a: onlinePresence() });
        const { result } = renderHook(() => useMutualPresence());
        expect(result.current.of('usr_zzz')).toBe('unknown');
    });

    it('不是 usr_ 开头的 id 一律 unknown', () => {
        seedRoster({ usr_a: onlinePresence() });
        const { result } = renderHook(() => useMutualPresence());
        // 对方没公开验证时 `vrcUser` 是空的，这时不该去好友表里瞎猜。
        expect(result.current.of('')).toBe('unknown');
        expect(result.current.of(null)).toBe('unknown');
        expect(result.current.of(undefined)).toBe('unknown');
    });

    it('好友表没加载好时 ready 为 false', () => {
        seedRoster({}, 'idle');
        const { result } = renderHook(() => useMutualPresence());
        // 界面靠这个标志决定要不要显示三态标签：没加载好时显示
        // 「加好友」会让玩家以为满列表都不是好友。
        expect(result.current.ready).toBe(false);
    });

    it('好友表就绪时 ready 为 true', () => {
        seedRoster({}, 'ready');
        const { result } = renderHook(() => useMutualPresence());
        expect(result.current.ready).toBe(true);
    });
});
