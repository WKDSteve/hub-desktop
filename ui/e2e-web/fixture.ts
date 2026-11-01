// The built file next to a neighbour, the way the clones lie on a machine: `<root>/hub-web/hub.html` and
// `<root>/tasks-web/tasks.html`. Every test gets a root of its own, so the files of one test are not the page of another.

import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { test as base, expect, type Page } from '@playwright/test';
import { violations, watchPage, type Watch } from 'appkit-web/e2e';

const here = dirname(fileURLToPath(import.meta.url));
const BUILT = resolve(here, '..', 'dist-web', 'hub.html');

export interface Site {
  /** The address of `hub.html`. */
  url: string;
  /** The address of the neighbour's page. */
  tasks: string;
  watch: (page: Page) => Promise<Watch>;
  violations: (page: Page) => Promise<string[]>;
}

export const test = base.extend<{ site: Site }>({
  // eslint-disable-next-line no-empty-pattern
  site: async ({}, use) => {
    if (!existsSync(BUILT)) throw new Error(`${BUILT} is not there: run "pnpm web:build" first`);
    const root = mkdtempSync(join(tmpdir(), 'hub-web-'));
    try {
      mkdirSync(join(root, 'hub-web'));
      mkdirSync(join(root, 'tasks-web'));
      copyFileSync(BUILT, join(root, 'hub-web', 'hub.html'));
      writeFileSync(join(root, 'tasks-web', 'tasks.html'), '<!doctype html><title>Tasks page</title><h1>Tasks page</h1>');
      const url = pathToFileURL(join(root, 'hub-web', 'hub.html')).href;
      await use({
        url,
        tasks: pathToFileURL(join(root, 'tasks-web', 'tasks.html')).href,
        watch: (page) => watchPage(page, { pageUrl: url }),
        violations: (page) => violations(page),
      });
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
});

export { expect };
export type { Watch };
