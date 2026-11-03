// The hub of `hub.html`: the same calls the page makes of the real hub, answered in the page. There is no machine to
// look at (the page may not even know the kind of computer), so nothing is detected: a web app is a link to its page
// next to this one, until the viewer says it is not there; a native app is listed as "native only".

import type { DirHandleLike, FileHandleLike } from 'appkit-web/folder';
import type { Apps, Hello, HubApi, HubApp, LaunchAsk, LaunchResult, Mode, StopResult } from '../lib/api';
import type { FolderOutcome, Presence } from '../lib/app.svelte';
import { type CasesReading, type MonitorReading, NOT_CONFIGURED } from '../lib/monitor';

/** One entry of `catalog.json` (the rendering of `catalog.rs`, see `webcatalog.rs`). */
export interface CatalogEntry {
  id: string;
  name: string;
  summary: string;
  status: 'available' | 'planned' | 'build_only';
  native: boolean;
  web: { dir: string; entry: string } | null;
  install: HubApp['install'];
  repo: HubApp['repo'];
}

/** What the viewer said, kept in the browser: the ids of the apps that are not on this machine. */
export interface MissingStore {
  /** `null`: the browser does not let the page keep anything (the page then remembers for as long as it is open). */
  load(): Promise<string[] | null>;
  save(ids: string[]): Promise<void>;
}

/** The page of a web variant, next to this one: `../<dir>/<entry>`. The names come from the catalog, never from outside. */
export function pageLink(web: { dir: string; entry: string }): string {
  return `../${web.dir}/${web.entry}`;
}

/** Where to get a web variant: its repository, which is public. */
function cloneLink(e: CatalogEntry, web: { dir: string }): string {
  return `https://github.com/${e.repo.owner}/${web.dir}`;
}

/** The app as the web hub shows it. */
export function view(e: CatalogEntry, missing: ReadonlySet<string>): HubApp {
  const base = {
    id: e.id,
    name: e.name,
    summary: e.summary,
    status: e.status,
    native: false,
    default_mode: 'web' as Mode,
    run: null,
    setup: null,
    stoppable: false,
    repo: e.repo,
  };
  if (e.web && e.status === 'available') {
    if (missing.has(e.id)) {
      return {
        ...base,
        installed: false,
        web: false,
        href: null,
        missing: true,
        install: { hint: 'clone', page: cloneLink(e, e.web), file: null },
      };
    }
    return { ...base, installed: true, web: true, href: pageLink(e.web), install: e.install };
  }
  // No web variant (yet): a program is "native only", the rest keeps what the catalog says (planned, build yourself).
  const install: HubApp['install'] =
    e.native && e.status === 'available' ? { hint: 'native_only', page: null, file: null } : e.install;
  return { ...base, installed: false, web: false, install };
}

/** What the browser's folder dialog gave (the part of `appkit-web/folder`'s `PickResult` the hub uses). */
export type FolderPick =
  | { kind: 'picked'; handle: DirHandleLike }
  | { kind: 'cancelled' }
  | { kind: 'blocked' | 'failed'; message: string };

/** Asks the person for a folder; `null`: this browser has no folder dialog. */
export type FolderPicker = (() => Promise<FolderPick>) | null;

/** At most this many entries of a folder are looked at: the parent of the clones holds a few dozen. */
const MAX_ENTRIES = 2000;

async function entriesOf(dir: DirHandleLike, kind: 'file' | 'directory'): Promise<Map<string, FileHandleLike | DirHandleLike>> {
  const out = new Map<string, FileHandleLike | DirHandleLike>();
  let seen = 0;
  for await (const entry of dir.values()) {
    if (++seen > MAX_ENTRIES) break;
    if (entry.kind === kind) out.set(entry.name, entry);
  }
  return out;
}

/**
 * The web variants whose page is in `parent`, the folder of the clones: `<dir>/<entry>` of the catalog is a file
 * there. Only names of the catalog are looked for; nothing of the folder is read but the names of its entries.
 */
export async function presentIn(parent: DirHandleLike, catalog: readonly CatalogEntry[]): Promise<Set<string>> {
  const dirs = await entriesOf(parent, 'directory');
  const present = new Set<string>();
  for (const e of catalog) {
    if (!e.web || e.status !== 'available') continue;
    const dir = dirs.get(e.web.dir);
    if (dir?.kind !== 'directory') continue;
    if ((await entriesOf(dir, 'file')).has(e.web.entry)) present.add(e.id);
  }
  return present;
}

export class WebHub implements HubApi, Presence {
  onUnauthorized: () => void = () => {};
  /** What the page itself last said: what is shown when the browser keeps nothing. */
  #local = new Set<string>();

  constructor(
    private readonly catalog: readonly CatalogEntry[],
    private readonly store: MissingStore,
    private readonly version: string,
    private readonly picker: FolderPicker = null,
  ) {}

  get canChooseFolder(): boolean {
    return this.picker !== null;
  }

  /**
   * The person picks the folder the `*-web` clones are in; every web variant of the catalog is then marked present
   * or not present by what is there (the marks of the menu, in the same store). The folder itself is not kept.
   */
  async chooseFolder(): Promise<FolderOutcome> {
    if (!this.picker) return { kind: 'unsupported' };
    const picked = await this.picker();
    if (picked.kind === 'cancelled') return picked;
    if (picked.kind !== 'picked') return { kind: 'failed', message: picked.message };
    let present: Set<string>;
    try {
      present = await presentIn(picked.handle, this.catalog);
    } catch {
      return { kind: 'failed', message: 'The folder could not be read.' };
    }
    const webApps = this.catalog.filter((e) => e.web && e.status === 'available');
    const next = new Set(await this.#ids());
    for (const e of webApps) {
      if (present.has(e.id)) next.delete(e.id);
      else next.add(e.id);
    }
    this.#local = next;
    try {
      await this.store.save([...next]);
    } catch {
      // The browser keeps nothing: the marks hold while the page is open.
    }
    return { kind: 'scanned', present: present.size, missing: webApps.length - present.size };
  }

  setToken(): void {}

  /** There is no server to pair with: any code is taken. */
  async exchange(): Promise<string> {
    return 'web';
  }

  async hello(): Promise<Hello> {
    return { instance: 'web', version: this.version, engine: { ready: true, problem: null, capabilities: null } };
  }

  /** Read every time: another tab of the page may have changed the marks, and a write must start from them. */
  async #ids(): Promise<Set<string>> {
    const kept = await this.store.load();
    if (kept === null) return this.#local;
    const known = new Set(this.catalog.map((e) => e.id));
    // What the browser kept may be old or foreign: only ids of the catalog count.
    this.#local = new Set(kept.filter((id) => known.has(id)));
    return this.#local;
  }

  async apps(): Promise<Apps> {
    const missing = await this.#ids();
    return { apps: this.catalog.map((e) => view(e, missing)) };
  }

  /** A link does the opening; a call for one that cannot be opened here is refused. */
  async launch(_id: string, _ask?: LaunchAsk): Promise<LaunchResult> {
    throw new Error('the web hub opens pages by link');
  }

  async setMode(): Promise<void> {}

  async stop(): Promise<StopResult> {
    throw new Error('the web hub starts and stops nothing');
  }

  async rescan(): Promise<void> {}

  /** A page of a file has no export folder to read. */
  async monitor(): Promise<MonitorReading> {
    return NOT_CONFIGURED;
  }

  async monitorCases(): Promise<CasesReading> {
    return { state: 'not_configured' };
  }

  async mark(id: string, missing: boolean): Promise<void> {
    const ids = await this.#ids();
    // Only an app with a web variant can be missing; the menu offers nothing else.
    if (!this.catalog.some((e) => e.id === id && e.web)) return;
    const next = new Set(ids);
    if (missing) next.add(id);
    else next.delete(id);
    this.#local = next;
    try {
      await this.store.save([...next]);
    } catch {
      // The browser keeps nothing: the mark holds while the page is open.
    }
  }
}

const DB = 'hub-web';
const STORE = 'kv';
const KEY = 'missing';

/** The viewer's marks in IndexedDB `hub-web` (all pages of `file://` share an origin, so each has a database of its own). */
export function indexedDbMissing(factory: IDBFactory | undefined): MissingStore {
  const open = (): Promise<IDBDatabase> =>
    new Promise((resolve, reject) => {
      if (!factory) {
        reject(new Error('no IndexedDB'));
        return;
      }
      const request = factory.open(DB, 1);
      request.onupgradeneeded = () => request.result.createObjectStore(STORE);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error ?? new Error('IndexedDB'));
    });
  const run = async <T>(mode: IDBTransactionMode, call: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> => {
    const db = await open();
    try {
      return await new Promise<T>((resolve, reject) => {
        const request = call(db.transaction(STORE, mode).objectStore(STORE));
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error ?? new Error('IndexedDB'));
      });
    } finally {
      db.close();
    }
  };
  return {
    async load() {
      // A browser that blocks the database: the page works, it just forgets the marks (`null`).
      try {
        const value: unknown = await run('readonly', (s) => s.get(KEY));
        return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : [];
      } catch {
        return null;
      }
    },
    async save(ids) {
      await run('readwrite', (s) => s.put(ids, KEY));
    },
  };
}
