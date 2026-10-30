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

import { VrccCrossingsPanel } from './VrccCrossingsPanel';
import { VrccMapsPanel } from './VrccMapsPanel';
import { VrccNotificationsPanel } from './VrccNotificationsPanel';

afterEach(() => {
    cleanup();
});

function renderCrossings(crossings: { why?: string | null; hidden?: boolean }) {
    const { container } = render(
        <VrccCrossingsPanel
            connected
            crossings={{
                stars: [],
                n: 0,
                why: null,
                hidden: false,
                me: null,
                ...crossings
            }}
            cards={null}
            crossingsError={null}
            onOpenLink={() => {}}
            onRefresh={() => {}}
        />
    );
    return container;
}

describe('VrccCrossingsPanel', () => {
    it('hidden 时整块都不渲染，连「不可用」也不说', () => {
        // 官方要求 `hide` 时客户端不显示此模块。把 `hide` 当成普通空列表，
        // 等于向不该看到的人确认「这里有星轨相交」这个事实本身——所以这里
        // 断言的是**一片空白**，不是某段文案。
        const container = renderCrossings({ why: 'hide', hidden: true });
        expect(container.innerHTML).toBe('');
    });

    it('其余 why 要显示原因，而不是把功能藏掉', () => {
        // `verify` / `friend` 这些是「功能在但暂时用不了」，玩家需要知道
        // 下一步做什么。藏起来会让人以为功能消失了。
        renderCrossings({ why: 'verify', hidden: false });
        expect(screen.getByText('vrcc.crossings.why.verify')).toBeTruthy();
    });

    it('服务器给没见过的 why 时退回 unknown，不拼键名', () => {
        // 原来的写法是 `t(`vrcc.crossings.why.${why}`)`——`why` 是服务器给的
        // 字符串，拼出来可能是 `constructor` / `__proto__` 这种本来就存在的
        // 对象属性，拿到的不是文案而是对象或 undefined，界面直接炸。
        // 现在只有白名单里的才拼键名，其余一律 unknown。
        renderCrossings({ why: 'brand-new-reason', hidden: false });
        expect(screen.getByText('vrcc.crossings.why.unknown')).toBeTruthy();
    });

    it('没连接时提示先去连接', () => {
        render(
            <VrccCrossingsPanel
                connected={false}
                crossings={null}
                cards={null}
                crossingsError={null}
                onOpenLink={() => {}}
                onRefresh={() => {}}
            />
        );
        expect(screen.getByText('vrcc.crossings.need_connect')).toBeTruthy();
    });
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
