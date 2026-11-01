// The hub's routes, as typed calls. The transport (the bearer token, the refusal, the pairing code, hello) is the
// kit's; this file is the routes of the hub.

import { ApiError, HubClient, type Hello } from 'appkit-ui/client';

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

/** What a click asks for; `setup` is the first-start questions of an app. */
export type LaunchAsk = Mode | 'setup';

/** What the page asks of its hub: the real one (over HTTP) or the one of `hub.html` (in the page). */
export interface HubApi {
  onUnauthorized: () => void;
  setToken(token: string | null): void;
  exchange(code: string): Promise<string>;
  hello(): Promise<Hello>;
  apps(): Promise<Apps>;
  launch(id: string, ask?: LaunchAsk): Promise<LaunchResult>;
  setMode(id: string, mode: Mode | null): Promise<void>;
  rescan(): Promise<void>;
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

  /** The mode a click opens for an app, kept by the hub; `null` goes back to the default of the app. */
  setMode(id: string, mode: Mode | null): Promise<void> {
    return this.post(this.route(`apps/${encodeURIComponent(id)}/mode`), { mode });
  }

  /** Forget what the hub found: the next `apps()` looks at the machine again. */
  rescan(): Promise<void> {
    return this.post(this.route('rescan'), {});
  }
}
