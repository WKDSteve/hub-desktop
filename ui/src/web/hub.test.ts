import { describe, expect, it } from 'vitest';
import catalog from './catalog.json';
import type { DirHandleLike } from 'appkit-web/folder';
import { App } from '../lib/app.svelte';
import { type CatalogEntry, type FolderPicker, type MissingStore, pageLink, presentIn, view, WebHub } from './hub';

const entries = catalog as CatalogEntry[];
const entry = (id: string): CatalogEntry => {
  const e = entries.find((x) => x.id === id);
  if (!e) throw new Error(id);
  return e;
};

function memory(start: string[] = []): MissingStore & { saved: string[][] } {
  const saved: string[][] = [];
  return {
    saved,
    load: async () => saved.at(-1) ?? start,
    save: async (ids) => void saved.push(ids),
  };
}

describe('view', () => {
  it('links a web variant that is there, with the page next to this one', () => {
    const tasks = view(entry('tasks'), new Set());
    expect(tasks).toMatchObject({ installed: true, web: true, native: false, default_mode: 'web', href: '../tasks-web/tasks.html' });
    expect(pageLink({ dir: 'a-web', entry: 'a.html' })).toBe('../a-web/a.html');
  });

  it('moves an app the viewer marked to the second area, with the page to clone it from', () => {
    const tasks = view(entry('tasks'), new Set(['tasks']));
    expect(tasks).toMatchObject({ installed: false, missing: true, href: null });
    expect(tasks.install).toEqual({ hint: 'clone', page: 'https://github.com/WKDSteve/tasks-web', file: null });
  });

  it('lists a program as native only and starts nothing', () => {
    for (const id of ['rules', 'docmap', 'ai']) {
      const app = view(entry(id), new Set());
      expect(app).toMatchObject({ installed: false, native: false, web: false });
      expect(app.install.hint).toBe('native_only');
      expect(app.href ?? null).toBeNull();
    }
  });

  it('keeps what the catalog says of an app that is planned or built by hand', () => {
    expect(view(entry('takt'), new Set()).installed).toBe(false);
    expect(view(entry('takt'), new Set()).install.hint).toBe('planned');
    expect(view(entry('streamtap'), new Set()).install.hint).toBe('native_only');
  });

  it('never makes a link from anything but the catalog', () => {
    for (const app of entries.map((e) => view(e, new Set()))) {
      if (app.href) expect(app.href).toMatch(/^\.\.\/[a-z-]+\/[a-z-]+\.html$/);
    }
  });
});

describe('WebHub', () => {
  it('answers the calls of the page', async () => {
    const hub = new WebHub(entries, memory(), '1');
    expect(await hub.exchange()).toBe('web');
    expect((await hub.hello()).version).toBe('1');
    expect((await hub.apps()).apps.map((a) => a.id)).toEqual(entries.map((e) => e.id));
    await expect(hub.launch('tasks')).rejects.toThrow();
    expect(await hub.monitor()).toEqual({ state: 'not_configured' });
  });

  it('keeps a mark, and takes it back', async () => {
    const store = memory();
    const hub = new WebHub(entries, store, '1');
    await hub.mark('tasks', true);
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.missing).toBe(true);
    await hub.mark('tasks', false);
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.installed).toBe(true);
    expect(store.saved).toEqual([['tasks'], []]);
  });

  it('ignores a mark for an app the catalog does not have, also one the browser kept', async () => {
    const store = memory(['tasks', 'nope']);
    const hub = new WebHub(entries, store, '1');
    await hub.mark('other', true);
    await hub.mark('rules', true);
    expect(store.saved).toEqual([]);
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.missing).toBe(true);
    await hub.mark('tasks', true);
    expect(store.saved).toEqual([['tasks']]);
  });

  it('starts from what another tab kept, not from what this one read first', async () => {
    const store = memory();
    // Two apps with a web variant, whatever the catalog has at the moment.
    const both = entries.map((e) => (e.id === 'testing' ? { ...e, web: { dir: 'testing-web', entry: 'testing.html' } } : e));
    const mine = new WebHub(both, store, '1');
    const other = new WebHub(both, store, '1');
    await mine.apps();
    await other.mark('tasks', true);
    await mine.mark('testing', true);
    expect(store.saved.at(-1)).toEqual(['tasks', 'testing']);
  });

  it('remembers a mark for as long as the page is open when the browser keeps nothing', async () => {
    const blocked: MissingStore = {
      load: async () => null,
      save: async () => {
        throw new Error('blocked');
      },
    };
    const hub = new WebHub(entries, blocked, '1');
    await hub.mark('tasks', true);
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.missing).toBe(true);
  });
});

/** A folder as the browser's handle gives it: names and kinds, files have no content that is read. */
type Tree = { [name: string]: Tree | 'file' };
function folder(name: string, tree: Tree): DirHandleLike {
  return {
    kind: 'directory',
    name,
    async *values() {
      for (const [child, sub] of Object.entries(tree)) {
        if (sub === 'file') {
          yield {
            kind: 'file',
            name: child,
            getFile: async () => {
              throw new Error('the content of a file is never read');
            },
          };
        } else yield folder(child, sub);
      }
    },
  };
}
const picks = (handle: DirHandleLike): FolderPicker => async () => ({ kind: 'picked', handle });

describe('the folder of the clones', () => {
  it('finds a web variant whose page is in its folder, and nothing else', async () => {
    expect(await presentIn(folder('repos', { 'tasks-web': { 'tasks.html': 'file' } }), entries)).toEqual(new Set(['tasks']));
    for (const tree of [
      {},
      { 'tasks-web': {} },
      { 'tasks-web': { 'other.html': 'file' } },
      { 'tasks-web': { 'tasks.html': {} } },
      { 'tasks-web': 'file' },
      { 'Tasks-Web': { 'tasks.html': 'file' } },
      // An app without a web variant is not looked for, even when a folder of that name is there.
      { 'rules-web': { 'rules.html': 'file' } },
    ] as Tree[]) {
      expect(await presentIn(folder('repos', tree), entries), JSON.stringify(tree)).toEqual(new Set());
    }
  });

  it('does not look for a planned app, even when its folder is there', async () => {
    const tree: Tree = { 'tasks-web': { 'tasks.html': 'file' } };
    expect(await presentIn(folder('repos', tree), entries)).toEqual(new Set(['tasks']));
    const planned = entries.map((e) => (e.id === 'tasks' ? { ...e, status: 'planned' as const } : e));
    expect(await presentIn(folder('repos', tree), planned)).toEqual(new Set());
  });

  it('looks at a bounded number of entries of a folder', async () => {
    const crowded: Tree = {};
    for (let i = 0; i < 2000; i++) crowded[`x${String(i).padStart(4, '0')}`] = {};
    crowded['tasks-web'] = { 'tasks.html': 'file' };
    expect(await presentIn(folder('repos', crowded), entries)).toEqual(new Set());
  });

  it('marks what is not in the picked folder and takes back the mark of what is', async () => {
    const store = memory();
    let tree: Tree = {};
    const hub = new WebHub(entries, store, '1', async () => ({ kind: 'picked', handle: folder('repos', tree) }));
    expect(hub.canChooseFolder).toBe(true);
    expect(await hub.chooseFolder()).toEqual({ kind: 'scanned', present: 0, missing: 2 });
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.missing).toBe(true);
    tree = { 'tasks-web': { 'tasks.html': 'file' } };
    expect(await hub.chooseFolder()).toEqual({ kind: 'scanned', present: 1, missing: 1 });
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.installed).toBe(true);
    expect((await hub.apps()).apps.find((a) => a.id === 'testing')?.missing).toBe(true);
    tree = { 'tasks-web': { 'tasks.html': 'file' }, 'testing-web': { 'testing.html': 'file' } };
    expect(await hub.chooseFolder()).toEqual({ kind: 'scanned', present: 2, missing: 0 });
    expect(store.saved).toEqual([['tasks', 'testing'], ['testing'], []]);
  });

  it('changes nothing when the person cancels, the browser refuses, or the folder cannot be read', async () => {
    const store = memory(['tasks']);
    const unreadable: DirHandleLike = {
      kind: 'directory',
      name: 'repos',
      async *values() {
        throw new Error('gone');
      },
    };
    expect(await new WebHub(entries, store, '1', async () => ({ kind: 'cancelled' })).chooseFolder()).toEqual({
      kind: 'cancelled',
    });
    expect(
      await new WebHub(entries, store, '1', async () => ({ kind: 'blocked', message: 'Not allowed here.' })).chooseFolder(),
    ).toEqual({ kind: 'failed', message: 'Not allowed here.' });
    expect(await new WebHub(entries, store, '1', picks(unreadable)).chooseFolder()).toEqual({
      kind: 'failed',
      message: 'The folder could not be read.',
    });
    expect(store.saved).toEqual([]);
  });

  it('is not offered in a browser without a folder dialog', async () => {
    const hub = new WebHub(entries, memory(), '1');
    expect(hub.canChooseFolder).toBe(false);
    expect(await hub.chooseFolder()).toEqual({ kind: 'unsupported' });
  });

  it('says on the page what it found, and the list shows it', async () => {
    const hub = new WebHub(entries, memory(), '1', picks(folder('repos', {})));
    const none = { getItem: () => null, setItem: () => undefined, removeItem: () => undefined };
    const state = new App(hub, none, undefined, hub);
    await state.start('web');
    expect(state.canChooseFolder).toBe(true);
    await state.chooseFolder();
    expect(state.toast).toEqual({ kind: 'ok', text: '0 web apps found, 2 not in the folder.' });
    expect(state.apps.find((a) => a.id === 'tasks')?.missing).toBe(true);
  });
});
