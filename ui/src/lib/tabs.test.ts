import { describe, expect, it } from 'vitest';
import { applyTheme, loadTheme, parseTheme, saveTheme, THEME_KEY } from './theme';
import { effectiveTab, tabAfterKey, visibleTabs } from './tabs';

describe('tabs', () => {
  it('shows Testing only where there is something to show', () => {
    expect(visibleTabs(true)).toEqual(['apps', 'testing', 'updates', 'options']);
    expect(visibleTabs(false)).toEqual(['apps', 'updates', 'options']);
  });

  it('falls back to the first tab when the one in front is gone', () => {
    expect(effectiveTab('testing', visibleTabs(false))).toBe('apps');
    expect(effectiveTab('updates', visibleTabs(false))).toBe('updates');
  });

  it('moves with the arrow keys, wraps round, and ignores other keys', () => {
    const shown = visibleTabs(true);
    expect(tabAfterKey(shown, 'apps', 'ArrowRight')).toBe('testing');
    expect(tabAfterKey(shown, 'apps', 'ArrowLeft')).toBe('options');
    expect(tabAfterKey(shown, 'options', 'ArrowRight')).toBe('apps');
    expect(tabAfterKey(shown, 'updates', 'Home')).toBe('apps');
    expect(tabAfterKey(shown, 'updates', 'End')).toBe('options');
    expect(tabAfterKey(shown, 'updates', 'a')).toBeNull();
    expect(tabAfterKey(visibleTabs(false), 'testing', 'ArrowRight')).toBeNull();
  });
});

describe('theme', () => {
  it('knows three themes and takes anything else for the system', () => {
    expect(parseTheme('dark')).toBe('dark');
    expect(parseTheme('light')).toBe('light');
    expect(parseTheme('purple')).toBe('system');
    expect(parseTheme(null)).toBe('system');
  });

  it('sets data-theme, and removes it for the system', () => {
    const attrs = new Map<string, string>();
    const root = {
      setAttribute: (k: string, v: string) => void attrs.set(k, v),
      removeAttribute: (k: string) => void attrs.delete(k),
    };
    applyTheme(root, 'dark');
    expect(attrs.get('data-theme')).toBe('dark');
    applyTheme(root, 'system');
    expect(attrs.has('data-theme')).toBe(false);
  });

  it('saves and loads, and survives a blocked storage', () => {
    const data = new Map<string, string>();
    const store = {
      getItem: (k: string) => data.get(k) ?? null,
      setItem: (k: string, v: string) => void data.set(k, v),
      removeItem: (k: string) => void data.delete(k),
    };
    saveTheme(store, 'light');
    expect(data.get(THEME_KEY)).toBe('light');
    expect(loadTheme(store)).toBe('light');
    saveTheme(store, 'system');
    expect(data.has(THEME_KEY)).toBe(false);
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
    expect(loadTheme(blocked)).toBe('system');
    expect(() => saveTheme(blocked, 'dark')).not.toThrow();
  });
});
