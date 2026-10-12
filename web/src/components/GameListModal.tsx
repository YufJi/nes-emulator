import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Input, Modal, Pagination, Skeleton, Tag } from 'animal-island-ui';
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
 * Rows per page. The list holds hundreds of games, so it is paged rather than
 * rendered in one go — only the current page reaches the DOM.
 */
const PAGE_SIZE = 10;

/**
 * Searchable game list, so a game can be picked without knowing its URL.
 *
 * The list is a static asset beside the page (`games.json`, regenerate it with
 * `npm run build:games`), fetched the first time the dialog is opened. Escape,
 * clicking the mask and the focus trap are the island Modal's own behaviour.
 */
export function GameListModal({ busy, currentUrl, onSelect, onClose }: GameListModalProps) {
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(1);

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

  // Narrowing the list can leave the current page past the end, so clamp it.
  const pageCount = Math.max(1, Math.ceil(results.length / PAGE_SIZE));
  const current = Math.min(page, pageCount);
  const visible = results.slice((current - 1) * PAGE_SIZE, current * PAGE_SIZE);

  const search = useCallback((value: string) => {
    setQuery(value);
    setPage(1);
  }, []);

  return (
    <Modal
      open
      title="游戏列表"
      width={620}
      typewriter={false}
      onClose={onClose}
      footer={
        results.length > PAGE_SIZE ? (
          <Pagination
            total={results.length}
            current={current}
            pageSize={PAGE_SIZE}
            showTotal
            onChange={(next) => setPage(next)}
          />
        ) : null
      }
    >
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
