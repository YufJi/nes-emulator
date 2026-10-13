import { useCallback, useRef, type MouseEvent, type PointerEvent } from 'react';

/**
 * Tap handling for the phone pages.
 *
 * `onClick` alone is not enough there. A touch that arrives while the list is
 * still coasting after a flick — which is exactly when a game is picked — is
 * spent stopping the scroll, and WebKit never synthesises the click: the first
 * tap is swallowed and the visitor has to tap a second time. The pointer events
 * of that same touch still arrive, so a tap recognised here is not lost.
 *
 * The click stays the preferred trigger, because it is the browser's last event
 * of the gesture: acting on `pointerup` changes the DOM *before* the trailing
 * click, which is then delivered to whatever has moved under the finger —
 * opening the tools drawer on `pointerup` closes it again when the click lands
 * on its freshly shown mask. So `pointerup` only arms a fallback, cancelled by
 * the click in the normal case and left to fire when the click never comes.
 *
 * A tap is a press and a release at (almost) the same place, so a finger that
 * dragged the list is not one; the browser reports that as `pointercancel`
 * anyway, and the movement check is the second line of defence.
 */

/** Movement still considered a tap rather than a drag, in CSS pixels. */
const TAP_SLOP_PX = 10;

/**
 * How long `pointerup` waits for the click that should follow it. A synthesised
 * click arrives within a few milliseconds; anything later than this was not
 * coming at all.
 */
const CLICK_FALLBACK_MS = 150;

/** Window in which a click following a handled tap counts as a duplicate. */
const CLICK_GRACE_MS = 500;

export interface TapHandlers<E extends HTMLElement> {
  onPointerDown: (event: PointerEvent<E>) => void;
  onPointerUp: (event: PointerEvent<E>) => void;
  onPointerCancel: (event: PointerEvent<E>) => void;
  onClick: (event: MouseEvent<E>) => void;
}

/**
 * Turn presses on an element into `onTap`.
 *
 * `onClick` stays wired up for mice, for keyboards (Enter/Space on a focused
 * button never reaches the pointer handlers) and for every touch whose click
 * does arrive; a click that would repeat a tap already handled is dropped.
 */
export function useTap<E extends HTMLElement = HTMLButtonElement>(onTap: () => void): TapHandlers<E> {
  const origin = useRef<{ id: number; x: number; y: number } | null>(null);
  /** Fallback armed on `pointerup`, cleared by the click it was waiting for. */
  const fallback = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** `null` until a tap is handled: a zero timestamp would swallow every click
   *  fired in the first moments of the page. */
  const handledAt = useRef<number | null>(null);

  const run = useCallback(() => {
    handledAt.current = performance.now();
    onTap();
  }, [onTap]);

  const cancelFallback = useCallback(() => {
    if (fallback.current === null) return;
    clearTimeout(fallback.current);
    fallback.current = null;
  }, []);

  const onPointerDown = useCallback(
    (event: PointerEvent<E>) => {
      cancelFallback();
      origin.current =
        event.isPrimary && event.button === 0
          ? { id: event.pointerId, x: event.clientX, y: event.clientY }
          : null;
    },
    [cancelFallback],
  );

  const onPointerUp = useCallback(
    (event: PointerEvent<E>) => {
      const start = origin.current;
      origin.current = null;
      if (!start || start.id !== event.pointerId) return;

      const moved = Math.hypot(event.clientX - start.x, event.clientY - start.y);
      if (moved > TAP_SLOP_PX) return;

      fallback.current = setTimeout(() => {
        fallback.current = null;
        run();
      }, CLICK_FALLBACK_MS);
    },
    [run],
  );

  const onPointerCancel = useCallback(() => {
    origin.current = null;
    cancelFallback();
  }, [cancelFallback]);

  const onClick = useCallback(
    (event: MouseEvent<E>) => {
      // The click this tap was waiting for — run it here, with the DOM settled,
      // instead of leaving the fallback to do it.
      if (fallback.current !== null) {
        cancelFallback();
        run();
        return;
      }

      const handled = handledAt.current;
      if (handled !== null && performance.now() - handled < CLICK_GRACE_MS) {
        event.preventDefault();
        return;
      }
      run();
    },
    [cancelFallback, run],
  );

  return { onPointerDown, onPointerUp, onPointerCancel, onClick };
}
