import { useEffect, useRef, useState } from 'react';
import { PAD_INPUTS, type PadInput } from '../emulator/constants';
import { gamepadButtons, gamepadLabel, unmappedButtons } from '../emulator/gamepad';

export interface GamepadStatus {
  /** Whether this browser has the Gamepad API at all. */
  supported: boolean;
  /** Names of the controllers it currently reports, in the browser's order. */
  names: string[];
  /**
   * What the controllers are holding right now, by the names the emulator uses.
   *
   * Reported so the interface can show a press arriving at all — the difference
   * between "the pad is not being read" and "it is being read, but that button
   * is not one this front end knows".
   */
  inputs: PadInput[];
  /**
   * Presses the pads are making that this front end has no button for, by
   * index. A pad is only useful to the extent its buttons land somewhere.
   */
  unmapped: number[];
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
  onButton: (input: PadInput, pressed: boolean) => void,
): GamepadStatus {
  const supported = typeof navigator.getGamepads === 'function';
  // The object handed to React is the one the poll builds, so a page that
  // re-renders for any other reason keeps the identity the emulator hook's
  // `useMemo` depends on.
  const [reported, setReported] = useState<GamepadStatus>(() => ({
    supported,
    names: [],
    inputs: [],
    unmapped: [],
  }));
  const press = useRef(onButton);

  useEffect(() => {
    press.current = onButton;
  }, [onButton]);

  useEffect(() => {
    if (!supported) return;

    let frame = 0;
    // What the pads held last frame: the emulator only needs the changes, plus
    // a release for everything still down when the pads go away.
    let held = new Set<PadInput>();
    let lastKey = '';

    const release = () => {
      if (held.size === 0) return;
      for (const input of held) press.current(input, false);
      held = new Set();
    };

    const poll = () => {
      frame = window.requestAnimationFrame(poll);

      const pads: Gamepad[] = [];
      for (const pad of navigator.getGamepads()) if (pad) pads.push(pad);

      const now = new Set<PadInput>();
      const unmapped: number[] = [];
      for (const pad of pads) {
        for (const input of gamepadButtons(pad)) now.add(input);
        unmapped.push(...unmappedButtons(pad));
      }

      for (const input of PAD_INPUTS) {
        const was = held.has(input);
        const is = now.has(input);
        if (was !== is) press.current(input, is);
      }
      held = now;

      // The pads and what they hold only change when something is pressed or a
      // pad connects, so React is woken when that happened and not every frame.
      const names = pads.map((pad) => gamepadLabel(pad.id));
      const down = PAD_INPUTS.filter((input) => now.has(input));
      const key = `${pads.map((pad) => `${pad.index}:${pad.id}`).join('|')}#${down.join(',')}|${unmapped.join(',')}`;
      if (key !== lastKey) {
        lastKey = key;
        setReported({ supported, names, inputs: down, unmapped });
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

  return reported;
}
