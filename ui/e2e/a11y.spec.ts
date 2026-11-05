import AxeBuilder from '@axe-core/playwright';
import { expect, FULL, test } from './fixture';

// Accessibility of the page (axe), light and dark, and with the pieces that open: the menu of the second action and
// the dialog of the setup. The scan is of the page as it is; a rule that does not apply is not a pass for another.
const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'];

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

    test('with the monitor shown', async ({ page, hubOn }) => {
      const hub = await hubOn({ ...FULL, testingExport: 'fleet' });
      await hub.open(page);
      await expect(page.getByRole('region', { name: 'Testing' })).toBeVisible();
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

    test('with an update shown, its output open', async ({ page, hubOn }) => {
      const hub = await hubOn({ ...FULL, tasksWebClone: 'dirty' });
      await hub.open(page);
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
