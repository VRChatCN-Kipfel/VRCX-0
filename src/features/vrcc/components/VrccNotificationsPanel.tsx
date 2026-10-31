import { BellOff, BellRing, Inbox } from 'lucide-react';
import { Fragment } from 'react';
import { useTranslation } from 'react-i18next';

import { formatRelativeTime } from '@/lib/dateTime';
import { openExternalLink } from '@/services/shellIntegrationService';
import { Button } from '@/ui/shadcn/button';
import {
    ToggleGroup,
    ToggleGroupItem,
    ToggleGroupSeparator
} from '@/ui/shadcn/toggle-group';

import type { VrccNotificationsView } from '../useVrccController';
import type {
    VrccNotes,
    VrccRecentNotifications,
    VrccUnread
} from '../vrccApi';

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

/**
 * 「提醒」页签：最近的提醒、几处未读数、通知中心。
 *
 * 未连接时给一条明确的去路（去「我的」连接），而不是空白表格。
 * 提醒条目如果带 `url`，整行可点，指向 vrcc.cc 对应的页面。
 *
 * 未读数（`me/unread`）和通知中心（`me/notes`）是 1.13.0 加的：服务器还没有
 * 时它们是 null，这两块就不显示，页面和以前一样。
 */
export function VrccNotificationsPanel({
    connected,
    recent,
    unread,
    view,
    notes,
    notesSupported,
    onViewChange,
    onConnect,
    onOpenWorld
}: {
    connected: boolean;
    recent: VrccRecentNotifications | null;
    unread: VrccUnread | null;
    view: VrccNotificationsView;
    notes: VrccNotes | null;
    /** 服务器有没有通知中心；还不知道时为 null。 */
    notesSupported: boolean | null;
    onViewChange: (view: VrccNotificationsView) => void;
    onConnect: () => void;
    /** 通知里带地图的，点开 VRCX-0 自己的世界详情窗。 */
    onOpenWorld: (worldId: string) => void;
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

    const showSwitch = notesSupported !== false;
    const current = showSwitch ? view : 'recent';

    return (
        <div className="flex h-full min-h-0 flex-col gap-2">
            {(showSwitch || unread) && (
                <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-1">
                    {showSwitch && (
                        <ToggleGroup
                            variant="outline"
                            size="sm"
                            value={[current]}
                            onValueChange={(next) => {
                                const value = next[0];
                                if (value === 'recent' || value === 'notes') {
                                    onViewChange(value);
                                }
                            }}
                        >
                            {(['recent', 'notes'] as const).map(
                                (value, index) => (
                                    <Fragment key={value}>
                                        {index > 0 ? (
                                            <ToggleGroupSeparator />
                                        ) : null}
                                        <ToggleGroupItem
                                            value={value}
                                            className="text-xs"
                                        >
                                            {t(
                                                `vrcc.notifications.view.${value}`
                                            )}
                                        </ToggleGroupItem>
                                    </Fragment>
                                )
                            )}
                        </ToggleGroup>
                    )}
                    {unread && (
                        <UnreadCounts
                            unread={unread}
                            onOpenNotes={
                                showSwitch
                                    ? () => onViewChange('notes')
                                    : undefined
                            }
                        />
                    )}
                </div>
            )}

            {current === 'notes' ? (
                <NotesList notes={notes} onOpenWorld={onOpenWorld} />
            ) : (
                <RecentList recent={recent} />
            )}
        </div>
    );
}

/**
 * 网页上几处红点的数字。连向我、星轨相交、私聊在网页「连向我」那一页；
 * 通知就在本页签的「通知」里。
 *
 * 服务器没给的那一项不显示：令牌没有 `crossings` 权限时没有 `cross` 这个键，
 * 显示成 0 会让人以为是「没有新的」。
 */
function UnreadCounts({
    unread,
    onOpenNotes
}: {
    unread: VrccUnread;
    onOpenNotes?: () => void;
}) {
    const { t } = useTranslation();
    const items: Array<{
        key: 'inbox' | 'cross' | 'chats' | 'notes';
        href?: string;
    }> = [
        { key: 'inbox', href: `${SITE}/match/?tab=inbox` },
        { key: 'cross', href: `${SITE}/match/?tab=inbox&cross` },
        { key: 'chats', href: `${SITE}/match/?tab=inbox` },
        { key: 'notes' }
    ];

    return (
        <div className="ml-auto flex items-center gap-3 text-xs">
            {items.map(({ key, href }) => {
                const count = unread[key];
                if (count == null) return null;
                const content = (
                    <>
                        {t(`vrcc.unread.${key}`)}{' '}
                        <span className="tabular-nums">{count}</span>
                    </>
                );
                const className =
                    count > 0
                        ? 'text-foreground font-medium hover:underline'
                        : 'text-muted-foreground hover:underline';
                if (href) {
                    return (
                        <button
                            key={key}
                            type="button"
                            className={className}
                            onClick={() => {
                                // 走系统的 app__open_link：WebView 里
                                // `target=_blank` 什么都不做。
                                void openExternalLink(href).catch(
                                    (error: unknown) => {
                                        console.error(
                                            'vrcc: failed to open notice link',
                                            error
                                        );
                                    }
                                );
                            }}
                        >
                            {content}
                        </button>
                    );
                }
                return onOpenNotes ? (
                    <button
                        key={key}
                        type="button"
                        className={className}
                        onClick={onOpenNotes}
                    >
                        {content}
                    </button>
                ) : (
                    <span key={key} className={className}>
                        {content}
                    </span>
                );
            })}
        </div>
    );
}

function RecentList({ recent }: { recent: VrccRecentNotifications | null }) {
    const { t } = useTranslation();
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
                            <button
                                type="button"
                                onClick={() => {
                                    // 走系统的 app__open_link：WebView 里
                                    // `target=_blank` 什么都不做。
                                    void openExternalLink(href).catch(
                                        (error: unknown) => {
                                            console.error(
                                                'vrcc: failed to open notice link',
                                                error
                                            );
                                        }
                                    );
                                }}
                                className="hover:bg-muted/40 block w-full px-3 py-2 text-left"
                            >
                                <div className="flex items-baseline gap-2">
                                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                        {item.title ??
                                            t('vrcc.notifications.untitled')}
                                    </span>
                                    <span className="text-muted-foreground shrink-0 text-[10px] tabular-nums">
                                        {item.at
                                            ? formatRelativeTime(item.at)
                                            : ''}
                                    </span>
                                </div>
                                {item.body && (
                                    <div className="text-muted-foreground mt-0.5 line-clamp-2 text-xs">
                                        {item.body}
                                    </div>
                                )}
                            </button>
                        </li>
                    );
                })}
            </ul>
        </div>
    );
}

/**
 * 通知中心：网页「通知」那张表，新的在前。读了不标已读，没看过的前面带个点。
 *
 * `kind` 以后可能加新的，这里不按 `kind` 分支，一律显示官方给的那句 `text`。
 */
function NotesList({
    notes,
    onOpenWorld
}: {
    notes: VrccNotes | null;
    onOpenWorld: (worldId: string) => void;
}) {
    const { t } = useTranslation();
    const items = notes?.notes ?? [];

    if (items.length === 0) {
        return (
            <div className="text-muted-foreground flex h-40 flex-col items-center justify-center gap-2 text-sm">
                <Inbox className="size-6" />
                {t('vrcc.notifications.notes_empty')}
            </div>
        );
    }

    return (
        <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border">
            <ul className="divide-y">
                {items.map((note) => {
                    const world = note.world ?? '';
                    const body = (
                        <div className="flex items-baseline gap-2">
                            <span
                                className={
                                    note.seen === false
                                        ? 'bg-primary mt-1.5 size-1.5 shrink-0 self-start rounded-full'
                                        : 'mt-1.5 size-1.5 shrink-0 self-start'
                                }
                            />
                            <span
                                className={
                                    note.seen === false
                                        ? 'min-w-0 flex-1 text-sm'
                                        : 'text-muted-foreground min-w-0 flex-1 text-sm'
                                }
                            >
                                {note.text}
                            </span>
                            <span className="text-muted-foreground shrink-0 text-[10px] tabular-nums">
                                {note.at ? formatRelativeTime(note.at) : ''}
                            </span>
                        </div>
                    );
                    return (
                        <li key={note.id}>
                            {world.startsWith('wrld_') ? (
                                <button
                                    type="button"
                                    className="hover:bg-muted/40 block w-full px-3 py-2 text-left"
                                    onClick={() => onOpenWorld(world)}
                                >
                                    {body}
                                </button>
                            ) : (
                                <div className="px-3 py-2">{body}</div>
                            )}
                        </li>
                    );
                })}
            </ul>
        </div>
    );
}
