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
  KeyJ: 'A',
  KeyI: 'A',
  KeyK: 'B',
  KeyO: 'B',
  Enter: 'START',
  Space: 'START',
  ShiftLeft: 'SELECT',
  ShiftRight: 'SELECT',
};

/** Rows rendered by the on-screen key legend; each entry is one key cap. */
export const KEY_GUIDE: ReadonlyArray<{ action: string; keys: readonly string[] }> = [
  { action: '方向', keys: ['W', 'A', 'S', 'D'] },
  { action: 'A', keys: ['J', 'I'] },
  { action: 'B', keys: ['K', 'O'] },
  { action: 'Start', keys: ['Enter', 'Space'] },
  { action: 'Select', keys: ['Shift'] },
];
