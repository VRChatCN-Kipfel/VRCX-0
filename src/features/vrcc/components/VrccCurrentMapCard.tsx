import { ExternalLink, MapPin } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Badge } from '@/ui/shadcn/badge';

import type { VrccMyMap } from '../vrccApi';

/**
 * 当前所在地图在榜上的状态。
 *
 * 数据来自 `GET /me/maps/{world}`（`maps` 普通权限，`dev-` 可用）。
 * 后端已经把 `off` 的地图转成 null，所以这里拿到的一定是可显示的地图。
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
                    {t('vrcc.current_map.label', '你所在的图')}
                </span>
                {world && (
                    <button
                        type="button"
                        className="text-muted-foreground hover:text-primary ml-auto shrink-0"
                        onClick={() => onOpenWorld(world)}
                        aria-label={t(
                            'vrcc.current_map.open',
                            '在世界详情里打开'
                        )}
                    >
                        <ExternalLink className="size-3.5" />
                    </button>
                )}
            </div>

            <div className="mb-2 truncate text-sm font-medium">
                {map.name ?? world}
            </div>

            {notRanked ? (
                <p className="text-muted-foreground text-xs leading-relaxed">
                    {t(
                        'vrcc.current_map.not_ranked',
                        '这张图还没进地图榜。在星遇同频打分后会提交收录审核。'
                    )}
                </p>
            ) : (
                <div className="flex items-center gap-4">
                    <Stat
                        label={t('vrcc.current_map.avg', '均分')}
                        value={map.avg != null ? map.avg.toFixed(1) : '—'}
                    />
                    <Stat
                        label={t('vrcc.current_map.raters', '人数')}
                        value={String(map.raters ?? 0)}
                    />
                    <Stat
                        label={t('vrcc.current_map.mine', '我打的')}
                        value={map.mine != null ? `${map.mine}` : '—'}
                    />
                    {map.pending && (
                        <Badge variant="secondary" className="shrink-0">
                            {t('vrcc.current_map.pending', '审核中')}
                        </Badge>
                    )}
                </div>
            )}

            {/* 不能打分时说明缺什么——官方给的 need 就三个值，翻成人话。 */}
            {map.can === false && map.need && (
                <p className="text-muted-foreground mt-2 text-[11px] leading-relaxed">
                    {needHint(map.need, t)}
                </p>
            )}

            {/* 短评的审核状态：审核中/未通过时玩家该知道。 */}
            {map.words && map.wordsOk && map.wordsOk !== 'ok' && (
                <p className="text-muted-foreground mt-1 text-[11px]">
                    {map.wordsOk === 'wait'
                        ? t('vrcc.current_map.words_pending', '短评审核中')
                        : t('vrcc.current_map.words_hidden', '短评未通过审核')}
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

/**
 * 把官方的 `need` 码翻成玩家能懂的话。
 *
 * 官方只有三个值：`verify` 先验证 VRChat 账号、`friend` 先同意机器人好友
 * 请求、`go` 要先在这张图上待一会儿。照官方插件的措辞，别让玩家看到码。
 */
function needHint(
    need: string,
    t: (key: string, options: { defaultValue: string }) => string
): string {
    switch (need) {
        case 'verify':
            return t('vrcc.current_map.need_verify', {
                defaultValue:
                    '打分前请先在星遇同频「我的资料 → VRChat 名字」验证账号。'
            });
        case 'friend':
            return t('vrcc.current_map.need_friend', {
                defaultValue:
                    '打分前请先在 VRChat 中同意机器人「VRCC的小助手」的好友请求。'
            });
        case 'go':
            return t('vrcc.current_map.need_go', {
                defaultValue: '机器人暂未看到你在 VRChat 中在线，请稍后再试。'
            });
        default:
            return t('vrcc.current_map.need_other', {
                defaultValue: '现在还打不了分。'
            });
    }
}
