import { useMemo, useState } from 'react';
import { Button, Input, Modal, Skeleton, Tag } from 'animal-island-ui';
import { SearchIcon } from 'naive-icons';
import { gameId } from '../games';
import { useGameList } from '../hooks/useGameList';
import { usePagedList } from '../hooks/usePagedList';

interface GameListModalProps {
  /** While a download is in flight the rows are disabled. */
  busy: boolean;
  /** URL of the running cartridge, marked in the list. */
  currentUrl: string | null;
  onSelect: (url: string) => void;
  onClose: () => void;
}

/**
 * Searchable game list, so a game can be picked without knowing its URL.
 *
 * The rows come from the shared catalogue (`src/games.ts`) and are rendered a
 * slice at a time as the list is scrolled — there is no pager to click. Escape,
 * clicking the mask and the focus trap are the island Modal's own behaviour.
 */
export function GameListModal({ busy, currentUrl, onSelect, onClose }: GameListModalProps) {
  const { games, error } = useGameList();
  const [query, setQuery] = useState('');

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return games ?? [];
    return (games ?? []).filter((game) => game.name.toLowerCase().includes(needle));
  }, [games, query]);

  const { visible, hasMore, sentinelRef } = usePagedList(results);

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
          onChange={(event) => setQuery(event.target.value)}
          onClear={() => setQuery('')}
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
            <li key={gameId(game)}>
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
          {hasMore && <li ref={sentinelRef} className="rom-list__more" aria-hidden />}
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
