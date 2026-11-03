import { expect, FULL, launchRequests, test, until, type Machine } from './fixture';

const tile = (page: import('@playwright/test').Page, id: string) => page.locator(`[data-tile="${id}"]`);

test('the hub shows what is on the machine as tiles and the rest, quieter, below', async ({ page, hub }) => {
  await hub.open(page);
  const installed = page.getByRole('region', { name: 'Installed' });
  const other = page.getByRole('region', { name: 'Install or download' });
  for (const [id, name] of [
    ['tasks', 'Tasks'],
    ['rules', 'Rules'],
    ['docmap', 'Docmap'],
    ['ai', 'AI Desktop'],
  ]) {
    await expect(installed.locator(`[data-tile="${id}"]`)).toContainText(name);
  }
  // A planned app is below, with its badge, and is no link; an app that is only not installed yet links to its release.
  await expect(other.getByRole('button', { name: /Testing/ })).toContainText('Planned');
  await expect(other.getByRole('link', { name: /streamtap/ })).toContainText('Download');
  await expect(other.getByRole('link')).toHaveCount(1);
  // Quieter: the surface and the text colour are not those of an installed tile.
  const look = (id: string) =>
    tile(page, id).evaluate((el) => {
      const style = getComputedStyle(el);
      return `${style.backgroundColor}|${style.color}`;
    });
  expect(await look('testing')).not.toBe(await look('tasks'));
});

test('every state is a word, and the mode a click opens is a chip', async ({ page, hub }) => {
  await hub.open(page);
  await expect(tile(page, 'tasks')).toContainText('Stopped');
  await expect(tile(page, 'tasks')).toContainText('native');
  await expect(tile(page, 'rules')).toContainText('Setup needed');
  await expect(tile(page, 'ai')).toContainText('Port busy');
  // A program with a window of its own has no state to show.
  await expect(tile(page, 'docmap')).not.toContainText(/Running|Stopped|Setup|busy/);
});

test('a tile says whether the app starts with the login, and says nothing when that is not known', async ({
  page,
  hub,
}) => {
  // The real registry decides what the hub reports; the page is shown a fixed answer.
  await page.route('**/api/hub/apps', async (route) => {
    const doc = await (await route.fetch()).json();
    for (const a of doc.apps) a.autostart = { tasks: true, rules: false }[a.id as string] ?? null;
    await route.fulfill({ json: doc });
  });
  await hub.open(page);
  await expect(tile(page, 'tasks')).toContainText('Starts at login');
  await expect(tile(page, 'rules')).toContainText('No autostart');
  await expect(tile(page, 'docmap')).not.toContainText(/login|autostart/i);
});

test('the update badge: nothing is asked at load, the button asks GitHub once, only about public apps with a build', async ({
  page,
  hub,
}) => {
  const asked: { url: string; headers: Record<string, string> }[] = [];
  await page.route('**/api/hub/apps', async (route) => {
    const doc = await (await route.fetch()).json();
    // tasks and rules are public, ai is private; only tasks and ai report a build.
    for (const a of doc.apps) a.build = { tasks: '9f3b7ba', ai: '9f3b7ba' }[a.id as string] ?? null;
    await route.fulfill({ json: doc });
  });
  await page.route('https://api.github.com/**', async (route) => {
    asked.push({ url: route.request().url(), headers: route.request().headers() });
    await route.fulfill({
      status: 200,
      headers: { 'access-control-allow-origin': '*', 'content-type': 'application/json' },
      body: JSON.stringify({ name: 'Rolling build (1234567)', tag_name: 'latest' }),
    });
  });
  await hub.open(page);
  expect(asked).toHaveLength(0);
  await expect(tile(page, 'tasks')).not.toContainText('Update available');

  await page.getByRole('button', { name: 'Check for updates' }).click();
  await expect(tile(page, 'tasks')).toContainText('Update available');
  await expect(page.getByRole('status')).toContainText('1 update is available.');
  await expect(tile(page, 'rules')).not.toContainText('Update available');
  await expect(tile(page, 'ai')).not.toContainText('Update available');
  expect(asked.map((a) => a.url)).toEqual([
    'https://api.github.com/repos/WKDSteve/tasks-desktop/releases/tags/latest',
  ]);
  expect(Object.keys(asked[0]?.headers ?? {})).not.toContain('authorization');
  expect(Object.keys(asked[0]?.headers ?? {})).not.toContain('cookie');

  // The same day, the same answer without a second question.
  await page.getByRole('button', { name: 'Check for updates' }).click();
  await expect(tile(page, 'tasks')).toContainText('Update available');
  expect(asked).toHaveLength(1);

  // The policy of the page lets it reach GitHub and nobody else.
  const elsewhere = await page.evaluate(() => fetch('https://example.org/', { mode: 'no-cors' }).then(() => 'reached', () => 'blocked'));
  expect(elsewhere).toBe('blocked');
});

test('an app that is not installed is a link to the page where it is had, opened safely', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, docmap: false, ai: 'none' });
  await hub.open(page);
  const link = page.getByRole('link', { name: /Docmap/ });
  await expect(link).toHaveAttribute('href', /^https:\/\/github\.com\/WKDSteve\/docmap-desktop\/releases\//);
  await expect(link).toHaveAttribute('target', '_blank');
  await expect(link).toHaveAttribute('rel', /noopener/);
  await expect(link).toContainText('docmap-desktop-windows-x86_64-setup.exe');
  await expect(page.getByRole('region', { name: 'Install or download' }).getByRole('link', { name: /AI Desktop/ })).toBeVisible();
});

test('one click starts the app once, a double click too, and the tile says it is starting', async ({ page, hub }) => {
  const requests = launchRequests(page);
  await hub.open(page);
  await tile(page, 'tasks').click();
  await expect(page.getByRole('status')).toContainText('Starting Tasks');
  await until(() => hub.starts('tasks').length === 1, 'the start of tasks');
  expect(hub.starts('tasks')).toEqual([['start', '--open']]);
  expect(requests).toHaveLength(1);
  await expect(tile(page, 'tasks')).toContainText('Starting');

  // A double click on another app: two clicks, one request, one program.
  await tile(page, 'docmap').dblclick();
  await until(() => hub.starts('docmap').length >= 1, 'the start of docmap');
  await page.waitForTimeout(700);
  expect(requests).toHaveLength(2);
  expect(hub.starts('docmap')).toEqual([[]]);
});

test('two tabs of the page start the app once', async ({ page, context, hub }) => {
  const second = await context.newPage();
  const first = launchRequests(page);
  const other = launchRequests(second);
  await hub.open(page);
  await hub.open(second);
  await Promise.all([tile(page, 'docmap').click(), tile(second, 'docmap').click()]);
  await until(() => hub.starts('docmap').length >= 1, 'the start of docmap');
  await page.waitForTimeout(700);
  expect(first.length + other.length).toBe(2);
  expect(hub.starts('docmap')).toHaveLength(1);
});

test('an app that needs setup says so instead of opening a dead page, and its setup is one click', async ({ page, hub }) => {
  await hub.open(page);
  await tile(page, 'rules').click();
  const dialog = page.getByRole('dialog', { name: /Rules needs to be set up/ });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('no vault is set');
  await page.waitForTimeout(500);
  expect(hub.starts('rules')).toEqual([]);

  await dialog.getByRole('button', { name: 'Open setup' }).click();
  await until(() => hub.starts('rules').length === 1, 'the setup of rules');
  // The program as a person starts it: no argument.
  expect(hub.starts('rules')).toEqual([[]]);
  await expect(dialog).toBeHidden();
});

test('something else on the port of ai-desktop is a refusal in words, and nothing starts', async ({ page, hub }) => {
  await hub.open(page);
  await tile(page, 'ai').click();
  await expect(page.getByRole('alert')).toContainText('busy');
  await page.waitForTimeout(500);
  expect(hub.starts('ai')).toEqual([]);
});

test('ai-desktop that runs is opened in the browser, not started again', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, ai: 'running' });
  await hub.open(page);
  await expect(tile(page, 'ai')).toContainText('Running');
  await tile(page, 'ai').click();
  await expect(page.getByRole('status')).toContainText('Opened AI Desktop');
  await until(() => hub.opened().length === 1, 'the browser');
  expect(hub.opened()[0]).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/$/);
  expect(hub.starts('ai')).toEqual([]);
});

test('a web variant opens from the listener of the hub, and the page there loads', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, tasks: { native: false, web: true, vault: false } });
  await hub.open(page);
  await expect(tile(page, 'tasks')).toContainText('web');
  await tile(page, 'tasks').click();
  await until(() => hub.opened().length === 1, 'the browser');
  expect(hub.opened()).toEqual([`http://127.0.0.1:${hub.webPort}/tasks-web/tasks.html`]);
  expect(hub.starts('tasks')).toEqual([]);
  const served = await page.request.get(hub.opened()[0] as string);
  expect(served.status()).toBe(200);
  expect(await served.text()).toContain('a page of the web variant');
  expect(served.headers()['content-security-policy']).toContain("connect-src 'none'");
});

test('the second action opens the other mode and can keep it as the default', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, tasks: { native: true, web: true, vault: true } });
  await hub.open(page);

  // Shift+Enter on the tile: the menu of the second action.
  await tile(page, 'tasks').focus();
  await page.keyboard.press('Shift+Enter');
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: 'Open as web app' }).click();
  await until(() => hub.opened().length === 1, 'the browser');
  expect(hub.opened()[0]).toContain('/tasks-web/tasks.html');
  expect(hub.starts('tasks')).toEqual([]);

  // Keep it: the choice is in config.json, and the chip and the click follow it.
  await page.waitForTimeout(3200);
  await tile(page, 'tasks').focus();
  await page.keyboard.press('Shift+Enter');
  await page.getByRole('menuitem', { name: 'Make web app the default' }).click();
  await expect(tile(page, 'tasks')).toContainText('web');
  expect(hub.settings()).toMatchObject({ tiles: { tasks: { mode: 'web' } } });
  await page.reload();
  await expect(tile(page, 'tasks')).toContainText('web');
});

test('an entry of the menu that the app cannot do is disabled, with the reason', async ({ page, hub }) => {
  await hub.open(page);
  await tile(page, 'docmap').focus();
  await page.keyboard.press('Shift+Enter');
  const item = page.getByRole('menuitem', { name: 'Open as web app' });
  await expect(item).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(tile(page, 'docmap')).toBeFocused();
});

test('the keyboard does everything: arrows between tiles, Enter, /, r, Escape, the menu key', async ({ page, hub }) => {
  const requests: string[] = [];
  page.on('request', (r) => {
    if (r.method() === 'POST') requests.push(r.url());
  });
  await hub.open(page);

  // The tiles are one stop in the tab order; the arrows move between them.
  await tile(page, 'tasks').focus();
  await page.keyboard.press('ArrowRight');
  await expect(tile(page, 'rules')).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(tile(page, 'tasks')).toBeFocused();
  await page.keyboard.press('End');
  await expect(tile(page, 'streamtap')).toBeFocused();
  await page.keyboard.press('Home');
  await expect(tile(page, 'tasks')).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('[data-tile]:focus')).toHaveCount(1);
  await page.keyboard.press('Home');

  // Roving tab index: only one tile can be reached by Tab.
  const stops = await page.locator('[data-tile][tabindex="0"]').count();
  expect(stops).toBe(1);

  // Enter does what a click does.
  await page.keyboard.press('Enter');
  await until(() => hub.starts('tasks').length === 1, 'the start by Enter');

  // The context menu key opens the menu of the focused tile.
  await tile(page, 'tasks').focus();
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menu')).toBeVisible();
  await page.keyboard.press('Escape');

  // `/` is the filter, Escape clears it, `r` looks at the machine again.
  await page.keyboard.press('/');
  await expect(page.getByRole('searchbox')).toBeFocused();
  await page.keyboard.type('doc');
  await expect(tile(page, 'docmap')).toBeVisible();
  await expect(tile(page, 'tasks')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(tile(page, 'tasks')).toBeVisible();
  await page.locator('body').click({ position: { x: 5, y: 300 } });
  const before = requests.filter((u) => u.endsWith('/rescan')).length;
  await page.keyboard.press('r');
  await expect.poll(() => requests.filter((u) => u.endsWith('/rescan')).length).toBe(before + 1);
});

test('the page only talks to its own origin and only in the ways the hub knows', async ({ page, hub }) => {
  const seen: string[] = [];
  page.on('request', (r) => seen.push(`${r.method()} ${r.url()}`));
  await hub.open(page);
  await tile(page, 'tasks').click();
  await until(() => hub.starts('tasks').length === 1, 'the start');
  const origin = `http://127.0.0.1:${hub.port}`;
  for (const line of seen) {
    const [, url = ''] = line.split(' ');
    expect(url.startsWith(origin), line).toBe(true);
  }
  expect(seen.some((l) => l.includes('/api/hub/apps'))).toBe(true);
});

test('without a link the page asks for one, and a refused code is the same', async ({ page, hub }) => {
  await page.goto(`http://127.0.0.1:${hub.port}/`);
  await expect(page.getByRole('heading', { name: 'Open the hub with its link' })).toBeVisible();
  await page.goto(`http://127.0.0.1:${hub.port}/#pair=${'0'.repeat(32)}`);
  await expect(page.getByRole('heading', { name: 'Open the hub with its link' })).toBeVisible();
});

test('an empty machine shows empty areas in words and every app below', async ({ page, hubOn }) => {
  const empty: Machine = {
    tasks: { native: false, web: false, vault: false },
    rules: false,
    docmap: false,
    ai: 'none',
    testingWeb: false,
  };
  const hub = await hubOn(empty);
  await hub.open(page);
  await expect(page.getByText('No app is installed on this machine yet.')).toBeVisible();
  await expect(page.getByRole('region', { name: 'Install or download' }).getByRole('listitem')).toHaveCount(6);
});

test('a running app of the kit can be stopped from the menu, once, and the others cannot', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, tasksRunning: true });
  await hub.open(page);
  await expect(tile(page, 'tasks')).toContainText('Running');
  await tile(page, 'tasks').focus();
  await page.keyboard.press('Shift+Enter');
  await page.getByRole('menuitem', { name: 'Stop' }).click();
  await expect(page.getByRole('status')).toContainText('Stopping Tasks');
  // The program of the app is asked with the one word, nothing else.
  await until(() => hub.starts('tasks').length === 1, 'the program was asked to stop');
  expect(hub.starts('tasks')).toEqual([['stop']]);
  // An app that is not of the kit has no Stop to press.
  await tile(page, 'docmap').focus();
  await page.keyboard.press('Shift+Enter');
  await expect(page.getByRole('menuitem', { name: 'Stop' })).toBeDisabled();
});

test('the overlay hides and orders tiles, and an app it hides is not there to start', async ({ page, hubOn }) => {
  const hub = await hubOn({
    ...FULL,
    overlay: JSON.stringify({ schema: 'hub-catalog/1', hidden: ['docmap', 'streamtap'], order: ['ai', 'rules'] }),
  });
  await hub.open(page);
  await expect(tile(page, 'docmap')).toHaveCount(0);
  await expect(tile(page, 'streamtap')).toHaveCount(0);
  const order = await page.locator('[data-tile]').evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset['tile']));
  expect(order.slice(0, 3)).toEqual(['ai', 'rules', 'tasks']);
});

test('an overlay of a newer schema is ignored and the hub still runs', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, overlay: JSON.stringify({ schema: 'hub-catalog/2', hidden: ['tasks'] }) });
  await hub.open(page);
  await expect(tile(page, 'tasks')).toBeVisible();
});

test('without an export folder the page shows no monitor', async ({ page, hub }) => {
  await hub.open(page);
  await expect(page.getByRole('heading', { name: 'Testing' })).toHaveCount(0);
});

test('the monitor shows a light for each repository of the export, red first, in words', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, testingExport: 'fleet' });
  await hub.open(page);
  const monitor = page.getByRole('region', { name: 'Testing' });
  await expect(monitor).toContainText('Export of 2026-11-01 10:00:00 UTC');
  const rows = await monitor.locator('[data-repo]').evaluateAll((els) =>
    els.map((e) => [(e as HTMLElement).dataset['repo'], (e as HTMLElement).dataset['light'], e.textContent?.replace(/\s+/g, ' ').trim()]),
  );
  expect(rows.map((r) => r[0])).toEqual(['alpha.nvim', 'gamma.nvim', 'beta.nvim', 'delta.nvim']);
  expect(rows.map((r) => r[1])).toEqual(['red', 'amber', 'green', 'unknown']);
  expect(rows[0]?.[2]).toContain('Red');
  expect(rows[0]?.[2]).toContain('local red (3 failing)');
  expect(rows[1]?.[2]).toContain('2 outdated');
  expect(rows[2]?.[2]).toContain('CI success');
  expect(rows[3]?.[2]).toContain('nothing recorded');
  // The hub reads, it starts nothing and writes nothing of the export.
  expect(hub.starts('tasks')).toEqual([]);
});

test('an export that cannot be read is said in a sentence, without its path', async ({ page, hubOn }) => {
  const hub = await hubOn({ ...FULL, testingExport: 'broken' });
  await hub.open(page);
  const monitor = page.getByRole('region', { name: 'Testing' });
  await expect(monitor).toContainText('The testing export cannot be shown: the export folder has no testing-export.json');
  expect(await monitor.innerText()).not.toContain(hub.root);
  // The apps are there all the same.
  await expect(tile(page, 'tasks')).toBeVisible();
});
