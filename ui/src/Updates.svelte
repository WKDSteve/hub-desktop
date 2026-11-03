<script lang="ts">
  // The updates of this session: per app where it is or what came of it, the parts, and the last lines the steps
  // printed (as text, never markup). The web hub runs nothing: it shows the lines to type in the folder of the
  // clones instead.
  import type { HubApp, UpdateInfo } from './lib/api';
  import { jobLabel, partText, webUpdateLines } from './lib/updating';

  let { apps, updates, web }: { apps: HubApp[]; updates: Record<string, UpdateInfo>; web: boolean } = $props();

  const jobs = $derived(
    apps.flatMap((a) => {
      const job = updates[a.id]?.job;
      return job ? [{ app: a, job, label: jobLabel(job) }] : [];
    }),
  );
  const typed = $derived(
    web ? apps.flatMap((a) => {
      const lines = webUpdateLines(a);
      return lines ? [{ app: a, lines }] : [];
    }) : [],
  );
</script>

{#if jobs.length > 0}
  <section class="updates" aria-labelledby="updates-heading">
    <h2 id="updates-heading">Updates</h2>
    <ul>
      {#each jobs as { app, job, label } (app.id)}
        <li>
          <span class="name">{app.name}</span>
          <span class="state {label.kind}" role={label.kind === 'active' ? 'status' : undefined}>{label.text}</span>
          {#if job.parts.length > 0}
            <ul class="parts">
              {#each job.parts as part (part.part)}<li>{partText(part)}</li>{/each}
            </ul>
          {/if}
          {#if job.lines.length > 0}
            <details>
              <summary>Output of {app.name}</summary>
              <pre>{job.lines.join('\n')}</pre>
            </details>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if typed.length > 0}
  <section class="updates" aria-labelledby="update-pages-heading">
    <h2 id="update-pages-heading">Update the pages</h2>
    <p class="note">This page runs nothing. In the folder of the clones, type:</p>
    <ul>
      {#each typed as { app, lines } (app.id)}
        <li>
          <span class="name">{app.name}</span>
          <pre>{lines.join('\n')}</pre>
        </li>
      {/each}
    </ul>
    <p class="note">Then reload the open pages.</p>
  </section>
{/if}

<style>
  .updates ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 0.5rem;
  }
  .updates > ul > li {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .name {
    font-weight: 600;
    margin-right: 0.5rem;
  }
  .state.failed {
    font-weight: 600;
  }
  .parts {
    margin-top: 0.25rem !important;
    color: var(--muted);
    font-size: 0.9em;
  }
  .note {
    color: var(--muted);
  }
  pre {
    margin: 0.35rem 0 0;
    padding: 0.4rem 0.5rem;
    max-height: 16rem;
    overflow: auto;
    font-size: 0.85em;
    color: var(--text);
    background: var(--surface-2);
    border-radius: var(--radius);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
