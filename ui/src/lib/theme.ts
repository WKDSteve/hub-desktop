// The look of the page: follow the system, or light, or dark. The tokens of the kit read `data-theme` on the root
// element and fall back to the system when it is absent.

export type Theme = 'system' | 'light' | 'dark';

export const THEMES: Theme[] = ['system', 'light', 'dark'];
export const THEME_KEY = 'hub-desktop.theme';

export function parseTheme(value: string | null | undefined): Theme {
  return value === 'light' || value === 'dark' ? value : 'system';
}

/** Put the theme on the root element; `system` removes the attribute so the system decides. */
export function applyTheme(root: Pick<HTMLElement, 'setAttribute' | 'removeAttribute'>, theme: Theme): void {
  if (theme === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', theme);
}

/** Read the saved theme; a profile that blocks the storage gives `system`. */
export function loadTheme(storage: Pick<Storage, 'getItem'> | null): Theme {
  try {
    return parseTheme(storage?.getItem(THEME_KEY));
  } catch {
    return 'system';
  }
}

export function saveTheme(storage: Pick<Storage, 'setItem' | 'removeItem'> | null, theme: Theme): void {
  try {
    if (theme === 'system') storage?.removeItem(THEME_KEY);
    else storage?.setItem(THEME_KEY, theme);
  } catch {
    // The choice then lasts until the tab closes.
  }
}
