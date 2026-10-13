import { useCallback, useEffect, useState } from 'react';

/**
 * Rows added each time the end of the list scrolls into view. The catalogue
 * holds almost a thousand games, so a scrolling list grows a slice at a time
 * rather than putting every row in the DOM at once.
 */
const PAGE_SIZE = 20;

/**
 * How far below the list the next slice starts rendering, so the rows are
 * usually in place before they are scrolled into sight.
 */
const LOAD_MARGIN = '200px 0px';

/**
 * The list is the scroller in the dialog, but just the page in the phone
 * layout, and the sentinel cannot see through to either — so it looks for its
 * own scrolling ancestor and falls back to the viewport.
 */
function scrollParent(node: Element): Element | null {
  for (let parent = node.parentElement; parent; parent = parent.parentElement) {
    const { overflowY } = window.getComputedStyle(parent);
    if (overflowY === 'auto' || overflowY === 'scroll') return parent;
  }
  return null;
}

export interface UsePagedListResult<T> {
  /** The rows to render for this slice. */
  visible: T[];
  hasMore: boolean;
  /** Attach to an element after the last row; it grows the slice when reached. */
  sentinelRef: (node: Element | null) => void;
}

/**
 * Render `items` a slice at a time, growing as the end of the list is reached.
 *
 * Pass a memoised array: a new identity means new contents (a new search term),
 * which puts the slice back to the first page.
 */
export function usePagedList<T>(items: readonly T[]): UsePagedListResult<T> {
  const [count, setCount] = useState(PAGE_SIZE);
  const [sentinel, setSentinel] = useState<Element | null>(null);

  // A stable callback ref, so React only calls it when the node appears or goes
  // away — and not on every slice, which would re-run the observer effect.
  const sentinelRef = useCallback((node: Element | null) => setSentinel(node), []);

  useEffect(() => setCount(PAGE_SIZE), [items]);

  const visible = items.slice(0, count);
  const hasMore = visible.length < items.length;

  // Re-observing after every slice re-runs the callback immediately, so a slice
  // too short to push the sentinel out of sight keeps the list filling instead
  // of stalling with room left in the scroller.
  useEffect(() => {
    if (!hasMore || !sentinel) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setCount((loaded) => loaded + PAGE_SIZE);
        }
      },
      { root: scrollParent(sentinel), rootMargin: LOAD_MARGIN },
    );
    observer.observe(sentinel);

    return () => observer.disconnect();
  }, [hasMore, count, sentinel]);

  return { visible, hasMore, sentinelRef };
}
