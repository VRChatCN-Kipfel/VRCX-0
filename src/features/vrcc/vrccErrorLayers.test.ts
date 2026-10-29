// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * 错误分层的前端行为。
 *
 * 这里只钉住两件事，都是玩家能直接感知的：
 *
 * 1. **归一化**：后端的 `layer` / `transportKind` / `op` 要原样落到 `VrccError`
 *    上；认不出的值落到安全的 `server`，不能落到 `transport`。
 * 2. **文案**：网络层失败要给一句人话，而不是把
 *    `error sending request for url (…)` 这种英文技术串丢给玩家。
 *
 * 注意这里**不测「老后端兼容」**：所有 `app__vrcc_*` 命令的错误都由
 * `AppError::Vrcc` 产生，`layer` 是必填字段。不存在的兼容路径不该有测试去
 * 假装它存在。
 */

const mocks = vi.hoisted(() => ({
    connection: vi.fn(),
    usage: vi.fn()
}));

vi.mock('@/services/toastService', () => ({ toast: { add: vi.fn() } }));
vi.mock('@/services/dialogService', () => ({
    openWorldDialog: vi.fn(),
    openUserDialog: vi.fn()
}));
vi.mock('@/platform/tauri/client', () => ({
    tauriClient: {
        subscribe: vi.fn(async () => () => {}),
        on: vi.fn(async () => () => {})
    }
}));
vi.mock('@/services/i18nService', () => ({
    default: {
        t: (key: string) => key,
        language: 'zh-CN'
    }
}));
vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccConnection: mocks.connection,
        appVrccUsage: mocks.usage
    }
}));

describe('vrcc 错误分层', () => {
    beforeEach(() => {
        vi.resetModules();
        mocks.connection.mockReset();
        mocks.usage.mockReset();
    });

    async function normalize(vrcc: unknown) {
        const { vrccApi, VrccError } = await import('./vrccApi');
        mocks.connection.mockRejectedValue(
            Object.assign(new Error('boom'), { vrcc })
        );
        mocks.usage.mockResolvedValue({});
        try {
            await vrccApi.connection();
        } catch (error) {
            return error as InstanceType<typeof VrccError>;
        }
        throw new Error('应当抛出');
    }

    it('把 layer、transportKind、op 原样带出来', async () => {
        const error = await normalize({
            kind: 'transport',
            layer: 'transport',
            need: null,
            transportKind: 'dns',
            op: 'boards'
        });
        expect(error.layer).toBe('transport');
        expect(error.transportKind).toBe('dns');
        expect(error.op).toBe('boards');
        expect(error.isNetwork).toBe(true);
        expect(error.isOurBug).toBe(false);
    });

    it('认不出的 layer 落到 server，而不是 transport', async () => {
        // 说成 `transport` 会把「服务器拒绝」误报成「你网络有问题」，
        // 玩家就会去排查一个根本没坏的网络。
        const error = await normalize({
            kind: 'other',
            layer: 'some-future-layer',
            need: null
        });
        expect(error.layer).toBe('server');
    });

    it('认不出的 transportKind 落到 null', async () => {
        const error = await normalize({
            kind: 'other',
            layer: 'some-future-layer',
            transportKind: 'quantum'
        });
        expect(error.layer).toBe('server');
        expect(error.transportKind).toBeNull();
    });

    it('input 层被标成我们自己的 bug', async () => {
        const error = await normalize({
            kind: 'other',
            layer: 'input',
            need: null
        });
        expect(error.isOurBug).toBe(true);
        expect(error.isNetwork).toBe(false);
    });
});

describe('网络层文案', () => {
    beforeEach(() => {
        vi.resetModules();
    });

    it('按 transportKind 出不同的文案，而不是用后端原文', async () => {
        const { describeVrccError } = await import('./useVrccController');
        const { VrccError } = await import('./vrccApi');

        const build = (
            transportKind: 'dns' | 'connect' | 'timeout' | 'body' | 'other'
        ) =>
            new VrccError(
                'error sending request for url (https://vrcc.cc/api/v1/status)',
                'transport',
                null,
                'transport',
                transportKind,
                'status'
            );

        // i18n 被 mock 成返回 key，所以这里断言的是「走了哪个 key」。
        expect(describeVrccError(build('dns'))).toBe('vrcc.error.network.dns');
        expect(describeVrccError(build('connect'))).toBe(
            'vrcc.error.network.connect'
        );
        expect(describeVrccError(build('timeout'))).toBe(
            'vrcc.error.network.timeout'
        );
        expect(describeVrccError(build('body'))).toBe(
            'vrcc.error.network.generic'
        );
        // 任何情况下都不该把后端英文原文丢给玩家。
        expect(describeVrccError(build('dns'))).not.toContain('error sending');
    });

    it('input 层只说操作失败，细节留给日志', async () => {
        const { describeVrccError } = await import('./useVrccController');
        const { VrccError } = await import('./vrccApi');

        const error = new VrccError(
            '请求正文超过 2048 字节上限',
            'other',
            null,
            'input'
        );
        expect(describeVrccError(error)).toBe('vrcc.error.unknown');
    });

    it('server 层沿用后端的描述', async () => {
        const { describeVrccError } = await import('./useVrccController');
        const { VrccError } = await import('./vrccApi');

        const error = new VrccError(
            '星遇同频接口返回 HTTP 429（too_many）',
            'throttled',
            null,
            'server',
            null,
            'boards'
        );
        expect(describeVrccError(error)).toContain('429');
    });
});
