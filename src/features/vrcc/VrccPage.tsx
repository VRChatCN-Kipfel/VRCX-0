import {
    AlertCircle,
    ExternalLink,
    RefreshCw,
    Star,
    Unplug
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import {
    PageBody,
    PageHeader,
    PageScaffold,
    PageTitle,
    PageToolbar
} from '@/components/layout/PageScaffold';
import { Badge } from '@/ui/shadcn/badge';
import { Button } from '@/ui/shadcn/button';
import { Spinner } from '@/ui/shadcn/spinner';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/ui/shadcn/tabs';

import { VrccMapsPanel } from './components/VrccMapsPanel';
import { VrccMePanel } from './components/VrccMePanel';
import { VrccNotificationsPanel } from './components/VrccNotificationsPanel';
import { useVrccController } from './useVrccController';

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
        recent,
        service,
        deviceAuth,
        deviceAuthBusy,
        deviceAuthMessage,
        reload,
        refreshBoards,
        beginDeviceAuth,
        disconnect
    } = useVrccController();

    const connected = connection?.connected ?? false;
    const unread = recent?.items?.length ?? 0;
    const serviceDown = service?.state !== 'ok';

    return (
        <PageScaffold>
            <PageHeader>
                <div className="flex min-w-0 items-center gap-2">
                    <Star className="text-muted-foreground size-4 shrink-0" />
                    <PageTitle>{t('vrcc.title', '星遇同频')}</PageTitle>
                    {connected ? (
                        <Badge variant="outline" className="shrink-0">
                            #{connection?.star ?? '—'}
                        </Badge>
                    ) : (
                        <Badge variant="secondary" className="shrink-0">
                            {t('vrcc.not_connected', '未连接')}
                        </Badge>
                    )}
                    {serviceDown && (
                        <Badge variant="destructive" className="shrink-0">
                            {t('vrcc.service_down', '服务不可用')}
                        </Badge>
                    )}
                    {unread > 0 && <Badge className="shrink-0">{unread}</Badge>}
                </div>
                <PageToolbar>
                    <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => void reload()}
                        disabled={loading}
                        aria-label={t('common.refresh', '刷新')}
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
                                    {t(
                                        'vrcc.reconnect_hint',
                                        '去「我的」重新连接'
                                    )}
                                </Button>
                            )}
                            {error.kind === 'missingScope' && error.scope && (
                                <div className="text-muted-foreground text-xs">
                                    {t('vrcc.missing_scope', {
                                        scope: error.scope,
                                        defaultValue:
                                            '这一项需要 {{scope}} 权限，重新连接时勾上'
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
                        value={tab}
                        onValueChange={(value) => setTab(value as typeof tab)}
                        className="flex min-h-0 flex-1 flex-col"
                    >
                        <TabsList className="shrink-0">
                            <TabsTrigger value="maps">
                                {t('vrcc.tab.maps', '地图榜')}
                            </TabsTrigger>
                            <TabsTrigger value="notifications">
                                {t('vrcc.tab.notifications', '提醒')}
                                {unread > 0 && (
                                    <span className="bg-primary/15 text-primary ml-1.5 rounded-full px-1.5 text-[10px] leading-4">
                                        {unread}
                                    </span>
                                )}
                            </TabsTrigger>
                            <TabsTrigger value="me">
                                {t('vrcc.tab.me', '我的')}
                            </TabsTrigger>
                        </TabsList>

                        <TabsContent value="maps" className="min-h-0 flex-1">
                            <VrccMapsPanel
                                boards={boards}
                                onRefresh={() => void refreshBoards()}
                            />
                        </TabsContent>

                        <TabsContent
                            value="notifications"
                            className="min-h-0 flex-1"
                        >
                            <VrccNotificationsPanel
                                connected={connected}
                                recent={recent}
                                onConnect={() => setTab('me')}
                            />
                        </TabsContent>

                        <TabsContent value="me" className="min-h-0 flex-1">
                            <VrccMePanel
                                connection={connection}
                                usage={usage}
                                deviceAuth={deviceAuth}
                                deviceAuthBusy={deviceAuthBusy}
                                deviceAuthMessage={deviceAuthMessage}
                                onBegin={beginDeviceAuth}
                                onDisconnect={disconnect}
                                onOpenLink={(url) =>
                                    window.open(
                                        url,
                                        '_blank',
                                        'noopener,noreferrer'
                                    )
                                }
                            />
                        </TabsContent>
                    </Tabs>
                )}
            </PageBody>
        </PageScaffold>
    );
}

/** 页面里用来打开 vrcc.cc 的小按钮，多处复用。 */
export function VrccExternalLink({
    href,
    children
}: {
    href: string;
    children: React.ReactNode;
}) {
    return (
        <a
            href={href}
            target="_blank"
            rel="noopener noreferrer"
            className="text-primary inline-flex items-center gap-1 text-xs hover:underline"
        >
            {children}
            <ExternalLink className="size-3" />
        </a>
    );
}

/** 断开按钮的图标在「我的」页里复用。 */
export const VrccUnplugIcon = Unplug;
