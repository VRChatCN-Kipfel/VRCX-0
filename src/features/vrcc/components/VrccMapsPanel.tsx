import { ExternalLink, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/ui/shadcn/button';
import { Spinner } from '@/ui/shadcn/spinner';

import type { VrccBoard } from '../vrccApi';

const SITE = 'https://vrcc.cc';

/**
 * 地图榜。开放数据，不需要连接。
 *
 * 密度取向：一行一张图，名次 + 名字 + 作者 + 均分 + 评分人数，全部单行，
 * 靠右对齐数字；悬停才出现外链，静止时不留视觉噪声。
 */
export function VrccMapsPanel({
    boards,
    onRefresh
}: {
    boards: VrccBoard | null;
    onRefresh: () => void;
}) {
    const { t } = useTranslation();
    const maps = boards?.maps ?? [];
    const min = boards?.min;

    return (
        <div className="flex h-full min-h-0 flex-col">
            <div className="text-muted-foreground flex shrink-0 items-center gap-2 px-1 pb-2 text-xs">
                <span>
                    {t('vrcc.maps.hint', {
                        min,
                        defaultValue: '满 {{min}} 人评分才进总榜'
                    })}
                </span>
                <Button
                    variant="ghost"
                    size="icon"
                    className="ml-auto size-6"
                    onClick={onRefresh}
                    aria-label={t('common.refresh', '刷新')}
                >
                    <RefreshCw className="size-3" />
                </Button>
            </div>

            {maps.length === 0 ? (
                <div className="text-muted-foreground flex h-32 items-center justify-center gap-2 text-sm">
                    <Spinner className="size-3" />
                    {t('vrcc.maps.empty', '暂无榜单数据')}
                </div>
            ) : (
                <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border">
                    <ul className="divide-y">
                        {maps.map((map) => (
                            <li
                                key={map.world}
                                className="hover:bg-muted/40 group flex items-center gap-3 px-3 py-2"
                            >
                                <span
                                    className={
                                        (map.rank ?? 99) <= 3
                                            ? 'text-primary w-6 shrink-0 text-right text-sm font-bold tabular-nums'
                                            : 'text-muted-foreground w-6 shrink-0 text-right text-sm tabular-nums'
                                    }
                                >
                                    {map.rank ?? '—'}
                                </span>
                                <div className="min-w-0 flex-1">
                                    <div className="truncate text-sm font-medium">
                                        {map.name ?? map.world}
                                    </div>
                                    <div className="text-muted-foreground truncate text-xs">
                                        {map.author ?? '—'}
                                    </div>
                                </div>
                                <div className="shrink-0 text-right">
                                    <div className="text-sm font-semibold tabular-nums">
                                        {map.avg != null
                                            ? map.avg.toFixed(1)
                                            : '—'}
                                    </div>
                                    <div className="text-muted-foreground text-[10px] tabular-nums">
                                        {map.raters ?? 0}
                                    </div>
                                </div>
                                <a
                                    href={
                                        map.page ?? `${SITE}/maps/${map.world}`
                                    }
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="text-muted-foreground hover:text-primary shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                                    aria-label={t(
                                        'vrcc.maps.open',
                                        '在星遇同频打开'
                                    )}
                                >
                                    <ExternalLink className="size-3.5" />
                                </a>
                            </li>
                        ))}
                    </ul>
                </div>
            )}
        </div>
    );
}
