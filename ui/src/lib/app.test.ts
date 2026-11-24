import { describe, expect, it } from 'vitest';
import { Hub } from './api';
import { App, type TokenStore } from './app.svelte';

/** A fetch that answers like the hub, and keeps what it was asked. */
function fakeHub(
  options: {
    acceptCode?: string;
    token?: string;
    monitor?: () => Response;
    ci?: () => Response;
    saveCi?: (body: unknown) => Response;
    cases?: (path: string) => Response | Promise<Response>;
    /** The start with the login of tasks, as the machine holds it; the switch writes it. */
    login?: { on: boolean; refuse?: boolean };
    /** `POST /api/session/logout` answers with this status (200: the session is closed). */
    logout?: number;
  } = {},
) {
  const asked: { path: string; auth: string | null }[] = [];
  const token = options.token ?? 'a'.repeat(64);
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    const headers = new Headers(init?.headers);
    asked.push({ path, auth: headers.get('Authorization') });
    const json = (status: number, body: unknown) =>
      new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
    if (path === '/auth/exchange') {
      const { code } = JSON.parse(String(init?.body)) as { code: string };
      return code === options.acceptCode
        ? json(200, { token })
        : json(401, { error: { code: 'unauthorized', message: 'no' } });
    }
    if (headers.get('Authorization') !== `Bearer ${token}`) {
      return json(401, { error: { code: 'unauthorized', message: 'no session' } });
    }
    if (path === '/api/session/logout') {
      return json(options.logout ?? 200, options.logout && options.logout !== 200 ? { error: { code: 'down', message: 'no' } } : { ok: true });
    }
    if (path === '/api/hub/hello') {
      return json(200, { instance: 'i', version: '1.2.3', engine: { ready: false, problem: null } });
    }
    if (path === '/api/hub/apps') return json(200, { apps: [{ id: 'tasks', name: 'Tasks', ...(options.login ? { autostart: options.login.on } : {}) }] });
    if (path === '/api/hub/apps/tasks/autostart' && options.login) {
      if (options.login.refuse) return json(502, { error: { code: 'autostart_failed', message: 'no' } });
      options.login.on = (JSON.parse(String(init?.body)) as { on: boolean }).on;
      return json(200, { result: 'saved' });
    }
    if (path === '/api/hub/monitor' && options.monitor) return options.monitor();
    if (path.startsWith('/api/hub/monitor/cases?') && options.cases) return options.cases(path);
    if (path === '/api/hub/ci' && options.ci) return options.ci();
    if (path === '/api/hub/ci/settings' && options.saveCi) return options.saveCi(JSON.parse(String(init?.body)));
    return json(404, { error: { code: 'not_found', message: path } });
  }) as typeof fetch;
  return { hub: new Hub(fetchFn), asked, token };
}

function memory(initial: Record<string, string> = {}): TokenStore & { data: Record<string, string> } {
  const data = { ...initial };
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => void (data[key] = value),
    removeItem: (key) => void delete data[key],
  };
}

describe('App', () => {
  it('trades the code, keeps the session of the tab and shows what the hub lists', async () => {
    const { hub, token } = fakeHub({ acceptCode: 'c0de' });
    const session = memory();
    const app = new App(hub, session);
    await app.start('c0de');
    expect(app.phase).toBe('ready');
    expect(app.version).toBe('1.2.3');
    expect(app.apps).toEqual([{ id: 'tasks', name: 'Tasks' }]);
    expect(Object.values(session.data)).toEqual([token]);
  });

  it('takes the session of the tab again after a reload, without a code', async () => {
    const { hub, token } = fakeHub();
    const app = new App(hub, memory({ 'hub-desktop.session': token }));
    await app.start(null);
    expect(app.phase).toBe('ready');
  });

  it('asks for a link when there is neither a code nor a session', async () => {
    const { hub, asked } = fakeHub();
    const app = new App(hub, memory());
    await app.start(null);
    expect(app.phase).toBe('unpaired');
    expect(asked).toEqual([]);
  });

  it('treats a refused code as a link that has to be asked for again', async () => {
    const { hub } = fakeHub({ acceptCode: 'good' });
    const session = memory();
    const app = new App(hub, session);
    await app.start('bad');
    expect(app.phase).toBe('unpaired');
    expect(session.data).toEqual({});
  });

  it('forgets a session the hub no longer knows', async () => {
    const { hub } = fakeHub();
    const session = memory({ 'hub-desktop.session': 'f'.repeat(64) });
    const app = new App(hub, session);
    await app.start(null);
    expect(app.phase).toBe('unpaired');
    expect(session.data).toEqual({});
    expect(hub.hasToken).toBe(false);
  });

  it('still works when the profile blocks the storage', async () => {
    const { hub } = fakeHub({ acceptCode: 'c0de' });
    const blocked: TokenStore = {
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
    const app = new App(hub, blocked);
    await app.start('c0de');
    expect(app.phase).toBe('ready');
  });

  it('says what went wrong when the hub is not there', async () => {
    const hub = new Hub((async () => {
      throw new TypeError('Failed to fetch');
    }) as typeof fetch);
    const app = new App(hub, memory());
    await app.start('c0de');
    expect(app.phase).toBe('error');
    expect(app.problem).toBe('Failed to fetch');
  });
  const ready = {
    state: 'ready',
    generated_at: '2026-11-01T10:00:00Z',
    local_read: true,
    repos: [{ key: 'a-1', name: 'alpha.nvim', light: 'red', local: 'red', fail: 2, stale: 0, ci_local: null, ci_remote: null, ts: null }],
  };
  const reply = (body: unknown, status = 200) => () =>
    new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

  it('shows the monitor the hub reads, and nothing when the hub has none', async () => {
    const withRows = new App(fakeHub({ acceptCode: 'c', monitor: reply(ready) }).hub, memory());
    await withRows.start('c');
    expect(withRows.monitor).toEqual(ready);
    const without = new App(fakeHub({ acceptCode: 'c', monitor: reply({ state: 'not_configured' }) }).hub, memory());
    await without.start('c');
    expect(without.monitor).toEqual({ state: 'not_configured' });
  });

  it('a hub that does not know the monitor, or answers nonsense, still shows the apps', async () => {
    for (const monitor of [undefined, reply({ state: 'fly' }), reply({ error: { code: 'x', message: 'x' } }, 500)]) {
      const app = new App(fakeHub({ acceptCode: 'c', monitor }).hub, memory());
      await app.start('c');
      expect(app.phase).toBe('ready');
      expect(app.monitor).toEqual({ state: 'not_configured' });
    }
  });

  it('shows what the CI watcher is at, keeps it when a later look fails, and nothing from a hub without it', async () => {
    const off = { state: 'off', why: 'the hub runs for a test' };
    let answer = reply(off);
    const app = new App(fakeHub({ acceptCode: 'c', ci: () => answer() }).hub, memory());
    await app.start('c');
    expect(app.ci).toEqual(off);
    answer = reply({ state: 'fly' });
    await app.reload();
    expect(app.ci).toEqual(off);
    const ran = { state: 'ran', at: 5, written: [], not_written: [], hints: [{ kind: 'no_github', repo: 'x' }], problem: null };
    answer = reply(ran);
    await app.reload();
    expect(app.ci).toEqual(ran);
    const older = new App(fakeHub({ acceptCode: 'c' }).hub, memory());
    await older.start('c');
    expect(older.phase).toBe('ready');
    expect(older.ci).toEqual({ state: 'none' });
  });

  it('saves the options of the CI watcher, says so, and says when the hub refused them', async () => {
    const sent: unknown[] = [];
    let status = 200;
    const app = new App(
      fakeHub({
        acceptCode: 'c',
        saveCi: (body) => (sent.push(body), reply(status === 200 ? { result: 'saved' } : { error: { code: 'bad_request', message: 'no' } }, status)()),
      }).hub,
      memory(),
    );
    await app.start('c');
    await app.saveCi(true, 12);
    expect(sent).toEqual([{ on: true, interval_min: 12 }]);
    expect(app.toast).toEqual({ kind: 'ok', text: 'The CI watcher runs every 12 min.' });
    await app.saveCi(false, 12);
    expect(app.toast).toEqual({ kind: 'ok', text: 'The CI watcher is switched off.' });
    status = 400;
    await app.saveCi(true, 5);
    expect(app.toast).toEqual({ kind: 'error', text: 'The options of the CI watcher could not be saved.' });
  });

  it('keeps the rows it has when a later look fails, and takes a new answer', async () => {
    let answer = reply(ready);
    const app = new App(fakeHub({ acceptCode: 'c', monitor: () => answer() }).hub, memory());
    await app.start('c');
    answer = reply({}, 500);
    await app.reload();
    expect(app.monitor).toEqual(ready);
    let reads = 0;
    answer = () => (reads++, reply({}, 500)());
    await app.reload(false);
    expect(reads, 'a watch does not read the monitor').toBe(0);
    answer = reply({ state: 'problem', problem: 'the export cannot be read' });
    await app.reload();
    expect(app.monitor).toEqual({ state: 'problem', problem: 'the export cannot be read' });
  });
});

describe('the start with the login of a tile', () => {
  it('switches what the machine holds and shows what it says afterwards', async () => {
    const login = { on: false };
    const { hub, token } = fakeHub({ login });
    const app = new App(hub, memory({ 'hub-desktop.session': token }));
    await app.start(null);
    expect(app.apps[0]?.autostart).toBe(false);
    const tile = app.apps[0]!;
    await app.setAutostart(tile, true);
    expect(login.on).toBe(true);
    expect(app.apps[0]?.autostart).toBe(true);
    // Switched off in the app itself: the next look shows it.
    login.on = false;
    await app.reload();
    expect(app.apps[0]?.autostart).toBe(false);
  });

  it('says so when the switch fails and keeps what the machine says', async () => {
    const login = { on: true, refuse: true };
    const { hub, token } = fakeHub({ login });
    const app = new App(hub, memory({ 'hub-desktop.session': token }));
    await app.start(null);
    await app.setAutostart(app.apps[0]!, false);
    expect(app.apps[0]?.autostart).toBe(true);
    expect(app.toast?.kind).toBe('error');
  });
});

describe('the red cases of the monitor', () => {
  const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const ready = (key: string) => ({
    state: 'ready',
    key,
    local_read: true,
    cases_red: 1,
    cases_omitted: 0,
    cases: [
      { file: 'a_spec.lua', line: 3, status: 'fail', name: 'n', describe: [], message: 'm', expected: null, actual: null, flaky: false },
    ],
  });

  it('asks the hub for the key of the row, shows the answer and hides it on a second press', async () => {
    const asked: string[] = [];
    const { hub } = fakeHub({
      acceptCode: 'c',
      cases: (path) => {
        asked.push(path);
        return json(ready('a b/c'));
      },
    });
    const app = new App(hub, memory());
    await app.start('c');
    await app.toggleCases('a b/c');
    expect(asked).toEqual(['/api/hub/monitor/cases?key=a%20b%2Fc']);
    expect(app.openCases).toBe('a b/c');
    expect(app.cases).toEqual(ready('a b/c'));
    await app.toggleCases('a b/c');
    expect(app.openCases).toBeNull();
  });

  it('says the hub did not answer, and takes a reply of another shape as that', async () => {
    for (const reply of [() => json({ error: { code: 'x', message: 'x' } }, 500), () => json({ state: 'fly' })]) {
      const { hub } = fakeHub({ acceptCode: 'c', cases: reply });
      const app = new App(hub, memory());
      await app.start('c');
      await app.toggleCases('k');
      expect(app.cases).toEqual({ state: 'failed' });
    }
  });

  it('shows the row that was opened last, even when an earlier answer comes later', async () => {
    let release: () => void = () => {};
    const slow = new Promise<void>((resolve) => (release = resolve));
    const { hub } = fakeHub({
      acceptCode: 'c',
      cases: async (path) => {
        if (path.endsWith('=first')) await slow;
        return json(ready(path.split('=')[1] ?? ''));
      },
    });
    const app = new App(hub, memory());
    await app.start('c');
    const first = app.toggleCases('first');
    await app.toggleCases('second');
    release();
    await first;
    expect(app.openCases).toBe('second');
    expect(app.cases).toEqual(ready('second'));
  });
});

describe('signing out', () => {
  it('signs out: the hub is asked, then the token is forgotten and the page asks for a link', async () => {
    const { hub, token, asked } = fakeHub();
    const session = memory({ 'hub-desktop.session': token });
    const app = new App(hub, session);
    await app.start(null);
    expect(app.canSignOut).toBe(true);
    await app.signOut();
    expect(asked.some((a) => a.path === '/api/session/logout' && a.auth === `Bearer ${token}`)).toBe(true);
    expect(app.phase).toBe('unpaired');
    expect(app.canSignOut).toBe(false);
    expect(session.data['hub-desktop.session']).toBeUndefined();
  });

  it('signs out in the page even when the hub does not answer', async () => {
    const { hub, token } = fakeHub({ logout: 500 });
    const session = memory({ 'hub-desktop.session': token });
    const app = new App(hub, session);
    await app.start(null);
    await app.signOut();
    expect(app.phase).toBe('unpaired');
    expect(session.data['hub-desktop.session']).toBeUndefined();
  });
});
