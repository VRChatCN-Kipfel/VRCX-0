import {
    AlertCircle,
    ExternalLink,
    Megaphone,
    RefreshCw,
    Star,
    Unplug,
    X
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router';

import {
    PageBody,
    PageHeader,
    PageScaffold,
    PageTitle,
    PageToolbar
} from '@/components/layout/PageScaffold';
import { formatCompactDateTime } from '@/lib/dateTime';
import { openExternalLink } from '@/services/shellIntegrationService';
import { toast } from '@/services/toastService';
import { Badge } from '@/ui/shadcn/badge';
import { Button } from '@/ui/shadcn/button';
import { Spinner } from '@/ui/shadcn/spinner';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/ui/shadcn/tabs';

import { VrccCrossingsPanel } from './components/VrccCrossingsPanel';
import { VrccCurrentMapCard } from './components/VrccCurrentMapCard';
import { VrccMapsPanel } from './components/VrccMapsPanel';
import { VrccMePanel } from './components/VrccMePanel';
import { VrccMutualsPanel } from './components/VrccMutualsPanel';
import { VrccNotificationsPanel } from './components/VrccNotificationsPanel';
import { useVrccController, type VrccTab } from './useVrccController';
import type {
    VrccAnnouncement,
    VrccRecentNotifications,
    VrccUnread
} from './vrccApi';

/**
 * 关掉的公告编号，记在本机。公告的编号只增不减：这条和更早的不再显示，
 * 有新的照常出来。
 */
const DISMISSED_ANNOUNCEMENT_KEY = 'vrcc.dismissedAnnouncement';

function readDismissedAnnouncement(): number | null {
    try {
        const id = Number(localStorage.getItem(DISMISSED_ANNOUNCEMENT_KEY));
        return Number.isFinite(id) && id > 0 ? id : null;
    } catch {
        return null;
    }
}

/**
 * 页签上的未读数。
 *
 * 服务器有 `me/unread`（1.13.0 起）时是网页上几处红点的合计；旧服务器上
 * 退回「最近的提醒」里 `id > read` 的条数（`read` 没有时都算未读）。
 */
function unreadCount(
    unread: VrccUnread | null,
    recent: VrccRecentNotifications | null
): number {
    if (unread) {
        return (
            (unread.inbox ?? 0) +
            (unread.cross ?? 0) +
            (unread.chats ?? 0) +
            (unread.notes ?? 0)
        );
    }
    const read = recent?.read ?? 0;
    return (recent?.items ?? []).filter((item) => (item.id ?? 0) > read).length;
}

const VRCC_TABS: readonly VrccTab[] = [
    'maps',
    'notifications',
    'mutuals',
    'me'
];

/** 页面认的页签名。未知值一律忽略，不让它把界面切到一个不存在的页签。 */
function isVrccTab(value: string | null): value is VrccTab {
    return value !== null && (VRCC_TABS as readonly string[]).includes(value);
}

export function VrccPage() {
    const { t } = useTranslation();
    const {
        tab,
        setTab,
        loading,
        error,
        connection,
        usage,
        boards,
        boardSort,
        boardSortSupported,
        recent,
        unread,
        notificationsView,
        notes,
        notesSupported,
        announcements,
        stickers,
        myMap,
        mutuals,
        mutualsError,
        crossings,
        crossingCards,
        crossingsError,
        refreshCrossings,
        service,
        deviceAuth,
        deviceAuthBusy,
        deviceAuthMessage,
        deviceAuthCooldown,
        reload,
        refreshBoards,
        setBoardSort,
        setNotificationsView,
        beginDeviceAuth,
        disconnect,
        openWorld,
        openUser,
        openGroup
    } = useVrccController();

    const connected = connection?.connected ?? false;
    const unreadTotal = connected ? unreadCount(unread, recent) : 0;
    // 服务端说这块不该显示时（16–17 岁星池，或玩家关了星轨相交），页签也
    // 一起藏起来，并顺手把停留在这页的玩家送回地图榜——否则会出现「所有
    // 页签都不选中、内容空白」的状态。
    const crossingsHidden = crossings?.hidden === true;

    // 从通知中心点进来时带的页签名。
    const [searchParams, setSearchParams] = useSearchParams();
    const requestedTab = searchParams.get('tab');
    const appliedRequestedTab = useRef(false);
    // 用 ref 读隐藏状态：`crossingsHidden` 是每次渲染新算的布尔值，直接进
    // 依赖数组会让这个 effect 在星轨相交数据回来时白跑一遍（此时
    // `appliedRequestedTab` 已经置位，行为其实不变）。
    const crossingsHiddenRef = useRef(crossingsHidden);
    crossingsHiddenRef.current = crossingsHidden;
    useEffect(() => {
        // 等连接就绪再应用：`setTab` 在未连接时会直接返回，那时「提醒」页
        // 该做的已读同步也一并跳过了，页签会落过去而同步没发生。
        if (
            appliedRequestedTab.current ||
            !connected ||
            !isVrccTab(requestedTab) ||
            // 「星轨相交」被服务端要求隐藏时不能落过去：那个页签根本不存在，
            // 落过去就是一个谁都没选中、内容空白的页面。
            (requestedTab === 'crossings' && crossingsHiddenRef.current)
        ) {
            return;
        }
        appliedRequestedTab.current = true;
        setTab(requestedTab);
        // 参数用掉就清掉，免得玩家手动切页签时又被它拽回去。
        const remaining = new URLSearchParams(searchParams);
        remaining.delete('tab');
        setSearchParams(remaining, { replace: true });
    }, [connected, requestedTab, searchParams, setSearchParams, setTab]);
    const activeTab = crossingsHidden && tab === 'crossings' ? 'maps' : tab;
    const stop = announcements?.stop ?? null;
    const [dismissedAnnouncement, setDismissedAnnouncement] = useState(
        readDismissedAnnouncement
    );
    // 公告新的在前：第一条比关掉的那条新的，就是要显示的。
    const announcement = (announcements?.posts ?? []).find(
        (post) => (post.id ?? 0) > (dismissedAnnouncement ?? 0)
    );
    const dismissAnnouncement = (id: number) => {
        setDismissedAnnouncement(id);
        try {
            localStorage.setItem(DISMISSED_ANNOUNCEMENT_KEY, String(id));
        } catch {
            // 存不下就只在这次打开时关掉。
        }
    };
    const openLink = (url: string) => {
        // 走系统的 `app__open_link`，不要用 `window.open`：Tauri 的 WebView
        // 会拦掉 `window.open` 且什么都不做，按钮点了没反应。
        void openExternalLink(url).catch((error: unknown) => {
            toast.add({
                type: 'error',
                title:
                    error instanceof Error
                        ? error.message
                        : t('vrcc.error.unknown')
            });
        });
    };
    // 只在这三种情况下才算「服务不可用」：没有状态对象、state 不是 ok、
    // 或后端明确报告了故障。
    //
    // 不能写成 `service?.state !== 'ok'`：`service` 初值是 `null`，
    // `null?.state` 是 `undefined`，`undefined !== 'ok'` 为真——于是请求
    // 还没发出去，徽章就先亮「服务不可用」，玩家看到的是一个凭空出现的故障。
    // 「还不知道」和「已经坏了」是两件事。
    const serviceDown = service !== null && service.state !== 'ok';

    return (
        <PageScaffold>
            <PageHeader>
                <div className="flex min-w-0 items-center gap-2">
                    <Star className="text-muted-foreground size-4 shrink-0" />
                    <PageTitle>{t('vrcc.title')}</PageTitle>
                    {connected ? (
                        <Badge variant="outline" className="shrink-0">
                            #{connection?.star ?? '—'}
                        </Badge>
                    ) : (
                        <Badge variant="secondary" className="shrink-0">
                            {t('vrcc.not_connected')}
                        </Badge>
                    )}
                    {serviceDown && (
                        <Badge variant="destructive" className="shrink-0">
                            {t('vrcc.service_down')}
                        </Badge>
                    )}
                    {unreadTotal > 0 && (
                        <Badge className="shrink-0">{unreadTotal}</Badge>
                    )}
                </div>
                <PageToolbar>
                    <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => void reload()}
                        disabled={loading}
                        aria-label={t('common.actions.refresh')}
                    >
                        <RefreshCw
                            className={
                                loading ? 'size-4 animate-spin' : 'size-4'
                            }
                        />
                    </Button>
                </PageToolbar>
            </PageHeader>

            <PageBody>
                {/* 停站维护（`announcements` 的 `stop`）：个人接口这时回答
                    `503 stop`，先把原因和预计恢复时间摆出来。 */}
                {stop && (
                    <div className="bg-muted/50 mb-3 flex items-start gap-2 rounded-lg border p-3 text-sm">
                        <AlertCircle className="mt-0.5 size-4 shrink-0" />
                        <div className="min-w-0">
                            <div className="font-medium">
                                {t('vrcc.stop.title')}
                            </div>
                            {stop.say && (
                                <div className="text-muted-foreground text-xs break-words whitespace-pre-line">
                                    {stop.say}
                                </div>
                            )}
                            {stop.until != null && (
                                <div className="text-muted-foreground text-xs">
                                    {t('vrcc.stop.until', {
                                        time: formatCompactDateTime(stop.until)
                                    })}
                                </div>
                            )}
                        </div>
                    </div>
                )}

                {announcement && (
                    <VrccAnnouncementLine
                        post={announcement}
                        onOpen={openLink}
                        onDismiss={dismissAnnouncement}
                    />
                )}

                {error && (
                    <div className="border-destructive/40 bg-destructive/10 text-destructive mb-3 flex items-start gap-2 rounded-lg border p-3 text-sm">
                        <AlertCircle className="mt-0.5 size-4 shrink-0" />
                        <div className="min-w-0">
                            <div className="break-words">{error.message}</div>
                            {error.kind === 'needsReauth' && (
                                <Button
                                    variant="link"
                                    size="sm"
                                    className="h-auto p-0 text-xs"
                                    onClick={() => setTab('me')}
                                >
                                    {t('vrcc.reconnect_hint')}
                                </Button>
                            )}
                            {error.kind === 'missingScope' && error.scope && (
                                <div className="text-muted-foreground text-xs">
                                    {t('vrcc.missing_scope', {
                                        scope: error.scope
                                    })}
                                </div>
                            )}
                        </div>
                    </div>
                )}

                {loading && !boards ? (
                    <div className="text-muted-foreground flex h-40 items-center justify-center gap-2 text-sm">
                        <Spinner className="size-4" />
                        {t('common.loading', '加载中…')}
                    </div>
                ) : (
                    <Tabs
                        value={activeTab}
                        onValueChange={(value) => setTab(value as typeof tab)}
                        className="flex min-h-0 flex-1 flex-col"
                    >
                        <TabsList className="shrink-0">
                            <TabsTrigger value="maps">
                                {t('vrcc.tab.maps')}
                            </TabsTrigger>
                            <TabsTrigger value="notifications">
                                {t('vrcc.tab.notifications')}
                                {unreadTotal > 0 && (
                                    <span className="bg-primary/15 text-primary ml-1.5 rounded-full px-1.5 text-[10px] leading-4">
                                        {unreadTotal}
                                    </span>
                                )}
                            </TabsTrigger>
                            <TabsTrigger value="mutuals">
                                {t('vrcc.tab.mutuals')}
                            </TabsTrigger>
                            {/* `hidden` 时**连页签都不出现**：面板自己返回
                                null 只藏住了内容，标签还写着「星轨相交」，
                                等于照样确认了「这个功能存在、只是没给你」。
                                官方要求的是客户端不显示此模块。 */}
                            {!crossingsHidden && (
                                <TabsTrigger value="crossings">
                                    {t('vrcc.tab.crossings')}
                                </TabsTrigger>
                            )}
                            <TabsTrigger value="me">
                                {t('vrcc.tab.me')}
                            </TabsTrigger>
                        </TabsList>

                        <TabsContent value="maps" className="min-h-0 flex-1">
                            {/* 所在地图卡片放在榜单上方：玩家最关心的是
                                «我现在这张图怎么样»，而不是全站排行。
                                `off` 是硬阻断：服务层不再把它折成 null，
                                所以这里必须自己判断——官方要求此时隐藏
                                这张图的卡片。 */}
                            {myMap && myMap.off !== true && (
                                <VrccCurrentMapCard
                                    map={myMap}
                                    onOpenWorld={openWorld}
                                />
                            )}
                            <VrccMapsPanel
                                boards={boards}
                                sort={boardSort}
                                sortSupported={boardSortSupported}
                                onSortChange={setBoardSort}
                                onRefresh={() => void refreshBoards()}
                                onOpenWorld={openWorld}
                            />
                        </TabsContent>

                        <TabsContent
                            value="notifications"
                            className="min-h-0 flex-1"
                        >
                            <VrccNotificationsPanel
                                connected={connected}
                                recent={recent}
                                unread={unread}
                                view={notificationsView}
                                notes={notes}
                                notesSupported={notesSupported}
                                onViewChange={setNotificationsView}
                                onConnect={() => setTab('me')}
                                onOpenWorld={openWorld}
                            />
                        </TabsContent>

                        <TabsContent value="mutuals" className="min-h-0 flex-1">
                            <VrccMutualsPanel
                                connected={connected}
                                mutuals={mutuals}
                                mutualsError={mutualsError}
                                onOpenUser={openUser}
                                onOpenGroup={openGroup}
                            />
                        </TabsContent>
                        <TabsContent
                            value="crossings"
                            className="min-h-0 flex-1"
                        >
                            <VrccCrossingsPanel
                                connected={connected}
                                crossings={crossings}
                                cards={crossingCards}
                                crossingsError={crossingsError}
                                onOpenLink={openLink}
                                onRefresh={() => {
                                    // 手动刷新不算「看过」：这一下可能只是
                                    // 想看看有没有新记录，不该顺手替玩家
                                    // 把红点清掉。`mark=true` 只留给「切到
                                    // 这一页」那一个动作（见 `setTab`）。
                                    void refreshCrossings(false).catch(
                                        (error: unknown) => {
                                            toast.add({
                                                type: 'error',
                                                title:
                                                    error instanceof Error
                                                        ? error.message
                                                        : t(
                                                              'vrcc.error.unknown'
                                                          )
                                            });
                                        }
                                    );
                                }}
                            />
                        </TabsContent>

                        <TabsContent value="me" className="min-h-0 flex-1">
                            <VrccMePanel
                                connection={connection}
                                usage={usage}
                                stickers={stickers}
                                deviceAuth={deviceAuth}
                                deviceAuthBusy={deviceAuthBusy}
                                deviceAuthMessage={deviceAuthMessage}
                                deviceAuthCooldown={deviceAuthCooldown}
                                onBegin={beginDeviceAuth}
                                onDisconnect={disconnect}
                                onOpenLink={openLink}
                            />
                        </TabsContent>
                    </Tabs>
                )}
            </PageBody>
        </PageScaffold>
    );
}

/**
 * 给所有玩家的公告（`announcements` 的 `posts`），只显示最新一条。
 *
 * 网页上分 `bar` / `pop` / `force` 三种显示法；这里是嵌在页面里的一行，
 * 不弹窗、不拦操作，所以三种一样处理。标题和正文是纯文字，不当 HTML。
 */
function VrccAnnouncementLine({
    post,
    onOpen,
    onDismiss
}: {
    post: VrccAnnouncement;
    onOpen: (url: string) => void;
    onDismiss: (id: number) => void;
}) {
    const { t } = useTranslation();
    const link = post.link ?? null;
    return (
        <div className="mb-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-sm">
            <Megaphone className="text-muted-foreground size-4 shrink-0" />
            <div className="min-w-0 flex-1">
                <span className="font-medium">{post.title}</span>
                {post.body && (
                    <span className="text-muted-foreground ml-2 text-xs">
                        {post.body}
                    </span>
                )}
            </div>
            {link && (
                <Button
                    variant="link"
                    size="sm"
                    className="h-auto shrink-0 p-0 text-xs"
                    onClick={() => onOpen(link)}
                >
                    {t('vrcc.announcement.open')}
                </Button>
            )}
            <Button
                variant="ghost"
                size="icon"
                className="size-6 shrink-0"
                onClick={() => onDismiss(post.id ?? 0)}
                aria-label={t('vrcc.announcement.dismiss')}
            >
                <X className="size-3" />
            </Button>
        </div>
    );
}

/**
 * 页面里用来打开 vrcc.cc 的小按钮，多处复用。
 *
 * **不要用 `<a target="_blank">`**：Tauri 的 WebView 处理 `target=_blank`
 * 走的是和 `window.open` 同一条新窗口路径，那里什么都不做（或者弹出一个
 * 不受管理的窗口），同时还会绕过 `app__open_link` 的 scheme 校验。
 * 统一走 `openExternalLink`，失败时由调用方的 toast 兜住。
 */
export function VrccExternalLink({
    href,
    children,
    onError
}: {
    href: string;
    children: React.ReactNode;
    /** 打开失败时的回调（一般是弹 toast）。不传就只记日志。 */
    onError?: (error: unknown) => void;
}) {
    return (
        <button
            type="button"
            onClick={() => {
                void openExternalLink(href).catch((error: unknown) => {
                    if (onError) {
                        onError(error);
                    } else {
                        console.error(
                            'vrcc: failed to open external link',
                            error
                        );
                    }
                });
            }}
            className="text-primary inline-flex items-center gap-1 text-xs hover:underline"
        >
            {children}
            <ExternalLink className="size-3" />
        </button>
    );
}

/** 断开按钮的图标在「我的」页里复用。 */
export const VrccUnplugIcon = Unplug;
