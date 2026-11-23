// The tabs of the page and the keys that move between them (the tab pattern of the WAI-ARIA practices).

export type Tab = 'apps' | 'testing' | 'updates' | 'options';

export const TAB_NAMES: Record<Tab, string> = {
  apps: 'Apps',
  testing: 'Testing',
  updates: 'Updates',
  options: 'Options',
};

/** The tabs that are shown: Testing only where there is something to show (an export folder or hints of the watcher). */
export function visibleTabs(testing: boolean): Tab[] {
  return testing ? ['apps', 'testing', 'updates', 'options'] : ['apps', 'updates', 'options'];
}

/** The tab in front, or the first one when the one asked for is not shown (Testing went away). */
export function effectiveTab(tab: Tab, shown: Tab[]): Tab {
  return shown.includes(tab) ? tab : (shown[0] ?? 'apps');
}

/** The tab an arrow key moves to; `null` for any other key. The ends wrap round. */
export function tabAfterKey(shown: Tab[], current: Tab, key: string): Tab | null {
  const at = shown.indexOf(current);
  if (at < 0 || shown.length === 0) return null;
  switch (key) {
    case 'ArrowRight':
      return shown[(at + 1) % shown.length] ?? null;
    case 'ArrowLeft':
      return shown[(at - 1 + shown.length) % shown.length] ?? null;
    case 'Home':
      return shown[0] ?? null;
    case 'End':
      return shown[shown.length - 1] ?? null;
    default:
      return null;
  }
}
