import { commands } from '@/platform/tauri/bindings';
import { tauriClient } from '@/platform/tauri/client';
import { isGroupId, isUserId } from '@/shared/constants/vrchatIds';
import { publishNavNavigateRequested } from '@/shared/events/navLayoutEvents';

import {
    openGroupDialog,
    openUserDialog,
    openWorldDialog
} from './dialogService';

const DESKTOP_NOTIFICATION_ACTIVATED_EVENT = 'desktopNotificationActivated';

/**
 * 星遇同频页签的路由。
 *
 * 和 `navMenuModel.ts` 里 `routePathByName.vrcc` 是同一个值。这里不 import
 * 那个模块：它属于 layout 组件层，一个后台通知服务不该依赖 UI 模块。
 */
const VRCC_ROUTE = '/vrcc';

export async function bindDesktopNotificationActivationEvents(): Promise<
    () => void
> {
    return tauriClient.events.subscribe(
        DESKTOP_NOTIFICATION_ACTIVATED_EVENT,
        () => {
            takePendingDesktopNotificationActivation().catch(
                logDesktopNotificationActivationFailure
            );
        }
    );
}

export async function takePendingDesktopNotificationActivation(): Promise<void> {
    const activation =
        await commands.appTakePendingDesktopNotificationActivation();
    if (!activation) {
        return;
    }
    if (activation.target.kind === 'openUserProfile') {
        if (!isUserId(activation.target.userId)) {
            console.warn(
                'Ignored desktop notification activation with invalid user id:',
                activation.target.userId
            );
            return;
        }
        openUserDialog({ userId: activation.target.userId });
        return;
    }
    if (activation.target.kind === 'openLocation') {
        openWorldDialog({ worldId: activation.target.location });
        return;
    }
    if (activation.target.kind === 'openVrcc') {
        // 星遇同频的提醒没有 VRChat 的用户/世界 ID，只能回到页签。
        // `notice_id` 暂时只用于将来定位到具体一条，现在不参与导航。
        publishNavNavigateRequested(VRCC_ROUTE);
        return;
    }
    if (!isGroupId(activation.target.groupId)) {
        console.warn(
            'Ignored desktop notification activation with invalid group id:',
            activation.target.groupId
        );
        return;
    }
    openGroupDialog({ groupId: activation.target.groupId });
}

function logDesktopNotificationActivationFailure(error: unknown): void {
    console.warn(
        'Failed to take pending desktop notification activation:',
        error
    );
}
