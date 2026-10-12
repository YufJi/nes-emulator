import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, Input, Modal, Skeleton, Tag } from 'animal-island-ui';
import { SearchIcon } from 'naive-icons';

export interface GameEntry {
  name: string;
  url: string;
}

interface GameListModalProps {
  /** While a download is in flight the rows are disabled. */
  busy: boolean;
  /** URL of the running cartridge, marked in the list. */
  currentUrl: string | null;
  onSelect: (url: string) => void;
  onClose: () => void;
}

/**
 * Rows added each time the end of the list scrolls into view. The list holds
 * hundreds of games, so it grows a slice at a time rather than rendering in one
 * go — only the rows reached so far are in the DOM.
 */
const PAGE_SIZE = 20;

/**
 * How far below the list the next slice starts rendering. Rendering rows is
 * cheap (the data is already in memory), so they are usually in place before
 * they are scrolled into sight.
 */
const LOAD_MARGIN = '200px 0px';

/**
 * The Modal body is the scroller and cannot be reached through a ref, so the
 * sentinel looks for its own scrolling ancestor.
 */
function scrollParent(node: HTMLElement): HTMLElement | null {
  for (let parent = node.parentElement; parent; parent = parent.parentElement) {
    const { overflowY } = window.getComputedStyle(parent);
    if (overflowY === 'auto' || overflowY === 'scroll') return parent;
  }
  return null;
}

/**
 * Searchable game list, so a game can be picked without knowing its URL.
 *
 * The list is a static asset beside the page (`games.json`, regenerate it with
 * `npm run build:games`), fetched the first time the dialog is opened. It loads
 * further slices as it is scrolled — there is no pager to click. Escape,
 * clicking the mask and the focus trap are the island Modal's own behaviour.
 */
export function GameListModal({ busy, currentUrl, onSelect, onClose }: GameListModalProps) {
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [count, setCount] = useState(PAGE_SIZE);
  const sentinel = useRef<HTMLLIElement | null>(null);

  useEffect(() => {
    let cancelled = false;

    fetch(`${import.meta.env.BASE_URL}games.json`)
      .then((response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response.json() as Promise<GameEntry[]>;
      })
      .then((list) => {
        if (!cancelled) setGames(list);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        setError(`游戏列表载入失败：${cause instanceof Error ? cause.message : String(cause)}`);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return games ?? [];
    return (games ?? []).filter((game) => game.name.toLowerCase().includes(needle));
  }, [games, query]);

  const visible = results.slice(0, count);
  const hasMore = visible.length < results.length;

  // Re-observing after every slice re-runs the callback immediately, so a slice
  // too short to push the sentinel out of sight keeps the list filling instead
  // of stalling with room left in the scroller.
  useEffect(() => {
    const node = sentinel.current;
    if (!hasMore || !node) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setCount((loaded) => loaded + PAGE_SIZE);
        }
      },
      { root: scrollParent(node), rootMargin: LOAD_MARGIN },
    );
    observer.observe(node);

    return () => observer.disconnect();
  }, [hasMore, count, results]);

  const search = useCallback((value: string) => {
    setQuery(value);
    setCount(PAGE_SIZE);
  }, []);

  return (
    <Modal
      open
      title="游戏列表"
      width={620}
      typewriter={false}
      onClose={onClose}
      footer={null}
    >
      {/* Sticky, so the box stays reachable however far the list is scrolled. */}
      <div className="rom-list__search">
        <Input
          size="large"
          shadow
          allowClear
          value={query}
          onChange={(event) => search(event.target.value)}
          onClear={() => search('')}
          placeholder="搜索游戏名…"
          aria-label="搜索游戏"
          spellCheck={false}
          autoComplete="off"
          prefix={<SearchIcon size={18} color="#b09a7e" aria-hidden />}
        />
      </div>

      {error && <p className="rom-list__empty">{error}</p>}

      {!games && !error && (
        <div className="rom-list__skeleton">
          <Skeleton variant="rect" widthValue="100%" heightValue={40} />
          <Skeleton variant="rect" widthValue="100%" heightValue={40} />
          <Skeleton variant="rect" widthValue="100%" heightValue={40} />
          <Skeleton variant="rect" widthValue="100%" heightValue={40} />
        </div>
      )}

      {games && results.length === 0 && (
        <p className="rom-list__empty">没有匹配 “{query.trim()}” 的游戏。</p>
      )}

      {visible.length > 0 && (
        <ul className="rom-list">
          {visible.map((game) => (
            <li key={game.url}>
              <Button
                type="text"
                block
                size="middle"
                className="rom-list__row"
                onClick={() => onSelect(game.url)}
                disabled={busy}
              >
                <span className="rom-list__inner">
                  <span className="ellipsis rom-list__name">{game.name}</span>
                  {game.url === currentUrl && (
                    <Tag size="small" color="app-green" variant="solid">
                      当前
                    </Tag>
                  )}
                </span>
              </Button>
            </li>
          ))}
          {hasMore && <li ref={sentinel} className="rom-list__more" aria-hidden />}
        </ul>
      )}

      <p className="rom-list__foot">
        {games ? `共 ${games.length} 个游戏` : '游戏列表'}
        {' · 资料来自 '}
        <a href="https://github.com/mengfei523060/nesrom" target="_blank" rel="noreferrer">
          mengfei523060/nesrom
        </a>
      </p>
    </Modal>
  );
}
