import { publishNavNavigateRequested } from '@/shared/events/navLayoutEvents';
import { useVrcNotificationStore } from '@/state/vrcNotificationStore';

/**
 * 星遇同频页签的路由与页签名。
 *
 * 与 `navMenuModel.ts` 的 `routePathByName.vrcc`、`useVrccController` 的
 * `VrccTab` 是同一组值。这里不 import 那两个模块：它是后台动作，
 * 不该为了拼一个地址去依赖布局组件和页面控制器。
 */
const VRCC_ROUTE = '/vrcc';
const VRCC_NOTIFICATIONS_TAB = 'notifications';

/**
 * 跳到星遇同频的「提醒」页。
 *
 * 页签通过查询参数传递，而不是另开一个事件：跳转本来就会带上 search，
 * 页面自己读它即可，少一条跨模块的事件契约。
 *
 * 先收起通知中心抽屉：它是浮层，不随路由关闭，不收起来的话跳过去的页面
 * 会被它盖住——和「查看全部」跳表格页是同样的处理。
 */
export function openVrccNotificationCenter(): void {
    useVrcNotificationStore.getState().setCenterOpen(false);
    publishNavNavigateRequested(`${VRCC_ROUTE}?tab=${VRCC_NOTIFICATIONS_TAB}`);
}
