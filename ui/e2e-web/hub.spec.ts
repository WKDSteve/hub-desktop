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
  await expect(page.getByRole('button', { name: 'Look again' })).toHaveCount(0);
  // GitHub is out of reach for this file (connect-src 'none'): there is no button for the update badge.
  await expect(page.getByRole('button', { name: 'Check for updates' })).toHaveCount(0);
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
  expect(await site.violations(page)).toEqual([]);
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
