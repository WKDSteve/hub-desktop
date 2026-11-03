import { expect, FULL, PAGE_AFTER, PAGE_BEFORE, test } from './fixture';

// Updates through the real hub. The machine of the tests is the workstation (see the fixture): the programs are never
// installed, the clone of tasks-web is a real git clone one commit behind an origin in the folder of the test.

const tile = (page: import('@playwright/test').Page, id: string) => page.locator(`[data-tile="${id}"]`);

function updateRequests(page: import('@playwright/test').Page): string[] {
  const seen: string[] = [];
  page.on('request', (r) => {
    if (r.method() === 'POST' && /\/api\/hub\/(apps\/[^/]+\/update|update-all)$/.test(r.url())) seen.push(r.url());
  });
  return seen;
}

test('the menu of a tile updates its clone, and the program is left on the workstation', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, tasksWebClone: 'behind' });
  const asked = updateRequests(page);
  await hub.open(page);
  await tile(page, 'tasks').focus();
  await page.keyboard.press('Shift+Enter');
  await page.getByRole('menuitem', { name: 'Update' }).click();
  const updates = page.getByRole('region', { name: 'Updates' });
  await expect(updates).toContainText('Updated: reload its open page', { timeout: 20_000 });
  await expect(updates).toContainText('Web page: updated');
  await expect(updates).toContainText('Program: not updated, not on the workstation');
  await expect(tile(page, 'tasks')).toContainText('Updated: reload its open page');
  expect(hub.file('Repos/tasks-web/tasks.html')).toBe(PAGE_AFTER);
  expect(asked).toHaveLength(1);
  // The output is text: the lines of git, the check of the sums.
  await updates.getByText('Output of Tasks').click();
  await expect(updates.locator('pre')).toContainText('$ git pull --ff-only');
  await expect(updates.locator('pre')).toContainText('tasks.html: matches SHA256SUMS.');
  // Nothing was started or installed: no program of the machine logged a run.
  expect(hub.starts('tasks')).toEqual([]);
});

test('update all leaves a clone with local changes as it is and says why', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, tasksWebClone: 'dirty' });
  const asked = updateRequests(page);
  await hub.open(page);
  const all = page.getByRole('button', { name: 'Update all' });
  await expect(all).toBeEnabled();
  await all.click();
  await expect(page.getByRole('region', { name: 'Updates' })).toContainText('Update failed: the clone has local changes', {
    timeout: 20_000,
  });
  // The message of the round (one app was queued: the only one with something to update here).
  await expect(page.getByText('Tasks: Update failed: the clone has local changes.')).toBeVisible();
  expect(hub.file('Repos/tasks-web/tasks.html')).toContain('a change of the person');
  expect(hub.file('Repos/tasks-web/tasks.html')).toContain(PAGE_BEFORE.trim());
  expect(asked.filter((u) => u.endsWith('/update-all'))).toHaveLength(1);
  await expect(all).toBeEnabled();
});

test('with nothing to update on the machine, update all and the entry of the menu say why they cannot', async ({
  page,
  hub,
}) => {
  const asked = updateRequests(page);
  await hub.open(page);
  await expect(page.getByRole('button', { name: 'Update all' })).toBeDisabled();
  await tile(page, 'tasks').focus();
  await page.keyboard.press('Shift+Enter');
  const entry = page.getByRole('menuitem', { name: /Update/ });
  await expect(entry).toBeDisabled();
  await entry.hover();
  await expect(page.getByRole('tooltip')).toContainText('not on the workstation');
  expect(asked).toEqual([]);
  await expect(page.getByRole('region', { name: 'Updates' })).toHaveCount(0);
});
