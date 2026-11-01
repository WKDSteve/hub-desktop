// A real hub for the browser tests: the binary of this repository, started against a machine that exists only for
// the test (fake roots, the fake program of crates/fake-app for every app, a fake ai-desktop on a port, the browser
// replaced by a file). Nothing of the real machine is looked at or started: the roots, the config folders, the ports
// and the environment are the test's, and the environment of the hub holds nothing else.

import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer as createHttp, type Server } from 'node:http';
import { createServer as createNet } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, type Page } from '@playwright/test';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const HUB = join(repo, 'target', 'debug', `hub-desktop${exe}`);
const FAKE = join(repo, 'target', 'debug', `fake-app${exe}`);

/** What the machine of a test has on it. */
export interface Machine {
  /** tasks: the program, the web variant (a page in a checkout), a vault. */
  tasks: { native: boolean; web: boolean; vault: boolean };
  rules: boolean;
  docmap: boolean;
  /** ai-desktop: not installed, or installed and its port is free, serves its health, or answers as somebody else. */
  ai: 'none' | 'stopped' | 'running' | 'busy';
  /** The page of testing-web is there (an app that is planned counts as installed then). */
  testingWeb: boolean;
}

export const FULL: Machine = {
  tasks: { native: true, web: false, vault: true },
  rules: true,
  docmap: true,
  ai: 'busy',
  testingWeb: false,
};

export interface Hub {
  port: number;
  webPort: number;
  root: string;
  /** A one-time pairing link of the hub (`hub-desktop open`). */
  link(): string;
  /** The starts the fake program of an app logged (its argument lists). */
  starts(app: 'tasks' | 'rules' | 'docmap' | 'ai'): string[][];
  /** The links the hub opened in "the browser" (a file in the test). */
  opened(): string[];
  /** `config.json` of the hub. */
  settings(): Record<string, unknown>;
  /** Open the page, paired. */
  open(page: Page): Promise<void>;
}

const PROGRAMS = {
  tasks: 'Local/tasks-desktop/bin/tasks-hub',
  rules: 'Local/rules-desktop/bin/rules-hub',
  docmap: 'Local/docmap-desktop/docmap-desktop',
  ai: 'Local/Programs/ai-desktop/app/ai-desktop',
} as const;

function freePort(): Promise<number> {
  return new Promise((resolvePort, reject) => {
    const server = createNet();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = typeof address === 'object' && address !== null ? address.port : 0;
      server.close(() => resolvePort(port));
    });
  });
}

function listen(server: Server, port: number): Promise<void> {
  return new Promise((resolveListen, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => resolveListen());
  });
}

/** Starts a hub on a machine and returns it with its teardown. */
export async function startHub(machine: Machine): Promise<{ hub: Hub; stop: () => Promise<void> }> {
  for (const file of [HUB, FAKE]) {
    if (!existsSync(file)) throw new Error(`${file} is missing: run \`cargo build --workspace\` first`);
  }
  const root = mkdtempSync(join(tmpdir(), 'hub-desktop-e2e-'));
  const at = (below: string) => join(root, ...below.split('/'));
  const install = (below: string) => {
    const file = at(below + exe);
    mkdirSync(dirname(file), { recursive: true });
    copyFileSync(FAKE, file);
  };
  const page = (below: string) => {
    mkdirSync(dirname(at(below)), { recursive: true });
    writeFileSync(at(below), '<!doctype html><title>page</title><p>a page of the web variant</p>');
  };

  if (machine.tasks.native) install(PROGRAMS.tasks);
  if (machine.tasks.web) page('Repos/tasks-web/tasks.html');
  if (machine.testingWeb) page('Repos/testing-web/testing.html');
  if (machine.rules) install(PROGRAMS.rules);
  if (machine.docmap) install(PROGRAMS.docmap);
  if (machine.ai !== 'none') install(PROGRAMS.ai);
  mkdirSync(at('Repos'), { recursive: true });
  mkdirSync(at('vault'), { recursive: true });

  const ai: Server[] = [];
  const aiPort = await freePort();
  if (machine.ai === 'running' || machine.ai === 'busy') {
    const body =
      machine.ai === 'running'
        ? { status: 'ok', model_router_ready: true, backends: [] }
        : { hello: 'somebody else' };
    const server = createHttp((_req, res) => {
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify(body));
    });
    await listen(server, aiPort);
    ai.push(server);
  }

  const port = await freePort();
  const webPort = await freePort();
  const env: Record<string, string> = {
    // Nothing of the real environment: a variable of the person (a vault, a port) must not reach the hub, and the
    // programs it starts, by accident. The system's own are what a program needs to run.
    PATH: process.env['PATH'] ?? '',
    SystemRoot: process.env['SystemRoot'] ?? '',
    windir: process.env['windir'] ?? '',
    TEMP: root,
    TMP: root,
    LOCALAPPDATA: at('Local'),
    APPDATA: at('Roaming'),
    PROGRAMFILES: at('Program Files'),
    USERPROFILE: at('Home'),
    HOME: at('Home'),
    REPOS_DIR: at('Repos'),
    HUB_DESKTOP_CONFIG_DIR: at('cfg-hub'),
    TASKS_DESKTOP_CONFIG_DIR: at('cfg-tasks'),
    RULES_DESKTOP_CONFIG_DIR: at('cfg-rules'),
    HUB_DESKTOP_PORT: String(port),
    HUB_DESKTOP_WEB_PORT: String(webPort),
    AI_DESKTOP_PORT: String(aiPort),
    HUB_DESKTOP_BROWSER_LOG: at('browser.log'),
    HUB_DESKTOP_NO_LAUNCH: '1',
  };
  if (machine.tasks.vault) env['TASKS_VAULT'] = at('vault');

  mkdirSync(at('cfg-hub'), { recursive: true });
  const child: ChildProcess = spawn(HUB, [], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  let out = '';
  child.stdout?.on('data', (chunk: Buffer) => (out += chunk.toString()));
  child.stderr?.on('data', (chunk: Buffer) => (out += chunk.toString()));
  const started = Date.now();
  while (!/#pair=[0-9a-f]+/.test(out)) {
    if (Date.now() - started > 20_000 || child.exitCode !== null) {
      child.kill();
      throw new Error(`the hub did not start:\n${out}`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }

  const link = (): string => {
    const run = spawnSync(HUB, ['open'], { env, encoding: 'utf8' });
    const text = run.stdout.trim();
    if (!/^http:\/\/127\.0\.0\.1:\d+\/#pair=[0-9a-f]+$/.test(text)) throw new Error(`no link: ${text} ${run.stderr}`);
    return text;
  };

  const hub: Hub = {
    port,
    webPort,
    root,
    link,
    starts: (app) => {
      const log = at(PROGRAMS[app] + exe).replace(/[^\\/]+$/, 'fake-app.log');
      if (!existsSync(log)) return [];
      return readFileSync(log, 'utf8')
        .split('\n')
        .filter((l) => l !== '')
        .map((l) => (JSON.parse(l) as { argv: string[] }).argv);
    },
    opened: () => {
      const log = at('browser.log');
      return existsSync(log) ? readFileSync(log, 'utf8').split('\n').filter((l) => l !== '') : [];
    },
    settings: () => {
      const file = at('cfg-hub/config.json');
      return existsSync(file) ? (JSON.parse(readFileSync(file, 'utf8')) as Record<string, unknown>) : {};
    },
    open: async (p) => {
      await p.goto(link());
      await expect(p.getByRole('heading', { name: 'Installed' })).toBeVisible();
    },
  };

  const stop = async () => {
    child.kill();
    await new Promise<void>((r) => (child.exitCode !== null ? r() : child.once('exit', () => r())));
    await Promise.all(ai.map((s) => new Promise<void>((r) => s.close(() => r()))));
    // The files of the fake programs may be held for a moment after the process ended.
    for (let i = 0; i < 20; i++) {
      try {
        rmSync(root, { recursive: true, force: true });
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 200));
      }
    }
  };
  return { hub, stop };
}

interface Fixtures {
  /** A hub on the full machine (every app installed, ai answering as somebody else). */
  hub: Hub;
  /** Make a hub on another machine; it is stopped with the test. */
  hubOn: (machine: Machine) => Promise<Hub>;
}

export const test = base.extend<Fixtures>({
  hubOn: async ({}, use) => {
    const stops: (() => Promise<void>)[] = [];
    await use(async (machine) => {
      const made = await startHub(machine);
      stops.push(made.stop);
      return made.hub;
    });
    for (const stop of stops) await stop();
  },
  hub: async ({ hubOn }, use) => {
    await use(await hubOn(FULL));
  },
});

export { expect };

/** The requests of the page that start something: `POST /api/hub/apps/<id>/launch`. */
export function launchRequests(page: Page): string[] {
  const seen: string[] = [];
  page.on('request', (r) => {
    if (r.method() === 'POST' && /\/api\/hub\/apps\/[^/]+\/launch$/.test(r.url())) seen.push(r.url());
  });
  return seen;
}

/** Wait until `check` holds (a program logs its start a moment after the click). */
export async function until(check: () => boolean, what: string): Promise<void> {
  const end = Date.now() + 10_000;
  while (Date.now() < end) {
    if (check()) return;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error(`timed out waiting for ${what}`);
}
