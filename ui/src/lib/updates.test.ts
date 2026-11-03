import { describe, expect, it } from 'vitest';
import type { HubApp } from './api';
import { App, type TokenStore } from './app.svelte';
import {
  askable,
  buildOfRelease,
  DAY_MS,
  differs,
  due,
  releaseUrl,
  stateOf,
  UpdateChecker,
  type Memory,
} from './updates';

function app(over: Partial<HubApp> = {}): HubApp {
  return {
    id: 'tasks',
    name: 'Tasks',
    summary: '',
    status: 'available',
    installed: true,
    native: true,
    web: false,
    default_mode: 'native',
    run: 'running',
    setup: null,
    stoppable: true,
    build: '9f3b7ba',
    install: { hint: 'release', page: null, file: null },
    repo: { owner: 'WKDSteve', name: 'tasks-desktop', private: false },
    ...over,
  };
}

const release = (name: unknown) => JSON.stringify({ name, tag_name: 'latest', assets: [] });

describe('buildOfRelease', () => {
  it('takes the short commit id out of the title of the rolling release', () => {
    expect(buildOfRelease({ name: 'Rolling build (9f3b7ba)' })).toBe('9f3b7ba');
    expect(buildOfRelease({ name: `Rolling build (${'a'.repeat(40)})` })).toBe('a'.repeat(40));
  });

  it('knows nothing of any other form', () => {
    for (const doc of [
      null,
      'Rolling build (9f3b7ba)',
      [],
      {},
      { name: 7 },
      { name: '' },
      { name: 'v1.2.3' },
      { tag_name: 'latest' },
      { name: 'Rolling build (9F3B7BA)' },
      { name: 'Rolling build (9f3b7b)' },
      { name: 'Rolling build (9f3b7ba) extra' },
      { name: 'Rolling build (<script>)' },
      { name: `Rolling build (${'a'.repeat(41)})` },
      { name: `Rolling build (9f3b7ba)${' '.repeat(200)}` },
    ]) {
      expect(buildOfRelease(doc), JSON.stringify(doc)).toBeNull();
    }
  });
});

describe('differs and stateOf', () => {
  it('compares short commit ids, of any length, as the same commit when one starts the other', () => {
    expect(differs('9f3b7ba', '9f3b7ba')).toBe(false);
    expect(differs('9f3b7ba', '9f3b7ba539ab')).toBe(false);
    expect(differs('9f3b7ba539ab', '9f3b7ba')).toBe(false);
    expect(differs('9F3B7BA', '9f3b7ba')).toBe(false);
    expect(differs('9f3b7ba', '1234567')).toBe(true);
  });

  it('is not an update when either side is not known', () => {
    for (const [a, b] of [
      [null, '9f3b7ba'],
      [undefined, '9f3b7ba'],
      ['', '9f3b7ba'],
      ['9f3b7ba', null],
    ] as const) {
      expect(differs(a, b)).toBe(false);
      expect(stateOf(a, b)).toBe('unknown');
    }
    expect(stateOf('9f3b7ba', '1234567')).toBe('available');
    expect(stateOf('9f3b7ba', '9f3b7ba')).toBe('current');
  });
});

describe('askable', () => {
  it('is an installed app of a public repository that reports its build', () => {
    expect(askable(app())).toBe(true);
  });

  it('is not a private repository, an app without a build, one that is not installed, or a link of the web hub', () => {
    expect(askable(app({ repo: { owner: 'WKDSteve', name: 'ai-desktop', private: true } }))).toBe(false);
    expect(askable(app({ build: null }))).toBe(false);
    expect(askable(app({ build: undefined }))).toBe(false);
    expect(askable(app({ build: '' }))).toBe(false);
    expect(askable(app({ installed: false }))).toBe(false);
    expect(askable(app({ href: '../tasks-web/tasks.html' }))).toBe(false);
  });

  it('is not a repository named with anything but plain characters', () => {
    for (const name of ['../x', 'a/b', 'a b', '', 'a?x=1', 'a#b', '.hidden', 'a'.repeat(101)]) {
      expect(askable(app({ repo: { owner: 'WKDSteve', name, private: false } })), name).toBe(false);
    }
  });
});

describe('releaseUrl and due', () => {
  it('asks the one release of the rolling build', () => {
    expect(releaseUrl({ owner: 'WKDSteve', name: 'tasks-desktop' })).toBe(
      'https://api.github.com/repos/WKDSteve/tasks-desktop/releases/tags/latest',
    );
  });

  it('allows one question a day, and none sooner, whatever the clock says', () => {
    expect(due(null, 1_000)).toBe(true);
    expect(due(1_000, 1_000 + DAY_MS)).toBe(true);
    expect(due(1_000, 1_000 + DAY_MS - 1)).toBe(false);
    expect(due(5_000, 1_000)).toBe(true);
  });
});

/** A fetch that answers like GitHub and keeps every question. */
function github(reply: (url: string) => Response | Promise<Response>) {
  const asked: { url: string; init: RequestInit | undefined }[] = [];
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    asked.push({ url: String(input), init });
    return reply(String(input));
  }) as typeof fetch;
  return { fetchFn, asked };
}

function memory(initial: Record<string, string> = {}): Memory & { data: Record<string, string> } {
  const data = { ...initial };
  return { data, getItem: (k) => data[k] ?? null, setItem: (k, v) => void (data[k] = v) };
}

const ok = (name: unknown) => new Response(release(name), { status: 200 });

describe('UpdateChecker', () => {
  it('asks GitHub without a token, a cookie or a referrer, and finds a newer build', async () => {
    const g = github(() => ok('Rolling build (1234567)'));
    const round = await new UpdateChecker(g.fetchFn, memory()).check([app()]);
    expect(round).toEqual({ states: { tasks: 'available' }, failed: false });
    expect(g.asked).toHaveLength(1);
    expect(g.asked[0]?.url).toBe('https://api.github.com/repos/WKDSteve/tasks-desktop/releases/tags/latest');
    const init = g.asked[0]?.init;
    expect(init?.credentials).toBe('omit');
    expect(init?.referrerPolicy).toBe('no-referrer');
    expect(Object.keys((init?.headers ?? {}) as Record<string, string>).map((k) => k.toLowerCase())).toEqual(['accept']);
  });

  it('says current when GitHub names the build that runs', async () => {
    const g = github(() => ok('Rolling build (9f3b7ba)'));
    const round = await new UpdateChecker(g.fetchFn, memory()).check([app()]);
    expect(round.states).toEqual({ tasks: 'current' });
  });

  it('asks an app once a day: the second round is judged from what was kept, even for another installed build', async () => {
    let now = 1_000_000;
    const g = github(() => ok('Rolling build (1234567)'));
    const checker = new UpdateChecker(g.fetchFn, memory(), () => now);
    await checker.check([app()]);
    now += DAY_MS - 1;
    expect((await checker.check([app()])).states).toEqual({ tasks: 'available' });
    // The app was updated in between: no new question, and the badge goes away.
    expect((await checker.check([app({ build: '1234567' })])).states).toEqual({ tasks: 'current' });
    expect(g.asked).toHaveLength(1);
    now += 1;
    await checker.check([app()]);
    expect(g.asked).toHaveLength(2);
  });

  it('never asks about a private repository, an app without a build, or one that is not installed', async () => {
    const g = github(() => ok('Rolling build (1234567)'));
    const round = await new UpdateChecker(g.fetchFn, memory()).check([
      app({ id: 'ai', repo: { owner: 'WKDSteve', name: 'ai-desktop', private: true } }),
      app({ id: 'rules', build: null }),
      app({ id: 'docmap', installed: false }),
    ]);
    expect(g.asked).toHaveLength(0);
    expect(round).toEqual({ states: {}, failed: false });
  });

  it('treats a title of any other form, and an answer that is not JSON or too large, as not known', async () => {
    for (const reply of [
      () => ok('v1.2.3'),
      () => ok(null),
      () => new Response('<html>', { status: 200 }),
      () => new Response('x'.repeat(600 * 1024), { status: 200 }),
    ]) {
      const checker = new UpdateChecker(github(reply).fetchFn, memory());
      const round = await checker.check([app()]);
      // A parsed answer of a form we do not know is "not known"; an answer we cannot read is a failure.
      expect(round.states['tasks']).toBe('unknown');
    }
  });

  it('stops asking after a refusal (a rate limit holds for every repository) and does not keep it', async () => {
    const store = memory();
    const g = github(() => new Response('{}', { status: 403 }));
    const round = await new UpdateChecker(g.fetchFn, store).check([
      app(),
      app({ id: 'rules', repo: { owner: 'WKDSteve', name: 'rules-desktop', private: false } }),
    ]);
    expect(round.failed).toBe(true);
    expect(g.asked).toHaveLength(1);
    expect(round.states).toEqual({ tasks: 'unknown', rules: 'unknown' });
    expect(store.data).toEqual({});
  });

  it('does not ask when the network fails, and says so', async () => {
    const checker = new UpdateChecker(
      (async () => {
        throw new TypeError('offline');
      }) as typeof fetch,
      memory(),
    );
    expect(await checker.check([app()])).toEqual({ states: { tasks: 'unknown' }, failed: true });
  });

  it('ignores a kept entry of a shape we did not write, and works with a storage that throws', async () => {
    const bad = memory({ 'hub-desktop.update.tasks': '{"at":"x","available":"<b>"}' });
    const g = github(() => ok('Rolling build (1234567)'));
    expect((await new UpdateChecker(g.fetchFn, bad).check([app()])).states).toEqual({ tasks: 'available' });
    expect(g.asked).toHaveLength(1);

    const broken: Memory = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect((await new UpdateChecker(g.fetchFn, broken).check([app()])).states).toEqual({ tasks: 'available' });
    expect((await new UpdateChecker(g.fetchFn, null).check([app()])).states).toEqual({ tasks: 'available' });
  });
});

describe('App and the update check', () => {
  const session: TokenStore = { getItem: () => null, setItem: () => undefined, removeItem: () => undefined };

  function hubWith(apps: HubApp[]) {
    const fetchFn = (async (input: RequestInfo | URL) => {
      const path = String(input);
      const json = (body: unknown) =>
        new Response(JSON.stringify(body), { status: 200, headers: { 'Content-Type': 'application/json' } });
      if (path === '/auth/exchange') return json({ token: 'a'.repeat(64) });
      if (path === '/api/hub/hello') return json({ instance: 'i', version: '1', engine: { ready: false, problem: null } });
      if (path === '/api/hub/apps') return json({ apps });
      return json({ state: 'not_configured' });
    }) as typeof fetch;
    return fetchFn;
  }

  it('asks GitHub only when the button is pressed, never while the page starts', async () => {
    const { Hub } = await import('./api');
    const g = github(() => ok('Rolling build (1234567)'));
    const state = new App(
      new Hub(hubWith([app()])),
      session,
      undefined,
      null,
      new UpdateChecker(g.fetchFn, memory()),
    );
    await state.start('code');
    expect(state.phase).toBe('ready');
    expect(g.asked).toHaveLength(0);
    expect(state.canCheckUpdates).toBe(true);
    expect(state.hasUpdate('tasks')).toBe(false);

    await state.checkUpdates();
    expect(g.asked).toHaveLength(1);
    expect(state.hasUpdate('tasks')).toBe(true);
    expect(state.toast?.text).toBe('1 update is available.');
  });

  it('has no button in the web hub (a presence is there) or without a checker', async () => {
    const { Hub } = await import('./api');
    const g = github(() => ok('Rolling build (1234567)'));
    const presence = { mark: async () => undefined };
    const web = new App(new Hub(hubWith([app()])), session, undefined, presence, new UpdateChecker(g.fetchFn, memory()));
    expect(web.canCheckUpdates).toBe(false);
    await web.start('code');
    await web.checkUpdates();
    expect(g.asked).toHaveLength(0);

    const none = new App(new Hub(hubWith([app()])), session);
    expect(none.canCheckUpdates).toBe(false);
  });

  it('says in a sentence when GitHub refuses, and shows no badge', async () => {
    const { Hub } = await import('./api');
    const g = github(() => new Response('{}', { status: 403 }));
    const state = new App(new Hub(hubWith([app()])), session, undefined, null, new UpdateChecker(g.fetchFn, memory()));
    await state.start('code');
    await state.checkUpdates();
    expect(state.toast).toEqual({ kind: 'error', text: 'GitHub did not answer. Try again later.' });
    expect(state.hasUpdate('tasks')).toBe(false);
  });

  it('says so when no app can be checked', async () => {
    const { Hub } = await import('./api');
    const g = github(() => ok('Rolling build (1234567)'));
    const state = new App(
      new Hub(hubWith([app({ build: null })])),
      session,
      undefined,
      null,
      new UpdateChecker(g.fetchFn, memory()),
    );
    await state.start('code');
    await state.checkUpdates();
    expect(g.asked).toHaveLength(0);
    expect(state.toast?.kind).toBe('ok');
  });
});
