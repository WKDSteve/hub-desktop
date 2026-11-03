// The hub's routes, as typed calls. The transport (the bearer token, the refusal, the pairing code, hello) is the
// kit's; this file is the routes of the hub.

import { ApiError, HubClient, type Hello } from 'appkit-ui/client';
import type { CasesReading, MonitorReading } from './monitor';

export { ApiError, type Hello };

export type Mode = 'native' | 'web';
/** `port_busy`: something else answers on the port of the app. No state at all: the app has no way to be looked at. */
export type Run = 'running' | 'stopped' | 'port_busy';

/** One app of the catalog as `GET /api/hub/apps` tells it. No path of the machine is in it. */
export interface HubApp {
  id: string;
  name: string;
  summary: string;
  status: 'available' | 'planned' | 'build_only';
  /** The native app or its web variant is on this machine: the tile belongs to "Installed". */
  installed: boolean;
  native: boolean;
  web: boolean;
  /** What a click opens: the default of the app, or the other mode when only that is there. */
  default_mode: Mode;
  run: Run | null;
  /** A short code (`no_vault`) of why the app cannot be used yet. */
  setup: string | null;
  /** The app runs and is one the hub can ask to stop. */
  stoppable: boolean;
  /** The native app starts with the login (an app of the kit on Windows); `null` or missing: not known. */
  autostart?: boolean | null;
  /** The build the running app reports (a short commit id); `null` or missing: not known. */
  build?: string | null;
  install: {
    hint: 'release' | 'clone' | 'planned' | 'build_yourself' | 'native_only';
    page: string | null;
    file: string | null;
  };
  repo: { owner: string; name: string; private: boolean };
  /** Only the web hub: the relative link to the page of the web variant, and whether the viewer said it is not there. */
  href?: string | null;
  missing?: boolean;
}

export interface Apps {
  apps: HubApp[];
}

/** What a start came to: a program was started, a start is on its way, a page was opened, or the app needs setup. */
export type LaunchResult = 'started' | 'starting' | 'opened' | 'setup_required';

/** What a stop came to: the app was asked, it does not run, or another start or stop of it is on its way. */
export type StopResult = 'stopping' | 'not_running' | 'starting';

/** What a click asks for; `setup` is the first-start questions of an app. */
export type LaunchAsk = Mode | 'setup';

/** What came of one part of an update: `result` is `updated`, `up_to_date`, `installed`, or the code of why not. */
export interface UpdatePart {
  part: 'native' | 'web';
  outcome: 'ok' | 'skipped' | 'failed';
  result: string;
}

/** The last update of an app as `GET /api/hub/updates` tells it. */
export interface UpdateJob {
  state: 'queued' | 'running' | 'done' | 'failed' | 'skipped';
  /** What it does now (`downloading`, `pulling`, ...). */
  step: string | null;
  parts: UpdatePart[];
  /** The last lines the steps printed (text). */
  lines: string[];
  /** The page of the web variant changed: an open one should be reloaded. */
  reload: boolean;
}

/** What can be updated of an app on this machine: `ready`, the code of why not, or `null` (that part is not installed). */
export interface UpdateInfo {
  id: string;
  native: string | null;
  web: string | null;
  job: UpdateJob | null;
}

/** What a request for an update came to: queued, or one of the app is queued or running already. */
export type UpdateResult = 'queued' | 'already';

/** What the page asks of its hub: the real one (over HTTP) or the one of `hub.html` (in the page). */
export interface HubApi {
  onUnauthorized: () => void;
  setToken(token: string | null): void;
  exchange(code: string): Promise<string>;
  hello(): Promise<Hello>;
  apps(): Promise<Apps>;
  launch(id: string, ask?: LaunchAsk): Promise<LaunchResult>;
  setMode(id: string, mode: Mode | null): Promise<void>;
  stop(id: string): Promise<StopResult>;
  rescan(): Promise<void>;
  /** The light of each repository of the testing export (the web hub has none). */
  monitor(): Promise<MonitorReading>;
  /** The red cases of the last run of one repository of the export. */
  monitorCases(key: string): Promise<CasesReading>;
  /** What can be updated of each app here, and its last update (the web hub has none). */
  updates(): Promise<{ apps: UpdateInfo[] }>;
  /** Queue an update of one app; what runs is the hub's to decide. */
  update(id: string): Promise<UpdateResult>;
  /** Queue an update of every app that has something to update; the number queued. */
  updateAll(): Promise<number>;
}

export class Hub extends HubClient implements HubApi {
  constructor(fetchFn: typeof fetch, base = '') {
    super(fetchFn, '/api/hub', base);
  }

  apps(): Promise<Apps> {
    return this.json<Apps>(this.route('apps'));
  }

  /** Start an app. The answer comes at once; the app is not waited for. */
  async launch(id: string, ask?: LaunchAsk): Promise<LaunchResult> {
    const body = ask ? { mode: ask } : {};
    const doc = await this.postJson<{ result?: unknown }>(this.route(`apps/${encodeURIComponent(id)}/launch`), body);
    const result = doc.result;
    if (result === 'started' || result === 'starting' || result === 'opened' || result === 'setup_required') return result;
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  /** Ask an app that runs to stop (its own program does it). The answer comes at once. */
  async stop(id: string): Promise<StopResult> {
    const doc = await this.postJson<{ result?: unknown }>(this.route(`apps/${encodeURIComponent(id)}/stop`), {});
    const result = doc.result;
    if (result === 'stopping' || result === 'not_running' || result === 'starting') return result;
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  /** The mode a click opens for an app, kept by the hub; `null` goes back to the default of the app. */
  setMode(id: string, mode: Mode | null): Promise<void> {
    return this.post(this.route(`apps/${encodeURIComponent(id)}/mode`), { mode });
  }

  /** What the hub reads of the export folder of testing.nvim: nothing configured, a problem, or the rows. */
  async monitor(): Promise<MonitorReading> {
    const doc = await this.json<MonitorReading>(this.route('monitor'));
    if (doc.state === 'not_configured' || doc.state === 'problem') return doc;
    if (doc.state === 'ready' && Array.isArray(doc.repos)) return doc;
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  /** The red cases of one repository of the export, by file and line. */
  async monitorCases(key: string): Promise<CasesReading> {
    const doc = await this.json<CasesReading>(`${this.route('monitor/cases')}?key=${encodeURIComponent(key)}`);
    if (doc.state === 'not_configured' || doc.state === 'problem') return doc;
    if (doc.state === 'ready' && Array.isArray(doc.cases)) return doc;
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  async updates(): Promise<{ apps: UpdateInfo[] }> {
    const doc = await this.json<{ apps?: unknown }>(this.route('updates'));
    if (Array.isArray(doc.apps)) return { apps: doc.apps as UpdateInfo[] };
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  /** Queue an update of an app. The answer comes at once; `updates()` tells how it goes. */
  async update(id: string): Promise<UpdateResult> {
    const doc = await this.postJson<{ result?: unknown }>(this.route(`apps/${encodeURIComponent(id)}/update`), {});
    if (doc.result === 'queued' || doc.result === 'already') return doc.result;
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  async updateAll(): Promise<number> {
    const doc = await this.postJson<{ queued?: unknown }>(this.route('update-all'), {});
    if (typeof doc.queued === 'number') return doc.queued;
    throw new ApiError(502, 'bad_reply', 'unexpected answer');
  }

  /** Forget what the hub found: the next `apps()` looks at the machine again. */
  rescan(): Promise<void> {
    return this.post(this.route('rescan'), {});
  }
}
