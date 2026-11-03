// What the page knows: whether it has a session, what the hub lists, which starts are on their way, and the
// one-line message of the last click.

import type { ToastMessage } from 'appkit-ui/toast';
import {
  ApiError,
  type Apps,
  type Hello,
  type HubApp,
  type HubApi,
  type LaunchAsk,
  type Mode,
  type UpdateInfo,
  type UpdateJob,
} from './api';
import { type CasesView, type MonitorReading, NOT_CONFIGURED } from './monitor';
import { askable, differs, type UpdateChecker } from './updates';
import { active, roundText, updatable, whyNot } from './updating';

/** `starting`: the code is traded. `unpaired`: no session and no code (open the link of `hub-desktop open`). */
export type Phase = 'starting' | 'ready' | 'unpaired' | 'error';

/** The place the session survives a reload of the tab: the token only, never in a store that outlives the tab. */
export interface TokenStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** Time, as the page gets it: replaced in the tests. */
export interface Clock {
  now(): number;
  sleep(ms: number): Promise<void>;
  /** Run `fn` after `ms`; the returned function cancels it. */
  later(ms: number, fn: () => void): () => void;
}

export const realClock: Clock = {
  now: () => Date.now(),
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  later: (ms, fn) => {
    const timer = setTimeout(fn, ms);
    return () => clearTimeout(timer);
  },
};

/** The viewer's word about an app of the web hub (the page cannot look at the machine). */
export interface Presence {
  mark(id: string, missing: boolean): Promise<void>;
  /** The browser has a folder dialog: the person can pick the folder of the clones once. */
  readonly canChooseFolder: boolean;
  chooseFolder(): Promise<FolderOutcome>;
}

/** What picking the folder of the clones came to. */
export type FolderOutcome =
  | { kind: 'scanned'; present: number; missing: number }
  | { kind: 'cancelled' }
  | { kind: 'unsupported' }
  | { kind: 'failed'; message: string };

const TOKEN_KEY = 'hub-desktop.session';

/** How long a start may take before the tile stops saying so, and how long a program that cannot be looked at. */
const START_LIMIT_MS = 15_000;
const BLIND_START_MS = 3_000;
/** Clicks on a tile within this time of the last one are one click (a double click). */
const SAME_CLICK_MS = 1_500;
const TOAST_MS = 6_000;

/** Run a storage call; a profile that blocks the storage keeps the page working, only without the reload. */
function safely<T>(call: () => T, fallback: T): T {
  try {
    return call();
  } catch {
    return fallback;
  }
}

/** What a refusal of a start says, in words for the person. */
function refusal(name: string, e: unknown): string {
  if (!(e instanceof ApiError)) return `${name} could not be started.`;
  switch (e.code) {
    case 'not_installed':
      return `${name} is not installed.`;
    case 'port_busy':
      return `The port of ${name} is busy: something else answers there.`;
    case 'not_available':
      return `${name} has no such mode.`;
    case 'web_unavailable':
      return 'The web variants are not served: the port of the listener is taken.';
    default:
      return `${name} could not be started.`;
  }
}

export class App {
  phase = $state<Phase>('starting');
  version = $state('');
  apps = $state<Apps['apps']>([]);
  /** What went wrong, in words, when `phase` is `error`. */
  problem = $state<string | null>(null);
  filter = $state('');
  /** The apps with a start on its way, by the time it began. */
  starting = $state<Record<string, number>>({});
  /** The code of the last refused start of an app, until a look finds it better. */
  failed = $state<Record<string, string>>({});
  toast = $state<ToastMessage | null>(null);
  /** The app whose setup the page is asking about. */
  setupFor = $state<HubApp | null>(null);
  /** The testing export the hub reads, as it showed last. */
  monitor = $state<MonitorReading>(NOT_CONFIGURED);
  /** The repository of the monitor whose red cases are shown (one at a time), and what was read for it. */
  openCases = $state<string | null>(null);
  cases = $state<CasesView>({ state: 'loading' });
  /** The build GitHub named at the last check, by app id; empty until a person pressed the button. */
  available = $state<Record<string, string | null>>({});
  /** A check for updates is under way. */
  checking = $state(false);
  /** What the hub can update of each app, and its last update, by app id. */
  updates = $state<Record<string, UpdateInfo>>({});

  #asked = new Map<string, number>();
  #watchingUpdates = false;
  /** The apps whose updates the running watch reports on when it ends. */
  #round = new Set<string>();
  #cancelToast: (() => void) | null = null;

  constructor(
    private readonly hub: HubApi,
    private readonly session: TokenStore,
    private readonly clock: Clock = realClock,
    /** What only the web hub can do: keep the word of the viewer that an app is not on this machine. */
    readonly presence: Presence | null = null,
    /** What only the page of the real hub can do: ask GitHub whether an app has a newer build. */
    private readonly checker: UpdateChecker | null = null,
  ) {
    // The session that was refused is gone: the page asks for a new link instead of showing stale data.
    hub.onUnauthorized = () => {
      safely(() => this.session.removeItem(TOKEN_KEY), undefined);
      this.hub.setToken(null);
      this.phase = 'unpaired';
    };
  }

  /** Trade the code of the link for a session (or take the one of this tab), then read what the hub shows. */
  async start(code: string | null): Promise<void> {
    this.phase = 'starting';
    this.problem = null;
    try {
      if (code) {
        const token = await this.hub.exchange(code);
        this.hub.setToken(token);
        safely(() => this.session.setItem(TOKEN_KEY, token), undefined);
      } else {
        const kept = safely(() => this.session.getItem(TOKEN_KEY), null);
        if (!kept) {
          this.phase = 'unpaired';
          return;
        }
        this.hub.setToken(kept);
      }
      const hello: Hello = await this.hub.hello();
      this.version = hello.version;
      this.apps = (await this.hub.apps()).apps;
      await this.#readMonitor();
      await this.#readUpdates();
      this.phase = 'ready';
      // Updates that run since before this page was opened are followed too.
      if (Object.values(this.updates).some((u) => active(u.job))) void this.watchUpdates();
    } catch (e) {
      // A refused session already moved the phase to `unpaired`; a refused code is the same thing for the person.
      if (e instanceof ApiError && (e.status === 401 || e.status === 400)) {
        this.hub.setToken(null);
        this.phase = 'unpaired';
        return;
      }
      this.problem = e instanceof Error ? e.message : String(e);
      this.phase = 'error';
    }
  }

  /**
   * Read the list again (the page got the focus back, a start is on its way). A failure keeps what is shown.
   * The watch of a start or stop reads the list every second and leaves the monitor (files of a folder) alone.
   */
  async reload(withMonitor = true): Promise<void> {
    if (this.phase !== 'ready') return;
    try {
      this.apps = (await this.hub.apps()).apps;
      // A look that finds the app well again takes the mark of a refused start away.
      const failed = { ...this.failed };
      for (const a of this.apps) if (a.run === 'running') delete failed[a.id];
      this.failed = failed;
    } catch {
      // The hub may be busy for a moment: the next look tries again.
    }
    if (withMonitor) {
      await this.#readMonitor();
      await this.#readUpdates();
    }
  }

  /** What can be updated, read from the hub; a failure keeps what is shown. The web hub has nothing to read. */
  async #readUpdates(): Promise<void> {
    if (!this.canUpdate) return;
    try {
      const list = await this.hub.updates();
      this.updates = Object.fromEntries(list.apps.map((u) => [u.id, u]));
    } catch {
      // A refused session already moved the phase; anything else keeps what is shown.
    }
  }

  /** Only the real hub runs updates (the web hub shows the lines to type instead). */
  get canUpdate(): boolean {
    return this.presence === null;
  }

  /** Something of some app can be updated here. */
  get anyUpdatable(): boolean {
    return Object.values(this.updates).some(updatable);
  }

  /** An update is queued or running. */
  get updating(): boolean {
    return Object.values(this.updates).some((u) => active(u.job));
  }

  /** Update one app: its program from the release, its web variant from the clone, as the hub decides. */
  async update(app: HubApp): Promise<void> {
    if (!this.canUpdate || this.phase !== 'ready') return;
    try {
      const result = await this.hub.update(app.id);
      this.say('ok', result === 'already' ? `${app.name} is being updated already.` : `Updating ${app.name}…`);
      // In the round from the start: a job that is over before the next read is still reported.
      this.#round.add(app.id);
      await this.#readUpdates();
      void this.watchUpdates();
    } catch (e) {
      const why = e instanceof ApiError && e.code === 'not_updatable' ? whyNot(this.updates[app.id]) : 'The hub did not answer.';
      this.say('error', `${app.name}: ${why}`);
    }
  }

  /** Update every app that has something to update here, one after the other. */
  async updateAll(): Promise<void> {
    if (!this.canUpdate || this.phase !== 'ready') return;
    try {
      const queued = await this.hub.updateAll();
      if (queued === 0) {
        this.say('ok', this.updating ? 'The updates are on their way.' : 'Nothing can be updated here.');
        return;
      }
      this.say('ok', queued === 1 ? 'Updating 1 app…' : `Updating ${queued} apps…`);
      // The hub queued what it can update here: the same apps the page was told can be updated.
      for (const u of Object.values(this.updates)) if (updatable(u)) this.#round.add(u.id);
      await this.#readUpdates();
      void this.watchUpdates();
    } catch {
      this.say('error', 'The hub did not answer.');
    }
  }

  /** Read the updates every second while one is queued or running; then the list, and a message of the round. */
  async watchUpdates(): Promise<void> {
    if (this.#watchingUpdates) return;
    this.#watchingUpdates = true;
    const round = this.#round;
    for (const u of Object.values(this.updates)) if (active(u.job)) round.add(u.id);
    try {
      while (this.phase === 'ready') {
        await this.clock.sleep(1000);
        await this.#readUpdates();
        const now = Object.values(this.updates).filter((u) => active(u.job));
        for (const u of now) round.add(u.id);
        if (now.length === 0) break;
      }
    } finally {
      this.#watchingUpdates = false;
      this.#round = new Set();
    }
    if (this.phase !== 'ready' || round.size === 0) return;
    await this.reload(false);
    const jobs = [...round].flatMap((id) => {
      const job: UpdateJob | null | undefined = this.updates[id]?.job;
      const name = this.apps.find((a) => a.id === id)?.name ?? id;
      return job ? [{ name, job }] : [];
    });
    if (jobs.length > 0) this.say(jobs.some((j) => j.job.state === 'failed') ? 'error' : 'ok', roundText(jobs));
  }

  /** The monitor is a side show: a hub that does not answer for it leaves what is shown. */
  async #readMonitor(): Promise<void> {
    try {
      this.monitor = await this.hub.monitor();
    } catch {
      // A refused session already moved the phase (`onUnauthorized`); anything else keeps the rows.
    }
  }

  /** Show the red cases of a repository of the monitor, read now; the same one again hides them. */
  async toggleCases(key: string): Promise<void> {
    if (this.openCases === key) {
      this.openCases = null;
      return;
    }
    this.openCases = key;
    this.cases = { state: 'loading' };
    let read: CasesView;
    try {
      read = await this.hub.monitorCases(key);
    } catch {
      read = { state: 'failed' };
    }
    // Another row may have been opened while this one was read.
    if (this.openCases === key) this.cases = read;
  }

  /** The web hub has no machine to look at again. */
  get canRescan(): boolean {
    return this.presence === null;
  }

  /** Only the page of the real hub may reach GitHub (the web hub has no such source in its policy). */
  get canCheckUpdates(): boolean {
    return this.checker !== null && this.presence === null;
  }

  /** Whether the badge "Update available" belongs on this app: judged against the build it runs now, so an app that was updated since the check loses it. */
  hasUpdate(id: string): boolean {
    const app = this.apps.find((a) => a.id === id);
    return app !== undefined && askable(app) && differs(app.build, this.available[id] ?? null);
  }

  /** Ask GitHub about the apps that may be asked about (a person pressed the button), and say what came of it. */
  async checkUpdates(): Promise<void> {
    if (!this.checker || !this.canCheckUpdates || this.checking || this.phase !== 'ready') return;
    this.checking = true;
    try {
      const askedAbout = this.apps.filter(askable);
      if (askedAbout.length === 0) {
        this.say('ok', 'No app can be checked: only running apps of public repositories report their build.');
        return;
      }
      const round = await this.checker.check(this.apps);
      this.available = { ...this.available, ...round.available };
      const newer = askedAbout.filter((a) => round.states[a.id] === 'available').length;
      const unknown = askedAbout.filter((a) => round.states[a.id] === 'unknown').length;
      if (round.failed) this.say('error', 'GitHub did not answer. Try again later.');
      else if (newer > 0) this.say('ok', newer === 1 ? '1 update is available.' : `${newer} updates are available.`);
      else if (unknown > 0) {
        const apps = unknown === 1 ? '1 app' : `${unknown} apps`;
        this.say('ok', `GitHub names no build for ${apps}; the others are up to date.`);
      } else this.say('ok', 'Everything that could be checked is up to date.');
    } finally {
      this.checking = false;
    }
  }

  /** Only the web hub has a folder of clones to look at, and only in a browser with a folder dialog. */
  get canChooseFolder(): boolean {
    return this.presence?.canChooseFolder === true;
  }

  /** The person picks the folder of the `*-web` clones; the marks follow what is there, and the list is read again. */
  async chooseFolder(): Promise<void> {
    if (!this.presence || !this.canChooseFolder || this.phase !== 'ready') return;
    const outcome = await this.presence.chooseFolder();
    if (outcome.kind === 'cancelled') return;
    if (outcome.kind !== 'scanned') {
      this.say('error', outcome.kind === 'failed' ? outcome.message : 'This browser cannot choose a folder.');
      return;
    }
    try {
      this.apps = (await this.hub.apps()).apps;
    } catch {
      // The marks are kept; the list shows them at the next read.
    }
    const found = outcome.present === 1 ? '1 web app found' : `${outcome.present} web apps found`;
    this.say('ok', outcome.missing === 0 ? `${found}.` : `${found}, ${outcome.missing} not in the folder.`);
  }

  /** Say an app is (not) on this machine; the list is read again. */
  async mark(app: HubApp, missing: boolean): Promise<void> {
    if (!this.presence) return;
    try {
      await this.presence.mark(app.id, missing);
      this.apps = (await this.hub.apps()).apps;
      this.say('ok', missing ? `${app.name} is marked as not present.` : `${app.name} is marked as present.`);
    } catch {
      this.say('error', 'The mark could not be kept.');
    }
  }

  /** Ask the hub to look at the machine again, then read the list. */
  async rescan(): Promise<void> {
    if (this.phase !== 'ready' || !this.canRescan) return;
    try {
      await this.hub.rescan();
      this.apps = (await this.hub.apps()).apps;
      this.say('ok', 'Looked at the machine again.');
    } catch {
      this.say('error', 'The hub did not answer.');
    }
  }

  say(kind: ToastMessage['kind'], text: string): void {
    this.#cancelToast?.();
    this.toast = { kind, text };
    this.#cancelToast = this.clock.later(TOAST_MS, () => (this.toast = null));
  }

  dismissToast(): void {
    this.#cancelToast?.();
    this.toast = null;
  }

  /** Whether a click on this app would be one too many: its start is on its way, or the same click came a moment ago. */
  busy(id: string, ask?: LaunchAsk): boolean {
    if (this.starting[id] !== undefined) return true;
    const before = this.#asked.get(`${id}:${ask ?? ''}`);
    return before !== undefined && this.clock.now() - before < SAME_CLICK_MS;
  }

  /** Open an app: its default mode, or the one that is asked for. One click is one request. */
  async launch(app: HubApp, ask?: LaunchAsk): Promise<void> {
    if (this.busy(app.id, ask)) return;
    this.#asked.set(`${app.id}:${ask ?? ''}`, this.clock.now());
    const failed = { ...this.failed };
    delete failed[app.id];
    this.failed = failed;
    try {
      const result = await this.hub.launch(app.id, ask);
      switch (result) {
        case 'started':
          this.say('ok', `Starting ${app.name}…`);
          void this.watch(app.id);
          break;
        case 'starting':
          this.say('ok', `${app.name} is already starting.`);
          break;
        case 'opened':
          this.say('ok', `Opened ${app.name}.`);
          break;
        case 'setup_required':
          this.setupFor = app;
          break;
      }
    } catch (e) {
      this.failed = { ...this.failed, [app.id]: e instanceof ApiError ? e.code : 'start_failed' };
      this.say('error', refusal(app.name, e));
    }
  }

  /** Ask an app that runs to stop; the list is read until it shows it stopped (a message says the app was asked). */
  async stop(app: HubApp): Promise<void> {
    try {
      const result = await this.hub.stop(app.id);
      if (result === 'not_running') this.say('ok', `${app.name} is not running.`);
      else if (result === 'starting') this.say('ok', `${app.name} is busy: a start or stop is on its way.`);
      else {
        this.say('ok', `Stopping ${app.name}…`);
        void this.watchStop(app.id);
      }
    } catch (e) {
      this.say('error', e instanceof ApiError && e.code === 'not_available' ? `${app.name} cannot be stopped from here.` : `${app.name} could not be asked to stop.`);
    }
  }

  /** Read the list until the app no longer runs, or the time is up. */
  async watchStop(id: string): Promise<void> {
    const began = this.clock.now();
    while (this.clock.now() - began < START_LIMIT_MS) {
      await this.clock.sleep(1000);
      await this.reload(false);
      if (this.apps.find((a) => a.id === id)?.run !== 'running') break;
    }
  }

  /** Keep a mode as the one a click opens. */
  async makeDefault(app: HubApp, mode: Mode): Promise<void> {
    try {
      await this.hub.setMode(app.id, mode);
      await this.reload();
      this.say('ok', `${app.name} now opens ${mode === 'web' ? 'as a web app' : 'natively'}.`);
    } catch {
      this.say('error', 'The choice could not be kept.');
    }
  }

  /** The tile says "starting" until the app shows up as running, or the time is up. */
  async watch(id: string): Promise<void> {
    const began = this.clock.now();
    this.starting = { ...this.starting, [id]: began };
    while (this.starting[id] === began) {
      await this.clock.sleep(1000);
      await this.reload(false);
      const app = this.apps.find((a) => a.id === id);
      const waited = this.clock.now() - began;
      if (app?.run === 'running' || (app?.run == null && waited >= BLIND_START_MS) || waited >= START_LIMIT_MS) break;
    }
    if (this.starting[id] === began) {
      const next = { ...this.starting };
      delete next[id];
      this.starting = next;
    }
  }
}
