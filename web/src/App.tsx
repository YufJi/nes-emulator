import { useRoute } from './router';
import { DesktopApp } from './components/DesktopApp';
import { PlayPage } from './mobile/PlayPage';
import { DashboardPage } from './mobile/DashboardPage';

/**
 * Route table.
 *
 * Everything under `/m` is the phone layout — a dashboard that lists the games
 * and a player page with the picture above and the touch pad below. Any other
 * location is the wide-screen page, so the site keeps answering on `/`.
 */
export default function App() {
  const { path, query } = useRoute();

  if (path === '/m' || path === '/m/') return <DashboardPage />;
  if (path === '/m/play') return <PlayPage query={query} />;
  if (path.startsWith('/m/')) return <DashboardPage />;
  return <DesktopApp />;
}
