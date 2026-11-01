<script lang="ts">
  import { LIGHTS, type MonitorReading, parts, stamped } from './lib/monitor';

  let { monitor }: { monitor: MonitorReading } = $props();
</script>

{#if monitor.state !== 'not_configured'}
  <section aria-labelledby="testing-heading" data-monitor>
    <h2 id="testing-heading">Testing</h2>
    {#if monitor.state === 'problem'}
      <p class="none" role="status">The testing export cannot be shown: {monitor.problem}.</p>
    {:else}
      <p class="stamp">{stamped(monitor.generated_at)}</p>
      {#if monitor.repos.length > 0}
        <ul class="rows">
          {#each monitor.repos as repo (repo.key)}
            <li class="row" data-light={repo.light} data-repo={repo.name}>
              <span class="light" data-light={repo.light}>
                <span aria-hidden="true">{LIGHTS[repo.light].mark}</span>
                {LIGHTS[repo.light].word}
              </span>
              <span class="name">{repo.name}</span>
              <span class="what">{parts(repo, monitor.local_read).join(' · ')}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="none">The export holds no repository (none is released for it on this device).</p>
      {/if}
    {/if}
  </section>
{/if}

<style>
  h2 {
    font-size: 1.1em;
    margin: 1rem 0 0.5rem;
  }
  .stamp,
  .none,
  .what {
    color: var(--muted);
  }
  .stamp {
    margin: 0 0 0.5rem;
  }
  .rows {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 0.25rem;
  }
  .row {
    display: grid;
    grid-template-columns: 7rem minmax(8rem, 14rem) 1fr;
    gap: 0.75rem;
    align-items: baseline;
    padding: 0.3rem 0.6rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .light {
    font-weight: 600;
    white-space: nowrap;
  }
  .light[data-light='red'] {
    color: var(--bad);
  }
  .light[data-light='amber'] {
    color: var(--warn);
  }
  .light[data-light='green'] {
    color: var(--ok);
  }
  .light[data-light='unknown'] {
    color: var(--muted);
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  @media (max-width: 40rem) {
    .row {
      grid-template-columns: 1fr;
      gap: 0.1rem;
    }
  }
</style>
