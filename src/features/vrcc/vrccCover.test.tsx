// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { VrccCover } from './components/VrccCover';
import { vrccCoverUrl } from './vrccCover';

afterEach(() => {
    cleanup();
});

describe('vrccCoverUrl', () => {
    it('优先用完整的 imgUrl', () => {
        expect(
            vrccCoverUrl('https://example.test/cover.png', '/api/match?xdimg=1')
        ).toBe('https://example.test/cover.png');
    });

    it('没有 imgUrl 时把相对 img 拼到站点 origin 后面', () => {
        expect(vrccCoverUrl(null, '/api/match?xdimg=1')).toBe(
            'https://vrcc.cc/api/match?xdimg=1'
        );
    });

    it('img 本身是绝对地址时原样用，不再拼 origin', () => {
        expect(vrccCoverUrl(undefined, 'https://cdn.test/a.png')).toBe(
            'https://cdn.test/a.png'
        );
    });

    it('缺前导斜杠的相对路径也拼得对', () => {
        expect(vrccCoverUrl(null, 'api/match?xdimg=2')).toBe(
            'https://vrcc.cc/api/match?xdimg=2'
        );
    });

    it('两个字段都没有时返回 null，而不是空字符串', () => {
        // 空字符串会变成 `<img src="">`，浏览器会去请求当前页面本身。
        expect(vrccCoverUrl(null, null)).toBeNull();
        expect(vrccCoverUrl('', '   ')).toBeNull();
    });

    it('imgUrl 不是 https 时不当绝对地址用', () => {
        // 官方也可能在这个字段给相对路径，那种情况要走 img 的拼装逻辑。
        expect(vrccCoverUrl('/api/match?xdimg=3', null)).toBe(
            'https://vrcc.cc/api/match?xdimg=3'
        );
    });
});

describe('VrccCover', () => {
    it('没有地址时不渲染任何节点', () => {
        const { container } = render(<VrccCover src={null} />);
        expect(container.querySelector('img')).toBeNull();
    });

    it('有地址时渲染惰性加载的图片', () => {
        const { container } = render(
            <VrccCover src="https://example.test/a.png" />
        );
        const img = container.querySelector('img');
        expect(img).toBeTruthy();
        expect(img?.getAttribute('loading')).toBe('lazy');
        expect(img?.getAttribute('aria-hidden')).toBe('true');
    });

    it('加载失败后彻底不渲染，行退回纯文字', () => {
        // 封面来自网页端内部接口，官方没承诺稳定，图裂了不能留破图标。
        const { container } = render(
            <VrccCover src="https://example.test/broken.png" />
        );
        const img = container.querySelector('img');
        expect(img).toBeTruthy();

        fireEvent.error(img as HTMLImageElement);

        expect(container.querySelector('img')).toBeNull();
    });
});
