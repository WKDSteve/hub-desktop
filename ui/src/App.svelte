<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Brand from 'appkit-ui/Brand.svelte';
  import ContextMenu from 'appkit-ui/ContextMenu.svelte';
  import Toast from 'appkit-ui/Toast.svelte';
  import { ContextRegistry } from 'appkit-ui/contextmenu';
  import type { App } from './lib/app.svelte';
  import { areas, hasMenu, hasMode, missingReason, moveFocus, otherMode, stopReason } from './lib/tiles';
  import Monitor from './Monitor.svelte';
  import Tile from './Tile.svelte';

  let { app, code }: { app: App; code: string | null } = $props();

  const groups = $derived(areas(app.apps, app.filter));
  // The tiles in the order of the page: what is on the machine, then the rest. One order for the arrow keys.
  const order = $derived([...groups.installed, ...groups.other]);
  let focused = $state<string | null>(null);
  // The one tile that is in the tab order: the one that had the focus last, else the first.
  const tabId = $derived(order.some((a) => a.id === focused) ? focused : (order[0]?.id ?? null));

  let filterBox: HTMLInputElement | undefined = $state();
  let setup: HTMLDialogElement | undefined = $state();

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
          label: tile.missing ? 'Mark as present' : 'Mark as not present',
          onselect: () => void app.mark(tile, !tile.missing),
        },
      ];
    }
    const other = otherMode(tile);
    const there = hasMode(tile, other);
    const hint = there ? undefined : missingReason(tile, other);
    const as = other === 'web' ? 'web app' : 'native app';
    return [
      {
        kind: 'action',
        label: other === 'web' ? 'Open as web app' : 'Open natively',
        disabled: !there,
        hint,
        onselect: () => void app.launch(tile, other),
      },
      {
        kind: 'action',
        label: `Make ${as} the default`,
        disabled: !there,
        hint,
        onselect: () => void app.makeDefault(tile, other),
      },
      {
        kind: 'action',
        label: 'Stop',
        disabled: !tile.stoppable,
        hint: tile.stoppable ? undefined : stopReason(tile),
        onselect: () => void app.stop(tile),
      },
    ];
  });

  onMount(() => {
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

  $effect(() => {
    if (app.setupFor && setup && !setup.open) setup.showModal();
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

  async function openSetup(): Promise<void> {
    const tile = app.setupFor;
    setup?.close();
    app.setupFor = null;
    if (tile) await app.launch(tile, 'setup');
    await tick();
  }

  const reason = (code: string | null): string =>
    code === 'vault_not_a_directory' ? 'its vault is not a folder' : code === 'no_vault' ? 'no vault is set' : 'it is not set up';
</script>

<svelte:window onkeydown={onWindowKey} />

<header class="bar">
  <Brand label="Hub" />
  <h1 class="sr-only">Hub</h1>
  <label class="filter">
    <span class="sr-only">Filter the apps</span>
    <input bind:this={filterBox} type="search" placeholder="Filter  (/)" aria-keyshortcuts="/" bind:value={app.filter} />
  </label>
  {#if app.canRescan}
    <button type="button" class="tool" aria-keyshortcuts="r" onclick={() => void app.rescan()}>Look again</button>
  {/if}
  {#if app.canCheckUpdates}
    <button type="button" class="tool" disabled={app.checking} onclick={() => void app.checkUpdates()}>
      Check for updates
    </button>
  {/if}
  {#if app.version}<span class="version">{app.version}</span>{/if}
</header>

<main>
  {#if app.phase === 'starting'}
    <p role="status">Connecting to the hub…</p>
  {:else if app.phase === 'unpaired'}
    <h2>Open the hub with its link</h2>
    <p>
      This page has no session. Run <code>hub-desktop open --launch</code> in a terminal, or start the hub from its
      link, and the browser opens it paired.
    </p>
  {:else if app.phase === 'error'}
    <h2>The hub does not answer</h2>
    <p role="alert">{app.problem}</p>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="areas" onkeydown={onTileKey} role="presentation">
      <section aria-labelledby="installed-heading">
        <h2 id="installed-heading">Installed</h2>
        {#if groups.installed.length > 0}
          <ul class="grid">
            {#each groups.installed as tile (tile.id)}
              <Tile
                app={tile}
                starting={app.starting[tile.id] !== undefined}
                failed={app.failed[tile.id]}
                tab={tabId === tile.id}
                update={app.hasUpdate(tile.id)}
                onlaunch={() => void app.launch(tile)}
                onmore={() => openMenu(tile.id)}
                onfocus={() => (focused = tile.id)}
              />
            {/each}
          </ul>
        {:else}
          <p class="none">{app.filter ? 'No installed app matches.' : 'No app is installed on this machine yet.'}</p>
        {/if}
      </section>
      <section aria-labelledby="other-heading">
        <h2 id="other-heading">Install or download</h2>
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
          <p class="none">{app.filter ? 'No other app matches.' : 'Everything is installed.'}</p>
        {/if}
      </section>
    </div>
    <Monitor monitor={app.monitor} />
  {/if}
  <!-- Inside the landmark: the menu is drawn at the pointer, but its content belongs to the page (axe: region). -->
  <ContextMenu {registry} label="Other ways to open the app" />
</main>

<Toast
  toast={app.toast}
  onundo={() => undefined}
  ondismiss={() => app.dismissToast()}
  labels={{ undo: 'Undo', dismiss: 'Dismiss the message' }}
/>

<dialog bind:this={setup} class="setup" aria-labelledby="setup-title" onclose={() => (app.setupFor = null)}>
  {#if app.setupFor}
    <h2 id="setup-title">{app.setupFor.name} needs to be set up</h2>
    <p>
      {app.setupFor.name} cannot be opened yet: {reason(app.setupFor.setup)}. The setup opens a console window where
      the app asks for what it needs.
    </p>
    <div class="actions">
      <button type="button" class="primary" onclick={() => void openSetup()}>Open setup</button>
      <button type="button" onclick={() => setup?.close()}>Close</button>
    </div>
  {/if}
</dialog>

<style>
  .bar {
    display: flex;
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
  .tool {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.2rem 0.6rem;
    cursor: pointer;
  }
  .version {
    color: var(--muted);
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
  .setup {
    max-width: 28rem;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .setup::backdrop {
    background: var(--backdrop);
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
  }
  .actions button {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.25rem 0.8rem;
    cursor: pointer;
  }
  .actions .primary {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent);
  }
</style>
