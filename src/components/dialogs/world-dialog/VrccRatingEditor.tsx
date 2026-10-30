import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';
import {
    deleteVrccMapRating,
    rateVrccMap
} from '@/repositories/vrccMapRepository';
import { Button } from '@/ui/shadcn/button';

import { StarRatingInput } from './StarRatingInput';

/** 短评最长字数，官方 `maxLength: 30`。 */
const MAX_CHARS = 30;

/**
 * 打分编辑器：选分 + 写短评 + 提交，外加一个「删掉我的评分」。
 *
 * ## 为什么是「两步」（选好 → 点提交）而不是点星即提交
 *
 * 官方《使用要求》把打分列为敏感写操作，明确要求「由玩家本人**每次**主动
 * 点击触发」。点星即提交会带来两个问题：
 * 1. 玩家想改分数时会连点几下，等于连发几个写请求；
 * 2. 短评还没写完，分数已经提交出去了。
 *
 * 所以分数与短评先在本地攒着，一次提交。
 *
 * ## 三种提交意图分得很清楚
 *
 * | 玩家做了什么 | textMode |
 * |---|---|
 * | 只改分数，短评框保持原样 | `keep` |
 * | 把短评清空了 | `clear` |
 * | 写了新的短评 | `write` |
 *
 * 这个区分不是我们发明的，是官方接口的语义；搞错就会把别人的短评删掉。
 *
 * ## 失败不吞
 *
 * 写操作失败一律显示成一行错误，**不做重试**。短评被审核拒绝时服务器会在
 * `why` 里说明具体原因（例如「请不要在短评里留联系方式」），那句话对玩家
 * 比「操作失败」有用得多，所以原样显示。
 */
export function VrccRatingEditor({
    worldId,
    currentScore,
    currentText,
    check,
    onDone
}: {
    worldId: string;
    currentScore: number | null;
    currentText: string | null;
    check: string | null;
    onDone: () => void;
}) {
    const { t } = useTranslation();
    // 本地草稿：玩家还没提交的选择。
    const [draftScore, setDraftScore] = useState<number | null>(currentScore);
    const [draftText, setDraftText] = useState<string>(currentText ?? '');
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const trimmed = draftText.trim();
    const scoreChanged = draftScore !== null && draftScore !== currentScore;
    const textChanged = trimmed !== (currentText ?? '').trim();
    const canSubmit =
        draftScore !== null && !busy && (scoreChanged || textChanged);
    const overLimit = trimmed.length > MAX_CHARS;

    async function submit() {
        if (!canSubmit || overLimit) {
            return;
        }
        setBusy(true);
        setError(null);
        try {
            // 意图判定：
            // - 短评没动 → keep（只改分数，保留原短评）
            // - 清空了 → clear
            // - 写了新的 → write
            const mode: 'keep' | 'clear' | 'write' = !textChanged
                ? 'keep'
                : trimmed.length === 0
                  ? 'clear'
                  : 'write';
            await rateVrccMap(
                worldId,
                draftScore as number,
                mode,
                mode === 'write' ? trimmed : null
            );
            onDone();
        } catch (caught) {
            // 写操作不重试。审核拒绝的 `why` 在 message 里，原样显示。
            setError(
                caught instanceof Error
                    ? caught.message
                    : t('dialog.world.vrcc.rate_failed')
            );
        } finally {
            setBusy(false);
        }
    }

    async function removeRating() {
        setBusy(true);
        setError(null);
        try {
            await deleteVrccMapRating(worldId);
            setDraftScore(null);
            setDraftText('');
            onDone();
        } catch (caught) {
            setError(
                caught instanceof Error
                    ? caught.message
                    : t('dialog.world.vrcc.rate_failed')
            );
        } finally {
            setBusy(false);
        }
    }

    return (
        <div className="bg-muted/40 flex min-w-0 flex-col gap-2 rounded-md border px-2 py-2">
            <div className="flex min-w-0 items-center gap-2">
                <span className="text-muted-foreground shrink-0 text-[11px]">
                    {t('dialog.world.vrcc.my_rating')}
                </span>
                {/* 审核中/未通过：改分不会重新提交短评，但要提示现状 */}
                {check && check !== 'ok' ? (
                    <span className="text-muted-foreground bg-background/60 shrink-0 rounded px-1 text-[10px]">
                        {t(
                            check === 'wait'
                                ? 'dialog.world.vrcc.check_wait'
                                : 'dialog.world.vrcc.check_hidden'
                        )}
                    </span>
                ) : null}
            </div>

            <StarRatingInput
                value={draftScore}
                onChange={setDraftScore}
                disabled={busy}
            />

            <div className="flex min-w-0 flex-col gap-1">
                <textarea
                    value={draftText}
                    onChange={(event) => setDraftText(event.target.value)}
                    disabled={busy}
                    rows={2}
                    placeholder={t('dialog.world.vrcc.words_placeholder')}
                    className={cn(
                        'bg-background min-h-0 w-full resize-none rounded border px-2 py-1 text-xs',
                        'focus-visible:ring-ring focus-visible:ring-1 focus-visible:outline-none'
                    )}
                />
                <div className="text-muted-foreground flex items-center gap-2 text-[10px]">
                    <span
                        className={cn(
                            'tabular-nums',
                            overLimit && 'text-destructive'
                        )}
                    >
                        {trimmed.length}/{MAX_CHARS}
                    </span>
                    <span className="ml-auto flex items-center gap-1.5">
                        {currentScore !== null ? (
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-6 px-1.5 text-[10px]"
                                disabled={busy}
                                onClick={() => void removeRating()}
                            >
                                {t('dialog.world.vrcc.rate_delete')}
                            </Button>
                        ) : null}
                        <Button
                            type="button"
                            size="sm"
                            className="h-6 px-2 text-[10px]"
                            disabled={!canSubmit || overLimit}
                            onClick={() => void submit()}
                        >
                            {busy
                                ? t('dialog.world.vrcc.rate_submitting')
                                : t('dialog.world.vrcc.rate_submit')}
                        </Button>
                    </span>
                </div>
                {error ? (
                    <p className="text-destructive text-[11px] leading-snug break-words">
                        {error}
                    </p>
                ) : null}
            </div>
        </div>
    );
}
