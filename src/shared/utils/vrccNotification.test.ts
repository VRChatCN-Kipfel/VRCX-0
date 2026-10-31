import { describe, expect, it } from 'vitest';

import {
    isVrccNotification,
    isVrccNotificationLink,
    VRCC_NOTIFICATION_TYPE
} from './vrccNotification';

describe('vrccNotification', () => {
    /**
     * 类型串是跨语言契约：Rust 侧按它写行，这里按它认行。
     *
     * 这条钉的就是那件事——Rust 侧改名而这里没跟上时，编译和类型检查都不报错，
     * 只会表现为「通知中心里出现了几条认不出来的条目」。
     */
    it('keeps the type string the backend writes', () => {
        expect(VRCC_NOTIFICATION_TYPE).toBe('vrcc.notice');
    });

    it('recognises the link scheme the backend writes', () => {
        expect(isVrccNotificationLink('vrcc:notifications')).toBe(true);
        expect(isVrccNotificationLink('  vrcc:notifications  ')).toBe(true);

        // 外部地址不能被当成站内跳转，否则点了会什么都不发生。
        expect(isVrccNotificationLink('https://vrcc.cc/match/')).toBe(false);
        expect(
            isVrccNotificationLink(
                'user:usr_00000000-0000-0000-0000-000000000000'
            )
        ).toBe(false);
        expect(isVrccNotificationLink('')).toBe(false);
        expect(isVrccNotificationLink(null)).toBe(false);
    });

    it('recognises a row by its type', () => {
        expect(isVrccNotification({ type: 'vrcc.notice' })).toBe(true);
        expect(isVrccNotification({ type: 'invite' })).toBe(false);
        expect(isVrccNotification(null)).toBe(false);
        expect(isVrccNotification(undefined)).toBe(false);
    });
});
