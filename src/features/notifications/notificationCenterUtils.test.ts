// @vitest-environment jsdom

import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { NotificationRow } from '@/repositories/notificationPersistenceRepository';
import { NAV_NAVIGATE_REQUESTED_EVENT } from '@/shared/events/navLayoutEvents';
import { VRCC_NOTIFICATION_TYPE } from '@/shared/utils/vrccNotification';
import { useVrcNotificationStore } from '@/state/vrcNotificationStore';

import { canDeclineNotification, openSender } from './notificationCenterUtils';

const t = ((key: string) => key) as TFunction;

function vrccRow(): NotificationRow {
    return {
        id: 'vrcc_7',
        version: 2,
        type: VRCC_NOTIFICATION_TYPE,
        link: 'vrcc:notifications',
        linkText: '',
        title: '星遇同频',
        message: '有人想和你连上',
        senderUserId: '',
        senderUsername: '星遇同频',
        seen: false,
        expired: false
    };
}

function captureNavigateRequests() {
    const targets: string[] = [];
    const listener = (event: Event) => {
        targets.push(String((event as CustomEvent<unknown>).detail));
    };
    window.addEventListener(NAV_NAVIGATE_REQUESTED_EVENT, listener);
    return {
        targets,
        stop: () =>
            window.removeEventListener(NAV_NAVIGATE_REQUESTED_EVENT, listener)
    };
}

describe('openSender', () => {
    /**
     * 站外提醒的行没有 VRChat 发送者可开，落点在 `link` 上：点了要跳到
     * 星遇同频自己的「提醒」页，而不是浏览器或一个空的玩家资料。
     */
    it('jumps to the vrcc notifications tab for an out-of-app notice', () => {
        const navigate = captureNavigateRequests();
        useVrcNotificationStore.setState({ isCenterOpen: true });
        try {
            openSender(vrccRow(), t);

            expect(navigate.targets).toEqual(['/vrcc?tab=notifications']);
            // 抽屉是浮层，不随路由关闭；不收起来跳过去的页面会被它盖住。
            expect(useVrcNotificationStore.getState().isCenterOpen).toBe(false);
        } finally {
            navigate.stop();
        }
    });
});

describe('canDeclineNotification', () => {
    /**
     * 「拒绝」走的是 VRChat 的隐藏接口。站外提醒的 id 在那边不存在，
     * 给这一行挂上按钮等于提供一个点了必然失败的入口。
     */
    it('is not offered for an out-of-app notice', () => {
        expect(canDeclineNotification(vrccRow())).toBe(false);
    });

    it('is still offered for an ordinary notice', () => {
        expect(
            canDeclineNotification({
                id: 'notif_1',
                type: 'invite',
                link: 'user:usr_1'
            })
        ).toBe(true);
    });
});
