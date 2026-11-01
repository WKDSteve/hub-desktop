// What the page knows: whether it has a session, what the hub lists, which starts are on their way, and the
// one-line message of the last click.

import type { ToastMessage } from 'appkit-ui/toast';
import { ApiError, type Apps, type Hello, type HubApp, type HubApi, type LaunchAsk, type Mode } from './api';
import { type MonitorReading, NOT_CONFIGURED } from './monitor';

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
}

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

  #asked = new Map<string, number>();
  #cancelToast: (() => void) | null = null;

  constructor(
    private readonly hub: HubApi,
    private readonly session: TokenStore,
    private readonly clock: Clock = realClock,
    /** What only the web hub can do: keep the word of the viewer that an app is not on this machine. */
    readonly presence: Presence | null = null,
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
      this.phase = 'ready';
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
    if (withMonitor) await this.#readMonitor();
  }

  /** The monitor is a side show: a hub that does not answer for it leaves what is shown. */
  async #readMonitor(): Promise<void> {
    try {
      this.monitor = await this.hub.monitor();
    } catch {
      // A refused session already moved the phase (`onUnauthorized`); anything else keeps the rows.
    }
  }

  /** The web hub has no machine to look at again. */
  get canRescan(): boolean {
    return this.presence === null;
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
