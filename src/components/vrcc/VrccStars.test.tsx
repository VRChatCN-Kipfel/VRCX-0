// @vitest-environment jsdom

import { cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { VrccStars } from './VrccStars';

afterEach(() => {
    cleanup();
});

/** 读出每颗星被填充的百分比（0–100），用来断言半星逻辑。 */
function fillRatios(container: HTMLElement): number[] {
    return [...container.querySelectorAll('span[aria-hidden="true"]')].map(
        (element) => Number.parseFloat((element as HTMLElement).style.width)
    );
}

describe('VrccStars', () => {
    it('10 分画满五颗星', () => {
        const { container } = render(<VrccStars score={10} />);
        expect(fillRatios(container)).toEqual([100, 100, 100, 100, 100]);
    });

    it('2 分只填满第一颗', () => {
        const { container } = render(<VrccStars score={2} />);
        expect(fillRatios(container)).toEqual([100]);
    });

    it('1 分填半颗（1–10 折成 0–5 星）', () => {
        const { container } = render(<VrccStars score={1} />);
        expect(fillRatios(container)).toEqual([50]);
    });

    it('9 分是四颗满加半颗', () => {
        const { container } = render(<VrccStars score={9} />);
        expect(fillRatios(container)).toEqual([100, 100, 100, 100, 50]);
    });

    it('小数分数按比例填充，8.4 比 8.2 多一点', () => {
        const a = render(<VrccStars score={8.4} />);
        const aRatios = fillRatios(a.container);
        a.unmount();
        const b = render(<VrccStars score={8.2} />);
        const bRatios = fillRatios(b.container);

        // 第 5 颗星的填充比例不同（分别是 20% 和 10%）
        expect(aRatios[4]).toBeCloseTo(20, 5);
        expect(bRatios[4]).toBeCloseTo(10, 5);
    });

    it('分数为 null 时五颗全空，不崩', () => {
        const { container } = render(<VrccStars score={null} />);
        expect(fillRatios(container)).toEqual([]);
    });

    it('超出范围的值被夹住（负数不炸、20 分也只满五颗）', () => {
        const high = render(<VrccStars score={20} />);
        expect(fillRatios(high.container)).toEqual([100, 100, 100, 100, 100]);
        high.unmount();

        const low = render(<VrccStars score={-5} />);
        expect(fillRatios(low.container)).toEqual([]);
    });

    it('读屏拿到的是一位小数的分数', () => {
        const { getByRole } = render(<VrccStars score={8.44} />);
        expect(getByRole('img').getAttribute('aria-label')).toBe('8.4');
    });
});
