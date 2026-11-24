import { afterEach, describe, expect, it } from 'vitest';
import { DE } from './de';
import {
  fill,
  LANG_KEY,
  loadChoice,
  msg,
  numberText,
  parseChoice,
  resolveLang,
  saveChoice,
  setLang,
  t,
} from './i18n.svelte';

// The source of the page itself, as text (the tests, the catalog and the module of `t` itself are not the code that
// speaks).
const FILES = import.meta.glob<string>(['../**/*.ts', '../**/*.svelte', '!../**/*.test.ts', '!../lib/de.ts', '!../lib/i18n.svelte.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** Every sentence the code hands to `t()` or `msg()` as a plain string. */
function sentences(): Set<string> {
  const found = new Set<string>();
  for (const text of Object.values(FILES)) {
    for (const m of text.matchAll(/\b(?:t|msg)\(\s*'((?:[^'\\]|\\.)*)'/g)) found.add(m[1] as string);
  }
  return found;
}

const places = (s: string): string[] => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1] as string).sort();

afterEach(() => setLang('en'));

describe('the catalog', () => {
  it('finds the sentences of the code (a scan that finds nothing proves nothing)', () => {
    const found = sentences();
    expect(found.size).toBeGreaterThan(150);
    expect(found.has('Update all')).toBe(true);
    expect(found.has('The CI watcher is switched off.')).toBe(true);
  });

  it('has a German text for every sentence of the code', () => {
    const missing = [...sentences()].filter((s) => !(s in DE));
    expect(missing).toEqual([]);
  });

  it('holds no entry the code no longer uses', () => {
    const found = sentences();
    expect(Object.keys(DE).filter((k) => !found.has(k))).toEqual([]);
  });

  it('names the same places on both sides, and leaves no German text empty', () => {
    for (const [en, de] of Object.entries(DE)) {
      expect(de.trim(), en).not.toBe('');
      expect(places(de), en).toEqual(places(en));
    }
  });
});

describe('t', () => {
  it('is the English sentence, with the places filled', () => {
    expect(t('{name} is not installed.', { name: 'Tasks' })).toBe('Tasks is not installed.');
    expect(fill('{a} and {b}', { a: 1 })).toBe('1 and {b}');
    expect(msg('x')).toBe('x');
  });

  it('is the German text after setLang, and the English one for a sentence it does not know', () => {
    setLang('de');
    expect(t('{name} is not installed.', { name: 'Tasks' })).toBe('Tasks ist nicht installiert.');
    expect(t('a sentence nobody translated')).toBe('a sentence nobody translated');
  });

  it('takes the browser language only when the person picked none', () => {
    expect(resolveLang('system', 'de-AT')).toBe('de');
    expect(resolveLang('system', 'en-US')).toBe('en');
    expect(resolveLang('system', undefined)).toBe('en');
    expect(resolveLang('en', 'de-DE')).toBe('en');
    expect(resolveLang('de', 'fr')).toBe('de');
    expect(parseChoice('de')).toBe('de');
    expect(parseChoice('klingon')).toBe('system');
  });

  it('saves and loads the choice, and survives a blocked storage', () => {
    const data = new Map<string, string>();
    const store = {
      getItem: (k: string) => data.get(k) ?? null,
      setItem: (k: string, v: string) => void data.set(k, v),
      removeItem: (k: string) => void data.delete(k),
    };
    saveChoice(store, 'de');
    expect(data.get(LANG_KEY)).toBe('de');
    expect(loadChoice(store)).toBe('de');
    saveChoice(store, 'system');
    expect(data.has(LANG_KEY)).toBe(false);
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
      removeItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadChoice(blocked)).toBe('system');
    expect(() => saveChoice(blocked, 'de')).not.toThrow();
  });
});

describe('numbers in the writing of the language', () => {
  afterEach(() => setLang('en'));

  it('a number of a sentence is grouped as the language does', () => {
    expect(t('{n} min ago', { n: 1200 })).toBe('1,200 min ago');
    setLang('de');
    expect(numberText(1200)).toBe('1.200');
    expect(fill('{n} of {m}', { n: 3, m: 1200 })).toBe('3 of 1.200');
  });

  it('a word stays as it is', () => {
    expect(fill('{a}', { a: '1200' })).toBe('1200');
  });
});
