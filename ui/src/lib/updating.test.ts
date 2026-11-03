import { describe, expect, it } from 'vitest';
import { Hub, type HubApp, type UpdateInfo, type UpdateJob } from './api';
import { App, type Clock, type TokenStore } from './app.svelte';
import { active, jobLabel, partText, reasonText, roundText, updatable, webUpdateLines, whyNot } from './updating';

const job = (over: Partial<UpdateJob>): UpdateJob => ({
  state: 'done',
  step: null,
  parts: [],
  lines: [],
  reload: false,
  ...over,
});

describe('the words of an update', () => {
  it('names every code the hub sends, and shows an unknown one as it is', () => {
    for (const code of ['workstation', 'dirty', 'not_on_main', 'checksum_failed', 'close_first', 'no_gh']) {
      expect(reasonText(code)).not.toContain('_');
    }
    expect(reasonText('something_new')).toBe('something new');
  });

  it('says an app can be updated when one part is ready, and why not otherwise', () => {
    expect(updatable({ id: 'a', native: 'ready', web: null, job: null })).toBe(true);
    expect(updatable({ id: 'a', native: 'workstation', web: 'ready', job: null })).toBe(true);
    expect(updatable({ id: 'a', native: 'workstation', web: null, job: null })).toBe(false);
    expect(updatable(undefined)).toBe(false);
    expect(whyNot({ id: 'a', native: 'workstation', web: null, job: null })).toContain('workstation');
    expect(whyNot(undefined)).toContain('nothing of it is installed');
  });

  it('words where a job is and what came of it', () => {
    expect(jobLabel(job({ state: 'queued' }))).toEqual({ kind: 'active', text: 'Update queued' });
    expect(jobLabel(job({ state: 'running', step: 'downloading' })).text).toBe('Downloading…');
    expect(jobLabel(job({ state: 'running', step: null })).text).toBe('Updating…');
    expect(jobLabel(job({ parts: [{ part: 'web', outcome: 'ok', result: 'up_to_date' }] })).text).toBe('Up to date');
    expect(jobLabel(job({ parts: [{ part: 'native', outcome: 'ok', result: 'installed' }] })).text).toBe('Updated');
    expect(jobLabel(job({ reload: true, parts: [{ part: 'web', outcome: 'ok', result: 'updated' }] })).text).toBe(
      'Updated: reload its open page',
    );
    const failed = jobLabel(
      job({
        state: 'failed',
        parts: [
          { part: 'native', outcome: 'skipped', result: 'workstation' },
          { part: 'web', outcome: 'failed', result: 'dirty' },
        ],
      }),
    );
    expect(failed).toEqual({ kind: 'failed', text: 'Update failed: the clone has local changes' });
    const skipped = job({ state: 'skipped', parts: [{ part: 'native', outcome: 'skipped', result: 'workstation' }] });
    expect(jobLabel(skipped).kind).toBe('skipped');
    expect(active(job({ state: 'running' }))).toBe(true);
    expect(active(job({ state: 'failed' }))).toBe(false);
    expect(active(null)).toBe(false);
  });

  it('words a part and a round', () => {
    expect(partText({ part: 'web', outcome: 'ok', result: 'updated' })).toBe('Web page: updated');
    expect(partText({ part: 'native', outcome: 'skipped', result: 'workstation' })).toMatch(/^Program: not updated, /);
    const done = job({ parts: [{ part: 'web', outcome: 'ok', result: 'updated' }] });
    const bad = job({ state: 'failed', parts: [{ part: 'web', outcome: 'failed', result: 'dirty' }] });
    expect(roundText([{ name: 'Tasks', job: done }])).toBe('Tasks: Updated.');
    expect(roundText([{ name: 'Tasks', job: done }, { name: 'Rules', job: done }])).toBe('2 apps updated.');
    expect(roundText([{ name: 'Tasks', job: bad }, { name: 'Rules', job: done }])).toBe('1 of 2 apps updated; failed: Tasks.');
  });

  it('gives the web hub the lines of a clone it links to, and nothing for anything else', () => {
    const app = { installed: true, href: '../tasks-web/tasks.html' } as HubApp;
    expect(webUpdateLines(app)).toEqual(['git -C tasks-web pull --ff-only', 'git -C tasks-web status --short']);
    for (const href of [null, undefined, '../a b/x.html', '../../x/y.html', 'https://evil.example/x/y', '../x;rm/y.html']) {
      expect(webUpdateLines({ installed: true, href } as HubApp), String(href)).toBeNull();
    }
    expect(webUpdateLines({ installed: false, href: '../tasks-web/tasks.html' } as HubApp)).toBeNull();
  });
});

/**
 * A hub that answers the updates with an idle state until a request was posted, and then with the states of
 * `after`, one per read (the last one stays).
 */
function updatingHub(script: { after: UpdateInfo[][]; queued?: number; refuse?: boolean }) {
  const token = 'a'.repeat(64);
  const posted: string[] = [];
  /** `-1`: nothing was requested yet; else the state the next read answers. */
  let next = -1;
  const idle: UpdateInfo[] = [
    { id: 'tasks', native: 'workstation', web: 'ready', job: null },
    { id: 'rules', native: 'workstation', web: null, job: null },
  ];
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    const json = (status: number, body: unknown) =>
      new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
    if (path === '/api/hub/hello') return json(200, { instance: 'i', version: '1', engine: { ready: false, problem: null } });
    if (path === '/api/hub/apps') {
      return json(200, {
        apps: [
          { id: 'tasks', name: 'Tasks' },
          { id: 'rules', name: 'Rules' },
        ],
      });
    }
    if (path === '/api/hub/updates') {
      if (next < 0) return json(200, { apps: idle });
      const state = script.after[Math.min(next, script.after.length - 1)];
      next += 1;
      return json(200, { apps: state });
    }
    if (init?.method === 'POST') {
      posted.push(path);
      if (script.refuse) return json(409, { error: { code: 'not_updatable', message: 'no' } });
      next = 0;
      if (path.endsWith('/update-all')) return json(200, { result: 'queued', queued: script.queued ?? 2 });
      return json(200, { result: 'queued' });
    }
    return json(404, { error: { code: 'not_found', message: path } });
  }) as typeof fetch;
  return { hub: new Hub(fetchFn), posted, token };
}

const store = (token: string): TokenStore => ({
  getItem: () => token,
  setItem: () => undefined,
  removeItem: () => undefined,
});

const instant: Clock = { now: () => 0, sleep: async () => undefined, later: () => () => undefined };

/** Let the watch of the page run (its sleeps are instant, its reads are promises) until `check` holds. */
async function until(check: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !check(); i++) await new Promise((r) => setTimeout(r, 0));
  expect(check()).toBe(true);
}

describe('App and updates', () => {
  it('updates one app, follows it until it is done and says what came of it', async () => {
    const running: UpdateInfo[] = [
      { id: 'tasks', native: 'workstation', web: 'ready', job: job({ state: 'running', step: 'pulling' }) },
    ];
    const done: UpdateInfo[] = [
      {
        id: 'tasks',
        native: 'workstation',
        web: 'ready',
        job: job({ reload: true, parts: [{ part: 'web', outcome: 'ok', result: 'updated' }] }),
      },
    ];
    const { hub, posted, token } = updatingHub({ after: [running, running, done] });
    const app = new App(hub, store(token), instant);
    await app.start(null);
    expect(app.anyUpdatable).toBe(true);
    expect(app.updating).toBe(false);
    await app.update(app.apps[0] as HubApp);
    expect(app.updating).toBe(true);
    await until(() => app.toast?.text === 'Tasks: Updated: reload its open page.');
    expect(posted).toEqual(['/api/hub/apps/tasks/update']);
    expect(app.updates['tasks']?.job?.state).toBe('done');
    expect(app.updating).toBe(false);
  });

  it('says what came of an update that was over before the page looked again', async () => {
    // A job with no step (an app that must be closed first) is done by the first read after the request.
    const over: UpdateInfo[] = [
      {
        id: 'tasks',
        native: 'ready',
        web: null,
        job: job({ state: 'failed', parts: [{ part: 'native', outcome: 'failed', result: 'close_first' }] }),
      },
    ];
    const { hub, token } = updatingHub({ after: [over] });
    const app = new App(hub, store(token), instant);
    await app.start(null);
    await app.update(app.apps[0] as HubApp);
    await until(() => app.toast?.kind === 'error');
    expect(app.toast?.text).toBe('Tasks: Update failed: it is running: close it first.');
  });

  it('updates all apps and says which one failed', async () => {
    const both = (a: UpdateJob, b: UpdateJob): UpdateInfo[] => [
      { id: 'tasks', native: 'workstation', web: 'ready', job: a },
      { id: 'rules', native: 'ready', web: null, job: b },
    ];
    const dirty = job({ state: 'failed', parts: [{ part: 'web', outcome: 'failed', result: 'dirty' }] });
    const { hub, posted, token } = updatingHub({
      after: [
        both(job({ state: 'running' }), job({ state: 'queued' })),
        both(dirty, job({ state: 'running' })),
        both(dirty, job({ parts: [{ part: 'native', outcome: 'ok', result: 'installed' }] })),
      ],
    });
    const app = new App(hub, store(token), instant);
    await app.start(null);
    await app.updateAll();
    await until(() => app.toast?.kind === 'error');
    expect(posted).toEqual(['/api/hub/update-all']);
    expect(app.toast).toEqual({ kind: 'error', text: '1 of 2 apps updated; failed: Tasks.' });
  });

  it('says why an app cannot be updated when the hub refuses it', async () => {
    const { hub, token } = updatingHub({ after: [[]], refuse: true });
    const app = new App(hub, store(token), instant);
    await app.start(null);
    await app.update({ id: 'rules', name: 'Rules' } as HubApp);
    expect(app.toast?.kind).toBe('error');
    expect(app.toast?.text).toContain('workstation');
  });

  it('says so when update all finds nothing to update', async () => {
    const { hub, token } = updatingHub({ after: [[]], queued: 0 });
    const app = new App(hub, store(token), instant);
    await app.start(null);
    await app.updateAll();
    expect(app.toast).toEqual({ kind: 'ok', text: 'Nothing can be updated here.' });
  });

  it('runs nothing in the web hub', async () => {
    const { hub, posted, token } = updatingHub({ after: [[]] });
    const presence = {
      mark: async () => undefined,
      canChooseFolder: false,
      chooseFolder: async () => ({ kind: 'cancelled' as const }),
    };
    const app = new App(hub, store(token), instant, presence);
    await app.start(null);
    expect(app.canUpdate).toBe(false);
    await app.update({ id: 'tasks', name: 'Tasks' } as HubApp);
    await app.updateAll();
    expect(posted).toEqual([]);
    expect(app.updates).toEqual({});
  });
});
