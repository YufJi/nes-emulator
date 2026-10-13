import { useCallback, useMemo, useRef, useState, type ChangeEvent } from 'react';
import { Background, Button, Card, Input, Skeleton, Title } from 'animal-island-ui';
import { ChevronRightIcon, FolderIcon, PlayIcon, SearchIcon, StarIcon } from 'naive-icons';
import { gameId, type GameEntry } from '../games';
import { useGameList } from '../hooks/useGameList';
import { usePagedList } from '../hooks/usePagedList';
import { navigate } from '../router';
import { setPendingRom } from './romHandoff';
import { useTap } from './useTap';

const ROM_ACCEPT = '.nes,.fds,.unf,.unif,application/octet-stream';

/** One game in the list; its own component so each row owns its tap state. */
function GameRow({ game, onPlay }: { game: GameEntry; onPlay: (game: GameEntry) => void }) {
  const tap = useTap(useCallback(() => onPlay(game), [game, onPlay]));

  return (
    <Button type="text" block className="rom-list__row mdash__row" {...tap}>
      <span className="rom-list__inner">
        <span className="ellipsis rom-list__name">{game.name}</span>
        <ChevronRightIcon size={18} color="#c4b89e" aria-hidden />
      </span>
    </Button>
  );
}

/**
 * Home of the phone layout (`/m`): the whole catalogue as a dashboard.
 *
 * It replaces the wide-screen dialog with a page of its own, because on a phone
 * a full-screen list with thumb-sized rows beats a dialog inside a page, and it
 * gives the player a place to come back to.
 */
export function DashboardPage() {
  const { games, error } = useGameList();
  const [query, setQuery] = useState('');
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return games ?? [];
    return (games ?? []).filter((game) => game.name.toLowerCase().includes(needle));
  }, [games, query]);

  const { visible, hasMore, sentinelRef } = usePagedList(results);

  const play = useCallback((game: GameEntry) => {
    navigate(`/m/play?game=${encodeURIComponent(gameId(game))}`);
  }, []);

  /** The player page owns the emulator, so the file travels through the hand-off. */
  const openLocal = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    setPendingRom(file);
    navigate('/m/play');
  }, []);

  const playRandom = useCallback(() => {
    if (!games || games.length === 0) return;
    play(games[Math.floor(Math.random() * games.length)]);
  }, [games, play]);

  // A row is picked while the list is still coasting after a flick, so the
  // action hangs off the tap rather than off the click (see `useTap`).
  const pickLocal = useTap(useCallback(() => fileInputRef.current?.click(), []));
  const pickRandom = useTap(playRandom);

  return (
    <Background type="sprinkles" className="app-shell">
      <div className="mdash">
        <header className="mdash__head">
          <div className="brand">
            <Card color="app-green" className="brand__badge">
              <PlayIcon size={22} color="currentColor" aria-hidden />
            </Card>
            <div className="brand__text">
              <h1 className="visually-hidden">NES Emulator 手机版</h1>
              <Title size="small" color="app-teal">
                NES Emulator
              </Title>
              <p className="brand__sub">手机版 · 点一个游戏开玩</p>
            </div>
          </div>
        </header>

        <div className="mdash__quick">
          <Button
            size="large"
            block
            icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
            {...pickLocal}
          >
            本地 ROM
          </Button>
          <Button
            size="large"
            block
            icon={<StarIcon size={18} color="currentColor" aria-hidden />}
            {...pickRandom}
            disabled={!games || games.length === 0}
          >
            随便玩一个
          </Button>
        </div>

        {/* Pinned while the list scrolls, so a different game is always a tap away. */}
        <div className="mdash__search">
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
            <Skeleton variant="rect" widthValue="100%" heightValue={46} />
            <Skeleton variant="rect" widthValue="100%" heightValue={46} />
            <Skeleton variant="rect" widthValue="100%" heightValue={46} />
            <Skeleton variant="rect" widthValue="100%" heightValue={46} />
          </div>
        )}

        {games && results.length === 0 && (
          <p className="rom-list__empty">没有匹配 “{query.trim()}” 的游戏。</p>
        )}

        {visible.length > 0 && (
          <ul className="rom-list">
            {visible.map((game) => (
              <li key={gameId(game)}>
                <GameRow game={game} onPlay={play} />
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
      </div>

      <input
        ref={fileInputRef}
        className="visually-hidden"
        type="file"
        accept={ROM_ACCEPT}
        onChange={openLocal}
      />
    </Background>
  );
}
