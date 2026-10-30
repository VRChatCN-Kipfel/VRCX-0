import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * 记录榜单里哪些行当前在滚动视口内。
 *
 * ## 为什么不用 `loading="lazy"`
 *
 * 浏览器原生懒加载不是「可见才加载」：Chrome 在距离视口约 **1250px** 处就
 * 开始请求。20 行的榜单通常一屏加一点就全落在这个范围内，等于没有懒加载。
 *
 * 而这里要对「可见」做两件不同的事（见 `useVrccWorldCovers`），必须知道
 * 精确的可见集合，所以用 `IntersectionObserver`。
 *
 * ## 用法
 *
 * 每一行把 DOM 节点注册进来，组件据此判断「这一行要不要现在就去取星遇的图」。
 * 离开视口时把它从集合里移除——滚回去时会重新加入，图会重新请求，但浏览器
 * 的 HTTP 缓存会兜住（星遇的图带 4 小时强缓存），所以不会真的重下。
 */
export function useVrccRowVisibility() {
    const [visible, setVisible] = useState<ReadonlySet<string>>(
        () => new Set()
    );
    const observerRef = useRef<IntersectionObserver | null>(null);
    // 元素 → 世界 id。用 Map 而不是 data 属性：不需要碰 DOM 字符串。
    const elementsRef = useRef(new WeakMap<Element, string>());
    const visibleRef = useRef(new Set<string>());

    useEffect(() => {
        if (typeof IntersectionObserver === 'undefined') {
            // 极旧环境：退化成「全部可见」，宁可多加载也不要一片空白。
            return;
        }
        const observer = new IntersectionObserver(
            (entries) => {
                let changed = false;
                for (const entry of entries) {
                    const worldId = elementsRef.current.get(entry.target);
                    if (!worldId) {
                        continue;
                    }
                    if (entry.isIntersecting) {
                        if (!visibleRef.current.has(worldId)) {
                            visibleRef.current.add(worldId);
                            changed = true;
                        }
                    } else if (visibleRef.current.delete(worldId)) {
                        changed = true;
                    }
                }
                if (changed) {
                    // 复制一份再 set：直接传同一个 Set 引用，React 认为没变。
                    setVisible(new Set(visibleRef.current));
                }
            },
            // **严格视口**：`rootMargin` 留 0，不提前预载。
            // 代价是快速滚动时可能有一拍空白；星遇的图带 4 小时强缓存，
            // 滚回去不会真的重下。若要消除那一拍，把它改成 `120px 0px` 即可。
            { rootMargin: '0px' }
        );
        observerRef.current = observer;
        // 清理函数里不能直接读写 `ref.current`（React 认为那样会漏掉后续
        // 赋值）。先把这一份集合抓到局部变量，清理时清它。
        const seen = visibleRef.current;
        return () => {
            observer.disconnect();
            observerRef.current = null;
            seen.clear();
        };
    }, []);

    /** 注册一行。React 的 ref 回调每帧都会带着 null 再调用一次，属正常。 */
    const observe = useCallback((element: Element | null, worldId: string) => {
        const observer = observerRef.current;
        if (!observer || !element) {
            return;
        }
        elementsRef.current.set(element, worldId);
        observer.observe(element);
    }, []);

    return { visible, observe };
}
