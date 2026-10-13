import type { PadInput } from './constants';

/**
 * Controllers, as the browser's Gamepad API reports them.
 *
 * The API exists because touch hardware has no key events to listen to: on an
 * iPad the D-pad of a paired DualSense produces no DOM event at all, so the
 * only way to see it is to poll `navigator.getGamepads()`. Every modern pad —
 * DualSense, Xbox, Switch Pro, 8BitDo — arrives in the W3C "standard" layout,
 * where the four face buttons keep fixed indices whichever letters are printed
 * on the shell, which is what makes one mapping enough.
 */

/**
 * NES input for each index of the standard layout.
 *
 * The four face buttons are a plain pair and a turbo pair, which is how the
 * turbo pads of the era were built — a NES Advantage or a 小霸王 pad puts its
 * auto-fire buttons beside the ordinary ones, not on the shoulders. The two a
 * thumb rests on are the plain pair (bottom is A, right is B), and each turbo
 * switch sits opposite the button it fires: left fires B, top fires A. On an
 * Xbox pad the printed letters then land where they say they do — A is A, B is
 * B — and X and Y are the turbo switches for the buttons across from them.
 *
 * The shoulders and triggers are turbo as well (the left pair fires B, the
 * right pair A), so a turbo can be held without the thumb leaving the plain
 * buttons. Browsers do not agree on which of the two comes first when a pad
 * does not map itself onto the standard layout, so both are taken.
 */
const STANDARD_BUTTONS: ReadonlyArray<readonly [index: number, input: PadInput]> = [
  [0, 'A'],
  [1, 'B'],
  [2, 'TURBO_B'],
  [3, 'TURBO_A'],
  [4, 'TURBO_B'],
  [5, 'TURBO_A'],
  [6, 'TURBO_B'],
  [7, 'TURBO_A'],
  // 8 is Select/Back/Share and 9 is Start/Options on every pad; the NES's two
  // centre buttons are the same pair under older names.
  [8, 'SELECT'],
  [9, 'START'],
  [12, 'UP'],
  [13, 'DOWN'],
  [14, 'LEFT'],
  [15, 'RIGHT'],
];

/** Stick deflection, in the API's [-1, 1], past which a direction counts. */
const AXIS_THRESHOLD = 0.5;

/** The button indices this mapping covers. */
const MAPPED_INDICES = new Set(STANDARD_BUTTONS.map(([index]) => index));

/**
 * Indices of the buttons a pad is holding that have no NES button here.
 *
 * Pads keep arriving with their buttons in an order of their own, and a press
 * that lands nowhere is indistinguishable from a pad that is not being read —
 * so the interface is told about it and can show it as it is: an index, with no
 * name to give it.
 */
export function unmappedButtons(pad: Gamepad): number[] {
  const found: number[] = [];
  pad.buttons.forEach((button, index) => {
    if (button.pressed && !MAPPED_INDICES.has(index)) found.push(index);
  });
  return found;
}

/**
 * The NES buttons a pad is holding right now.
 *
 * Both the D-pad and the left stick steer — a wireless pad in front of a
 * screen is usually held on the stick, and the two never disagree in practice.
 * The axes are compared separately, so a diagonal is two directions at once,
 * which is exactly what the NES D-pad does.
 */
export function gamepadButtons(pad: Gamepad): Set<PadInput> {
  const held = new Set<PadInput>();
  for (const [index, input] of STANDARD_BUTTONS) {
    if (pad.buttons[index]?.pressed) held.add(input);
  }

  const x = pad.axes[0] ?? 0;
  const y = pad.axes[1] ?? 0;
  if (x <= -AXIS_THRESHOLD) held.add('LEFT');
  else if (x >= AXIS_THRESHOLD) held.add('RIGHT');
  if (y <= -AXIS_THRESHOLD) held.add('UP');
  else if (y >= AXIS_THRESHOLD) held.add('DOWN');

  return held;
}

/**
 * A pad's name without the browser's parenthetical ride-along.
 *
 * Chrome reports e.g. `Wireless Controller (STANDARD GAMEPAD Vendor: 054c
 * Product: 0ce6)`; only the name is worth showing.
 */
export function gamepadLabel(id: string): string {
  const name = id.replace(/\s*\(.*\)\s*$/, '').trim();
  return name || id;
}
