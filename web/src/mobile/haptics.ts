/**
 * Haptic feedback for the touch controls.
 *
 * `navigator.vibrate` is the only haptic a web page has, and only Android
 * browsers implement it. WebKit never has — on an iPhone the call is a silent
 * no-op because the method is simply absent — and nothing else is worth faking
 * there: a Web Audio "thump" is not a haptic, and iOS ignores programmatic
 * clicks on the `switch` control that does produce one natively.
 *
 * A call also needs the document to be visible and the visitor to have touched
 * the page at least once, which a gamepad press always satisfies.
 */

export function hapticsAvailable(): boolean {
  return typeof navigator.vibrate === 'function';
}

/**
 * Durations are deliberately the shortest a phone renders distinctly. Anything
 * below a few milliseconds is rounded up or dropped, and buzzing on every
 * direction change of a rolling thumb turns feedback into noise.
 */
const BUTTON_MS = 12;
const STICK_MS = 6;

/** A face button went down. */
export function buzzButton(): void {
  navigator.vibrate?.(BUTTON_MS);
}

/** The stick left neutral — once per push, not once per direction. */
export function buzzStick(): void {
  navigator.vibrate?.(STICK_MS);
}
