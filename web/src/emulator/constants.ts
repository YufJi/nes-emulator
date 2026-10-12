/**
 * NTSC frame rate. The NES renders 262 scanlines of 341 PPU cycles each, which
 * works out to 60.0988 frames per second — not the 60 Hz of a typical display.
 */
export const FRAME_RATE = 60.0988;
export const FRAME_DURATION_MS = 1000 / FRAME_RATE;

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
