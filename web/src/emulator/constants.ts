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

/**
 * The pad's turbo controls.
 *
 * Not buttons of the console: each is a switch on the pad in the player's hand
 * that fires its button again and again while it is held, which is what the NES
 * Advantage and its rivals had. Keeping them separate from A and B is what
 * leaves an ordinary press an ordinary press — a game that wants A held down
 * still gets it.
 */
export type TurboName = 'TURBO_A' | 'TURBO_B';

export const TURBO_NAMES: readonly TurboName[] = ['TURBO_A', 'TURBO_B'];

/** Everything a pad can send: the console's eight buttons, plus the two turbo controls. */
export type PadInput = ButtonName | TurboName;

/** Every input, in the order the pads and the key guide list them. */
export const PAD_INPUTS: readonly PadInput[] = [...BUTTON_NAMES, ...TURBO_NAMES];

export function isTurbo(input: PadInput): input is TurboName {
  return input === 'TURBO_A' || input === 'TURBO_B';
}

/** The button a turbo control auto-fires. */
export function turboTarget(turbo: TurboName): ButtonName {
  return turbo === 'TURBO_A' ? 'A' : 'B';
}

/**
 * Frames per turbo cycle, half of them pressed.
 *
 * Four frames at 60.0988 Hz is about 15 pulses a second, the rate a NES
 * Advantage fires at: fast enough that a game sampling input once a frame sees
 * a stream of separate presses, slow enough that the release lasts long enough
 * to be seen at all.
 */
export const TURBO_PERIOD_FRAMES = 4;

/** Keyboard `event.code` to pad input mapping. */
export const KEY_BINDINGS: Record<string, PadInput> = {
  KeyW: 'UP',
  KeyS: 'DOWN',
  KeyA: 'LEFT',
  KeyD: 'RIGHT',
  // A and B sit on the home row, with each button's turbo switch right above
  // it — I fires B, O fires A, so the hand does not move between them.
  KeyJ: 'B',
  KeyK: 'A',
  KeyI: 'TURBO_B',
  KeyO: 'TURBO_A',
  Enter: 'START',
  Space: 'START',
  ShiftLeft: 'SELECT',
  ShiftRight: 'SELECT',
};

/** Rows rendered by the on-screen key legend. */
type GuideAction = 'dpad' | 'TurboA' | 'TurboB' | 'A' | 'B' | 'Start' | 'Select';

/**
 * Rows rendered by the on-screen key legend; each entry is one key cap.
 *
 * `action` names the button, except for the rows that need a word of their own —
 * the D-pad and the turbo controls — which `KeyGuide` translates.
 */
export const KEY_GUIDE: ReadonlyArray<{
  action: GuideAction;
  keys: readonly string[];
}> = [
  { action: 'dpad', keys: ['W', 'A', 'S', 'D'] },
  { action: 'B', keys: ['J'] },
  { action: 'A', keys: ['K'] },
  { action: 'TurboB', keys: ['I'] },
  { action: 'TurboA', keys: ['O'] },
  { action: 'Start', keys: ['Enter', 'Space'] },
  { action: 'Select', keys: ['Shift'] },
];
