// What the page knows: whether it has a session, and what the hub told it.

import { ApiError, type Apps, type Hub, type Hello } from './api';

/** `starting`: the code is traded. `unpaired`: no session and no code (open the link of `hub-desktop open`). */
export type Phase = 'starting' | 'ready' | 'unpaired' | 'error';

/** The place the session survives a reload of the tab: the token only, never in a store that outlives the tab. */
export interface TokenStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

const TOKEN_KEY = 'hub-desktop.session';

/** Run a storage call; a profile that blocks the storage keeps the page working, only without the reload. */
function safely<T>(call: () => T, fallback: T): T {
  try {
    return call();
  } catch {
    return fallback;
  }
}

export class App {
  phase = $state<Phase>('starting');
  version = $state('');
  apps = $state<Apps['apps']>([]);
  /** What went wrong, in words, when `phase` is `error`. */
  problem = $state<string | null>(null);

  constructor(
    private readonly hub: Hub,
    private readonly session: TokenStore,
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
}
