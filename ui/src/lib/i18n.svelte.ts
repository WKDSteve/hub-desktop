// The language of the page. The English sentence is the key (as in gettext): `t('Update failed')` is the English
// text itself, and `de.ts` holds the German for every sentence; a sentence without a German one shows in English.
// `{name}` in a sentence is filled from the second argument. `msg` marks a sentence that is only translated later
// (a table of words): it returns the text unchanged, and the test that every sentence has a German one reads it too.
//
// The current language is a rune, so everything a component derives from `t()` follows a change of it.

import { DE } from './de';

export type Lang = 'en' | 'de';
/** What the person picked: a language, or the one of the browser. */
export type LangChoice = 'system' | Lang;

export const LANGS: Lang[] = ['en', 'de'];
export const LANG_KEY = 'hub-desktop.lang';

/** The names of the languages, each in its own (never translated: a person must find theirs). */
export const LANG_NAMES: Record<Lang, string> = { en: 'English', de: 'Deutsch' };

let current = $state<Lang>('en');

export const lang = (): Lang => current;

export function setLang(next: Lang): void {
  current = next;
}

/** Mark a sentence for the catalog without translating it now. */
export const msg = (text: string): string => text;

export type Params = Record<string, string | number>;

/** Fill the `{name}` places of a sentence; a place without a value stays as it is. */
export function fill(text: string, params?: Params): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (place: string, key: string) => (key in params ? String(params[key]) : place));
}

/** The sentence in the current language. */
export function t(text: string, params?: Params): string {
  return fill(current === 'de' ? (DE[text] ?? text) : text, params);
}

export function parseChoice(value: string | null | undefined): LangChoice {
  return value === 'en' || value === 'de' ? value : 'system';
}

/** The language a choice means: the browser's when it is German, else English. */
export function resolveLang(choice: LangChoice, browser: string | null | undefined): Lang {
  if (choice !== 'system') return choice;
  return (browser ?? '').toLowerCase().startsWith('de') ? 'de' : 'en';
}

export function loadChoice(storage: Pick<Storage, 'getItem'> | null): LangChoice {
  try {
    return parseChoice(storage?.getItem(LANG_KEY));
  } catch {
    return 'system';
  }
}

export function saveChoice(storage: Pick<Storage, 'setItem' | 'removeItem'> | null, choice: LangChoice): void {
  try {
    if (choice === 'system') storage?.removeItem(LANG_KEY);
    else storage?.setItem(LANG_KEY, choice);
  } catch {
    // The choice then lasts until the tab closes.
  }
}
