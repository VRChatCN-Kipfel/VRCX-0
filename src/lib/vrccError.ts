/**
 * 星遇同频错误的归一化。
 *
 * ## 为什么单独一个模块
 *
 * 这套东西原先只存在于 `features/vrcc/vrccApi.ts` 里。但 `src/components/**`
 * 按架构约束**不能引用 `@/features/**`**，于是凡是「组件直接读数据」的路径
 * （世界详情页走的是 `repositories/`）都拿不到分类，只能把一切失败当成同一个
 * 「出错了」——而错误其实分得很细：令牌失效要重新连接、缺权限要勾同意、
 * 传输失败要查网络。
 *
 * 所以把这些类型与归一化下沉到 `lib`（最低的共享层），`features` 和
 * `repositories` 都能用，`vrccApi` 再原样导出一遍保持旧引用可用。
 *
 * ## 分类结论是后端给的
 *
 * 前端**不解析错误文案**。`kind` / `layer` / `need` 随 `AppErrorPayload.vrcc`
 * 从后端过来。早先这里要再走一次 IPC 对中文错误串做子串匹配，结果
 * `401 token` 落进 `other`、缺权限的 `need` 在转字符串时丢失——都源于
 * 「让消费方去猜分类」。
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

    /** 需要玩家重新授权。 */
    get needsReauth(): boolean {
        return this.kind === 'needsReauth' || this.kind === 'missingScope';
    }

    /** 网络层的问题：让玩家去查网络，而不是怪服务器。 */
    get isNetwork(): boolean {
        return this.layer === 'transport';
    }
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

/**
 * 把命令抛出的错误归一化成 [`VrccError`]。
 *
 * 任何调用 `commands.app__vrcc_*` 的地方都应该经过它，否则拿到的是原始
 * `AppError`，里面那些分类字段就浪费了。
 */
export async function normalizeVrccError(error: unknown): Promise<VrccError> {
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

/** 这个错误是不是星遇同频错误（带结构化细节）。 */
export function isVrccError(error: unknown): error is VrccError {
    return error instanceof VrccError;
}
