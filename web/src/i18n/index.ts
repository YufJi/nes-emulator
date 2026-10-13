import { useSyncExternalStore } from 'react';
import {
  DICTIONARIES,
  LOCALES,
  type Dictionary,
  type Locale,
  type MessageKey,
} from './messages';

export { APP_NAME, LOCALE_LABELS, LOCALE_SHORT, LOCALES } from './messages';
export type { Locale, MessageKey } from './messages';

/**
 * The interface's language, in one module rather than a React context.
 *
 * Strings are needed outside components too — the cartridge loader and the
 * emulator produce the errors the pages show — so the current locale lives here
 * and both `t()` and the hook read it. A context would have been prettier, and
 * useless to `rom.ts`.
 *
 * The choice is remembered in `localStorage`, and Chinese is what a visitor who
 * has never chosen gets.
 */

const STORAGE_KEY = 'nes-locale';

const HTML_LANG: Record<Locale, string> = { zh: 'zh-CN', en: 'en', ja: 'ja' };

const FALLBACK: Locale = 'zh';

function isLocale(value: string | null): value is Locale {
  return value !== null && (LOCALES as readonly string[]).includes(value);
}

function storedLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (isLocale(stored)) return stored;
  } catch {
    // Storage can be blocked (private mode, third-party context); the default
    // language is a fine answer then.
  }
  return FALLBACK;
}

let current = storedLocale();
const listeners = new Set<() => void>();

function syncDocumentLanguage(): void {
  document.documentElement.lang = HTML_LANG[current];
}

syncDocumentLanguage();

export function locale(): Locale {
  return current;
}

export function setLocale(next: Locale): void {
  if (next === current) return;
  current = next;
  try {
    window.localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // As above: the choice then lasts for this page load only.
  }
  syncDocumentLanguage();
  for (const listener of listeners) listener();
}

/**
 * Translate a key, filling `{name}` placeholders.
 *
 * A placeholder with no matching parameter is left as it is rather than
 * rendered as `undefined`, which makes a typo visible instead of silent.
 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const template: string = DICTIONARIES[current][key];
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (placeholder, name: string) =>
    name in params ? String(params[name]) : placeholder,
  );
}

export interface I18n {
  locale: Locale;
  setLocale: (next: Locale) => void;
  t: (key: MessageKey, params?: Record<string, string | number>) => string;
}

/** Subscribe to the language and get a translator for it. */
export function useI18n(): I18n {
  const active = useSyncExternalStore(subscribe, locale);
  return { locale: active, setLocale, t };
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The dictionary for a locale, for tests and for checking coverage. */
export function dictionary(next: Locale = current): Dictionary {
  return DICTIONARIES[next];
}
