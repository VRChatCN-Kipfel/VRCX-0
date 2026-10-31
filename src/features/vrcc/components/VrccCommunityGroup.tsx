import { Users } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/ui/shadcn/button';

import {
    VRCC_COMMUNITY_GROUP_ID,
    useVrccCommunityGroup
} from '../useVrccCommunityGroup';

/**
 * 「VRChat 中文社区」群组的加入入口。
 *
 * 官方 VRCX 脚本在面板里放了这一节；这里同样只做一个入口。
 *
 * **合规边界**：点下去只会打开 VRCX-0 自己的群组详情窗，加入动作由玩家
 * 在那里自己点。这里没有、也不应该有代发加入请求的通道。
 *
 * 已加入时按钮变灰并写「已加入」——这是玩家确认状态的方式，比把整节藏起来
 * 更容易理解「这个入口存在，只是我已经在里面了」。
 */
export function VrccCommunityGroup({
    onOpenGroup
}: {
    onOpenGroup: (groupId: string) => void;
}) {
    const { t } = useTranslation();
    const { joined, loading } = useVrccCommunityGroup();
    const isJoined = joined === true;

    return (
        <section className="shrink-0 rounded-xl border p-3">
            <div className="mb-2 flex items-center gap-2">
                <Users className="text-muted-foreground size-3.5 shrink-0" />
                <span className="text-muted-foreground text-xs">
                    {t('vrcc.community.label')}
                </span>
            </div>
            <div className="flex items-center gap-3">
                <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">
                        {t('vrcc.community.name')}
                    </div>
                    <div className="text-muted-foreground truncate text-xs">
                        {isJoined
                            ? t('vrcc.community.joined_hint')
                            : t('vrcc.community.hint')}
                    </div>
                </div>
                <Button
                    variant={isJoined ? 'secondary' : 'default'}
                    size="sm"
                    className="shrink-0"
                    disabled={isJoined || loading}
                    onClick={() => onOpenGroup(VRCC_COMMUNITY_GROUP_ID)}
                    title={
                        isJoined
                            ? t('vrcc.community.joined')
                            : t('vrcc.community.open')
                    }
                >
                    {isJoined
                        ? t('vrcc.community.joined')
                        : t('vrcc.community.join')}
                </Button>
            </div>
        </section>
    );
}
