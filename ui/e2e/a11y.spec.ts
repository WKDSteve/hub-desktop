import AxeBuilder from '@axe-core/playwright';
import type { Locator } from '@playwright/test';
import { expect, FULL, test } from './fixture';

// Accessibility of the page (axe), light and dark, and with the pieces that open: the menu of the second action and
// the dialog of the setup. The scan is of the page as it is; a rule that does not apply is not a pass for another.
const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'];

/** Axe is content with a button that does not react, so the colours are compared: the pointer must change its fill. */
async function showsThePointer(button: Locator): Promise<void> {
  await button.page().mouse.move(0, 0);
  const fill = () => button.evaluate((node) => getComputedStyle(node).backgroundColor);
  const atRest = await fill();
  await button.hover();
  expect(await fill()).not.toBe(atRest);
  const axe = async () => {
    const result = await new AxeBuilder({ page: button.page() }).withTags(TAGS).analyze();
    expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
  };
  await axe();
  await button.focus();
  await axe();
}

for (const scheme of ['light', 'dark'] as const) {
  test.describe(`axe, ${scheme}`, () => {
    test.beforeEach(async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
    });

    test('the page with both areas has no violation', async ({ page, hub }) => {
      await hub.open(page);
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    // The colours of the palette under the pointer and with the focus ring: axe reads the state the tile is in.
    test('a tile under the pointer and with the focus ring has no violation', async ({ page, hub }) => {
      await hub.open(page);
      const tile = page.locator('[data-tile="tasks"]');
      await tile.hover();
      const hovered = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(hovered.violations, JSON.stringify(hovered.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
      await tile.focus();
      const focused = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(focused.violations, JSON.stringify(focused.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    // Axe does not compare an edge with what is behind it: the fill of a tile of an app that is not installed is --line
    // under the pointer, the colour of a chip's edge at rest, so the chip ("Download") would lose its edge.
    test('the chip of a tile that is not installed keeps its edge under the pointer', async ({ page, hub }) => {
      await hub.open(page);
      const tile = page.locator('[data-tile="testing"]');
      await page.mouse.move(0, 0);
      await tile.hover();
      const [fill, edge] = await tile.evaluate((node) => {
        const chip = node.querySelector('.badge, .chip');
        return [getComputedStyle(node).backgroundColor, chip ? getComputedStyle(chip).borderTopColor : ''];
      });
      expect(edge).not.toBe('');
      expect(edge).not.toBe(fill);
    });

    test('the buttons of the page show the pointer', async ({ page, hub }) => {
      await hub.open(page);
      await showsThePointer(page.getByRole('button', { name: 'Look again' }));
      await showsThePointer(page.getByRole('button', { name: 'Check for updates' }));
      // A tile of an app that is not installed (its rest is --surface-2) shows the pointer too.
      await showsThePointer(page.locator('[data-tile="testing"]'));
    });

    test('with the monitor shown', async ({ page, hubOn }) => {
      const hub = await hubOn({ ...FULL, testingExport: 'fleet' });
      await hub.open(page);
      await expect(page.getByRole('region', { name: 'Testing' })).toBeVisible();
      await showsThePointer(page.getByRole('button', { name: 'Red cases' }));
      await page.getByRole('button', { name: 'Red cases' }).click();
      await expect(page.getByRole('region', { name: 'Red cases of alpha.nvim' }).locator('[data-case]')).toHaveCount(2);
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    test('with the menu of the second action open', async ({ page, hub }) => {
      await hub.open(page);
      await page.locator('[data-tile="tasks"]').focus();
      await page.keyboard.press('Shift+Enter');
      await expect(page.getByRole('menu')).toBeVisible();
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    test('with the update badge shown', async ({ page, hub }) => {
      await page.route('**/api/hub/apps', async (route) => {
        const doc = await (await route.fetch()).json();
        for (const a of doc.apps) a.build = a.id === 'tasks' ? '9f3b7ba' : null;
        await route.fulfill({ json: doc });
      });
      await page.route('https://api.github.com/**', (route) =>
        route.fulfill({
          status: 200,
          headers: { 'access-control-allow-origin': '*', 'content-type': 'application/json' },
          body: JSON.stringify({ name: 'Rolling build (1234567)' }),
        }),
      );
      await hub.open(page);
      await page.getByRole('button', { name: 'Check for updates' }).click();
      await expect(page.locator('[data-tile="tasks"]')).toContainText('Update available');
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    test('the filled button of a tile shows the pointer', async ({ page, hubOn }) => {
      test.slow(); // a real clone to ask GitHub about and axe twice: 10 to 25 s, over 30 s on a loaded runner
      const hub = await hubOn({ ...FULL, tasksWebClone: 'behind' });
      await page.route('**/api/hub/apps', async (route) => {
        const doc = await (await route.fetch()).json();
        for (const a of doc.apps) a.build = a.id === 'tasks' ? '9f3b7ba' : null;
        await route.fulfill({ json: doc });
      });
      await page.route('https://api.github.com/**', (route) =>
        route.fulfill({
          status: 200,
          headers: { 'access-control-allow-origin': '*', 'content-type': 'application/json' },
          body: JSON.stringify({ name: 'Rolling build (1234567)' }),
        }),
      );
      await hub.open(page);
      await page.getByRole('button', { name: 'Check for updates' }).click();
      await showsThePointer(page.getByRole('button', { name: 'Update Tasks' }));
    });

    test('with an update shown, its output open', async ({ page, hubOn }) => {
      test.slow(); // the real update (git against a dirty clone, up to 20 s to fail) and axe twice: 30 to 43 s on the runner
      const hub = await hubOn({ ...FULL, tasksWebClone: 'dirty' });
      await hub.open(page);
      await showsThePointer(page.getByRole('button', { name: 'Update all' }));
      await page.getByRole('button', { name: 'Update all' }).click();
      const updates = page.getByRole('region', { name: 'Updates' });
      await expect(updates).toContainText('Update failed', { timeout: 20_000 });
      await updates.getByText('Output of Tasks').click();
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    test('with the dialog of the setup open', async ({ page, hub }) => {
      await hub.open(page);
      await page.locator('[data-tile="rules"]').click();
      await expect(page.getByRole('dialog')).toBeVisible();
      await showsThePointer(page.getByRole('dialog').getByRole('button', { name: 'Open setup' }));
      await showsThePointer(page.getByRole('dialog').getByRole('button', { name: 'Close' }));
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });

    test('with a message shown and a tile that is starting', async ({ page, hub }) => {
      await hub.open(page);
      await page.locator('[data-tile="tasks"]').click();
      await expect(page.getByRole('status')).toContainText('Starting');
      const result = await new AxeBuilder({ page }).withTags(TAGS).analyze();
      expect(result.violations, JSON.stringify(result.violations.map((v) => [v.id, v.nodes.map((n) => n.html)]), null, 1)).toEqual([]);
    });
  });
}
