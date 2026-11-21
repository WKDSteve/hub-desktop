<script lang="ts">
  // The options of the hub that a person may change on the page: the CI watcher (on or off, the minutes between two
  // of its rounds). The hub writes them into `config.json`; the watcher reads them before its next round, without a
  // new start. A page of a file (the web hub) has no watcher and shows nothing.
  import { untrack } from 'svelte';
  import { type CiSettings, intervalProblem } from './lib/ci';

  let {
    settings,
    onsave = () => {},
  }: {
    /** What `config.json` says now; `null`: there is nothing to set here. */
    settings: CiSettings | null;
    onsave?: (on: boolean, intervalMin: number) => void;
  } = $props();

  // The fields follow the hub until the person changes one; a saved change comes back from the hub.
  let on = $state(true);
  // `bind:value` of a number field is a number, or null while it is empty or no number.
  let minutes = $state<number | null>(5);
  const text = $derived(minutes === null ? '' : String(minutes));
  let shown: string | null = null;
  $effect(() => {
    // Only what the hub says is tracked: the fields of the person are not.
    const key = settings ? `${settings.on}:${settings.interval_min}` : null;
    untrack(() => {
      if (!settings || key === shown) return;
      shown = key;
      on = settings.on;
      minutes = settings.interval_min;
    });
  });

  const problem = $derived(settings ? intervalProblem(text, settings) : null);
  const changed = $derived(
    settings !== null && (on !== settings.on || minutes !== settings.interval_min),
  );

  function save(event: SubmitEvent): void {
    event.preventDefault();
    if (problem || !settings || minutes === null) return;
    onsave(on, minutes);
  }
</script>

{#if settings}
  <section aria-labelledby="options-heading" data-options>
    <h2 id="options-heading">Options</h2>
    <form class="ci-options" onsubmit={save} aria-label="CI watcher">
      <label class="switch">
        <input type="checkbox" bind:checked={on} />
        CI watcher: write the verdict of the hosted CI into finished tasks
      </label>
      <label class="interval">
        Minutes between two rounds
        <input
          type="number"
          inputmode="numeric"
          min={settings.min}
          max={settings.max}
          step="1"
          bind:value={minutes}
          aria-invalid={problem !== null}
          aria-describedby="ci-interval-help"
          disabled={!on}
        />
      </label>
      <p id="ci-interval-help" class="help" class:bad={problem !== null}>
        {problem ?? `${settings.min} to ${settings.max}, default ${settings.default}. It counts from the next round.`}
      </p>
      <button type="submit" class="save" disabled={!changed || problem !== null}>Save</button>
    </form>
  </section>
{/if}

<style>
  h2 {
    font-size: 1.1em;
    margin: 1rem 0 0.5rem;
  }
  .ci-options {
    display: grid;
    gap: 0.5rem;
    justify-items: start;
    padding: 0.6rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  label {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex-wrap: wrap;
  }
  input[type='number'] {
    font: inherit;
    width: 5rem;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.1rem 0.4rem;
  }
  input[type='number']:disabled {
    color: var(--muted);
  }
  input:focus-visible,
  .save:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .help {
    margin: 0;
    color: var(--muted);
  }
  .help.bad {
    color: var(--bad);
  }
  .save {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.1rem 0.75rem;
    cursor: pointer;
  }
  .save:hover:not(:disabled) {
    background: var(--line);
  }
  .save:disabled {
    cursor: default;
    color: var(--muted);
  }
</style>
