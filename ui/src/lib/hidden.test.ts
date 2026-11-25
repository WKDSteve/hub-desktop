// The loops that follow a start, a stop or an update look at the list every second while the tab is shown. A tab that is
// hidden asks nothing; when it is shown again the loop looks at once.

import { describe, expect, it } from 'vitest';
import { Hub } from './api';
import { App, type Clock, type TokenStore } from './app.svelte';

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

/** A hub whose list says the app runs, and a count of how often the list was read. */
function counting() {
  const reads = { apps: 0 };
  const fetchFn = (async (input: RequestInfo | URL) => {
    const path = String(input);
    if (path === '/auth/exchange') return json(200, { token: 't'.repeat(64) });
    if (path === '/api/hub/hello') return json(200, { instance: 'i', version: '1', engine: { ready: false, problem: null } });
    if (path === '/api/hub/apps') {
      reads.apps += 1;
      return json(200, { apps: [{ id: 'tasks', name: 'Tasks', installed: true, run: 'running' }] });
    }
    return json(404, { error: { code: 'not_found', message: path } });
  }) as typeof fetch;
  return { hub: new Hub(fetchFn), reads };
}

/** Short sleeps are over at once; the long wait of a hidden tab never ends by itself. */
function clock(state: { shown: boolean }): Clock {
  return {
    now: () => 0,
    sleep: (ms) => (ms >= 30_000 ? new Promise<void>(() => undefined) : Promise.resolve()),
    later: () => () => undefined,
    visible: () => state.shown,
  };
}

const memory = (): TokenStore => ({ getItem: () => null, setItem: () => undefined, removeItem: () => undefined });
const settle = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setTimeout(resolve, 0));
};

describe('a watch loop in a hidden tab', () => {
  it('asks the list nothing while the tab is hidden and looks once when it is shown again', async () => {
    const { hub, reads } = counting();
    const state = { shown: false };
    const app = new App(hub, memory(), clock(state));
    await app.start('c0de');
    const before = reads.apps;

    let ended = false;
    const watching = app.watch('tasks').then(() => (ended = true));
    await settle();
    expect(reads.apps).toBe(before);
    expect(ended).toBe(false);

    state.shown = true;
    app.tabShown();
    await watching;
    expect(reads.apps).toBe(before + 1);
  });

  it('a shown tab looks every beat, without waiting for anything', async () => {
    const { hub, reads } = counting();
    const app = new App(hub, memory(), clock({ shown: true }));
    await app.start('c0de');
    const before = reads.apps;
    await app.watch('tasks');
    expect(reads.apps).toBe(before + 1);
  });
});
