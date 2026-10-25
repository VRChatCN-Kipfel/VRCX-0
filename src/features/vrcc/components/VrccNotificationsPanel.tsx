import { BellOff, BellRing } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/ui/shadcn/button';

import type { VrccRecentNotifications } from '../vrccApi';

const SITE = 'https://vrcc.cc';

/**
 * 把官方给的**相对路径**拼成完整地址。
 *
 * 官方 `/me/notifications/recent` 的 `url` 形如 `/match/?tab=inbox`，
 * 接在 `https://vrcc.cc` 后面才是可点的链接。已经是绝对地址的原样返回。
 */
function toAbsolute(url: string): string {
    const trimmed = url.trim();
    if (!trimmed) return SITE;
    if (trimmed.startsWith('http://') || trimmed.startsWith('https://'))
        return trimmed;
    return `${SITE}${trimmed.startsWith('/') ? '' : '/'}${trimmed}`;
}

/** 相对时间：刚 / N 分钟 / N 小时 / N 天。 */
function relativeTime(at: number | null | undefined): string {
    if (!at) return '';
    const diff = Date.now() - at;
    if (diff < 60_000) return '刚刚';
    const minutes = Math.floor(diff / 60_000);
    if (minutes < 60) return `${minutes} 分钟前`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} 小时前`;
    return `${Math.floor(hours / 24)} 天前`;
}

/**
 * 最近的提醒。
 *
 * 未连接时给一条明确的去路（去「我的」连接），而不是空白表格。
 * 提醒条目如果带 `url`，整行可点，指向 vrcc.cc 对应的页面。
 */
export function VrccNotificationsPanel({
    connected,
    recent,
    onConnect
}: {
    connected: boolean;
    recent: VrccRecentNotifications | null;
    onConnect: () => void;
}) {
    const { t } = useTranslation();

    if (!connected) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-3 text-sm">
                <BellOff className="size-6" />
                <div className="text-center">
                    {t('vrcc.notifications.need_connect')}
                </div>
                <Button variant="outline" size="sm" onClick={onConnect}>
                    {t('vrcc.go_connect')}
                </Button>
            </div>
        );
    }

    const items = recent?.items ?? [];

    if (items.length === 0) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 text-sm">
                <BellRing className="size-6" />
                {t('vrcc.notifications.empty')}
            </div>
        );
    }

    return (
        <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border">
            <ul className="divide-y">
                {items.map((item) => {
                    // 官方 recent 的 url 是**相对路径**（以 /match/ 开头）。
                    // 契约层的 absolute_url() 会拼上站点根；命令层把拼好的
                    // 结果放在同一个字段里，所以我们这里拿到的可以直接用。
                    const href = item.url ? toAbsolute(item.url) : SITE;
                    return (
                        <li key={item.id}>
                            <a
                                href={href}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="hover:bg-muted/40 block px-3 py-2"
                            >
                                <div className="flex items-baseline gap-2">
                                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                        {item.title ??
                                            t('vrcc.notifications.untitled')}
                                    </span>
                                    <span className="text-muted-foreground shrink-0 text-[10px] tabular-nums">
                                        {relativeTime(item.at)}
                                    </span>
                                </div>
                                {item.body && (
                                    <div className="text-muted-foreground mt-0.5 line-clamp-2 text-xs">
                                        {item.body}
                                    </div>
                                )}
                            </a>
                        </li>
                    );
                })}
            </ul>
        </div>
    );
}
