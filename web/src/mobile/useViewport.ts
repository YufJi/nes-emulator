import { useEffect } from 'react';

/**
 * Where the phone layout reads its viewport from: the four safe-area insets and
 * the height of the visible area, published as custom properties on `:root`.
 * The stylesheet keeps `env(safe-area-inset-*)` and `100dvh` as the fallback,
 * so the first paint — before the effect below has run — is laid out from CSS
 * alone, and a browser that never gets here still shows the phone pages.
 */
const VIEWPORT_HEIGHT = '--viewport-height';
const SAFE_AREA = {
  top: '--safe-area-top',
  right: '--safe-area-right',
  bottom: '--safe-area-bottom',
  left: '--safe-area-left',
} as const;

/** Computed styles answer in pixels, with `0px` where the device has no inset. */
function pixels(value: string): number {
  const size = Number.parseFloat(value);
  return Number.isFinite(size) ? size : 0;
}

/**
 * The safe-area insets as they are *now*, which is not the same question as the
 * ones the page was laid out with.
 *
 * `env()` is resolved when a style is computed, and a home-screen web app is
 * first laid out while iOS still has its loading bar above the page — a bar
 * about the height of the status bar — so the phone layout is wearing the
 * insets of that bar. The bar leaves a moment later, but none of that resolves
 * the styles again: nothing about the page changes size, so no `resize` arrives
 * to invalidate them, and the top of the page keeps a strip of margin that
 * belongs to a bar nobody can see any more.
 *
 * Hence the probe: an element created here and now, whose padding is resolved
 * against the insets of this moment (a fresh element has no earlier resolution
 * to be stuck with). It is measured, dropped, and the answer is published for
 * the phone stylesheet to read.
 */
function measureInsets(): Record<keyof typeof SAFE_AREA, number> {
  const probe = document.createElement('div');
  probe.setAttribute('aria-hidden', 'true');
  probe.style.cssText =
    'position:fixed;top:0;left:0;width:0;height:0;visibility:hidden;pointer-events:none;' +
    'padding:env(safe-area-inset-top,0px) env(safe-area-inset-right,0px) ' +
    'env(safe-area-inset-bottom,0px) env(safe-area-inset-left,0px)';
  document.body.appendChild(probe);
  const { paddingTop, paddingRight, paddingBottom, paddingLeft } = getComputedStyle(probe);
  probe.remove();

  return {
    top: pixels(paddingTop),
    right: pixels(paddingRight),
    bottom: pixels(paddingBottom),
    left: pixels(paddingLeft),
  };
}

/** Written only when it moved, so nothing is invalidated for free. */
function publish(property: string, size: number): void {
  const style = document.documentElement.style;
  const length = `${size}px`;
  if (style.getPropertyValue(property) !== length) style.setProperty(property, length);
}

/**
 * Hand the phone layout the window it is actually drawn in, and keep handing it
 * over as that window changes — the page never has to guess with `dvh` or
 * `env()` alone (see `measureInsets` for what goes wrong there).
 *
 * `active` keeps the measurement to the phone routes: the wide-screen page has
 * no use for it, and nothing else in the app reads the properties it publishes.
 */
export function useViewport(active: boolean): void {
  useEffect(() => {
    if (!active) return;

    const measure = () => {
      const insets = measureInsets();
      publish(SAFE_AREA.top, insets.top);
      publish(SAFE_AREA.right, insets.right);
      publish(SAFE_AREA.bottom, insets.bottom);
      publish(SAFE_AREA.left, insets.left);
      // The visible height rather than `100dvh`: both are resolved with the
      // page, and both can therefore be the launch bar's answer instead.
      publish(VIEWPORT_HEIGHT, Math.round(window.visualViewport?.height ?? window.innerHeight));
    };

    measure();

    // Everything that reshapes the window: a rotation, the URL bar coming back,
    // another app taking the screen and giving it back. `visualViewport` is the
    // one iOS updates first, and it carries rotations before `resize` does.
    const viewport = window.visualViewport;
    window.addEventListener('resize', measure);
    window.addEventListener('orientationchange', measure);
    window.addEventListener('pageshow', measure);
    document.addEventListener('visibilitychange', measure);
    viewport?.addEventListener('resize', measure);

    // The launch bar leaving is not an event at all, so the app is looked at
    // again over its first seconds — longer than it takes a page to come up, so
    // the bar is never the last word, and short enough not to be a poll.
    const settling = [100, 250, 500, 1000, 2000, 4000, 8000].map((delay) =>
      window.setTimeout(measure, delay),
    );

    return () => {
      window.removeEventListener('resize', measure);
      window.removeEventListener('orientationchange', measure);
      window.removeEventListener('pageshow', measure);
      document.removeEventListener('visibilitychange', measure);
      viewport?.removeEventListener('resize', measure);
      settling.forEach((timer) => window.clearTimeout(timer));
    };
  }, [active]);
}
