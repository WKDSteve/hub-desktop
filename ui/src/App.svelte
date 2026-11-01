<script lang="ts">
  import { onMount } from 'svelte';
  import Brand from 'appkit-ui/Brand.svelte';
  import type { App } from './lib/app.svelte';

  let { app, code }: { app: App; code: string | null } = $props();

  onMount(() => {
    void app.start(code);
  });
</script>

<header class="bar">
  <Brand label="Hub" />
  {#if app.version}<span class="version">{app.version}</span>{/if}
</header>

<main>
  {#if app.phase === 'starting'}
    <p role="status">Connecting to the hub…</p>
  {:else if app.phase === 'unpaired'}
    <h1>Open the hub with its link</h1>
    <p>
      This page has no session. Run <code>hub-desktop open --launch</code> in a terminal, or start the hub from its
      link, and the browser opens it paired.
    </p>
  {:else if app.phase === 'error'}
    <h1>The hub does not answer</h1>
    <p role="alert">{app.problem}</p>
  {:else if app.apps.length === 0}
    <h1>No apps yet</h1>
    <p>The hub runs. The apps it can start will appear here.</p>
  {:else}
    <ul>
      {#each app.apps as item (item.id)}
        <li>{item.name}</li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  .bar {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }
  .version {
    color: var(--muted);
  }
  main {
    padding: 1rem;
  }
</style>
