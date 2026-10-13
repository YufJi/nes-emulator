import { currentPath, redirect } from '../router';

/**
 * Which layout the site opens in on `/`.
 *
 * A phone that lands on the wide-screen page has to hunt for the 「手机版」
 * button before it can play, so an unmistakable phone user agent is sent to
 * `/m` before the first render — no flash of the wrong layout. There is
 * deliberately no switch in the UI: the split follows the device.
 *
 * The redirect happens once per session. Coming back to `/` in the same tab —
 * typing the address, following an old bookmark — is the visitor asking for the
 * wide-screen page, and the only way there is without a switch; being bounced
 * to `/m` again would leave them with no way out.
 */

const ENTERED_KEY = 'nes-phone-entry';

/**
 * Only agents that cannot be anything but a phone are matched. `Mobile` alone
 * would be too eager — some desktop browsers still send it — so the platforms
 * are named instead. Tablets are left on the wide-screen page: they have the
 * room for it, and `/m` is a thumb layout.
 */
const PHONE_UA = /Android.*Mobile|iPhone|iPod|Windows Phone|IEMobile|BlackBerry|Opera Mini/i;

export function looksLikePhone(): boolean {
  return PHONE_UA.test(navigator.userAgent);
}

function hasEnteredAlready(): boolean {
  try {
    return window.sessionStorage.getItem(ENTERED_KEY) !== null;
  } catch {
    // Storage can be blocked (private mode, third-party context) — then the
    // redirect simply happens on every visit to `/`.
    return false;
  }
}

function markEntered(): void {
  try {
    window.sessionStorage.setItem(ENTERED_KEY, '1');
  } catch {
    // As above: nothing to remember, nothing to lose.
  }
}

/** Send a phone that opens `/` to the phone layout. */
export function enterPhoneLayoutOnRoot(): void {
  if (currentPath() !== '/') return;
  if (!looksLikePhone()) return;
  if (hasEnteredAlready()) return;
  markEntered();
  redirect('/m');
}
