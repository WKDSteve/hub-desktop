<script lang="ts">
  // One app as a tile. On the machine: a button that does what the app's mode says, a small button beside it for the
  // other way (the menu of the second action), and a line with the state as a word and the mode as a chip. Not on the
  // machine: the same tile quieter, as a link to the page where the app is to be had, or a badge when there is none.
  import { contextTarget } from 'appkit-ui/contextmenu';
  import type { HubApp, UpdateJob } from './lib/api';
  import { iconOf } from './lib/icons';
  import { autostartWord, getting, hasMenu, modeName, stateOf } from './lib/tiles';
  import { jobLabel } from './lib/updating';

  let {
    app,
    starting,
    failed,
    tab,
    onlaunch,
    onmore,
    onfocus,
    update = false,
    job = null,
    canUpdate = false,
    onupdate = () => undefined,
  }: {
    app: HubApp;
    starting: boolean;
    failed: string | undefined;
    /** The one tile of the page that is in the tab order (roving tabindex). */
    tab: boolean;
    onlaunch: () => void;
    onmore: () => void;
    onfocus: () => void;
    /** GitHub names a newer build than the one that runs (the check found it; never a guess). */
    update?: boolean;
    /** The last update of the app (the hub runs it), if there was one. */
    job?: UpdateJob | null;
    /** The hub can update the app here and no update of it is on its way. */
    canUpdate?: boolean;
    onupdate?: () => void;
  } = $props();

  const updating = $derived(job ? jobLabel(job) : null);

  const state = $derived(stateOf(app, starting, failed));
  const get = $derived(getting(app));
  const login = $derived(autostartWord(app));
  const initial = $derived(app.name.slice(0, 1).toUpperCase());
  const image = $derived(iconOf(app));
</script>

{#snippet icon()}
  <span class="icon" class:letter={!image} aria-hidden="true">
    {#if image}<img src={image} alt="" />{:else}{initial}{/if}
  </span>
{/snippet}

{#snippet more(label: string)}
  <button type="button" class="more" tabindex="-1" aria-haspopup="menu" aria-label={label} onclick={onmore}>▾</button>
{/snippet}

<li class="tile" class:dim={!app.installed} use:contextTarget={hasMenu(app) ? { kind: 'tile', id: app.id } : undefined}>
  {#if app.installed && app.href}
    <!-- The web hub: the page of the web variant is one link away (same file:// origin), so a click is a navigation. -->
    <a
      class="main"
      data-tile={app.id}
      href={app.href}
      target="_blank"
      rel="noopener"
      tabindex={tab ? 0 : -1}
      onfocus={onfocus}
    >
      {@render icon()}
      <span class="name">{app.name}</span>
      <span class="line">
        <span class="note">Not checked</span>
        <span class="chip">web</span>
      </span>
    </a>
    {@render more(`More about ${app.name}`)}
  {:else if app.installed}
    <button
      type="button"
      class="main"
      data-tile={app.id}
      tabindex={tab ? 0 : -1}
      aria-busy={starting}
      onclick={onlaunch}
      onfocus={onfocus}
    >
      {@render icon()}
      <span class="name">{app.name}</span>
      <span class="line">
        {#if state.kind !== 'none'}
          <span class="state {state.kind}"
            ><span class="dot" aria-hidden="true">{state.kind === 'starting' ? '' : '●'}</span>{state.label}</span
          >
        {/if}
        <span class="chip">{modeName(app.default_mode)}</span>
        {#if update}<span class="update"><span aria-hidden="true">↑</span> Update available</span>{/if}
        {#if login}<span class="note">{login}</span>{/if}
      </span>
      {#if updating}
        <span class="job {updating.kind}" data-update={app.id}>{updating.text}</span>
      {/if}
    </button>
    {#if update && canUpdate}
      <button type="button" class="do-update" tabindex="-1" aria-label={`Update ${app.name}`} onclick={onupdate}
        >Update</button
      >
    {/if}
    <button
      type="button"
      class="more"
      tabindex="-1"
      aria-haspopup="menu"
      aria-label={`Other ways to open ${app.name}`}
      onclick={onmore}>▾</button
    >
  {:else if get.linked}
    <a
      class="main"
      data-tile={app.id}
      href={app.install.page}
      target="_blank"
      rel="noopener noreferrer"
      tabindex={tab ? 0 : -1}
      onfocus={onfocus}
    >
      {@render icon()}
      <span class="name">{app.name}</span>
      <span class="line"><span class="badge">{get.badge}</span></span>
      {#if get.note}<span class="file">{get.note}</span>{/if}
      {#if app.install.file}<span class="file">{app.install.file}</span>{/if}
    </a>
    {#if hasMenu(app)}
      {@render more(`More about ${app.name}`)}
    {/if}
  {:else}
    <button type="button" class="main" data-tile={app.id} tabindex={tab ? 0 : -1} aria-disabled="true" onfocus={onfocus}>
      {@render icon()}
      <span class="name">{app.name}</span>
      <span class="line"><span class="badge">{get.badge}</span></span>
      {#if get.note}<span class="file">{get.note}</span>{/if}
    </button>
    {#if hasMenu(app)}
      {@render more(`More about ${app.name}`)}
    {/if}
  {/if}
</li>

<style>
  .tile {
    position: relative;
    list-style: none;
    min-height: 140px;
    display: flex;
  }
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.35rem;
    padding: 0.75rem;
    text-align: left;
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: pointer;
    text-decoration: none;
  }
  .main:hover {
    background: var(--accent-soft);
  }
  .dim .main {
    background: var(--surface-2);
    color: var(--muted);
  }
  /* After .dim .main, which would otherwise take the hover of a tile that is not installed away. One that can do
     nothing (a planned app) does not react. */
  .dim .main:hover:not([aria-disabled='true']) {
    background: var(--line);
  }
  .dim .main[aria-disabled='true'] {
    cursor: default;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 2.25rem;
    height: 2.25rem;
  }
  .icon img {
    display: block;
    width: 100%;
    height: 100%;
  }
  .letter {
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 700;
  }
  .dim .icon {
    filter: grayscale(1);
    opacity: 0.6;
  }
  .name {
    font-weight: 600;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem 0.5rem;
    align-items: center;
    font-size: 0.85em;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .dot {
    font-size: 0.7em;
  }
  .state.running .dot {
    color: var(--ok);
  }
  .state.setup .dot,
  .state.busy .dot,
  .state.failed .dot {
    color: var(--warn);
  }
  .state.starting .dot {
    width: 0.8em;
    height: 0.8em;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .state.starting .dot {
      animation: none;
    }
  }
  .note {
    color: var(--muted);
  }
  .update {
    color: var(--text);
    font-weight: 600;
  }
  .job {
    font-size: 0.85em;
    color: var(--muted);
  }
  .job.failed {
    color: var(--text);
    font-weight: 600;
  }
  .do-update {
    position: absolute;
    right: 0.4rem;
    bottom: 0.4rem;
    padding: 0 0.5rem;
    font: inherit;
    font-size: 0.85em;
    color: var(--accent-text);
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .do-update:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }
  .chip,
  .badge {
    padding: 0 0.4rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
  }
  .file {
    font-size: 0.8em;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .more {
    position: absolute;
    top: 0.4rem;
    right: 0.4rem;
    padding: 0 0.4rem;
    font: inherit;
    color: var(--muted);
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .more:hover {
    border-color: var(--line);
    color: var(--text);
  }
</style>
