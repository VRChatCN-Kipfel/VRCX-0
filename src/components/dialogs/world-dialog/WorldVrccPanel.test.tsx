// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const appVrccMapPage = vi.fn();

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccMapPage: (...args: unknown[]) => appVrccMapPage(...args)
    }
}));

vi.mock('react-i18next', () => ({
    useTranslation: () => ({
        // 模仿真实语言包：复数键有 `_one` / `_other`，插值是**单**花括号。
        t: (key: string, options?: Record<string, unknown>) => {
            const count = options?.count;
            const rank = options?.rank;
            if (key === 'dialog.world.vrcc.raters') {
                return `${count} 人打分`;
            }
            if (key === 'dialog.world.vrcc.went_count') {
                return `${count} 人去过`;
            }
            if (key === 'dialog.world.vrcc.rank') {
                return `地图榜第 ${rank} 名`;
            }
            return key;
        }
    })
}));

import { WorldVrccPanel } from './WorldVrccPanel';

function renderWithClient(ui: ReactNode) {
    const client = new QueryClient({
        defaultOptions: { queries: { retry: false } }
    });
    return render(
        <QueryClientProvider client={client}>{ui}</QueryClientProvider>
    );
}

function mapPage(
    mapOverrides: Record<string, unknown> = {},
    // 顶层字段（spread / went / cap / perf）要单独传：直接塞进第一个参数
    // 会被 spread 到 `map` 里去，那是这组测试第一次写错的地方。
    topOverrides: Record<string, unknown> = {}
) {
    return {
        map: {
            world: 'wrld_test',
            name: '测试世界',
            avg: 8.4,
            raters: 412,
            rank: 37,
            hot: {
                text: '光影很舒服，适合安静聊天',
                by: '某位漫游星',
                up: 12,
                went: true
            },
            ...mapOverrides
        },
        spread: [
            { from: 9, to: 10, pct: 52 },
            { from: 7, to: 8, pct: 31 },
            { from: 5, to: 6, pct: 11 },
            { from: 3, to: 4, pct: 4 },
            { from: 1, to: 2, pct: 2 }
        ],
        went: 1200,
        cap: 32,
        perf: {
            from: 'test',
            weight: 'mid',
            vram: null,
            mem: null,
            load: 4200,
            size: 44826624,
            platforms: ['PC', 'Quest'],
            minutes: null
        },
        at: 1,
        ...topOverrides
    };
}

describe('WorldVrccPanel', () => {
    beforeEach(() => {
        appVrccMapPage.mockReset();
    });

    afterEach(() => {
        cleanup();
    });

    it('显示均分、人数与名次', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(await screen.findByText('8.4')).toBeTruthy();
        expect(screen.getByText('412 人打分')).toBeTruthy();
        // 名次在这一页要显示（概览那一块刻意不显示）
        expect(screen.getByText('地图榜第 37 名')).toBeTruthy();
    });

    it('画出每一段分数分布', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('8.4');
        expect(screen.getByText('52%')).toBeTruthy();
        expect(screen.getByText('2%')).toBeTruthy();
    });

    it('显示去过人数、房间上限与实测', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('8.4');
        expect(screen.getByText('1200 人去过')).toBeTruthy();
        expect(screen.getByText('32')).toBeTruthy();
        expect(screen.getByText('PC · Quest')).toBeTruthy();
        expect(screen.getByText('42.8 MB')).toBeTruthy();
        expect(screen.getByText('4200 ms')).toBeTruthy();
    });

    it('热评带署名与「去过」标记', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('光影很舒服，适合安静聊天')
        ).toBeTruthy();
        expect(screen.getByText('某位漫游星')).toBeTruthy();
    });

    it('不在榜上时给出明确结论与提示，而不是空白', async () => {
        appVrccMapPage.mockResolvedValue(null);
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.not_listed')
        ).toBeTruthy();
        expect(
            screen.getByText('dialog.world.vrcc.not_listed_hint')
        ).toBeTruthy();
    });

    it('请求失败时说读不到，不说「不在榜上」', async () => {
        // 这条要等重试跑完。策略里 `retry: 1` 会盖掉 QueryClient 的默认值，
        // 重试期间 `isPending` 仍为真，界面一直是「加载中」——所以给足超时。
        appVrccMapPage.mockRejectedValue(new Error('boom'));
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText(
                'dialog.world.vrcc.load_failed',
                {},
                { timeout: 5000 }
            )
        ).toBeTruthy();
        // 把网络问题说成「这图不在榜」会误导玩家
        expect(screen.queryByText('dialog.world.vrcc.not_listed')).toBeNull();
    }, 10000);

    it('perf 缺失时整块实测内容不渲染，也不崩', async () => {
        appVrccMapPage.mockResolvedValue(mapPage({}, { perf: null }));
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('8.4');
        // 平台/大小/加载三行都该消失，而不是显示 0
        expect(screen.queryByText('PC · Quest')).toBeNull();
        expect(screen.queryByText('42.8 MB')).toBeNull();
    });

    it('没人打分时不显示满分误导成 0.0', async () => {
        appVrccMapPage.mockResolvedValue(
            mapPage({ avg: null, raters: 0, rank: null, hot: null })
        );
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.no_rating')
        ).toBeTruthy();
        expect(screen.queryByText('0.0')).toBeNull();
    });

    it('世界 ID 为空时一个请求都不发', () => {
        renderWithClient(<WorldVrccPanel worldId="" />);
        expect(appVrccMapPage).not.toHaveBeenCalled();
    });
});
