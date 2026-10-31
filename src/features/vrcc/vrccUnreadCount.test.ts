import { describe, expect, it } from 'vitest';

import { countVrccUnread } from './vrccUnreadCount';

describe('countVrccUnread', () => {
    it('把四处未读数加总', () => {
        expect(
            countVrccUnread({ inbox: 1, cross: 2, chats: 3, notes: 4 }, null)
        ).toBe(10);
    });

    it('缺字段按 0 算，不因 undefined 变成 NaN', () => {
        // `cross` 只在令牌有 crossings 权限时才有，旧账号上它就是 undefined。
        expect(countVrccUnread({ inbox: 2, chats: 1 }, null)).toBe(3);
        expect(
            countVrccUnread({ inbox: 0, cross: 0, chats: 0, notes: 0 }, null)
        ).toBe(0);
    });

    it('没有 me/unread 时退回 recent 里 id > read 的条数', () => {
        // 1.13.0 之前没有 me/unread，只能从最近的提醒里数。
        expect(
            countVrccUnread(null, {
                items: [{ id: 3 }, { id: 2 }, { id: 1 }],
                read: 1
            })
        ).toBe(2);
    });

    it('read 缺失时把所有提醒都算未读', () => {
        expect(countVrccUnread(null, { items: [{ id: 7 }, { id: 8 }] })).toBe(
            2
        );
    });

    it('两边都没有时是 0，而不是报错', () => {
        expect(countVrccUnread(null, null)).toBe(0);
    });

    it('有 me/unread 时不看 recent，即使 recent 里还有更新的条目', () => {
        // 以服务端的 unread 为准：recent 只是 24 小时内的窗口，数它会少算。
        expect(
            countVrccUnread(
                { inbox: 1, cross: 0, chats: 0, notes: 0 },
                {
                    items: [{ id: 99 }],
                    read: 0
                }
            )
        ).toBe(1);
    });
});
