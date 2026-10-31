/**
 * 星遇同频（vrcc.cc）前端数据层。
 *
 * 所有调用都走生成的绑定，不手写命令名。这一层只做两件事：
 * 把后端的错误收成统一的 `VrccError`，以及把「是否需要重新授权」这类
 * 判断交给后端（前端不解析错误文案）。
 *
 * 错误类型与归一化本身住在 `@/lib/vrccError`——`repositories/` 和
 * `components/` 也要用，而它们（按架构约束）不能引用 `@/features/**`。
 * 这里原样再导出一次，旧引用路径继续可用。
 */
export { VrccError, normalizeVrccError, isVrccError } from '@/lib/vrccError';
export type {
    VrccErrorKind,
    VrccErrorLayer,
    VrccTransportKind
} from '@/lib/vrccError';

import { normalizeVrccError } from '@/lib/vrccError';
import { commands } from '@/platform/tauri/bindings';

async function call<T>(run: () => Promise<T>): Promise<T> {
    try {
        return await run();
    } catch (error) {
        throw await normalizeVrccError(error);
    }
}

export const vrccApi = {
    connection: () => call(() => commands.appVrccConnection()),
    usage: () => call(() => commands.appVrccUsage()),

    /**
     * 地图榜。不需要连接。
     *
     * `sort` 是本周榜或新上榜、回答里却没有 `sort` 时，说明服务器还不支持
     * （回的是总榜），界面要藏起切换，不能把总榜当本周榜显示。
     */
    boards: (limit = 20, sort: VrccBoardSort = 'top') =>
        call(() => commands.appVrccBoards(limit, sort)),

    /**
     * 按世界 ID 批量查地图（`GET /maps?ids=`）。不需要连接。
     *
     * 一次最多 20 张，**只返回在榜上的**——结果条数可能少于传入的数量，
     * 用 `world` 去配，不要按下标对应。
     */
    maps: (worlds: string[]) => call(() => commands.appVrccMaps(worlds)),

    status: () => call(() => commands.appVrccStatus()),

    /** 公告和停站。不需要连接。服务器还没有这个接口时为 null。 */
    announcements: () => call(() => commands.appVrccAnnouncements()),

    /** 所有贴纸和材质名。不需要连接也不需要令牌。服务器还没有这个接口时为 null。 */
    stickers: () => call(() => commands.appVrccStickers()),

    /**
     * 我打过分的地图和短评。需要 `maps` 普通权限，最近的在前。
     *
     * 这是「我给它打了多少分」，不是那张图在榜上的均分——两者形状不同，
     * 不要混用同一条渲染路径。
     */
    myMaps: () => call(() => commands.appVrccMyMaps()),

    /**
     * 公开地图页：分数分布、去过的人数、房间上限、多小咕实测。
     *
     * 不需要连接。这张图不在榜上时服务器回 `404`，那**是正常状态**，
     * 后端把它转成 null，不是错误。
     */
    mapPage: (world: string) => call(() => commands.appVrccMapPage(world)),

    /**
     * 带令牌的地图页：**完整短评列表** + 我的评分 + 地图介绍。
     *
     * 需要 `maps` 普通权限（已有，不需要重新授权）。公开版按官方说明「短评和
     * 介绍不给」，想看别人的评价只能走这一条；未连接时会失败。
     *
     * `sort` 只认 `hot` / `new`，传 null 用服务器默认。
     * 不在榜上时（`404`）后端转成 null，不是错误。
     */
    myMapPage: (world: string, sort: 'hot' | 'new' | null = null) =>
        call(() => commands.appVrccMyMapPage(world, sort)),

    /**
     * 给地图打分 / 写短评。**需要 `maps.rate` 敏感权限。**
     *
     * ## 这是写操作
     *
     * 官方要求「由玩家本人每次主动点击触发」，所以这个包装里**不做任何重试、
     * 不做乐观更新、不批量合并**。失败就把错误原样抛给界面，由玩家决定要不
     * 要再点一次。
     *
     * ## `textMode` 的三种取值
     *
     * 官方对 `text` 有微妙的三态语义，用 `textMode` 显式表达，避免把
     * 「只改分数」误做成「删掉短评」：
     *
     * - `keep`  —— 只改分数，保留原短评
     * - `clear` —— 删掉短评，保留分数
     * - `write` —— 提交或更新短评（需同时给 `text`）
     *
     * 返回更新后的地图状态，可直接用来刷新界面。
     */
    rateMap: (
        world: string,
        score: number,
        textMode: 'keep' | 'clear' | 'write' = 'keep',
        text: string | null = null
    ) => call(() => commands.appVrccRateMap(world, score, textMode, text)),

    /** 删掉自己给这张图的评分。需要 `maps.rate` 敏感权限。 */
    deleteMapRating: (world: string) =>
        call(() => commands.appVrccDeleteMapRating(world)),

    /**
     * 玩家视角地图榜：比公开榜多「我去过几次、热评、名次、介绍首行」。
     *
     * 需要 `maps` 普通权限。`sort` 多一个 `went`（我去过的）。
     */
    myBoard: (sort: VrccMyBoardSort = 'top') =>
        call(() => commands.appVrccMyBoard(sort)),

    /**
     * 第一步设备授权：拿码。
     *
     * 要哪些权限由 Rust 侧按当前 `X-Client` 决定（`device_auth_scope`）：
     * `dev-` 只能要普通权限，正式标识再加上「连上」用的 `mutuals`。
     * 前端不再自己写一份，`scope` 只在确实要别的权限时才传。
     */
    beginDeviceAuth: (scope: string | null = null) =>
        call(() => commands.appVrccDeviceAuthBegin(scope)),

    /** 第二步：问一次令牌。返回 pending / connected / terminal。 */
    pollDeviceAuth: (
        prompt: Awaited<ReturnType<typeof commands.appVrccDeviceAuthBegin>>
    ) => call(() => commands.appVrccDeviceAuthPoll(prompt)),

    disconnect: () => call(() => commands.appVrccDisconnect()),

    /**
     * 最近的提醒。
     *
     * `mark` 传 true 表示玩家正在看这个列表，服务器会把已读同步到账号；
     * 后台只为算数字时传 false，否则等于替玩家把提醒标成看过了。
     */
    recentNotifications: (mark = false) =>
        call(() => commands.appVrccRecentNotifications(mark)),

    /**
     * 几处未读数（连向我、星轨相交、私聊、通知）。读了不标已读。
     *
     * `refresh` 为 false 时只读后台轮询的缓存、不发请求；收到后端「有变化」
     * 的事件时用它。服务器还没有 `me/unread` 时为 null。
     */
    unread: (refresh = true) => call(() => commands.appVrccUnread(refresh)),

    /** 通知中心。读了不标已读。服务器还没有 `me/notes` 时为 null。 */
    notes: () => call(() => commands.appVrccNotes()),

    /** 增量轮询。游标由后端维护。 */
    pollNotifications: () => call(() => commands.appVrccPollNotifications()),

    /**
     * 地图在榜上的状态与自己的评分。
     *
     * - 不传 `world`：问「我现在在的地图」，位置由后端从游戏日志取。
     * - 传 `world`：问那张具体的图。世界详情对话框走这条。
     *
     * 返回 `null` 的情况包括「这张图被下架或不对当前玩家开放」——官方要求
     * 此时隐藏整块，所以**不要**在 null 时再显示打分入口。
     *
     * 这是唯一能区分「未收录」和「待审核」的接口：公开的 `/maps/{world}/page`
     * 对这两种都回 `404`。
     */
    myMap: (world?: string) => call(() => commands.appVrccMyMap(world ?? null)),

    /** 今天这一轮。需要 today 敏感权限（审核后才可用）。 */
    today: () => call(() => commands.appVrccToday()),

    /**
     * 「互相连上」的星友列表。需要 mutuals 敏感权限（审核后才可用）。
     *
     * 条目带 `vrcUser` 时只能用来打开 VRCX-0 自己的玩家详情页——官方合规
     * 要求加好友由玩家在原生界面里自己点，禁止程序代发好友请求。
     */
    mutuals: () => call(() => commands.appVrccMutuals())
};

export type VrccBoard = Awaited<ReturnType<typeof commands.appVrccBoards>>;
/** 总榜 / 本周榜 / 新上榜（1.13.0 起）。 */
export type VrccBoardSort = NonNullable<
    Parameters<typeof commands.appVrccBoards>[1]
>;
/** 榜单里的一行。`maps` 在契约里是可选的，先把 undefined 排掉再取元素类型。 */
export type VrccBoardMap = NonNullable<VrccBoard['maps']>[number];
export type VrccConnection = Awaited<
    ReturnType<typeof commands.appVrccConnection>
>;
export type VrccUsage = Awaited<ReturnType<typeof commands.appVrccUsage>>;
export type VrccRecentNotifications = Awaited<
    ReturnType<typeof commands.appVrccRecentNotifications>
>;
export type VrccNotificationItem = NonNullable<
    VrccRecentNotifications['items']
>[number];
export type VrccDeviceAuthPrompt = Awaited<
    ReturnType<typeof commands.appVrccDeviceAuthBegin>
>;
export type VrccDeviceAuthStep = Awaited<
    ReturnType<typeof commands.appVrccDeviceAuthPoll>
>;
export type VrccStatus = Awaited<ReturnType<typeof commands.appVrccStatus>>;
export type VrccToday = Awaited<ReturnType<typeof commands.appVrccToday>>;
export type VrccMutuals = Awaited<ReturnType<typeof commands.appVrccMutuals>>;
export type VrccMutualStar = NonNullable<VrccMutuals['stars']>[number];
export type VrccMyMap = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccMyMap>>
>;
export type VrccUnread = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccUnread>>
>;
export type VrccNotes = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccNotes>>
>;
export type VrccNote = NonNullable<VrccNotes['notes']>[number];
export type VrccAnnouncements = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccAnnouncements>>
>;
export type VrccAnnouncement = NonNullable<VrccAnnouncements['posts']>[number];
export type VrccStickers = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccStickers>>
>;
export type VrccSticker = NonNullable<VrccStickers['stickers']>[string];
export type VrccRatedMaps = Awaited<ReturnType<typeof commands.appVrccMyMaps>>;
export type VrccRatedMap = NonNullable<VrccRatedMaps['maps']>[number];
export type VrccMapPage = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccMapPage>>
>;
export type VrccMyMapPage = NonNullable<
    Awaited<ReturnType<typeof commands.appVrccMyMapPage>>
>;
export type VrccMapRate = NonNullable<VrccMyMapPage['rates']>[number];
export type VrccMapRateWho = NonNullable<VrccMapRate['who']>;
export type VrccMyBoard = Awaited<ReturnType<typeof commands.appVrccMyBoard>>;
export type VrccMyBoardSort = NonNullable<
    Parameters<typeof commands.appVrccMyBoard>[0]
>;
export type VrccMyBoardMap = NonNullable<VrccMyBoard['maps']>[number];
