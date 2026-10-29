import { ExternalLink, Users } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatRelativeTime } from '@/lib/dateTime';
import { Button } from '@/ui/shadcn/button';

import type { VrccMutuals } from '../vrccApi';

/**
 * 「互相连上」的星友列表。
 *
 * 需要 `mutuals` 敏感权限，`dev-` 标识一定拿不到（会回 `403 scope_unreviewed`）。
 * 所以这里对「没连上 / 没权限 / 空列表」三种情况分别给出人话，而不是统一空白。
 *
 * **合规边界（重要）**：条目只提供「打开 VRCX-0 自己的玩家详情页」这一个动作。
 * 官方明文要求——客户端若提供加好友入口，只能打开客户端自己的玩家页面让玩家
 * 手动点添加，**严禁由程序自动代替玩家向 VRChat 发送好友请求**。
 * 所以这里没有、也不应该有「加好友」按钮：加好友在玩家详情页里由玩家自己点。
 */
export function VrccMutualsPanel({
    connected,
    mutuals,
    mutualsError,
    onOpenUser
}: {
    connected: boolean;
    mutuals: VrccMutuals | null;
    /**
     * 这项功能没开通的原因（`dev-` 标识必然要不到 `mutuals` 敏感权限）。
     * 只在本页签显示——它是「某个功能没开通」，不是「整页坏了」。
     */
    mutualsError?: string | null;
    onOpenUser: (userId: string) => void;
}) {
    const { t } = useTranslation();

    if (!connected) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 text-sm">
                <Users className="size-6" />
                {t('vrcc.mutuals.need_connect')}
            </div>
        );
    }

    const stars = mutuals?.stars ?? [];

    if (stars.length === 0) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 px-6 text-center text-sm">
                <Users className="size-6" />
                <div className="leading-relaxed">
                    {mutualsError
                        ? t('vrcc.mutuals.unavailable')
                        : t('vrcc.mutuals.empty')}
                </div>
                {/* 只有真的因为缺权限失败时才提示去开权限。
                    已经有 `mutuals` 权限、只是确实没连上人的玩家，
                    看到「重新连接一次」只会困惑。 */}
                {mutualsError && (
                    <div className="text-[11px] leading-relaxed">
                        {t('vrcc.mutuals.needs_scope')}
                    </div>
                )}
            </div>
        );
    }

    return (
        <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border">
            <ul className="divide-y">
                {stars.map((star) => {
                    const userId = star.vrcUser ?? '';
                    // 没有 vrcUser 说明对方没公开「已验证」，就无法打开玩家页。
                    // 这时只显示名字，不给一个点了没反应的按钮。
                    const canOpen = userId.startsWith('usr_');
                    // 我给 TA 的分（1.12.0 起）：0–5，半颗星一档，0 是没打；
                    // 连上当天和旧服务器都没有。
                    const mine = star.rate?.mine ?? 0;
                    // 连上的时间（毫秒时间戳）。旧服务器和缺少字段时没有，
                    // 这时整块不渲染——留一个「—」会让人以为记录丢了。
                    const linkedAt = star.at
                        ? formatRelativeTime(star.at, { style: 'short' })
                        : '';
                    return (
                        <li
                            key={star.n}
                            className="flex items-center gap-3 px-3 py-2"
                        >
                            <span className="text-muted-foreground w-8 shrink-0 text-right text-xs tabular-nums">
                                #{star.n}
                            </span>
                            <div className="min-w-0 flex-1">
                                <div className="truncate text-sm font-medium">
                                    {star.name ?? t('vrcc.mutuals.unnamed')}
                                </div>
                                <div className="text-muted-foreground truncate text-xs">
                                    {star.vrc ?? '—'}
                                </div>
                            </div>
                            {mine > 0 && (
                                <span
                                    className="text-muted-foreground shrink-0 text-xs tabular-nums"
                                    title={t('vrcc.mutuals.rate_mine')}
                                >
                                    ★ {mine}
                                </span>
                            )}
                            {/* 「什么时候连上的」和「我打的分」是两件事，
                                所以各占一个位置，不合并进同一处文案。 */}
                            {linkedAt && (
                                <span
                                    className="text-muted-foreground shrink-0 text-xs tabular-nums"
                                    title={t('vrcc.mutuals.linked_at')}
                                >
                                    {linkedAt}
                                </span>
                            )}
                            {canOpen && (
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    className="shrink-0"
                                    onClick={() => onOpenUser(userId)}
                                    title={t('vrcc.mutuals.open_profile')}
                                >
                                    <ExternalLink className="size-3.5" />
                                </Button>
                            )}
                        </li>
                    );
                })}
            </ul>
        </div>
    );
}
