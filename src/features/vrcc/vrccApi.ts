import { commands } from '@/platform/tauri/bindings';

/**
 * 星遇同频（vrcc.cc）前端数据层。
 *
 * 所有调用都走生成的绑定，不手写命令名。这一层只做两件事：
 * 把后端的错误收成统一的 `VrccError`，以及把「是否需要重新授权」这类
 * 判断交给后端（前端不解析错误文案）。
 */

/** 后端已经分好类的错误类别。 */
export type VrccErrorKind =
    | 'needsReauth'
    | 'missingScope'
    | 'throttled'
    | 'notReady'
    | 'transport'
    | 'other';

/**
 * 失败发生在哪一层。
 *
 * 这是「是网络问题还是业务问题」这个问题的答案，一个字段就够了：
 * - `input`：我们自己构造请求出了错，**不该显示给玩家**
 * - `client`：被本机限流拦下，请求没发出去（服务可能是好的）
 * - `transport`：请求发了但没拿到响应（DNS / 建连 / 超时 / 响应中断）
 * - `server`：拿到了响应，服务器拒绝
 * - `session`：令牌缺失或失效
 */
export type VrccErrorLayer =
    | 'input'
    | 'client'
    | 'transport'
    | 'server'
    | 'session';

/**
 * 传输层卡在哪一步。
 *
 * 判据来自 reqwest 的类型判断，不是对错误文案做字符串匹配。
 *
 * **`dns` 有盲区**：走 SOCKS5 代理时代理串是 `socks5h://`，域名由代理那边
 * 解析，本机根本不做 DNS 查询。那种情况下解析失败会表现为 `connect`。
 * 所以看到 `connect` 不能断言「不是 DNS 问题」。
 *
 * 另外 RST、端口拒绝、运营商封锁在系统层面表现一致，都是建连失败，
 * 这里无法进一步区分。
 */
export type VrccTransportKind =
    | 'dns'
    | 'connect'
    | 'timeout'
    | 'body'
    | 'other';

export class VrccError extends Error {
    readonly kind: VrccErrorKind;
    readonly layer: VrccErrorLayer;
    readonly scope: string | null;
    /** 传输层卡在哪一步；不是传输层错误时为 `null`。 */
    readonly transportKind: VrccTransportKind | null;
    /**
     * 发起这次调用的接口标识（`device_code` / `today` …）。
     *
     * 同一个错误码在不同接口下含义可能不同。典型例子：`429 too_many` 在
     * `device_code` 上是「按网络出口 IP 算的每天 30 个，换网络可立刻再用」，
     * 在别的接口上才是「你自己的次数用完了」。
     */
    readonly op: string | null;

    constructor(
        message: string,
        kind: VrccErrorKind,
        scope: string | null = null,
        layer: VrccErrorLayer = 'server',
        transportKind: VrccTransportKind | null = null,
        op: string | null = null
    ) {
        super(message);
        this.name = 'VrccError';
        this.kind = kind;
        this.layer = layer;
        this.scope = scope;
        this.transportKind = transportKind;
        this.op = op;
    }

    /** 我们自己的 bug：界面不该把它当成玩家可处理的问题展示。 */
    get isOurBug(): boolean {
        return this.layer === 'input';
    }

    /** 是不是网络层的问题。 */
    get isNetwork(): boolean {
        return this.layer === 'transport';
    }
}

/**
 * 把命令抛出的错误归一化。
 *
 * 分类结论**随错误一起从后端过来**（`AppErrorPayload.vrcc`），前端不再解析
 * 文案。早先这里要再走一次 IPC 对中文错误串做子串匹配，结果 `401 token`
 * 落进 `other`、缺权限的 `need` 在转字符串时丢失、`missingScope` 提示
 * 永远不显示——都源于「让消费方去猜分类」。
 */
async function normalizeError(error: unknown): Promise<VrccError> {
    const message = error instanceof Error ? error.message : String(error);
    const detail = readVrccDetail(error);
    return new VrccError(
        message,
        normalizeKind(detail?.kind),
        detail?.need ?? null,
        normalizeLayer(detail?.layer),
        normalizeTransportKind(detail?.transportKind),
        detail?.op ?? null
    );
}

/**
 * 后端给出的结构化细节。
 *
 * `null` 只意味着「这个错误不是星遇同频错误」——比如 IPC 本身失败、命令名
 * 对不上、或未来加了别的 `AppError` 变体。所有 `app__vrcc_*` 命令的错误都由
 * `AppError::Vrcc` 产生，所以只要是星遇同频错误，`kind` 和 `layer` 一定在。
 */
function readVrccDetail(error: unknown): {
    kind?: string;
    layer?: string;
    need?: string | null;
    transportKind?: string | null;
    op?: string | null;
} | null {
    if (!error || typeof error !== 'object') {
        return null;
    }
    const detail = (error as { vrcc?: unknown }).vrcc;
    if (!detail || typeof detail !== 'object') {
        return null;
    }
    return detail as {
        kind?: string;
        layer?: string;
        need?: string | null;
        transportKind?: string | null;
        op?: string | null;
    };
}

/** 只接受已知的六个类别，别的一律当 `other`，免得后端加了新类别前端崩。 */
function normalizeKind(kind: string | undefined): VrccErrorKind {
    switch (kind) {
        case 'needsReauth':
        case 'missingScope':
        case 'throttled':
        case 'notReady':
        case 'transport':
            return kind;
        default:
            return 'other';
    }
}

/**
 * 只接受已知的五个层级。
 *
 * 认不出的值一律当 `server`：这个值只影响提示措辞，而「服务器拒绝了」是最
 * 不会误导人的说法——说成网络问题会让玩家去排查一个没坏的网络。
 *
 * 这**不是**给「老后端」留的兼容：所有 `app__vrcc_*` 命令的错误都由
 * `AppError::Vrcc` 产生，`layer` 是必填字段，不存在不带它的后端。
 *
 * 生产端（`vrcc_layer_name`）是穷举 `match`，加层级会直接编译失败，所以这里
 * 认不出只可能是 bindings 与源码不同步。那是构建问题，不该在运行时「处理」，
 * 但要让它响一声，别静默吸收。
 */
function normalizeLayer(layer: string | undefined): VrccErrorLayer {
    switch (layer) {
        case 'input':
        case 'client':
        case 'transport':
        case 'server':
        case 'session':
            return layer;
        default:
            if (import.meta.env.DEV && layer !== undefined) {
                console.warn(
                    '[vrcc] 认不出的错误层级，bindings 可能未同步',
                    layer
                );
            }
            return 'server';
    }
}

/** 只接受已知的五种传输环节。 */
function normalizeTransportKind(
    kind: string | null | undefined
): VrccTransportKind | null {
    switch (kind) {
        case 'dns':
        case 'connect':
        case 'timeout':
        case 'body':
        case 'other':
            return kind;
        default:
            return null;
    }
}

async function call<T>(run: () => Promise<T>): Promise<T> {
    try {
        return await run();
    } catch (error) {
        throw await normalizeError(error);
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
     * 当前所在地图在榜上的状态与自己的评分。
     *
     * 地图被下架或不对当前玩家开放时返回 null，此时**不要**显示打分入口。
     */
    myMap: () => call(() => commands.appVrccMyMap()),

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
