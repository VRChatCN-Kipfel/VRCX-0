import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
    loadLocaleMessages: vi.fn()
}));

vi.mock('@/localization/index', async (importOriginal) => ({
    ...(await importOriginal<typeof import('@/localization/index')>()),
    loadLocaleMessages: mocks.loadLocaleMessages
}));

import i18n, { getTimeUnitLabels, setI18nLanguage } from './i18nService';

function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((done) => {
        resolve = done;
    });
    return { promise, resolve };
}

describe('i18nService', () => {
    beforeEach(async () => {
        mocks.loadLocaleMessages.mockReset();
        await setI18nLanguage('en');
    });

    it('keeps the newest language when an older locale finishes loading later', async () => {
        const jaLoad = deferred<Record<string, unknown>>();
        mocks.loadLocaleMessages.mockImplementation((code: string) =>
            code === 'ja'
                ? jaLoad.promise
                : Promise.resolve({ common: { time_units: { h: '시간' } } })
        );

        const staleSwitch = setI18nLanguage('ja');
        await setI18nLanguage('ko');
        jaLoad.resolve({ common: { time_units: { h: '時間' } } });
        await staleSwitch;

        expect(i18n.language).toBe('ko');
        expect(getTimeUnitLabels().h).toBe('시간');
    });

    it('locale 文件里必须用单层花括号，否则玩家看到的是 {{name}} 原文', async () => {
        // 本项目把插值定成了 `prefix: '{'` / `suffix: '}'`（见 i18nService.ts）。
        // 这是 i18next 的默认写法，所以从别处抄来的 `{{count}}` 不会被替换，
        // 而是**原样显示**成 `{{count}} crossings`——不报错，只是文案难看，
        // 而且只有切到那个语言才看得见，很容易漏。
        //
        // 星轨相交那批文案最早就是 `{{count}}` / `{{left}}`；主题更新提示也
        // 中过同一个招。一次扫全部语言包，别再靠肉眼。
        const bad: string[] = [];
        const walk = (node: unknown, path: string) => {
            if (typeof node === 'string') {
                if (/\{\{[^}]+\}\}/.test(node)) bad.push(`${path}: ${node}`);
                return;
            }
            if (node && typeof node === 'object') {
                for (const [key, value] of Object.entries(node)) {
                    walk(value, path ? `${path}.${key}` : key);
                }
            }
        };

        const modules = import.meta.glob('@/localization/*.json', {
            eager: true
        }) as Record<string, { default: unknown }>;
        expect(Object.keys(modules).length).toBeGreaterThan(5);
        for (const [file, module] of Object.entries(modules)) {
            walk(module.default, file.replace(/^.*\//, ''));
        }

        expect(bad).toEqual([]);
    });
});
