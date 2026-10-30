// @vitest-environment jsdom

import {
    cleanup,
    fireEvent,
    render,
    screen,
    waitFor
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const rateMap = vi.fn();
const deleteMapRating = vi.fn();

vi.mock('@/repositories/vrccMapRepository', () => ({
    rateVrccMap: (...args: unknown[]) => rateMap(...args),
    deleteVrccMapRating: (...args: unknown[]) => deleteMapRating(...args)
}));

vi.mock('react-i18next', () => ({
    useTranslation: () => ({
        t: (key: string, options?: Record<string, unknown>) => {
            if (key === 'dialog.world.vrcc.rate_value') {
                return `${options?.score}/${options?.max}`;
            }
            return key;
        }
    })
}));

import { VrccRatingEditor } from './VrccRatingEditor';

function renderEditor(overrides: Record<string, unknown> = {}) {
    const onDone = vi.fn();
    render(
        <VrccRatingEditor
            worldId="wrld_test"
            currentScore={null}
            currentText={null}
            check={null}
            onDone={onDone}
            {...overrides}
        />
    );
    return { onDone };
}

beforeEach(() => {
    rateMap.mockReset();
    deleteMapRating.mockReset();
    rateMap.mockResolvedValue({});
    deleteMapRating.mockResolvedValue(null);
});

afterEach(() => {
    cleanup();
});

describe('VrccRatingEditor', () => {
    it('没选分数时提交按钮不可用', () => {
        renderEditor();
        const submit = screen.getByText(
            'dialog.world.vrcc.rate_submit'
        ) as HTMLButtonElement;
        expect(submit.disabled).toBe(true);
    });

    it('只改分数时用 keep，**不能**把短评删掉', async () => {
        const { onDone } = renderEditor({
            currentScore: 5,
            currentText: '原来的短评'
        });

        // 点第 9 档（第 5 颗星左半）
        fireEvent.click(screen.getAllByRole('radio')[8]);
        fireEvent.click(screen.getByText('dialog.world.vrcc.rate_submit'));

        await waitFor(() => expect(rateMap).toHaveBeenCalled());
        // 第三个参数是 textMode，第四个是 text
        expect(rateMap).toHaveBeenCalledWith('wrld_test', 9, 'keep', null);
        await waitFor(() => expect(onDone).toHaveBeenCalled());
    });

    it('把短评清空时用 clear，分数保留', async () => {
        renderEditor({ currentScore: 5, currentText: '原来的短评' });

        const box = screen.getByRole('textbox') as HTMLTextAreaElement;
        fireEvent.change(box, { target: { value: '   ' } });
        fireEvent.click(screen.getByText('dialog.world.vrcc.rate_submit'));

        await waitFor(() =>
            expect(rateMap).toHaveBeenCalledWith('wrld_test', 5, 'clear', null)
        );
    });

    it('写新短评时用 write，并带上正文', async () => {
        renderEditor({ currentScore: 5, currentText: null });

        const box = screen.getByRole('textbox') as HTMLTextAreaElement;
        fireEvent.change(box, { target: { value: '光影很舒服' } });
        fireEvent.click(screen.getByText('dialog.world.vrcc.rate_submit'));

        await waitFor(() =>
            expect(rateMap).toHaveBeenCalledWith(
                'wrld_test',
                5,
                'write',
                '光影很舒服'
            )
        );
    });

    it('超过 30 字时不许提交', () => {
        renderEditor({ currentScore: 5 });
        const box = screen.getByRole('textbox') as HTMLTextAreaElement;
        fireEvent.change(box, { target: { value: '一'.repeat(31) } });

        const submit = screen.getByText(
            'dialog.world.vrcc.rate_submit'
        ) as HTMLButtonElement;
        expect(submit.disabled).toBe(true);
    });

    it('提交失败时把服务器的话原样显示出来（审核拒绝要说原因）', async () => {
        rateMap.mockRejectedValue(new Error('请不要在短评里留联系方式'));
        renderEditor({ currentScore: null });

        fireEvent.click(screen.getAllByRole('radio')[9]);
        fireEvent.click(screen.getByText('dialog.world.vrcc.rate_submit'));

        expect(
            await screen.findByText('请不要在短评里留联系方式')
        ).toBeTruthy();
    });

    it('已打过分时才显示删除按钮，且删除会调接口', async () => {
        renderEditor({ currentScore: 7, currentText: 'x' });

        fireEvent.click(screen.getByText('dialog.world.vrcc.rate_delete'));

        await waitFor(() =>
            expect(deleteMapRating).toHaveBeenCalledWith('wrld_test')
        );
    });

    it('没打过分时不显示删除按钮', () => {
        renderEditor({ currentScore: null });
        expect(screen.queryByText('dialog.world.vrcc.rate_delete')).toBeNull();
    });
});
