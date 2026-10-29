// @vitest-environment jsdom

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', async (importOriginal) => {
    const actual = await importOriginal<typeof import('react-i18next')>();
    return {
        ...actual,
        useTranslation: () => ({ t: (key: string) => key })
    };
});

vi.mock('@/lib/dateTime', () => ({
    formatRelativeTime: () => 'now'
}));

import { VrccMapsPanel } from './VrccMapsPanel';
import { VrccMutualsPanel } from './VrccMutualsPanel';
import { VrccNotificationsPanel } from './VrccNotificationsPanel';

afterEach(() => {
    cleanup();
});

function renderNotifications(unread: {
    inbox?: number;
    cross?: number | null;
    chats?: number;
    notes?: number;
}) {
    render(
        <VrccNotificationsPanel
            connected
            recent={{ items: [], read: 0 }}
            unread={unread}
            view="recent"
            notes={null}
            notesSupported={null}
            onViewChange={() => {}}
            onConnect={() => {}}
            onOpenWorld={() => {}}
        />
    );
}

describe('VrccNotificationsPanel', () => {
    it('令牌没有 crossings 时不显示「星轨相交」，而不是显示 0', () => {
        renderNotifications({ inbox: 2, chats: 1, notes: 0 });
        expect(screen.getByText(/vrcc\.unread\.inbox/)).toBeTruthy();
        expect(screen.queryByText(/vrcc\.unread\.cross/)).toBeNull();
    });

    it('有 crossings 时照常显示，0 也显示', () => {
        renderNotifications({ inbox: 0, cross: 0, chats: 0, notes: 0 });
        expect(screen.getByText(/vrcc\.unread\.cross/)).toBeTruthy();
    });
});

describe('VrccMapsPanel', () => {
    const board = {
        board: 'maps',
        min: 1,
        at: 1,
        maps: [
            { world: 'wrld_a', name: 'A', rank: 7, week: 5 },
            { world: 'wrld_b', name: 'B', rank: 1, week: 2 }
        ]
    };

    function renderMaps(sort: 'top' | 'week') {
        render(
            <VrccMapsPanel
                boards={{ ...board, sort }}
                sort={sort}
                sortSupported
                onSortChange={() => {}}
                onRefresh={() => {}}
                onOpenWorld={() => {}}
            />
        );
    }

    it('本周榜按列表位置编号，不显示总榜名次', () => {
        // 本周榜只按本周的打分排，`rank` 还是总榜名次。
        renderMaps('week');
        expect(screen.queryByText('7')).toBeNull();
        expect(screen.getByText('2')).toBeTruthy();
    });

    it('总榜显示服务器给的名次', () => {
        renderMaps('top');
        expect(screen.getByText('7')).toBeTruthy();
    });
});

describe('VrccMutualsPanel', () => {
    function renderMutuals(stars: Array<Record<string, unknown>>) {
        render(
            <VrccMutualsPanel
                connected
                mutuals={{ stars } as never}
                onOpenUser={() => {}}
            />
        );
    }

    it('有连上时间时显示它', () => {
        // `formatRelativeTime` 在本文件顶部被 mock 成固定返回 'now'。
        renderMutuals([{ n: 1, name: 'A', at: 1_700_000_000_000 }]);
        expect(screen.getByText('now')).toBeTruthy();
    });

    it('没有连上时间时整块不渲染，而不是留一个占位符', () => {
        renderMutuals([{ n: 2, name: 'B' }]);
        // 留「—」会让玩家以为记录丢了，所以这里应该是彻底没有这个节点。
        expect(screen.queryByText('now')).toBeNull();
    });
});
