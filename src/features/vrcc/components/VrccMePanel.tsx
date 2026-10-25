import { ExternalLink, ShieldCheck, Unplug } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Badge } from '@/ui/shadcn/badge';
import { Button } from '@/ui/shadcn/button';
import { Progress } from '@/ui/shadcn/progress';
import { Spinner } from '@/ui/shadcn/spinner';

import type {
    VrccConnection,
    VrccDeviceAuthPrompt,
    VrccUsage
} from '../vrccApi';

const SITE = 'https://vrcc.cc';

/**
 * 「我的」页：连接状态、设备授权、本地用量。
 *
 * 用量口径是**「本客户端今日已用 N/300」**，不是「剩余」——官方没有查询
 * 剩余额度的接口，服务器按令牌计数，我们看不到那个数。这里只报自己记的数。
 */
export function VrccMePanel({
    connection,
    usage,
    deviceAuth,
    deviceAuthBusy,
    deviceAuthMessage,
    onBegin,
    onDisconnect,
    onOpenLink
}: {
    connection: VrccConnection | null;
    usage: VrccUsage | null;
    deviceAuth: VrccDeviceAuthPrompt | null;
    deviceAuthBusy: boolean;
    deviceAuthMessage: string | null;
    onBegin: () => void;
    onDisconnect: () => void | Promise<void>;
    onOpenLink: (url: string) => void;
}) {
    const { t } = useTranslation();
    const connected = connection?.connected ?? false;

    return (
        <div className="flex h-full min-h-0 flex-col gap-3 overflow-y-auto">
            {/* 连接状态 */}
            <section className="rounded-xl border p-3">
                <div className="mb-2 flex items-center gap-2">
                    <ShieldCheck className="text-muted-foreground size-4" />
                    <h3 className="text-sm font-semibold">
                        {t('vrcc.me.status', '连接')}
                    </h3>
                    <Badge
                        variant={connected ? 'outline' : 'secondary'}
                        className="ml-auto shrink-0"
                    >
                        {connected
                            ? t('vrcc.connected', '已连接')
                            : t('vrcc.not_connected', '未连接')}
                    </Badge>
                </div>

                {connected ? (
                    <div className="space-y-1 text-xs">
                        <Row
                            label={t('vrcc.me.star', '星号')}
                            value={
                                connection?.star != null
                                    ? `#${connection.star}`
                                    : '—'
                            }
                        />
                        <Row
                            label={t('vrcc.me.vrc_name', 'VRChat 名字')}
                            value={connection?.vrcName ?? '—'}
                        />
                        <Row
                            label={t('vrcc.me.scope', '权限')}
                            value={connection?.scope ?? '—'}
                        />
                        <Row
                            label={t('vrcc.me.client', '客户端标识')}
                            value={connection?.client ?? '—'}
                        />
                        {connection?.nextPollAt && (
                            <Row
                                label={t('vrcc.me.next_poll', '下次轮询')}
                                value={new Date(
                                    connection.nextPollAt
                                ).toLocaleTimeString()}
                            />
                        )}
                        <Button
                            variant="outline"
                            size="sm"
                            className="mt-2 w-full"
                            onClick={() => void onDisconnect()}
                        >
                            <Unplug className="mr-1 size-3" />
                            {t('vrcc.me.disconnect', '断开连接')}
                        </Button>
                    </div>
                ) : (
                    <div className="space-y-2">
                        <p className="text-muted-foreground text-xs leading-relaxed">
                            {t(
                                'vrcc.me.connect_hint',
                                '用下面的连接码在星遇同频网页上同意，这里会自动连上。令牌只存在本机，只发给 vrcc.cc。'
                            )}
                        </p>
                        <Button
                            size="sm"
                            className="w-full"
                            disabled={deviceAuthBusy || deviceAuth != null}
                            onClick={onBegin}
                        >
                            {deviceAuthBusy && (
                                <Spinner className="mr-1 size-3" />
                            )}
                            {t('vrcc.me.begin', '开始连接')}
                        </Button>
                    </div>
                )}
            </section>

            {/* 设备授权进行中 */}
            {deviceAuth && (
                <section className="rounded-xl border p-3">
                    <h3 className="mb-2 text-sm font-semibold">
                        {t('vrcc.me.auth_pending', '等待同意')}
                    </h3>
                    <div className="text-muted-foreground text-xs">
                        {t(
                            'vrcc.me.auth_hint',
                            '在浏览器里打开下面的链接，或手动输入连接码：'
                        )}
                    </div>
                    <div className="bg-muted my-2 rounded-lg border border-dashed py-3 text-center font-mono text-lg font-bold tracking-[0.3em]">
                        {deviceAuth.userCode}
                    </div>
                    {(deviceAuth.verificationUriComplete ??
                        deviceAuth.verificationUri) && (
                        <Button
                            variant="outline"
                            size="sm"
                            className="w-full"
                            onClick={() =>
                                onOpenLink(
                                    deviceAuth.verificationUriComplete ??
                                        deviceAuth.verificationUri ??
                                        SITE
                                )
                            }
                        >
                            <ExternalLink className="mr-1 size-3" />
                            {t('vrcc.me.open_browser', '在浏览器打开')}
                        </Button>
                    )}
                    <div className="text-muted-foreground mt-2 text-center text-[10px]">
                        {t('vrcc.me.auth_expires', {
                            minutes: Math.floor(deviceAuth.expiresInSecs / 60),
                            defaultValue:
                                '{{minutes}} 分钟内有效，每 {{sec}} 秒检查一次',
                            sec: deviceAuth.intervalSecs
                        })}
                    </div>
                </section>
            )}

            {deviceAuthMessage && (
                <div className="border-destructive/40 bg-destructive/10 text-destructive rounded-lg border p-2 text-xs">
                    {deviceAuthMessage}
                </div>
            )}

            {/* 本地用量 */}
            {usage && (
                <section className="rounded-xl border p-3">
                    <div className="mb-2 flex items-baseline gap-2">
                        <h3 className="text-sm font-semibold">
                            {t('vrcc.me.usage', '本客户端用量')}
                        </h3>
                        <span className="text-muted-foreground ml-auto text-[10px] tabular-nums">
                            {usage.date}
                        </span>
                    </div>
                    <UsageBar
                        label={t('vrcc.me.personal_used', '个人接口')}
                        used={usage.personalUsedToday}
                        limit={usage.personalDailyLimit}
                    />
                    <UsageBar
                        label={t('vrcc.me.device_code_used', '申请连接码')}
                        used={usage.deviceCodeUsedToday}
                        limit={usage.deviceCodeDailyLimit}
                    />
                    <UsageBar
                        label={t('vrcc.me.window', '本分钟请求')}
                        used={usage.windowUsed}
                        limit={usage.windowLimit}
                    />
                    <p className="text-muted-foreground mt-2 text-[10px] leading-relaxed">
                        {t(
                            'vrcc.me.usage_note',
                            '这是本客户端自己记的次数，不是服务器的剩余额度——官方没有查询剩余额度的接口。'
                        )}
                    </p>
                </section>
            )}

            <div className="text-muted-foreground text-center text-[10px] leading-relaxed">
                {t('vrcc.me.disclaimer', '第三方客户端，不是星遇同频官方。')}{' '}
                <a
                    href={SITE}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="hover:text-primary inline-flex items-center gap-0.5 hover:underline"
                >
                    vrcc.cc
                    <ExternalLink className="size-2.5" />
                </a>
            </div>
        </div>
    );
}

function Row({ label, value }: { label: string; value: string }) {
    return (
        <div className="flex items-baseline gap-2">
            <span className="text-muted-foreground shrink-0">{label}</span>
            <span className="min-w-0 flex-1 truncate text-right font-mono text-[11px]">
                {value}
            </span>
        </div>
    );
}

function UsageBar({
    label,
    used,
    limit
}: {
    label: string;
    used: number;
    limit: number;
}) {
    // 口径：已用 N/上限，不显示「剩余」。
    const percent = limit > 0 ? Math.min(100, (used / limit) * 100) : 0;
    return (
        <div className="mb-1.5">
            <div className="mb-0.5 flex items-baseline gap-2 text-[11px]">
                <span className="text-muted-foreground">{label}</span>
                <span className="ml-auto tabular-nums">
                    {used}/{limit}
                </span>
            </div>
            <Progress value={percent} className="h-1" />
        </div>
    );
}
