/** Logical buttons of the standard controller. */
export type ButtonName =
  | 'A'
  | 'B'
  | 'SELECT'
  | 'START'
  | 'UP'
  | 'DOWN'
  | 'LEFT'
  | 'RIGHT';

export const BUTTON_NAMES: readonly ButtonName[] = [
  'A',
  'B',
  'SELECT',
  'START',
  'UP',
  'DOWN',
  'LEFT',
  'RIGHT',
];

/** Keyboard `event.code` to controller button mapping. */
export const KEY_BINDINGS: Record<string, ButtonName> = {
  KeyW: 'UP',
  KeyS: 'DOWN',
  KeyA: 'LEFT',
  KeyD: 'RIGHT',
  KeyJ: 'B',
  KeyI: 'B',
  KeyK: 'A',
  KeyO: 'A',
  Enter: 'START',
  Space: 'START',
  ShiftLeft: 'SELECT',
  ShiftRight: 'SELECT',
};

/**
 * Rows rendered by the on-screen key legend; each entry is one key cap.
 *
 * `action` is the button name, except for the D-pad — the only row that needs a
 * word of its own, so `KeyGuide` translates it.
 */
export const KEY_GUIDE: ReadonlyArray<{
  action: 'dpad' | 'A' | 'B' | 'Start' | 'Select';
  keys: readonly string[];
}> = [
  { action: 'dpad', keys: ['W', 'A', 'S', 'D'] },
  { action: 'B', keys: ['J', 'I'] },
  { action: 'A', keys: ['K', 'O'] },
  { action: 'Start', keys: ['Enter', 'Space'] },
  { action: 'Select', keys: ['Shift'] },
];
