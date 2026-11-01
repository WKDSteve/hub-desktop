import { describe, expect, it } from 'vitest';
import { Hub } from './api';
import { App, type TokenStore } from './app.svelte';

/** A fetch that answers like the hub, and keeps what it was asked. */
function fakeHub(options: { acceptCode?: string; token?: string } = {}) {
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
    if (path === '/api/hub/hello') {
      return json(200, { instance: 'i', version: '1.2.3', engine: { ready: false, problem: null } });
    }
    if (path === '/api/hub/apps') return json(200, { apps: [{ id: 'tasks', name: 'Tasks' }] });
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
});
