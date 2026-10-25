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
    | 'transport'
    | 'other';

export class VrccError extends Error {
    readonly kind: VrccErrorKind;
    readonly scope: string | null;

    constructor(
        message: string,
        kind: VrccErrorKind,
        scope: string | null = null
    ) {
        super(message);
        this.name = 'VrccError';
        this.kind = kind;
        this.scope = scope;
    }
}

/**
 * 把命令抛出的错误归一化。
 *
 * 后端返回的是 `AppError`（普通字符串），分类靠 `appVrccClassifyError`——
 * 这样「要不要显示重新连接」的判断只有一个来源。
 */
async function normalizeError(error: unknown): Promise<VrccError> {
    const message = error instanceof Error ? error.message : String(error);
    try {
        const kind = await commands.appVrccClassifyError(message);
        return new VrccError(message, kind);
    } catch {
        return new VrccError(message, 'other');
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

    /** 地图榜。不需要连接。 */
    boards: (limit = 20) => call(() => commands.appVrccBoards(limit)),

    status: () => call(() => commands.appVrccStatus()),

    /** 第一步设备授权：拿码。`scope` 传 null 用官方默认。 */
    beginDeviceAuth: (scope: string | null = null) =>
        call(() => commands.appVrccDeviceAuthBegin(scope)),

    /** 第二步：问一次令牌。返回 pending / connected / terminal。 */
    pollDeviceAuth: (
        prompt: Awaited<ReturnType<typeof commands.appVrccDeviceAuthBegin>>
    ) => call(() => commands.appVrccDeviceAuthPoll(prompt)),

    disconnect: () => call(() => commands.appVrccDisconnect()),

    recentNotifications: () =>
        call(() => commands.appVrccRecentNotifications()),

    /** 增量轮询。游标由后端维护。 */
    pollNotifications: () => call(() => commands.appVrccPollNotifications()),

    /** 今天这一轮。需要 today 敏感权限（审核后才可用）。 */
    today: () => call(() => commands.appVrccToday())
};

export type VrccBoard = Awaited<ReturnType<typeof commands.appVrccBoards>>;
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
