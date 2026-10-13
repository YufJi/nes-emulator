import { useEffect, useState } from 'react';
import { loadGameList, type GameEntry } from '../games';

export interface UseGameListResult {
  /** Null until the catalogue arrives. */
  games: GameEntry[] | null;
  error: string | null;
}

/** The game catalogue, fetched lazily by whichever page needs it. */
export function useGameList(): UseGameListResult {
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    loadGameList()
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

  return { games, error };
}
