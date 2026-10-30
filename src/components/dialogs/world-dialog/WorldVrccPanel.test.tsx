// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor
} from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const appVrccMapPage = vi.fn();
const appVrccMyMap = vi.fn();
const appVrccMyMapPage = vi.fn();
const appVrccConnection = vi.fn();
const appVrccRateMap = vi.fn();
const appVrccDeleteMapRating = vi.fn();
const appVrccVoteMapContent = vi.fn();

vi.mock('@/platform/tauri/bindings', () => ({
    commands: {
        appVrccMapPage: (...args: unknown[]) => appVrccMapPage(...args),
        appVrccMyMap: (...args: unknown[]) => appVrccMyMap(...args),
        appVrccMyMapPage: (...args: unknown[]) => appVrccMyMapPage(...args),
        appVrccConnection: (...args: unknown[]) => appVrccConnection(...args),
        appVrccRateMap: (...args: unknown[]) => appVrccRateMap(...args),
        appVrccDeleteMapRating: (...args: unknown[]) =>
            appVrccDeleteMapRating(...args),
        appVrccVoteMapContent: (...args: unknown[]) =>
            appVrccVoteMapContent(...args)
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
        appVrccConnection.mockReset();
        appVrccRateMap.mockReset();
        appVrccDeleteMapRating.mockReset();
        appVrccVoteMapContent.mockReset();
        appVrccMyMap.mockReset();
        // 默认「未连接」：绝大多数测试关心的是公开那一半，不该被额外的
        // 登录查询干扰。需要登录数据的用例自己覆盖这个默认值。
        appVrccMyMapPage.mockRejectedValue(new Error('not connected'));
        // 默认「未收录」：区分「待审核」的用例自己覆盖。
        appVrccMyMap.mockResolvedValue({
            world: 'wrld_test',
            name: '测试世界',
            board: false,
            pending: false,
            off: false
        });
        // 默认令牌里有 maps.rate（能打分），需要「缺权限」的用例自己覆盖。
        appVrccConnection.mockResolvedValue({
            connected: true,
            scope: 'profile notifications maps mutuals maps.rate maps.write'
        });
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

    // -- 四种状态：公开接口都回 404，只有带令牌的能区分 -------------------

    it('已投稿待审核时说「等待审核」，不说「不在榜上」', async () => {
        // 公开页仍然 404（还没过审），但带令牌的状态是 pending
        appVrccMapPage.mockResolvedValue(null);
        appVrccMyMap.mockResolvedValue({
            world: 'wrld_test',
            board: false,
            pending: true,
            off: false
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.pending_title')
        ).toBeTruthy();
        // 这是关键：不能同时说「不在榜上」，那会让玩家以为投稿失败
        expect(screen.queryByText('dialog.world.vrcc.not_listed')).toBeNull();
    });

    it('未收录时说「不在榜上」', async () => {
        appVrccMapPage.mockResolvedValue(null);
        appVrccMyMap.mockResolvedValue({
            world: 'wrld_test',
            board: false,
            pending: false,
            off: false
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.not_listed')
        ).toBeTruthy();
        expect(
            screen.queryByText('dialog.world.vrcc.pending_title')
        ).toBeNull();
    });

    it('未连接时不会永远停在「加载中」', async () => {
        appVrccMapPage.mockResolvedValue(null);
        // 没有 maps 权限 → 状态查询被禁用。禁用时 `isPending` 仍是 true，
        // 但 `fetchStatus` 是 idle，不能据此显示加载中。
        appVrccConnection.mockResolvedValue({
            connected: false,
            scope: null
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.not_listed')
        ).toBeTruthy();
        // 关键：不该停在加载中
        expect(screen.queryByText('common.loading')).toBeNull();
    });

    it('在榜但公开页还没同步时，不说「还不在榜上」', async () => {
        appVrccMapPage.mockResolvedValue(null);
        appVrccMyMap.mockResolvedValue({
            world: 'wrld_test',
            board: true,
            pending: false,
            off: false
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.syncing_title')
        ).toBeTruthy();
        // 这一格最容易写错：说「还没有这张图」是事实错误
        expect(screen.queryByText('dialog.world.vrcc.not_listed')).toBeNull();
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

    // -- 配置四宫格的展开 / 折叠 -------------------------------------------

    it('配置默认收起，点「配置」才展开', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('8.4');
        // 触发点存在且报告未展开
        const trigger = screen.getByRole('button', {
            name: 'dialog.world.vrcc.perf_toggle'
        });
        expect(trigger.getAttribute('aria-expanded')).toBe('false');

        fireEvent.click(trigger);
        expect(trigger.getAttribute('aria-expanded')).toBe('true');
    });

    it('收起时四宫格不可交互（inert），展开后可交互', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('8.4');
        // 收起状态：内容还在 DOM 里（这样才能做高度动画），但被标记
        // aria-hidden，键盘不会跑到看不见的东西上。
        const hiddenRegion = document.querySelector('[aria-hidden="true"]');
        expect(hiddenRegion).toBeTruthy();

        fireEvent.click(
            screen.getByRole('button', {
                name: 'dialog.world.vrcc.perf_toggle'
            })
        );
        // 展开后不该再有 aria-hidden 的折叠区
        expect(
            document.querySelector('[aria-hidden="true"].opacity-0')
        ).toBeNull();
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

    it('没打过分时给打分器，不显示 0 分', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({ mine: null, rates: [] })
        );
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        // 有 maps.rate 时，没打过分应该是「可以打」而不是一句静态说明：
        // 打分器出现，内部显示「未选」。
        expect(
            await screen.findByText('dialog.world.vrcc.rate_none')
        ).toBeTruthy();
        expect(screen.queryByText('0.0')).toBeNull();
        expect(screen.getByText('dialog.world.vrcc.no_words_yet')).toBeTruthy();
    });

    it('连上了但令牌缺 maps.rate 时，提示重连而不是给个点不动的打分器', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({ mine: null, rates: [] })
        );
        // 加权限之前签发的令牌：有 maps，没有 maps.rate
        appVrccConnection.mockResolvedValue({
            connected: true,
            scope: 'profile notifications maps mutuals'
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        expect(
            await screen.findByText('dialog.world.vrcc.rate_needs_scope')
        ).toBeTruthy();
        // 不该出现打分控件（只有排序切换那一个 radiogroup 也没有）
        expect(screen.queryAllByRole('radio').length).toBe(0);
    });

    // -- 短评的赞 / 踩 ------------------------------------------------------

    it('有 maps.write 时赞踩是真按钮，点一下投赞', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(myMapPage());
        appVrccVoteMapContent.mockResolvedValue({
            kind: 'rate',
            id: 1,
            up: 6,
            down: 0,
            mine: 1
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        const up = await screen.findByRole('button', {
            name: 'dialog.world.vrcc.vote_up'
        });
        fireEvent.click(up);

        await waitFor(() =>
            expect(appVrccVoteMapContent).toHaveBeenCalledWith('rate', 1, 'up')
        );
    });

    it('点已经赞过的赞 = 取消（发 none）', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({
                rates: [
                    {
                        id: 5,
                        text: '我已经赞过',
                        score: 8,
                        up: 3,
                        vote: 1,
                        mine: false
                    }
                ]
            })
        );
        appVrccVoteMapContent.mockResolvedValue({
            kind: 'rate',
            id: 5,
            up: 2,
            down: 0,
            mine: 0
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        const up = await screen.findByRole('button', {
            name: 'dialog.world.vrcc.vote_up'
        });
        fireEvent.click(up);

        await waitFor(() =>
            expect(appVrccVoteMapContent).toHaveBeenCalledWith(
                'rate',
                5,
                'none'
            )
        );
    });

    it('自己的短评不给投票按钮', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({
                rates: [{ id: 7, text: '我写的', score: 9, up: 1, mine: true }]
            })
        );
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('我写的');
        expect(
            screen.queryByRole('button', {
                name: 'dialog.world.vrcc.vote_up'
            })
        ).toBeNull();
    });

    it('没有 maps.write 时赞踩退化成纯计数，不是按钮', async () => {
        appVrccMapPage.mockResolvedValue(mapPage());
        appVrccMyMapPage.mockResolvedValue(
            myMapPage({
                rates: [
                    {
                        id: 5,
                        text: '别人写的',
                        score: 8,
                        up: 5,
                        down: 1,
                        mine: false
                    }
                ]
            })
        );
        // 有 maps.rate 但没有 maps.write
        appVrccConnection.mockResolvedValue({
            connected: true,
            scope: 'profile notifications maps mutuals maps.rate'
        });
        renderWithClient(<WorldVrccPanel worldId="wrld_test" />);

        await screen.findByText('别人写的');
        expect(
            screen.queryByRole('button', {
                name: 'dialog.world.vrcc.vote_up'
            })
        ).toBeNull();
        // 计数仍然显示，但只是文字而不是按钮
        expect(screen.getByText('👍 5')).toBeTruthy();
        expect(screen.getByText('👎 1')).toBeTruthy();
    });
});
