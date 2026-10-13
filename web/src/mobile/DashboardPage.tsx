import { useCallback, useMemo, useRef, useState, type ChangeEvent } from 'react';
import { Background, Button, Input, Skeleton, Title } from 'animal-island-ui';
import { ChevronRightIcon, FolderIcon, SearchIcon, StarIcon } from 'naive-icons';
import { gameId, type GameEntry } from '../games';
import { useGameList } from '../hooks/useGameList';
import { usePagedList } from '../hooks/usePagedList';
import { APP_NAME, useI18n } from '../i18n';
import { navigate } from '../router';
import { Disclaimer } from '../components/Disclaimer';
import { LanguageSwitcher } from '../components/LanguageSwitcher';
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
  const { t } = useI18n();

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
            {/* The tab icon doubles as the logo, so the tab and the page can
                never drift apart. */}
            <img
              className="brand__logo"
              src={`${import.meta.env.BASE_URL}favicon.svg`}
              alt=""
              width={46}
              height={46}
            />
            <div className="brand__text">
              <h1 className="visually-hidden">{t('brand.mobileTitle')}</h1>
              <Title size="small" color="app-teal">
                {APP_NAME}
              </Title>
              <p className="brand__sub">{t('dashboard.sub')}</p>
            </div>
          </div>
          <LanguageSwitcher />
        </header>

        <div className="mdash__quick">
          <Button
            size="large"
            block
            icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
            {...pickLocal}
          >
            {t('dashboard.localRom')}
          </Button>
          <Button
            size="large"
            block
            icon={<StarIcon size={18} color="currentColor" aria-hidden />}
            {...pickRandom}
            disabled={!games || games.length === 0}
          >
            {t('dashboard.random')}
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
            placeholder={t('gameList.searchPlaceholder')}
            aria-label={t('gameList.searchLabel')}
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
          <p className="rom-list__empty">{t('gameList.noMatch', { query: query.trim() })}</p>
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
          {games ? t('gameList.footCount', { count: games.length }) : t('gameList.footList')}
          {t('gameList.footSource')}
          <a href="https://github.com/mengfei523060/nesrom" target="_blank" rel="noreferrer">
            mengfei523060/nesrom
          </a>
        </p>

        <Disclaimer className="mdash__disclaimer" />
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
