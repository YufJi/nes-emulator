import type { ButtonName } from './constants';

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
 * NES button for each index of the standard layout.
 *
 * The face buttons are mapped by side rather than by label. The bottom and
 * right ones (Cross / Circle, or A / B on an Xbox pad) are the primary button
 * and the left and top ones (Square / Triangle, or X / Y) the secondary one:
 * the NES's own A sits on the right, so "the right-hand button jumps" stays
 * true on any pad, and either hand's instinctive press works.
 */
const STANDARD_BUTTONS: ReadonlyArray<readonly [index: number, button: ButtonName]> = [
  [0, 'A'],
  [1, 'A'],
  [2, 'B'],
  [3, 'B'],
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

/**
 * The NES buttons a pad is holding right now.
 *
 * Both the D-pad and the left stick steer — a wireless pad in front of a
 * screen is usually held on the stick, and the two never disagree in practice.
 * The axes are compared separately, so a diagonal is two directions at once,
 * which is exactly what the NES D-pad does.
 */
export function gamepadButtons(pad: Gamepad): Set<ButtonName> {
  const held = new Set<ButtonName>();
  for (const [index, button] of STANDARD_BUTTONS) {
    if (pad.buttons[index]?.pressed) held.add(button);
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
