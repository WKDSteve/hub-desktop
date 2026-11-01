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
  install: { hint: 'release' | 'clone' | 'planned' | 'build_yourself'; page: string | null; file: string | null };
  repo: { owner: string; name: string; private: boolean };
}

export interface Apps {
  apps: HubApp[];
}

export class Hub extends HubClient {
  constructor(fetchFn: typeof fetch, base = '') {
    super(fetchFn, '/api/hub', base);
  }

  apps(): Promise<Apps> {
    return this.json<Apps>(this.route('apps'));
  }
}
