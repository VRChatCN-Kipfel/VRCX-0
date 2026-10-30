// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

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

import { StarRatingInput } from './StarRatingInput';

afterEach(() => {
    cleanup();
});

describe('StarRatingInput', () => {
    it('十档：每颗星左右各是一个 radio', () => {
        render(<StarRatingInput value={null} onChange={() => {}} />);
        // 5 颗星 × 2 半 = 10 个档位
        expect(screen.getAllByRole('radio').length).toBe(10);
    });

    it('点第 1 颗星左半是 1 分、右半是 2 分', () => {
        const onChange = vi.fn();
        render(<StarRatingInput value={null} onChange={onChange} />);
        const radios = screen.getAllByRole('radio');

        fireEvent.click(radios[0]);
        expect(onChange).toHaveBeenLastCalledWith(1);

        fireEvent.click(radios[1]);
        expect(onChange).toHaveBeenLastCalledWith(2);
    });

    it('最后一颗星右半是 10 分（上界）', () => {
        const onChange = vi.fn();
        render(<StarRatingInput value={null} onChange={onChange} />);
        const radios = screen.getAllByRole('radio');

        fireEvent.click(radios[9]);
        expect(onChange).toHaveBeenLastCalledWith(10);
    });

    it('奇数档位只由左半产生、偶数档位只由右半产生', () => {
        const onChange = vi.fn();
        render(<StarRatingInput value={null} onChange={onChange} />);
        const radios = screen.getAllByRole('radio');

        // 逐档点一遍，确认 1..10 都能取到且不重复
        const seen: number[] = [];
        for (const radio of radios) {
            onChange.mockClear();
            fireEvent.click(radio);
            seen.push(onChange.mock.calls[0][0] as number);
        }
        expect(seen).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    });

    it('aria-checked 标出当前分数所在的那一档', () => {
        render(<StarRatingInput value={7} onChange={() => {}} />);
        const radios = screen.getAllByRole('radio');
        // 7 分 = 第 4 颗星的左半（索引 6）
        expect(radios[6].getAttribute('aria-checked')).toBe('true');
        expect(radios[5].getAttribute('aria-checked')).toBe('false');
    });

    it('禁用时点击不触发 onChange', () => {
        const onChange = vi.fn();
        render(<StarRatingInput value={3} onChange={onChange} disabled />);
        const radios = screen.getAllByRole('radio');
        fireEvent.click(radios[9]);
        expect(onChange).not.toHaveBeenCalled();
    });

    it('右方向键逐档前进，且不越过 10', () => {
        const onChange = vi.fn();
        render(<StarRatingInput value={10} onChange={onChange} />);
        const radios = screen.getAllByRole('radio');

        fireEvent.keyDown(radios[9], { key: 'ArrowRight' });
        // 已经在 10，不该变成 11
        expect(onChange).toHaveBeenLastCalledWith(10);
    });

    it('左方向键逐档后退，且不越过 1', () => {
        const onChange = vi.fn();
        render(<StarRatingInput value={1} onChange={onChange} />);
        const radios = screen.getAllByRole('radio');

        fireEvent.keyDown(radios[0], { key: 'ArrowLeft' });
        expect(onChange).toHaveBeenLastCalledWith(1);
    });
});
