import { useEffect } from 'react';
import { useRoute } from './router';
import { DesktopApp } from './components/DesktopApp';
import { PlayPage } from './mobile/PlayPage';
import { DashboardPage } from './mobile/DashboardPage';
import { useViewport } from './mobile/useViewport';

/** Marks the phone pages on `body`; see `body.phone-ui` in `styles.css`. */
const PHONE_UI = 'phone-ui';

function isPhonePath(path: string): boolean {
  return path === '/m' || path.startsWith('/m/');
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

  // The phone layout sizes itself from the viewport this measures — the CSS
  // `dvh`/`env()` numbers alone are the ones the app opened under, not the ones
  // it plays in. See `useViewport`.
  useViewport(phone);

  // On `body` rather than on a wrapper: the island Drawer and Modal render into
  // portals, so styling that stops at the page subtree would miss them.
  useEffect(() => {
    document.body.classList.toggle(PHONE_UI, phone);
  }, [phone]);

  if (path === '/m' || path === '/m/') return <DashboardPage />;
  if (path === '/m/play') return <PlayPage query={query} />;
  if (phone) return <DashboardPage />;
  return <DesktopApp />;
}
