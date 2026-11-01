// What a tile says and how the keys move between tiles: plain functions, no DOM.

import type { HubApp, Mode } from './api';

/** The apps that match the filter, in two areas: those on this machine, and the rest. */
export function areas(apps: readonly HubApp[], filter: string): { installed: HubApp[]; other: HubApp[] } {
  const q = filter.trim().toLowerCase();
  const shown = q === '' ? apps : apps.filter((a) => `${a.name} ${a.summary} ${a.id}`.toLowerCase().includes(q));
  return { installed: shown.filter((a) => a.installed), other: shown.filter((a) => !a.installed) };
}

export type StateKind = 'running' | 'stopped' | 'starting' | 'setup' | 'failed' | 'busy' | 'none';

export interface TileState {
  kind: StateKind;
  /** A word, never only a colour. Empty for an app that has no state (a program with a window of its own). */
  label: string;
}

/** The state of an installed tile. `failed` is the code of a start that was refused (`start_failed`, ...). */
export function stateOf(app: HubApp, starting: boolean, failed?: string): TileState {
  if (starting) return { kind: 'starting', label: 'Starting…' };
  if (failed === 'port_busy' || app.run === 'port_busy') return { kind: 'busy', label: 'Port busy' };
  if (failed) return { kind: 'failed', label: 'Start failed' };
  if (app.setup) return { kind: 'setup', label: 'Setup needed' };
  if (app.run === 'running') return { kind: 'running', label: 'Running' };
  if (app.run === 'stopped') return { kind: 'stopped', label: 'Stopped' };
  return { kind: 'none', label: '' };
}

export const modeName = (mode: Mode): string => (mode === 'web' ? 'web' : 'native');

/** The mode a second action opens: the one a click does not. */
export function otherMode(app: HubApp): Mode {
  return app.default_mode === 'web' ? 'native' : 'web';
}

/** Whether the app can be opened in `mode` on this machine. */
export function hasMode(app: HubApp, mode: Mode): boolean {
  return mode === 'web' ? app.web : app.native;
}

/** Why a mode is not there, for the hint of a disabled entry. */
export function missingReason(app: HubApp, mode: Mode): string {
  return mode === 'web'
    ? `${app.name} has no web variant on this machine`
    : `${app.name} is not installed as a program on this machine`;
}

/** What a tile of the second area says: the badge, and whether it is a link to the page to get the app. */
export function getting(app: HubApp): { badge: string; linked: boolean } {
  switch (app.install.hint) {
    case 'planned':
      return { badge: 'Planned', linked: false };
    case 'build_yourself':
      return { badge: 'Build it yourself', linked: false };
    case 'clone':
      return { badge: 'Clone the web variant', linked: app.install.page !== null };
    default:
      return { badge: 'Download', linked: app.install.page !== null };
  }
}

/** The place of a tile on the page. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Where the focus goes from tile `from` on a key: Left and Right to the neighbour in the order, Home and End to the
 * ends, Up and Down to the nearest tile in the row above or below (the one whose middle is closest). `null` for a
 * key that moves nothing or when there is no tile that way.
 */
export function moveFocus(boxes: readonly Box[], from: number, key: string): number | null {
  const here = boxes[from];
  if (!here || boxes.length === 0) return null;
  switch (key) {
    case 'ArrowLeft':
      return from > 0 ? from - 1 : null;
    case 'ArrowRight':
      return from < boxes.length - 1 ? from + 1 : null;
    case 'Home':
      return 0;
    case 'End':
      return boxes.length - 1;
    case 'ArrowUp':
    case 'ArrowDown': {
      const down = key === 'ArrowDown';
      const middle = here.left + here.width / 2;
      // A row is as far from this one as its top is; the nearest row that way, then the nearest middle in it.
      const rows = boxes
        .map((b, i) => ({ b, i }))
        .filter(({ b }) => (down ? b.top > here.top + here.height / 2 : b.top < here.top - here.height / 2));
      if (rows.length === 0) return null;
      const nearest = down ? Math.min(...rows.map(({ b }) => b.top)) : Math.max(...rows.map(({ b }) => b.top));
      const row = rows.filter(({ b }) => Math.abs(b.top - nearest) < here.height / 2);
      let best = row[0];
      for (const c of row) {
        if (best && Math.abs(c.b.left + c.b.width / 2 - middle) < Math.abs(best.b.left + best.b.width / 2 - middle)) best = c;
      }
      return best ? best.i : null;
    }
    default:
      return null;
  }
}
