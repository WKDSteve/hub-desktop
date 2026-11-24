<script lang="ts">
  import { onMount } from 'svelte';
  import Brand from 'appkit-ui/Brand.svelte';
  import { logoFor } from './lib/icons';
  import ContextMenu from 'appkit-ui/ContextMenu.svelte';
  import Toast from 'appkit-ui/Toast.svelte';
  import { ContextRegistry } from 'appkit-ui/contextmenu';
  import type { App } from './lib/app.svelte';
  import { areas, hasMenu, hasMode, missingReason, moveFocus, otherMode, stopReason } from './lib/tiles';
  import { active, updatable, whyNot } from './lib/updating';
  import Monitor from './Monitor.svelte';
  import Options from './Options.svelte';
  import { ciLines, settingsOf } from './lib/ci';
  import { effectiveTab, TAB_NAMES, type Tab, tabAfterKey, visibleTabs } from './lib/tabs';
  import { applyTheme, loadTheme, saveTheme, type Theme } from './lib/theme';
  import { lang, loadChoice, type LangChoice, resolveLang, saveChoice, setLang, t } from './lib/i18n.svelte';
  import Tile from './Tile.svelte';
  import Updates from './Updates.svelte';

  let { app, code }: { app: App; code: string | null } = $props();

  const groups = $derived(areas(app.apps, app.filter));
  // The tiles in the order of the page: what is on the machine, then the rest. One order for the arrow keys.
  const order = $derived([...groups.installed, ...groups.other]);
  let focused = $state<string | null>(null);
  // The one tile that is in the tab order: the one that had the focus last, else the first.
  const tabId = $derived(order.some((a) => a.id === focused) ? focused : (order[0]?.id ?? null));

  let filterBox: HTMLInputElement | undefined = $state();

  // The tabs: Testing only where there is something to show. A tab that went away gives way to the first.
  const shown = $derived(visibleTabs(app.monitor.state !== 'not_configured' || ciLines(app.ci).length > 0));
  const tab = $derived(effectiveTab(app.tab, shown));
  $effect(() => {
    // The focus on an update is for one visit of the tab: the next jump to the same app must work again.
    if (tab !== 'updates' && app.updateFocus !== null) app.updateFocus = null;
  });

  function onTabKey(event: KeyboardEvent): void {
    const to = tabAfterKey(shown, tab, event.key);
    if (to === null) return;
    event.preventDefault();
    app.tab = to;
    document.getElementById(`tab-${to}`)?.focus();
  }

  let langChoice = $state<LangChoice>('system');
  function pickLang(next: LangChoice): void {
    langChoice = next;
    saveChoice(themeStorage(), next);
    setLang(resolveLang(next, navigator.language));
  }
  // The root element says which language the page is in (screen readers, hyphenation, spell check).
  $effect(() => {
    document.documentElement.lang = lang();
  });

  let theme = $state<Theme>('system');
  function themeStorage(): Storage | null {
    try {
      return window.localStorage;
    } catch {
      return null;
    }
  }
  function setTheme(next: Theme): void {
    theme = next;
    saveTheme(themeStorage(), next);
    applyTheme(document.documentElement, next);
  }

  // The second action of a tile (concept 6.1): the mode a click does not open, and keeping it as the default.
  const registry = new ContextRegistry();
  registry.provide('tile', (target) => {
    const tile = app.apps.find((a) => a.id === target.id);
    if (!tile) return undefined;
    // The web hub cannot look at a machine: the one thing a viewer can do is say whether the page is there.
    if (tile.href !== undefined || tile.missing) {
      return [
        {
          kind: 'action',
          label: tile.missing ? t('Mark as present') : t('Mark as not present'),
          onselect: () => void app.mark(tile, !tile.missing),
        },
      ];
    }
    const other = otherMode(tile);
    const there = hasMode(tile, other);
    const hint = there ? undefined : missingReason(tile, other);
    const as = other === 'web' ? t('web app') : t('native app');
    return [
      {
        kind: 'action',
        label: other === 'web' ? t('Open as web app') : t('Open natively'),
        disabled: !there,
        hint,
        onselect: () => void app.launch(tile, other),
      },
      {
        kind: 'action',
        label: t('Make {as} the default', { as }),
        disabled: !there,
        hint,
        onselect: () => void app.makeDefault(tile, other),
      },
      {
        kind: 'action',
        label: t('Stop'),
        disabled: !tile.stoppable,
        hint: tile.stoppable ? undefined : stopReason(tile),
        onselect: () => void app.stop(tile),
      },
      {
        kind: 'action',
        label: t('Update'),
        disabled: !canUpdateTile(tile.id),
        hint: canUpdateTile(tile.id) ? undefined : updateHint(tile.id),
        onselect: () => void app.update(tile),
      },
      // The output of the last update (the lines git and the steps printed), in the Updates tab.
      {
        kind: 'action',
        label: t('Update output →'),
        disabled: !app.updates[tile.id]?.job,
        hint: app.updates[tile.id]?.job ? undefined : t('No update of this app has run yet'),
        onselect: () => app.showUpdate(tile.id),
      },
      // The start with the login: the same state the app shows and switches itself. Only where it can be told.
      ...(typeof tile.autostart === 'boolean'
        ? [
            {
              kind: 'check' as const,
              label: t('Start at login'),
              checked: tile.autostart,
              onselect: () => void app.setAutostart(tile, !tile.autostart),
            },
          ]
        : []),
    ];
  });

  /** The hub can update the app here, and no update of it is on its way. */
  const canUpdateTile = (id: string): boolean => updatable(app.updates[id]) && !active(app.updates[id]?.job);
  const updateHint = (id: string): string =>
    active(app.updates[id]?.job) ? t('An update is on its way') : whyNot(app.updates[id]);

  onMount(() => {
    langChoice = loadChoice(themeStorage());
    setLang(resolveLang(langChoice, navigator.language));
    theme = loadTheme(themeStorage());
    applyTheme(document.documentElement, theme);
    void app.start(code);
    // Fresh when the page is looked at again (concept 5.1), and now and then while it stays in front.
    const refresh = () => void app.reload();
    const onVisible = () => {
      if (document.visibilityState === 'visible') refresh();
    };
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', onVisible);
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') refresh();
    }, 10_000);
    return () => {
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', onVisible);
      clearInterval(timer);
    };
  });

  // Bar and tab show one spoke per app on this machine. A filter changes neither: it only hides tiles.
  const mark = $derived(logoFor(app.apps.filter((a) => a.installed).length));
  $effect(() => {
    document.head.querySelector('link[rel="icon"]')?.setAttribute('href', mark);
  });

  function typing(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLElement &&
      (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
    );
  }

  /** The main element of every tile, in the order of the page. */
  const mains = (): HTMLElement[] => [...document.querySelectorAll<HTMLElement>('[data-tile]')];

  function openMenu(id: string): void {
    const el = mains().find((m) => m.dataset['tile'] === id);
    if (!el) return;
    const box = el.getBoundingClientRect();
    el.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: box.left, clientY: box.bottom }),
    );
  }

  function onWindowKey(event: KeyboardEvent): void {
    if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
    if (typing(event.target)) {
      if (event.key === 'Escape' && event.target === filterBox) {
        app.filter = '';
        filterBox?.blur();
      }
      return;
    }
    if (event.key === '/') {
      event.preventDefault();
      filterBox?.focus();
    } else if (event.key === 'r') {
      event.preventDefault();
      void app.rescan();
    }
  }

  function onTileKey(event: KeyboardEvent): void {
    const el = event.target instanceof HTMLElement ? event.target.closest<HTMLElement>('[data-tile]') : null;
    if (!el || event.ctrlKey || event.metaKey || event.altKey) return;
    const id = el.dataset['tile'] ?? '';
    if (event.key === 'Enter' && event.shiftKey) {
      // The second action: the menu, from the keyboard (an installed tile has one).
      event.preventDefault();
      const tile = app.apps.find((a) => a.id === id);
      if (tile && hasMenu(tile)) openMenu(id);
      return;
    }
    const list = mains();
    const boxes = list.map((m) => m.getBoundingClientRect());
    const to = moveFocus(boxes, list.indexOf(el), event.key);
    if (to !== null) {
      event.preventDefault();
      list[to]?.focus();
    }
  }

</script>

<svelte:window onkeydown={onWindowKey} />

<header class="bar">
  <Brand label="Hub"><span class="brand-mark"><img src={mark} alt="" width="20" height="20" />Hub</span></Brand>
  <h1 class="sr-only">Hub</h1>
  <label class="filter">
    <span class="sr-only">{t('Filter the apps')}</span>
    <input bind:this={filterBox} type="search" spellcheck="false" autocomplete="off" placeholder={t('Filter  (/)')} aria-keyshortcuts="/" bind:value={app.filter} />
  </label>
  {#if app.canRescan}
    <button type="button" class="tool" aria-keyshortcuts="r" onclick={() => void app.rescan()}>{t('Look again')}</button>
  {/if}
  {#if app.canChooseFolder}
    <button type="button" class="tool" onclick={() => void app.chooseFolder()}>{t('Choose the folder of the clones')}</button>
  {/if}
  {#if app.canCheckUpdates}
    <button type="button" class="tool" disabled={app.checking} onclick={() => void app.checkUpdates()}>
      {t('Check for updates')}
    </button>
  {/if}
  {#if app.canUpdate}
    <button
      type="button"
      class="tool"
      disabled={!app.anyUpdatable || app.updating}
      title={app.anyUpdatable ? undefined : t('Nothing can be updated on this machine')}
      onclick={() => void app.updateAll()}
    >
      {app.updating ? t('Updating…') : t('Update all')}
    </button>
  {/if}
</header>

<main>
  {#if app.phase === 'starting'}
    <p role="status">{t('Connecting to the hub…')}</p>
  {:else if app.phase === 'unpaired'}
    <h2>{t('Open the hub with its link')}</h2>
    <p>
      {t('This page has no session. Run')} <code translate="no">hub-desktop open</code>
      {t('in a terminal, or start the hub from its link, and the browser opens it paired.')}
    </p>
  {:else if app.phase === 'error'}
    <h2>{t('The hub does not answer')}</h2>
    <p role="alert" class="problem">{app.problem}</p>
  {:else}
    <div class="tabs" role="tablist" aria-label={t('Areas of the hub')} tabindex="-1" onkeydown={onTabKey}>
      {#each shown as name (name)}
        <button
          type="button"
          role="tab"
          id={`tab-${name}`}
          class="tab"
          aria-selected={tab === name}
          aria-controls={`panel-${name}`}
          tabindex={tab === name ? 0 : -1}
          onclick={() => (app.tab = name)}
        >
          {t(TAB_NAMES[name])}{#if name === 'updates' && app.updating}<span class="dot" aria-hidden="true"></span>{/if}
        </button>
      {/each}
    </div>
    {#if tab === 'apps'}
      <div role="tabpanel" id="panel-apps" aria-labelledby="tab-apps">
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="areas" onkeydown={onTileKey} role="presentation">
        <section aria-labelledby="installed-heading">
          <h2 id="installed-heading">{t('Installed')}</h2>
          {#if groups.installed.length > 0}
            <ul class="grid">
              {#each groups.installed as tile (tile.id)}
                <Tile
                  app={tile}
                  starting={app.starting[tile.id] !== undefined}
                  failed={app.failed[tile.id]}
                  tab={tabId === tile.id}
                  update={app.hasUpdate(tile.id)}
                  job={app.updates[tile.id]?.job ?? null}
                  canUpdate={canUpdateTile(tile.id)}
                  onupdate={() => void app.update(tile)}
                  onlaunch={() => void app.launch(tile)}
                  onmore={() => openMenu(tile.id)}
                  onfocus={() => (focused = tile.id)}
                />
              {/each}
            </ul>
          {:else}
            <p class="none">{app.filter ? t('No installed app matches.') : t('No app is installed on this machine yet.')}</p>
          {/if}
        </section>
        <section aria-labelledby="other-heading">
          <h2 id="other-heading">{t('Install or download')}</h2>
          {#if groups.other.length > 0}
            <ul class="grid">
              {#each groups.other as tile (tile.id)}
                <Tile
                  app={tile}
                  starting={false}
                  failed={undefined}
                  tab={tabId === tile.id}
                  onlaunch={() => undefined}
                  onmore={() => openMenu(tile.id)}
                  onfocus={() => (focused = tile.id)}
                />
              {/each}
            </ul>
          {:else}
            <p class="none">{app.filter ? t('No other app matches.') : t('Everything is installed.')}</p>
          {/if}
        </section>
      </div>
      </div>
    {:else if tab === 'testing'}
      <div role="tabpanel" id="panel-testing" aria-labelledby="tab-testing">
        <Monitor
          monitor={app.monitor}
          ci={app.ci}
          open={app.openCases}
          cases={app.cases}
          ontoggle={(key) => void app.toggleCases(key)}
        />
      </div>
    {:else if tab === 'updates'}
      <div role="tabpanel" id="panel-updates" aria-labelledby="tab-updates">
        <Updates apps={app.apps} updates={app.updates} web={!app.canUpdate} focus={app.updateFocus} />
      </div>
    {:else}
      <div role="tabpanel" id="panel-options" aria-labelledby="tab-options">
        <Options
          settings={settingsOf(app.ci)}
          onsave={(on, minutes) => void app.saveCi(on, minutes)}
          {theme}
          onTheme={setTheme}
          {langChoice}
          onLang={pickLang}
          version={app.version}
        />
      </div>
    {/if}
  {/if}
  <!-- Inside the landmark: the menu is drawn at the pointer, but its content belongs to the page (axe: region). -->
  <ContextMenu {registry} label={t('Other ways to open the app')} />
</main>

<Toast
  toast={app.toast}
  onundo={() => undefined}
  ondismiss={() => app.dismissToast()}
  labels={{ undo: t('Undo'), dismiss: t('Dismiss the message') }}
/>


<style>
  .brand-mark {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }
  .problem {
    overflow-wrap: anywhere;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }
  .filter {
    margin-left: auto;
  }
  .filter input {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.2rem 0.5rem;
    min-width: 12rem;
  }
  .tool:disabled {
    cursor: default;
    color: var(--muted);
  }
  .tool {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.2rem 0.6rem;
    cursor: pointer;
  }
  /* --line, not --accent-soft: the button rests on --surface-2, which the translucent accent hardly changes. */
  .tool:hover:not(:disabled) {
    background: var(--line);
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
    border-bottom: 1px solid var(--line);
  }
  .tab {
    font: inherit;
    color: var(--muted);
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    padding: 0.4rem 0.9rem;
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab[aria-selected='true'] {
    color: var(--text);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }
  .dot {
    display: inline-block;
    width: 0.45rem;
    height: 0.45rem;
    margin-left: 0.4rem;
    border-radius: 50%;
    background: var(--accent);
  }
  main {
    padding: 1rem;
  }
  h2 {
    font-size: 1.1em;
    margin: 1rem 0 0.5rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: 0.75rem;
    padding: 0;
    margin: 0;
  }
  .none {
    color: var(--muted);
  }
</style>
