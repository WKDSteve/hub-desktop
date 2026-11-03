// The update badge: does GitHub name a newer build than the one the app runs? The page asks (the hub does not, concept
// 5.3): only when a person presses the button, at most once a day for each app, only for public repositories, without
// a token or a cookie. What comes back is read as foreign data: one short commit id out of the title of the rolling
// release, anything else is "not known".

import type { HubApp } from './api';

/** The one address besides the hub that the page may reach (the same as `with_connect_src` in the hub). */
export const RELEASES_ORIGIN = 'https://api.github.com';

/** An app is asked about at most this often. */
export const DAY_MS = 24 * 60 * 60 * 1000;

/** More than this is no answer of ours (a release with its assets is a few kilobytes). */
const MAX_REPLY_CHARS = 512 * 1024;
const MAX_TITLE_CHARS = 120;

/** `Rolling build (<sha7>)`: the title the release kit gives the rolling release (`release-kit/README.md`). */
const ROLLING_TITLE = /^Rolling build \(([0-9a-f]{7,40})\)$/;

/** What the check found out about one app. */
export type UpdateState = 'available' | 'current' | 'unknown';

/** The build a release says it is: the short commit id of its title, or `null` for any other form. */
export function buildOfRelease(doc: unknown): string | null {
  if (typeof doc !== 'object' || doc === null) return null;
  const name = (doc as { name?: unknown }).name;
  if (typeof name !== 'string' || name.length > MAX_TITLE_CHARS) return null;
  return ROLLING_TITLE.exec(name)?.[1] ?? null;
}

/**
 * Whether `available` is another build than `installed`. Both must be known; the ids are short commit ids that may
 * differ in length, so one that starts the other is the same commit.
 */
export function differs(installed: string | null | undefined, available: string | null): boolean {
  if (!installed || !available) return false;
  const a = installed.toLowerCase();
  return !(available.startsWith(a) || a.startsWith(available));
}

/** The state of an app from what is known of it. */
export function stateOf(installed: string | null | undefined, available: string | null): UpdateState {
  if (!installed || !available) return 'unknown';
  return differs(installed, available) ? 'available' : 'current';
}

/** Whether the app may be asked about: installed, public, with a build it reports, and a repository named plainly. */
export function askable(app: HubApp): boolean {
  return (
    app.installed &&
    !app.href &&
    !app.repo.private &&
    typeof app.build === 'string' &&
    app.build !== '' &&
    PLAIN.test(app.repo.owner) &&
    PLAIN.test(app.repo.name)
  );
}

const PLAIN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;

/** The release the rolling build is published as. */
export function releaseUrl(repo: { owner: string; name: string }): string {
  return `${RELEASES_ORIGIN}/repos/${repo.owner}/${repo.name}/releases/tags/latest`;
}

/** Whether the last question is old enough for a new one. A time in the future (a clock set back) is not young. */
export function due(last: number | null, now: number): boolean {
  return last === null || last > now || now - last >= DAY_MS;
}

/** The part of `localStorage` the checker uses; a profile that blocks it keeps the check working, only without the limit. */
export interface Memory {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** What was last learned about an app: when it was asked, and which build GitHub named. */
interface Kept {
  at: number;
  available: string | null;
}

const KEY = 'hub-desktop.update.';

function safely<T>(call: () => T, fallback: T): T {
  try {
    return call();
  } catch {
    return fallback;
  }
}

/** What a round of asking came to. */
export interface Round {
  states: Record<string, UpdateState>;
  /** GitHub did not answer in a way we can use (rate limit, offline, a form we do not know). */
  failed: boolean;
}

export class UpdateChecker {
  constructor(
    private readonly fetchFn: typeof fetch,
    private readonly memory: Memory | null,
    private readonly now: () => number = () => Date.now(),
  ) {}

  #read(id: string): Kept | null {
    const raw = safely(() => this.memory?.getItem(KEY + id) ?? null, null);
    if (raw === null) return null;
    const doc = safely<unknown>(() => JSON.parse(raw), null);
    if (typeof doc !== 'object' || doc === null) return null;
    const { at, available } = doc as { at?: unknown; available?: unknown };
    if (typeof at !== 'number' || !Number.isFinite(at)) return null;
    if (available !== null && (typeof available !== 'string' || !/^[0-9a-f]{7,40}$/.test(available))) return null;
    return { at, available };
  }

  #keep(id: string, kept: Kept): void {
    safely(() => this.memory?.setItem(KEY + id, JSON.stringify(kept)), undefined);
  }

  /** Ask GitHub about one repository. `undefined`: no usable answer. */
  async #ask(app: HubApp): Promise<string | null | undefined> {
    try {
      const res = await this.fetchFn(releaseUrl(app.repo), {
        credentials: 'omit',
        referrerPolicy: 'no-referrer',
        headers: { Accept: 'application/vnd.github+json' },
      });
      if (!res.ok) return undefined;
      const text = await res.text();
      if (text.length > MAX_REPLY_CHARS) return undefined;
      return buildOfRelease(JSON.parse(text));
    } catch {
      return undefined;
    }
  }

  /** Check the apps that may be asked about; an app asked less than a day ago is judged from what was kept. */
  async check(apps: readonly HubApp[]): Promise<Round> {
    const states: Record<string, UpdateState> = {};
    let failed = false;
    for (const app of apps.filter(askable)) {
      let kept = this.#read(app.id);
      if (kept === null || due(kept.at, this.now())) {
        // After one refusal (a rate limit holds for every repository) the other apps are not asked this round.
        const found = failed ? undefined : await this.#ask(app);
        if (found === undefined) {
          failed = true;
          states[app.id] = kept ? stateOf(app.build, kept.available) : 'unknown';
          continue;
        }
        kept = { at: this.now(), available: found };
        this.#keep(app.id, kept);
      }
      states[app.id] = stateOf(app.build, kept.available);
    }
    return { states, failed };
  }
}
