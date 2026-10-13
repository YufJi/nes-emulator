import { useEffect, useState } from 'react';

/**
 * The smallest router that covers the two entry points of the app.
 *
 * Locations are plain paths (`/` for the desktop page, `/m` and
 * `/m/play?game=…` for the phone layout) so a link can be shared as-is. Only
 * two things are needed to make that work without a routing library: read the
 * current location, and be told when it changes.
 *
 * GitHub Pages serves the site from a sub-path and cannot rewrite a deep link
 * onto `index.html`, so `404.html` (emitted by `vite.config.ts`) bounces the
 * request back to the app with the original location in `?route=`;
 * {@link restoreRedirectedRoute} puts it back before the first render.
 */

/** `/` in dev, `/<repo>/` on GitHub Pages. */
const BASE = import.meta.env.BASE_URL;

export interface Route {
  /** Path below the app's base, always starting with `/`. */
  path: string;
  query: URLSearchParams;
}

/** Drop the deploy sub-path so `/repo/m/play` and `/m/play` read the same. */
function appPath(pathname: string): string {
  const base = BASE.replace(/\/$/, '');
  const rest = base && pathname.startsWith(base) ? pathname.slice(base.length) : pathname;
  if (rest === '') return '/';
  return rest.startsWith('/') ? rest : `/${rest}`;
}

/** Absolute URL for an in-app path, so links keep working under a sub-path. */
function href(path: string): string {
  return `${BASE.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

/** Path of the current location, below the app's base. */
export function currentPath(): string {
  return appPath(window.location.pathname);
}

function readRoute(): Route {
  return {
    path: currentPath(),
    query: new URLSearchParams(window.location.search),
  };
}

/**
 * Current location, re-read on every history change (back/forward included).
 *
 * The app has a single page per route, so a plain state hook is enough — there
 * is no nested routing that would need a context.
 */
export function useRoute(): Route {
  const [route, setRoute] = useState(readRoute);

  useEffect(() => {
    const sync = () => setRoute(readRoute());
    window.addEventListener('popstate', sync);
    return () => window.removeEventListener('popstate', sync);
  }, []);

  return route;
}

/** Navigate in-app, leaving one history entry behind for the back button. */
export function navigate(path: string): void {
  move(path, 'pushState');
}

/**
 * Move to a location without leaving a history entry behind.
 *
 * Used for entry redirects, where going back would only land on the redirecting
 * URL and bounce forward again.
 */
export function redirect(path: string): void {
  move(path, 'replaceState');
}

function move(path: string, method: 'pushState' | 'replaceState'): void {
  window.history[method](null, '', href(path));
  // `history` does not notify anyone, so the route hook is woken by hand.
  window.dispatchEvent(new PopStateEvent('popstate'));
}

/** Put the location from `404.html`'s `?route=` payload back into the address bar. */
export function restoreRedirectedRoute(): void {
  const params = new URLSearchParams(window.location.search);
  const target = params.get('route');
  if (!target) return;

  // Anything else the redirect carried (e.g. a campaign tag) is kept.
  params.delete('route');
  const rest = params.toString();
  const joined = rest ? `${target.includes('?') ? '&' : '?'}${rest}` : '';
  window.history.replaceState(null, '', `${target}${joined}`);
}
