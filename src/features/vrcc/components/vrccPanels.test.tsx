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

/**
 * 好友表由每个用例自己决定。默认「就绪但空」——三态标签会显示「加好友」，
 * 和真实玩家没加过这些人的情况一致。
 */
const presenceState = {
    ready: true,
    online: new Set<string>()
};

vi.mock('../useMutualPresence', () => ({
    useMutualPresence: () => ({
        ready: presenceState.ready,
        of: (userId: string | null | undefined) => {
            const id = (userId ?? '').trim();
            if (!id) return 'unknown';
            return presenceState.online.has(id) ? 'online' : 'unknown';
        }
    })
}));

import { VrccCrossingsPanel } from './VrccCrossingsPanel';
import { VrccMapsPanel } from './VrccMapsPanel';
import { VrccMutualsPanel } from './VrccMutualsPanel';
import { VrccNotificationsPanel } from './VrccNotificationsPanel';

afterEach(() => {
    cleanup();
    presenceState.ready = true;
    presenceState.online.clear();
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

describe('VrccMutualsPanel', () => {
    function renderMutuals(stars: Array<Record<string, unknown>>) {
        render(
            <VrccMutualsPanel
                connected
                mutuals={{ stars } as never}
                onOpenUser={() => {}}
                onOpenGroup={() => {}}
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

    it('四个有专属色的星群各上各的色', () => {
        const { container } = render(
            <VrccMutualsPanel
                connected
                mutuals={
                    {
                        stars: [
                            { n: 1, name: 'G', group: 'G' },
                            { n: 2, name: 'W', group: 'W' },
                            { n: 3, name: 'Z', group: 'Z' },
                            { n: 4, name: 'L', group: 'L' }
                        ]
                    } as never
                }
                onOpenUser={() => {}}
                onOpenGroup={() => {}}
            />
        );
        const dots = container.querySelectorAll('li > span[aria-hidden]');
        expect(dots).toHaveLength(4);
        const colors = Array.from(dots).map(
            (dot) => (dot as HTMLElement).style.backgroundColor
        );
        expect(colors).toEqual([
            'rgb(77, 180, 255)',
            'rgb(255, 181, 71)',
            'rgb(214, 155, 255)',
            'rgb(95, 224, 180)'
        ]);
    });

    it('万象（A）没有专属色，退回前景色而不是硬造一个', () => {
        const { container } = render(
            <VrccMutualsPanel
                connected
                mutuals={{ stars: [{ n: 5, name: 'A', group: 'A' }] } as never}
                onOpenUser={() => {}}
                onOpenGroup={() => {}}
            />
        );
        const dot = container.querySelector(
            'li > span[aria-hidden]'
        ) as HTMLElement;
        // 官方颜色表里没有 A，官方也用文字色，所以这里不该有内联底色。
        expect(dot.style.backgroundColor).toBe('');
    });

    it('在线的好友排到最前面，其余保持服务端顺序', () => {
        presenceState.online.add('usr_second');
        const { container } = render(
            <VrccMutualsPanel
                connected
                mutuals={
                    {
                        stars: [
                            { n: 1, name: 'first', vrcUser: 'usr_first' },
                            { n: 2, name: 'second', vrcUser: 'usr_second' },
                            { n: 3, name: 'third', vrcUser: 'usr_third' }
                        ]
                    } as never
                }
                onOpenUser={() => {}}
                onOpenGroup={() => {}}
            />
        );
        const names = Array.from(container.querySelectorAll('li')).map(
            (row) => row.textContent ?? ''
        );
        // 在线的 #2 提到最前；#1 和 #3 都不是好友，保持原顺序。
        expect(names[0]).toContain('second');
        expect(names[1]).toContain('first');
        expect(names[2]).toContain('third');
    });

    it('同状态之间保持服务端顺序，即使是十几条的大列表', () => {
        // 三个元素时怎么排都像对的——V8 对短数组用插入排序，天然稳定，
        // 上面那条用例因此证不了什么。这里用 13 条、交错两种状态，让
        // 「同状态被重新排序」有机会真的暴露出来。
        const stars = Array.from({ length: 13 }, (_, index) => ({
            n: index + 1,
            name: `star-${index + 1}`,
            vrcUser: `usr_${index + 1}`
        }));
        // 下标 0/3/6/9/12 在线，其余离线，交错排列。
        for (const index of [0, 3, 6, 9, 12]) {
            presenceState.online.add(`usr_${index + 1}`);
        }

        const { container } = render(
            <VrccMutualsPanel
                connected
                mutuals={{ stars } as never}
                onOpenUser={() => {}}
                onOpenGroup={() => {}}
            />
        );
        const names = Array.from(container.querySelectorAll('li')).map(
            (row) => row.textContent ?? ''
        );

        // 期望：先把在线的按原顺序排出来，再是离线的按原顺序。
        const onlineNames = [1, 4, 7, 10, 13].map((n) => `star-${n}`);
        const offlineNames = [2, 3, 5, 6, 8, 9, 11, 12].map((n) => `star-${n}`);
        const expected = [...onlineNames, ...offlineNames];

        expected.forEach((name, position) => {
            expect(names[position]).toContain(name);
        });
    });

    it('好友表没就绪时不显示三态标签', () => {
        presenceState.ready = false;
        const { container } = render(
            <VrccMutualsPanel
                connected
                mutuals={
                    {
                        stars: [{ n: 1, name: 'a', vrcUser: 'usr_a' }]
                    } as never
                }
                onOpenUser={() => {}}
                onOpenGroup={() => {}}
            />
        );
        // 没就绪时所有人都是 unknown，显示「加好友」会误导玩家。
        expect(container.textContent).not.toContain('vrcc.mutuals.presence.');
    });
});
