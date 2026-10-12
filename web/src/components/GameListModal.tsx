import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react';

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
 * Rows rendered at once. The list holds hundreds of games, and rendering every
 * one of them would make typing in the search box sluggish.
 */
const VISIBLE_LIMIT = 200;

/**
 * Searchable game list, so a game can be picked without knowing its URL.
 *
 * The list is a static asset beside the page (`games.json`, regenerate it with
 * `npm run build:games`), fetched the first time the dialog is opened.
 */
export function GameListModal({ busy, currentUrl, onSelect, onClose }: GameListModalProps) {
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const searchRef = useRef<HTMLInputElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);

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

  // The search box is the only thing worth typing into, so start there.
  useEffect(() => {
    searchRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return games ?? [];
    return (games ?? []).filter((game) => game.name.toLowerCase().includes(needle));
  }, [games, query]);

  // Clicks on the backdrop close the dialog; clicks inside it must not.
  const onBackdropDown = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      if (event.target === event.currentTarget) onClose();
    },
    [onClose],
  );

  /**
   * Keep Tab inside the dialog. The page behind stays mounted (and its keyboard
   * handler keeps listening), so focus escaping would hand the arrow keys to a
   * running game while the dialog is still open.
   */
  const onDialogKeyDown = useCallback((event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Tab') return;

    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'input:not([disabled]), button:not([disabled]), a[href]',
    );
    if (!focusable || focusable.length === 0) return;

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    const inside = dialogRef.current?.contains(active) ?? false;

    if (event.shiftKey && (active === first || !inside)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }, []);

  return (
    <div className="modal" onMouseDown={onBackdropDown}>
      <div
        ref={dialogRef}
        className="modal__card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="game-list-title"
        onKeyDown={onDialogKeyDown}
      >
        <header className="modal__head">
          <h2 className="modal__title" id="game-list-title">
            游戏列表
          </h2>
          <button type="button" className="modal__close" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </header>

        <input
          ref={searchRef}
          type="search"
          className="modal__search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="搜索游戏名…"
          aria-label="搜索游戏"
          spellCheck={false}
          autoComplete="off"
        />

        <div className="modal__body">
          {error && <p className="modal__empty">{error}</p>}
          {!games && !error && <p className="modal__empty">正在载入游戏列表…</p>}

          {games && results.length === 0 && (
            <p className="modal__empty">没有匹配 “{query.trim()}” 的游戏。</p>
          )}

          {games && results.length > 0 && (
            <ul className="modal__list">
              {results.slice(0, VISIBLE_LIMIT).map((game) => (
                <li key={game.url}>
                  <button
                    type="button"
                    className="modal__row"
                    onClick={() => onSelect(game.url)}
                    disabled={busy}
                  >
                    <span className="modal__name">{game.name}</span>
                    {game.url === currentUrl && <span className="modal__badge">当前</span>}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <p className="modal__foot">
          {games ? `共 ${games.length} 个游戏` : '游戏列表'}
          {' · 资料来自 '}
          <a href="https://github.com/mengfei523060/nesrom" target="_blank" rel="noreferrer">
            mengfei523060/nesrom
          </a>
          {results.length > VISIBLE_LIMIT && ` · 仅显示前 ${VISIBLE_LIMIT} 个匹配结果`}
        </p>
      </div>
    </div>
  );
}
