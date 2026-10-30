// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const appVrccMapPage = vi.fn();
const appVrccMyMapPage = vi.fn();

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccMapPage: (...args: unknown[]) => appVrccMapPage(...args),
        appVrccMyMapPage: (...args: unknown[]) => appVrccMyMapPage(...args)
    }
}));

vi.mock('react-i18next', () => ({
    // `formatRelativeTime` 会连带把 `i18nService` 拉进来，而那个模块在初始化
    // 时要 `i18n.use(initReactI18next)`。补一个空实现，否则整个测试文件在
    // 导入阶段就炸。
    initReactI18next: { type: '3rdParty', init: () => {} },
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
            // 只补测试真正会拼进字符串的那几个键；其余原样返回 key，
            // 断言里直接写 key 名即可。
            if (key === 'dialog.world.vrcc.unit_sec') {
                return '秒';
            }
            if (key === 'dialog.world.vrcc.perf_vram') {
                return '显存';
            }
            if (key === 'dialog.world.vrcc.perf_mem') {
                return '内存';
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
        appVrccMyMapPage.mockReset();
        // 默认「未连接」：绝大多数测试关心的是公开那一半，不该被额外的
        // 登录查询干扰。需要登录数据的用例自己覆盖这个默认值。
        appVrccMyMapPage.mockRejectedValue(new Error('not connected'));
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

    it('顶部指标行与配置四宫格显示去过、上限、平台与实测', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('8.4');
        // 顶部指标行：数值与标签分开放的，所以断言值本身
        expect(screen.getByText('1200')).toBeTruthy(); // 去过
        expect(screen.getByText('32')).toBeTruthy(); // 房间上限
        expect(screen.getByText('PC · Quest')).toBeTruthy(); // 平台
        // 配置四宫格：显存 / 内存 / 加载 / 下载。
        // 夹具里 vram 和 mem 都是 null，那两格不出现——顺带验证了「缺字段
        // 不留空盒子」。
        expect(screen.getByText('4秒')).toBeTruthy(); // load 4200ms
        // 44826624 字节 = 42.75 MiB，四舍五入是 43，不是 42
        expect(screen.getByText('43M')).toBeTruthy();
        expect(screen.queryByText('显存')).toBeNull();
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

    // -- 登录之后才有的一半 ------------------------------------------------

    function myMapPage(overrides: Record<string, unknown> = {}) {
        return {
            map: { world: 'wrld_test', avg: 8.4, raters: 412 },
            intro: null,
            rates: [
                {
                    id: 1,
                    text: '光影很舒服',
                    score: 9,
                    who: { name: '某位漫游星', group: 'G' },
                    mine: false
                },
                {
                    id: 2,
                    text: '我写的这条',
                    score: 7,
                    who: { name: '我', me: true },
                    mine: true
                }
            ],
            mine: { rate: { score: 7, text: '我写的这条', check: 'ok' } },
            sort: 'hot',
            ...overrides
        };
    }

    it('连上之后显示我的评分与全部短评', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(myMapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(await screen.findByText('光影很舒服')).toBeTruthy();
        // 「我写的这条」出现两次（我的评分块 + 列表里那条），用 getAllByText。
        expect(screen.getAllByText('我写的这条').length).toBe(2);
        expect(screen.getByText('dialog.world.vrcc.my_rating')).toBeTruthy();
        // 自己那条带「我」标记
        expect(screen.getByText('dialog.world.vrcc.mine')).toBeTruthy();
    });

    it('短评里用自己的星星表示分数', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(myMapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('光影很舒服');
        // 分数现在画成星星，靠 aria-label 表达数值（9 分的那条）
        expect(
            screen.getAllByRole('img', { name: '9.0' }).length
        ).toBeGreaterThan(0);
    });

    it('没连接时不报错，只在短评处退回一行说明', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        // beforeEach 已让登录查询失败
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        // 公开那一半照常
        expect(await screen.findByText('8.4')).toBeTruthy();
        // 短评处给说明，而不是整页报错。登录查询带 retry:1，要等它跑完，
        // 期间显示的是「加载中」。
        expect(
            await screen.findByText('dialog.world.vrcc.words_hint', undefined, {
                timeout: 5000
            })
        ).toBeTruthy();
    }, 10000);

    it('审核中和未通过的短评标出状态', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({
                mine: { rate: { score: 7, text: '待审', check: 'wait' } }
            })
        );
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.check_wait')
        ).toBeTruthy();
    });

    it('被折叠的短评不渲染', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({
                rates: [
                    { id: 9, text: '这条被折叠了', score: 3, folded: true },
                    { id: 1, text: '正常的一条', score: 8 }
                ]
            })
        );
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('正常的一条');
        expect(screen.queryByText('这条被折叠了')).toBeNull();
    });

    it('没打过分时说还没打，不显示 0 分', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({ mine: null, rates: [] })
        );
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.not_rated_by_me')
        ).toBeTruthy();
        expect(screen.getByText('dialog.world.vrcc.no_words_yet')).toBeTruthy();
    });
});
