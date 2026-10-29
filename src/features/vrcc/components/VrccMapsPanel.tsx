import { ExternalLink, RefreshCw } from 'lucide-react';
import { Fragment } from 'react';
import { useTranslation } from 'react-i18next';

import { openExternalLink } from '@/services/shellIntegrationService';
import { Button } from '@/ui/shadcn/button';
import { Spinner } from '@/ui/shadcn/spinner';
import {
    ToggleGroup,
    ToggleGroupItem,
    ToggleGroupSeparator
} from '@/ui/shadcn/toggle-group';

import type { VrccBoard, VrccBoardSort } from '../vrccApi';
import { vrccCoverUrl } from '../vrccCover';
import { VrccCover } from './VrccCover';

const SORTS: readonly VrccBoardSort[] = ['top', 'week', 'new'];

function isBoardSort(value: unknown): value is VrccBoardSort {
    return SORTS.includes(value as VrccBoardSort);
}

const SITE = 'https://vrcc.cc';

/**
 * 地图榜。开放数据，不需要连接。
 *
 * 密度取向：一行一张图，名次 + 名字 + 作者 + 均分 + 评分人数，全部单行，
 * 靠右对齐数字；悬停才出现外链，静止时不留视觉噪声。
 *
 * 排序（1.13.0 起）：总榜、本周、新上榜，和网页地图榜一样。服务器还不认
 * `sort` 时（`sortSupported` 为 false）不显示切换。
 */
export function VrccMapsPanel({
    boards,
    sort,
    sortSupported,
    onSortChange,
    onRefresh,
    onOpenWorld
}: {
    boards: VrccBoard | null;
    sort: VrccBoardSort;
    sortSupported: boolean;
    onSortChange: (sort: VrccBoardSort) => void;
    onRefresh: () => void;
    /** 点一行时打开 VRCX-0 自己的世界详情窗，不跳网页。 */
    onOpenWorld: (worldId: string) => void;
}) {
    const { t } = useTranslation();
    const maps = boards?.maps ?? [];
    const min = boards?.min;

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="text-muted-foreground flex shrink-0 items-center gap-2 px-1 pb-2 text-xs">
                {sortSupported && (
                    <ToggleGroup
                        variant="outline"
                        size="sm"
                        value={[sort]}
                        onValueChange={(next) => {
                            const value = next[0];
                            if (isBoardSort(value)) onSortChange(value);
                        }}
                    >
                        {SORTS.map((value, index) => (
                            <Fragment key={value}>
                                {index > 0 ? <ToggleGroupSeparator /> : null}
                                <ToggleGroupItem
                                    value={value}
                                    className="text-xs"
                                >
                                    {t(`vrcc.maps.sort.${value}`)}
                                </ToggleGroupItem>
                            </Fragment>
                        ))}
                    </ToggleGroup>
                )}
                {/* `min` 只在榜单拉到之后才有值。加载中、拉失败、或旧服务器
                    没给这个字段时都不该渲染——否则会显示成「满  人评分才进总榜」。 */}
                {min != null && (
                    <span className="min-w-0 truncate">
                        {t('vrcc.maps.hint', { min })}
                    </span>
                )}
                <Button
                    variant="ghost"
                    size="icon"
                    className="ml-auto size-6"
                    onClick={onRefresh}
                    aria-label={t('common.actions.refresh')}
                >
                    <RefreshCw className="size-3" />
                </Button>
            </div>

            {maps.length === 0 ? (
                <div className="text-muted-foreground flex h-32 items-center justify-center gap-2 text-sm">
                    <Spinner className="size-3" />
                    {t('vrcc.maps.empty')}
                </div>
            ) : (
                <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border">
                    <ul className="divide-y">
                        {maps.map((map, index) => {
                            // 本周榜只按本周的打分排，`rank` 却还是总榜名次：
                            // 本周的名次就是在列表里的位置，和网页「本周」一样。
                            const place =
                                sort === 'week' ? index + 1 : map.rank;
                            return (
                                <li
                                    key={map.world}
                                    className="hover:bg-muted/40 group flex items-center gap-3 px-3 py-2"
                                >
                                    {/* 点整行打开 VRCX-0 自己的世界详情窗：这是
                                    官方插件的做法，玩家熟悉，也不需要联网。 */}
                                    <button
                                        type="button"
                                        className="flex min-w-0 flex-1 items-center gap-3 text-left"
                                        onClick={() => onOpenWorld(map.world)}
                                    >
                                        <span
                                            className={
                                                (place ?? 99) <= 3
                                                    ? 'text-primary w-6 shrink-0 text-right text-sm font-bold tabular-nums'
                                                    : 'text-muted-foreground w-6 shrink-0 text-right text-sm tabular-nums'
                                            }
                                        >
                                            {place ?? '—'}
                                        </span>
                                        {/* 封面。`boards/maps` 至今只给 `img`
                                        （相对路径），没有 `imgUrl`，所以这里
                                        靠 `vrccCoverUrl` 自己拼站点 origin。
                                        没有封面或加载失败时组件返回 null，
                                        行退回纯文字，不占位也不撑高。 */}
                                        <VrccCover
                                            src={vrccCoverUrl(null, map.img)}
                                            className="size-8 shrink-0 rounded object-cover"
                                        />
                                        <div className="min-w-0 flex-1">
                                            <div className="truncate text-sm font-medium">
                                                {map.name ?? map.world}
                                            </div>
                                            {/* 有口碑短评就显示短评，没有才显示作者。
                                            短评是官方数据里唯一一句「这图好在哪」，
                                            对玩家比作者名有用，但只有一行位置。 */}
                                            <div className="text-muted-foreground truncate text-xs">
                                                {map.hot?.text
                                                    ? `「${map.hot.text}」${
                                                          map.hot.by
                                                              ? ` · ${map.hot.by}`
                                                              : ''
                                                      }`
                                                    : (map.author ?? '—')}
                                            </div>
                                        </div>
                                        <div className="shrink-0 text-right">
                                            <div className="text-sm font-semibold tabular-nums">
                                                {map.avg != null
                                                    ? map.avg.toFixed(1)
                                                    : '—'}
                                            </div>
                                            <div className="text-muted-foreground text-[10px] tabular-nums">
                                                {/* 本周榜显示本周几人打分，和网页上的「本周 N」一样。 */}
                                                {sort === 'week' &&
                                                map.week != null
                                                    ? t(
                                                          'vrcc.maps.week_count',
                                                          {
                                                              count: map.week
                                                          }
                                                      )
                                                    : (map.raters ?? 0)}
                                            </div>
                                        </div>
                                    </button>
                                    {/* 外链单独留一个：想去网页看详情时用，
                                    和「在 VRCX 里看」是两件事。 */}
                                    <button
                                        type="button"
                                        onClick={() => {
                                            // 走系统的 app__open_link：WebView 里
                                            // `target=_blank` 什么都不做。
                                            void openExternalLink(
                                                map.page ??
                                                    `${SITE}/maps/${map.world}`
                                            ).catch((error: unknown) => {
                                                console.error(
                                                    'vrcc: failed to open map page',
                                                    error
                                                );
                                            });
                                        }}
                                        className="text-muted-foreground hover:text-primary shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                                        aria-label={t('vrcc.maps.open')}
                                    >
                                        <ExternalLink className="size-3.5" />
                                    </button>
                                </li>
                            );
                        })}
                    </ul>
                </div>
            )}
        </div>
    );
}
