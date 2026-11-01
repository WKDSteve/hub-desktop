import { describe, expect, it } from 'vitest';
import type { HubApp } from './api';
import { areas, getting, hasMenu, hasMode, moveFocus, otherMode, stateOf, type Box } from './tiles';

const base: HubApp = {
  id: 'tasks',
  name: 'Tasks',
  summary: 'The task overview',
  status: 'available',
  installed: true,
  native: true,
  web: false,
  default_mode: 'native',
  run: 'stopped',
  setup: null,
  stoppable: false,
  install: { hint: 'release', page: 'https://github.com/x/y/releases/latest', file: 'x.exe' },
  repo: { owner: 'x', name: 'y', private: false },
};
const app = (over: Partial<HubApp>): HubApp => ({ ...base, ...over });

describe('areas', () => {
  const all = [
    app({ id: 'tasks', name: 'Tasks' }),
    app({ id: 'ai', name: 'AI Desktop', summary: 'workbench', installed: false }),
    app({ id: 'docmap', name: 'Docmap', summary: 'The map', installed: true }),
  ];

  it('puts what is on the machine first and keeps the order of the catalog', () => {
    const got = areas(all, '');
    expect(got.installed.map((a) => a.id)).toEqual(['tasks', 'docmap']);
    expect(got.other.map((a) => a.id)).toEqual(['ai']);
  });

  it('filters by name, summary or id, without regard to case', () => {
    expect(areas(all, 'MAP').installed.map((a) => a.id)).toEqual(['docmap']);
    expect(areas(all, 'bench').other.map((a) => a.id)).toEqual(['ai']);
    expect(areas(all, '  tasks ').installed.map((a) => a.id)).toEqual(['tasks']);
    expect(areas(all, 'zzz')).toEqual({ installed: [], other: [] });
  });
});

describe('stateOf', () => {
  it('says a word for every state, and a start that is on its way wins', () => {
    expect(stateOf(app({ run: 'running' }), false)).toEqual({ kind: 'running', label: 'Running' });
    expect(stateOf(app({ run: 'stopped' }), false)).toEqual({ kind: 'stopped', label: 'Stopped' });
    expect(stateOf(app({ run: null }), false)).toEqual({ kind: 'none', label: '' });
    expect(stateOf(app({ run: 'stopped' }), true).kind).toBe('starting');
    expect(stateOf(app({ run: 'running', setup: 'no_vault' }), false).kind).toBe('setup');
    expect(stateOf(app({ run: 'port_busy' }), false)).toEqual({ kind: 'busy', label: 'Port busy' });
  });

  it('shows a refused start until the next look says otherwise', () => {
    expect(stateOf(app({}), false, 'start_failed').kind).toBe('failed');
    expect(stateOf(app({}), false, 'port_busy').kind).toBe('busy');
  });
});

describe('modes', () => {
  it('the second action is the mode a click does not open', () => {
    expect(otherMode(app({ default_mode: 'native' }))).toBe('web');
    expect(otherMode(app({ default_mode: 'web' }))).toBe('native');
  });

  it('a mode is there when its program or page is', () => {
    expect(hasMode(app({ native: true, web: false }), 'native')).toBe(true);
    expect(hasMode(app({ native: true, web: false }), 'web')).toBe(false);
  });
});

describe('getting', () => {
  it('links only what has a page, and names what is planned or built by hand', () => {
    expect(getting(app({}))).toEqual({ badge: 'Download', linked: true, note: null });
    expect(getting(app({ install: { hint: 'planned', page: null, file: null } }))).toEqual({ badge: 'Planned', linked: false, note: null });
    expect(getting(app({ install: { hint: 'build_yourself', page: null, file: null } }))).toEqual({
      badge: 'Build it yourself',
      linked: false,
      note: null,
    });
    expect(getting(app({ install: { hint: 'release', page: null, file: null } })).linked).toBe(false);
  });

  it('says that a native app is not startable in the web hub, and what the viewer marked', () => {
    const native = getting(app({ install: { hint: 'native_only', page: null, file: null } }));
    expect(native).toEqual({ badge: 'Native only', linked: false, note: 'Not startable in the web hub' });
    const gone = getting(app({ missing: true, install: { hint: 'clone', page: 'https://github.com/x/y-web', file: null } }));
    expect(gone).toEqual({ badge: 'Clone the web variant', linked: true, note: 'Marked as not present' });
  });
});

describe('hasMenu', () => {
  it('is for an installed app, a link of the web hub and an app the viewer marked', () => {
    expect(hasMenu(app({}))).toBe(true);
    expect(hasMenu(app({ installed: false }))).toBe(false);
    expect(hasMenu(app({ installed: true, href: '../tasks-web/tasks.html' }))).toBe(true);
    expect(hasMenu(app({ installed: false, missing: true }))).toBe(true);
  });
});

describe('moveFocus', () => {
  // Two rows of three tiles, and a fourth row with one.
  const box = (col: number, row: number): Box => ({ left: col * 170, top: row * 150, width: 160, height: 140 });
  const grid = [box(0, 0), box(1, 0), box(2, 0), box(0, 1), box(1, 1), box(2, 1), box(0, 2)];

  it('walks the order with Left and Right and stops at the ends', () => {
    expect(moveFocus(grid, 1, 'ArrowRight')).toBe(2);
    expect(moveFocus(grid, 1, 'ArrowLeft')).toBe(0);
    expect(moveFocus(grid, 0, 'ArrowLeft')).toBeNull();
    expect(moveFocus(grid, 6, 'ArrowRight')).toBeNull();
    expect(moveFocus(grid, 3, 'Home')).toBe(0);
    expect(moveFocus(grid, 3, 'End')).toBe(6);
  });

  it('goes to the tile below or above whose middle is the nearest', () => {
    expect(moveFocus(grid, 1, 'ArrowDown')).toBe(4);
    expect(moveFocus(grid, 4, 'ArrowUp')).toBe(1);
    // The last row has one tile: from the right column the nearest is that one.
    expect(moveFocus(grid, 5, 'ArrowDown')).toBe(6);
    expect(moveFocus(grid, 0, 'ArrowUp')).toBeNull();
    expect(moveFocus(grid, 6, 'ArrowDown')).toBeNull();
  });

  it('ignores a key that is no move and an index that is no tile', () => {
    expect(moveFocus(grid, 0, 'a')).toBeNull();
    expect(moveFocus(grid, 99, 'ArrowRight')).toBeNull();
    expect(moveFocus([], 0, 'Home')).toBeNull();
  });
});
