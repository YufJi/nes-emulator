import { useEffect, useMemo, useRef, useState } from 'react';
import { BUTTON_NAMES, type ButtonName } from '../emulator/constants';
import { gamepadButtons, gamepadLabel } from '../emulator/gamepad';

export interface GamepadStatus {
  /** Whether this browser has the Gamepad API at all. */
  supported: boolean;
  /** Names of the controllers it currently reports, in the browser's order. */
  names: string[];
}

/**
 * Feed physical controllers into the emulated one.
 *
 * The Gamepad API has no press/release events, only a snapshot that has to be
 * read again and again, so this polls `navigator.getGamepads()` every frame and
 * forwards the difference from the previous one. What the emulator ends up with
 * is a set of held buttons, exactly as the keyboard path and the touch pad
 * produce it — nothing downstream needs to know where a press came from.
 *
 * Polling starts before any pad is connected, because a controller is only ever
 * reported *after* one of its buttons has been pressed; waiting for that first
 * press to start looking is how the press gets missed.
 */
export function useGamepad(
  onButton: (button: ButtonName, pressed: boolean) => void,
): GamepadStatus {
  const [names, setNames] = useState<string[]>([]);
  const press = useRef(onButton);
  const supported = typeof navigator.getGamepads === 'function';

  useEffect(() => {
    press.current = onButton;
  }, [onButton]);

  useEffect(() => {
    if (!supported) return;

    let frame = 0;
    // What the pads held last frame: the emulator only needs the changes, plus
    // a release for everything still down when the pads go away.
    let held = new Set<ButtonName>();
    let reported = '';

    const release = () => {
      if (held.size === 0) return;
      for (const button of held) press.current(button, false);
      held = new Set();
    };

    const poll = () => {
      frame = window.requestAnimationFrame(poll);

      const pads: Gamepad[] = [];
      for (const pad of navigator.getGamepads()) if (pad) pads.push(pad);

      const now = new Set<ButtonName>();
      for (const pad of pads) {
        for (const button of gamepadButtons(pad)) now.add(button);
      }

      for (const button of BUTTON_NAMES) {
        const was = held.has(button);
        const is = now.has(button);
        if (was !== is) press.current(button, is);
      }
      held = now;

      // The list only changes when a pad connects or goes, so React is woken
      // when it actually did rather than on every frame.
      const key = pads.map((pad) => `${pad.index}:${pad.id}`).join('|');
      if (key !== reported) {
        reported = key;
        setNames(pads.map((pad) => gamepadLabel(pad.id)));
      }
    };

    // A pad that was carried off screen holds its last state; letting go on the
    // way out is what the keyboard handler does when the window loses focus.
    const onVisibility = () => {
      if (document.hidden) release();
    };

    frame = window.requestAnimationFrame(poll);
    window.addEventListener('blur', release);
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener('blur', release);
      document.removeEventListener('visibilitychange', onVisibility);
      release();
    };
  }, [supported]);

  // A fresh object every render would invalidate the emulator hook's own
  // `useMemo`, so the identity here only changes when the pads really do.
  return useMemo(() => ({ supported, names }), [supported, names]);
}
