import AxeBuilder from '@axe-core/playwright';
import { expect, test } from './fixture';

const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'];

test('the page lists the apps, asks nothing of the network and breaks no rule of its policy', async ({ page, site }) => {
  const seen = await site.watch(page);
  await page.goto(site.url);
  await expect(page.getByRole('heading', { name: 'Installed' })).toBeVisible();
  // The web variant is a link to the page next to this one; "not checked" is said in words.
  const tasks = page.locator('[data-tile="tasks"]');
  await expect(tasks).toHaveAttribute('href', '../tasks-web/tasks.html');
  await expect(tasks).toContainText('Not checked');
  // A program is listed, and said not to be startable here.
  const rules = page.locator('[data-tile="rules"]');
  await expect(rules).toContainText('Native only');
  await expect(rules).toContainText('Not startable in the web hub');
  // The icons are in the file: tiles, bar and tab name data: URLs, and the policy lets them through.
  const drawn = (selector: string) =>
    page.locator(selector).evaluateAll((all) => all.map((i) => (i as HTMLImageElement).naturalWidth > 0));
  await expect.poll(() => drawn('[data-tile] img')).toEqual(Array(6).fill(true));
  await expect.poll(() => drawn('.kit-brand img')).toEqual([true]);
  await expect(page.locator('head link[rel="icon"]')).toHaveAttribute('href', /^data:image\/svg\+xml/);
  await expect(page.getByRole('button', { name: 'Look again' })).toHaveCount(0);
  // GitHub is out of reach for this file (connect-src 'none'): there is no button for the update badge.
  await expect(page.getByRole('button', { name: 'Check for updates' })).toHaveCount(0);
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
  expect(await site.violations(page)).toEqual([]);
});

test('the web hub runs no update: it shows the lines to type in the folder of the clones', async ({ page, site }) => {
  const seen = await site.watch(page);
  await page.goto(site.url);
  await expect(page.locator('[data-tile="tasks"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Update all' })).toHaveCount(0);
  const pages = page.getByRole('region', { name: 'Update the pages' });
  await expect(pages.locator('pre')).toHaveText('git -C tasks-web pull --ff-only\ngit -C tasks-web status --short');
  // Only what is a web variant on this machine: no line for a program.
  await expect(pages).not.toContainText('Rules');
  await page.locator('[data-tile="tasks"]').focus();
  await page.keyboard.press('Shift+Enter');
  await expect(page.getByRole('menuitem', { name: 'Update' })).toHaveCount(0);
  expect(seen.requests).toEqual([]);
  expect(await site.violations(page)).toEqual([]);
  const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
  expect(result.violations.map((v) => v.id)).toEqual([]);
});

test('the policy of the file forbids every request', async ({ page, site }) => {
  await site.watch(page);
  await page.goto(site.url);
  await expect(page.locator('[data-tile="tasks"]')).toBeVisible();
  const csp = await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute('content');
  expect(csp).toContain("connect-src 'none'");
  expect(csp).not.toContain('worker-src');
  const failed = await page.evaluate(() => fetch(location.href).then(() => false, () => true));
  expect(failed).toBe(true);
  expect(await site.violations(page)).toEqual(expect.arrayContaining([expect.stringContaining('connect-src')]));
});

test('a click opens the page of the neighbour in a new tab', async ({ page, site }) => {
  await page.goto(site.url);
  const popup = page.waitForEvent('popup');
  await page.locator('[data-tile="tasks"]').click();
  const opened = await popup;
  await expect(opened).toHaveURL(site.tasks);
  await expect(opened.getByRole('heading', { name: 'Tasks page' })).toBeVisible();
  // The hub stays where it was.
  await expect(page).toHaveURL(site.url);
});

test('a mark "not present" moves the app, survives a reload and can be taken back', async ({ page, site }) => {
  await page.goto(site.url);
  await page.locator('[data-tile="tasks"]').focus();
  await page.keyboard.press('Shift+Enter');
  await page.getByRole('menuitem', { name: 'Mark as not present' }).click();
  const other = page.getByRole('region', { name: 'Install or download' });
  const tile = other.locator('[data-tile="tasks"]');
  await expect(tile).toContainText('Marked as not present');
  await expect(tile).toHaveAttribute('href', 'https://github.com/WKDSteve/tasks-web');
  await page.reload();
  await expect(page.getByRole('region', { name: 'Install or download' }).locator('[data-tile="tasks"]')).toBeVisible();
  await page.locator('[data-tile="tasks"]').focus();
  await page.keyboard.press('Shift+Enter');
  await page.getByRole('menuitem', { name: 'Mark as present' }).click();
  await expect(page.getByRole('region', { name: 'Installed' }).locator('[data-tile="tasks"]')).toBeVisible();
});

test('the folder of the clones is picked once: what is not in it is marked, the rest is not', async ({ page, site }) => {
  // The browser's dialog is a fake that hands over a folder with the given entries (names only, nothing is read).
  await page.addInitScript(() => {
    type Tree = { [name: string]: Tree | 'file' };
    const handle = (name: string, tree: Tree): unknown => ({
      kind: 'directory',
      name,
      async *values() {
        for (const [child, sub] of Object.entries(tree)) yield sub === 'file' ? { kind: 'file', name: child } : handle(child, sub);
      },
    });
    const w = window as unknown as { picks: number; tree: Tree; showDirectoryPicker: () => Promise<unknown> };
    w.picks = 0;
    w.tree = { 'rules-web': {} };
    w.showDirectoryPicker = async () => {
      w.picks++;
      return handle('repos', w.tree);
    };
  });
  const seen = await site.watch(page);
  await page.goto(site.url);
  await expect(page.locator('[data-tile="tasks"]')).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { picks: number }).picks)).toBe(0);
  await page.getByRole('button', { name: 'Choose the folder of the clones' }).click();
  await expect(page.getByRole('status')).toContainText('0 web apps found, 1 not in the folder.');
  const other = page.getByRole('region', { name: 'Install or download' });
  await expect(other.locator('[data-tile="tasks"]')).toContainText('Marked as not present');
  // The marks are the menu's: they survive a reload, and a second pick with the clone there takes them back.
  await page.reload();
  await expect(page.getByRole('region', { name: 'Install or download' }).locator('[data-tile="tasks"]')).toBeVisible();
  await page.evaluate(() => {
    (window as unknown as { tree: unknown }).tree = { 'tasks-web': { 'tasks.html': 'file' } };
  });
  await page.getByRole('button', { name: 'Choose the folder of the clones' }).click();
  await expect(page.getByRole('status')).toContainText('1 web app found.');
  await expect(page.getByRole('region', { name: 'Installed' }).locator('[data-tile="tasks"]')).toHaveAttribute(
    'href',
    '../tasks-web/tasks.html',
  );
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
});

test('a browser without a folder dialog keeps the marks of the menu, with no button', async ({ page, site }) => {
  await page.addInitScript(() => {
    delete (window as unknown as { showDirectoryPicker?: unknown }).showDirectoryPicker;
  });
  await page.goto(site.url);
  await expect(page.locator('[data-tile="tasks"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Choose the folder of the clones' })).toHaveCount(0);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`axe, ${scheme}: the page, and with the menu open`, async ({ page, site }) => {
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
    await page.goto(site.url);
    await expect(page.locator('[data-tile="tasks"]')).toBeVisible();
    const check = async () => {
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    };
    await check();
    await page.locator('[data-tile="tasks"]').focus();
    await page.keyboard.press('Shift+Enter');
    await expect(page.getByRole('menu')).toBeVisible();
    await check();
  });
}
