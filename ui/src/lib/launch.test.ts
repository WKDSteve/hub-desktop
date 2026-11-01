import { describe, expect, it } from 'vitest';
import { Hub, type HubApp } from './api';
import { App, type Clock, type TokenStore } from './app.svelte';

const tasks: HubApp = {
  id: 'tasks',
  name: 'Tasks',
  summary: '',
  status: 'available',
  installed: true,
  native: true,
  web: true,
  default_mode: 'native',
  run: 'stopped',
  setup: null,
  install: { hint: 'release', page: null, file: null },
  repo: { owner: 'x', name: 'y', private: false },
};

/** Time that only moves when the test says so. */
class FakeClock implements Clock {
  t = 0;
  timers: { at: number; fn: () => void; live: boolean }[] = [];
  now = () => this.t;
  sleep = async (ms: number) => {
    this.t += ms;
  };
  later = (ms: number, fn: () => void) => {
    const timer = { at: this.t + ms, fn, live: true };
    this.timers.push(timer);
    return () => {
      timer.live = false;
    };
  };
  advance(ms: number) {
    this.t += ms;
    for (const timer of this.timers) if (timer.live && timer.at <= this.t) timer.live = false, timer.fn();
  }
}

/** A hub that answers like the real one: a list that the test changes, and a launch that answers as told. */
function fakeHub(options: { apps?: Partial<HubApp>[]; launch?: (id: string, body: unknown) => Response } = {}) {
  const calls: { path: string; body: unknown }[] = [];
  let apps = (options.apps ?? [{}]).map((a) => ({ ...tasks, ...a }));
  const json = (status: number, body: unknown) =>
    new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    const body = init?.body ? (JSON.parse(String(init.body)) as unknown) : undefined;
    if (path === '/auth/exchange') return json(200, { token: 't'.repeat(64) });
    calls.push({ path, body });
    if (path === '/api/hub/hello') return json(200, { instance: 'i', version: '1', engine: { ready: false, problem: null } });
    if (path === '/api/hub/apps') return json(200, { apps });
    if (path === '/api/hub/rescan') return json(200, { result: 'ok' });
    const launch = /^\/api\/hub\/apps\/([^/]+)\/launch$/.exec(path);
    if (launch) return options.launch ? options.launch(decodeURIComponent(launch[1] ?? ''), body) : json(200, { result: 'started' });
    if (/^\/api\/hub\/apps\/[^/]+\/mode$/.test(path)) return json(200, { result: 'saved' });
    return json(404, { error: { code: 'not_found', message: path } });
  }) as typeof fetch;
  return {
    hub: new Hub(fetchFn),
    calls,
    launches: () => calls.filter((c) => c.path.endsWith('/launch')),
    setApps(next: Partial<HubApp>[]) {
      apps = next.map((a) => ({ ...tasks, ...a }));
    },
  };
}

const memory = (): TokenStore => ({ getItem: () => null, setItem: () => undefined, removeItem: () => undefined });

async function ready(options?: Parameters<typeof fakeHub>[0]) {
  const fake = fakeHub(options);
  const clock = new FakeClock();
  const app = new App(fake.hub, memory(), clock);
  await app.start('c0de');
  expect(app.phase).toBe('ready');
  return { app, clock, ...fake };
}

describe('launch', () => {
  it('one click is one request, and the mode is sent only when asked for', async () => {
    const { app, launches } = await ready();
    await app.launch(app.apps[0] as HubApp);
    expect(launches().map((c) => c.body)).toEqual([{}]);
    const second = await ready();
    await second.app.launch(second.app.apps[0] as HubApp, 'web');
    expect(second.launches().map((c) => c.body)).toEqual([{ mode: 'web' }]);
  });

  it('a double click, or a click while the start is on its way, sends nothing more', async () => {
    const { app, launches, clock } = await ready();
    const tile = app.apps[0] as HubApp;
    await Promise.all([app.launch(tile), app.launch(tile)]);
    await app.launch(tile);
    expect(launches()).toHaveLength(1);
    // Later, when the start is over, a click is a click again.
    await clock.sleep(20_000);
    app.starting = {};
    await app.launch(tile);
    expect(launches()).toHaveLength(2);
  });

  it('says what happened in one line, an error as an error', async () => {
    const opened = await ready({ launch: () => new Response(JSON.stringify({ result: 'opened' }), { status: 200 }) });
    await opened.app.launch(opened.app.apps[0] as HubApp);
    expect(opened.app.toast).toEqual({ kind: 'ok', text: 'Opened Tasks.' });

    const refused = await ready({
      launch: () =>
        new Response(JSON.stringify({ error: { code: 'port_busy', message: 'x' } }), {
          status: 409,
          headers: { 'Content-Type': 'application/json' },
        }),
    });
    await refused.app.launch(refused.app.apps[0] as HubApp);
    expect(refused.app.toast?.kind).toBe('error');
    expect(refused.app.toast?.text).toContain('busy');
    expect(refused.app.failed['tasks']).toBe('port_busy');
  });

  it('asks about the setup instead of opening a page that cannot work', async () => {
    const { app } = await ready({
      apps: [{ setup: 'no_vault' }],
      launch: () => new Response(JSON.stringify({ result: 'setup_required' }), { status: 200 }),
    });
    await app.launch(app.apps[0] as HubApp);
    expect(app.setupFor?.id).toBe('tasks');
    expect(app.toast).toBeNull();
  });

  it('the tile says starting until the app runs, and not for ever', async () => {
    const { app, setApps, clock } = await ready();
    const tile = app.apps[0] as HubApp;
    const watching = app.watch(tile.id);
    await Promise.resolve();
    expect(app.starting['tasks']).toBeDefined();
    setApps([{ run: 'running' }]);
    await watching;
    expect(app.starting['tasks']).toBeUndefined();
    expect(app.apps[0]?.run).toBe('running');

    // An app that never shows up: the time is up after the limit.
    setApps([{ run: 'stopped' }]);
    const before = clock.now();
    await app.watch('tasks');
    expect(clock.now() - before).toBeGreaterThanOrEqual(15_000);
    expect(app.starting['tasks']).toBeUndefined();
  });

  it('a program that cannot be looked at is not waited for', async () => {
    const { app, setApps, clock } = await ready();
    setApps([{ run: null }]);
    const before = clock.now();
    await app.watch('tasks');
    expect(clock.now() - before).toBeLessThan(5_000);
  });

  it('a look that finds the app running takes the mark of a refused start away', async () => {
    const { app, setApps } = await ready();
    app.failed = { tasks: 'start_failed' };
    setApps([{ run: 'running' }]);
    await app.reload();
    expect(app.failed).toEqual({});
  });

  it('keeps the list when the hub does not answer for a moment', async () => {
    const { app } = await ready();
    const hub = app['hub'] as Hub;
    hub.apps = () => Promise.reject(new Error('busy'));
    await app.reload();
    expect(app.apps).toHaveLength(1);
    expect(app.phase).toBe('ready');
  });
});

describe('messages and choices', () => {
  it('a message goes by itself after a while, and a newer one replaces it', async () => {
    const { app, clock } = await ready();
    app.say('ok', 'one');
    clock.advance(3000);
    app.say('ok', 'two');
    clock.advance(4000);
    expect(app.toast?.text).toBe('two');
    clock.advance(3000);
    expect(app.toast).toBeNull();
  });

  it('keeping a mode asks the hub once and reads the list again', async () => {
    const { app, calls } = await ready();
    await app.makeDefault(app.apps[0] as HubApp, 'web');
    expect(calls.filter((c) => c.path.endsWith('/mode')).map((c) => c.body)).toEqual([{ mode: 'web' }]);
    expect(calls.filter((c) => c.path === '/api/hub/apps').length).toBeGreaterThanOrEqual(2);
    expect(app.toast?.text).toContain('web app');
  });

  it('a rescan asks the hub to look again and then reads the list', async () => {
    const { app, calls } = await ready();
    await app.rescan();
    const order = calls.map((c) => c.path).filter((p) => p.endsWith('/rescan') || p.endsWith('/apps'));
    expect(order.slice(-2)).toEqual(['/api/hub/rescan', '/api/hub/apps']);
  });

  it('an id that is not a plain word is encoded in the path of the request', async () => {
    const { hub, calls } = fakeHub();
    await hub.launch('a/b c', 'native').catch(() => undefined);
    expect(calls[0]?.path).toBe('/api/hub/apps/a%2Fb%20c/launch');
  });
});
