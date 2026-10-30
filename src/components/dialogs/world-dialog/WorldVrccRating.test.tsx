// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi, beforeEach } from 'vitest';

const appVrccMapPage = vi.fn();

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccMapPage: (...args: unknown[]) => appVrccMapPage(...args)
    }
}));

vi.mock('react-i18next', () => ({
    useTranslation: () => ({
        t: (key: string, options?: { count?: number }) => {
            // 把插值也拼出来，好断言人数文案确实带上了数字
            if (key === 'dialog.world.vrcc.raters') {
                return `${options?.count ?? 0} 人打分`;
            }
            return key;
        }
    })
}));

import { WorldVrccRating } from './WorldVrccRating';

function renderWithClient(ui: ReactNode) {
    const client = new QueryClient({
        defaultOptions: { queries: { retry: false } }
    });
    return render(
        <QueryClientProvider client={client}>{ui}</QueryClientProvider>
    );
}

/** 造一份够用的公开地图页响应。 */
function mapPage(overrides: Record<string, unknown> = {}) {
    return {
        map: {
            world: 'wrld_test',
            name: '测试世界',
            avg: 8.4,
            raters: 412,
            rank: 37,
            ...overrides
        },
        spread: [],
        went: 100,
        cap: 32,
        perf: null,
        at: 1
    };
}

describe('WorldVrccRating', () => {
    beforeEach(() => {
        appVrccMapPage.mockReset();
    });

    afterEach(() => {
        cleanup();
    });

    it('有评分时显示星级、均分与打分人数', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccRating worldId="wrld_test" />);

        expect(await screen.findByText('8.4')).toBeTruthy();
        expect(screen.getByText('412 人打分')).toBeTruthy();
    });

    it('名次不出现在概览里', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccRating worldId="wrld_test" />);

        await screen.findByText('8.4');
        // 37 是 rank，概览刻意不显示，免得作者之间攀比
        expect(screen.queryByText(/#?\s*37/)).toBeNull();
    });

    it('不在榜上时说「不在榜上」，不是空白也不是错误', async () => {
        // 后端把 404 not_found 转成 null
        appVrccMapPage.mockResolvedValue(null);
        renderWithClient(<WorldVrccRating worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.not_listed')
        ).toBeTruthy();
    });

    it('没人打分时不显示满分误导成 0.0', async () => {
        appVrccMapPage.mockResolvedValue(
            mapPage({ avg: null, raters: 0, rank: null })
        );
        renderWithClient(<WorldVrccRating worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.no_rating')
        ).toBeTruthy();
        expect(screen.queryByText('0.0')).toBeNull();
    });

    it('世界 ID 不是 wrld_ 时一个请求都不发', () => {
        renderWithClient(<WorldVrccRating worldId="" />);

        // enabled 为 false，queryFn 不该被调用
        expect(appVrccMapPage).not.toHaveBeenCalled();
    });

    it('来源声明始终在', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccRating worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.source')
        ).toBeTruthy();
    });
});
