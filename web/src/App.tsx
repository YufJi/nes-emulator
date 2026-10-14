import { useEffect } from 'react';
import { useRoute } from './router';
import { DesktopApp } from './components/DesktopApp';
import { PlayPage } from './mobile/PlayPage';
import { DashboardPage } from './mobile/DashboardPage';
import { useViewport } from './mobile/useViewport';

/** Marks the no-selection UI on `html`; see `html.phone-ui` in `styles.css`. */
const PHONE_UI = 'phone-ui';

function isPhonePath(path: string): boolean {
  return path === '/m' || path.startsWith('/m/');
}

/**
 * Whether every press this device makes is a finger press.
 *
 * A phone that has already been through `/m` is served the wide-screen page —
 * the redirect happens once per session — and a long press there drags out the
 * same selection magnifier the phone layout suppresses. So the device decides
 * as well as the route, and a device with no pointer at all is treated like the
 * phone layout wherever it is. A device that has a pointer (a desktop browser,
 * a tablet with a trackpad) keeps selecting as it always could.
 */
function isTouchOnlyDevice(): boolean {
  return window.matchMedia('(hover: none) and (pointer: coarse)').matches;
}

/**
 * Route table.
 *
 * Everything under `/m` is the phone layout — a dashboard that lists the games
 * and a player page with the picture above and the touch pad below. Any other
 * location is the wide-screen page, so the site keeps answering on `/`.
 */
export default function App() {
  const { path, query } = useRoute();
  const phone = isPhonePath(path);
  const phoneUi = phone || isTouchOnlyDevice();

  // The phone layout sizes itself from the viewport this measures — the CSS
  // `dvh`/`env()` numbers alone are the ones the app opened under, not the ones
  // it plays in. See `useViewport`.
  useViewport(phone);

  // On `html` rather than on a wrapper: the island Drawer and Modal render into
  // portals, so styling that stops at the page subtree would miss them, and the
  // root itself is the element a long press resolves against — see `styles.css`.
  useEffect(() => {
    document.documentElement.classList.toggle(PHONE_UI, phoneUi);
  }, [phoneUi]);

  if (path === '/m' || path === '/m/') return <DashboardPage />;
  if (path === '/m/play') return <PlayPage query={query} />;
  if (phone) return <DashboardPage />;
  return <DesktopApp />;
}
