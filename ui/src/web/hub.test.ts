import { describe, expect, it } from 'vitest';
import catalog from './catalog.json';
import { type CatalogEntry, type MissingStore, pageLink, view, WebHub } from './hub';

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
    load: async () => start,
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
    expect(view(entry('testing'), new Set()).installed).toBe(false);
    expect(view(entry('testing'), new Set()).install.hint).toBe('planned');
    expect(view(entry('streamtap'), new Set()).install.hint).toBe('build_yourself');
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
    expect(store.saved).toEqual([]);
    expect((await hub.apps()).apps.find((a) => a.id === 'tasks')?.missing).toBe(true);
    await hub.mark('tasks', true);
    expect(store.saved).toEqual([['tasks']]);
  });
});
