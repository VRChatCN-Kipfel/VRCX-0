import { ExternalLink, MapPin } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Badge } from '@/ui/shadcn/badge';

import type { VrccMyMap } from '../vrccApi';
import { vrccCoverUrl } from '../vrccCover';
import { VrccCover } from './VrccCover';

/**
 * 当前所在地图在榜上的状态。
 *
 * 数据来自 `GET /me/maps/{world}`（`maps` 普通权限，`dev-` 可用）。
 *
 * **调用方必须自己判断 `off`**：服务层不再把 `off` 的地图折成 null（那样会
 * 和「没连接」「查不到」混在一起，界面说不出真正原因）。`off: true` 表示
 * 星遇不对当前账号下发这张图，此时整张卡片都不该渲染。
 *
 * 显示取向：一行地图名 + 几个数字。均分、评分人数、我打的分三样并排，
 * 因为玩家最常看的就是这三个；「还没上榜」用文字说明而不是空白。
 */
export function VrccCurrentMapCard({
    map,
    onOpenWorld
}: {
    map: VrccMyMap;
    onOpenWorld: (worldId: string) => void;
}) {
    const { t } = useTranslation();
    const world = map.world ?? '';
    const notRanked = !map.board && !map.pending;

    return (
        <section className="mb-3 shrink-0 rounded-xl border p-3">
            <div className="mb-1.5 flex items-center gap-2">
                <MapPin className="text-muted-foreground size-3.5 shrink-0" />
                <span className="text-muted-foreground text-xs">
                    {t('vrcc.current_map.label')}
                </span>
                {world && (
                    <button
                        type="button"
                        className="text-muted-foreground hover:text-primary ml-auto shrink-0"
                        onClick={() => onOpenWorld(world)}
                        aria-label={t('vrcc.current_map.open')}
                    >
                        <ExternalLink className="size-3.5" />
                    </button>
                )}
            </div>

            <div className="mb-2 flex items-center gap-2">
                <VrccCover
                    src={vrccCoverUrl(map.imgUrl, map.img)}
                    className="size-10 shrink-0 rounded object-cover"
                />
                <div className="min-w-0 truncate text-sm font-medium">
                    {map.name ?? world}
                </div>
            </div>

            {notRanked ? (
                <p className="text-muted-foreground text-xs leading-relaxed">
                    {t('vrcc.current_map.not_ranked')}
                </p>
            ) : (
                <div className="flex items-center gap-4">
                    <Stat
                        label={t('vrcc.current_map.avg')}
                        value={map.avg != null ? map.avg.toFixed(1) : '—'}
                    />
                    <Stat
                        label={t('vrcc.current_map.raters')}
                        value={String(map.raters ?? 0)}
                    />
                    <Stat
                        label={t('vrcc.current_map.mine')}
                        value={map.mine != null ? `${map.mine}` : '—'}
                    />
                    {map.pending && (
                        <Badge variant="secondary" className="shrink-0">
                            {t('vrcc.current_map.pending')}
                        </Badge>
                    )}
                </div>
            )}

            {/* 短评的审核状态：审核中/未通过时玩家该知道。 */}
            {map.words && map.wordsOk && map.wordsOk !== 'ok' && (
                <p className="text-muted-foreground mt-1 text-[11px]">
                    {map.wordsOk === 'wait'
                        ? t('vrcc.current_map.words_pending')
                        : t('vrcc.current_map.words_hidden')}
                </p>
            )}
        </section>
    );
}

function Stat({ label, value }: { label: string; value: string }) {
    return (
        <div className="min-w-0">
            <div className="text-sm font-semibold tabular-nums">{value}</div>
            <div className="text-muted-foreground text-[10px]">{label}</div>
        </div>
    );
}
