<script lang="ts">
  // The updates of this session: per app where it is or what came of it, the parts, and the last lines the steps
  // printed (as text, never markup). The web hub runs nothing: it shows the lines to type in the folder of the
  // clones instead.
  import type { HubApp, UpdateInfo } from './lib/api';
  import { attestationText, jobLabel, partText, webUpdateLines } from './lib/updating';
  import { t } from './lib/i18n.svelte';

  let {
    apps,
    updates,
    web,
    focus = null,
  }: {
    apps: HubApp[];
    updates: Record<string, UpdateInfo>;
    web: boolean;
    /** The app whose output is opened and scrolled to (the "Update" entry of its tile menu). */
    focus?: string | null;
  } = $props();

  // Open the output of the app the person came for, and bring it into view.
  $effect(() => {
    if (!focus) return;
    const id = focus;
    queueMicrotask(() => {
      const item = document.querySelector<HTMLElement>(`[data-update-item="${CSS.escape(id)}"]`);
      if (!item) return;
      item.querySelector('details')?.setAttribute('open', '');
      item.scrollIntoView({ block: 'start' });
      item.focus({ preventScroll: true });
    });
  });

  const jobs = $derived(
    apps.flatMap((a) => {
      const job = updates[a.id]?.job;
      return job ? [{ app: a, job, label: jobLabel(job), attestation: attestationText(job) }] : [];
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
    <h2 id="updates-heading">{t('Updates')}</h2>
    <ul>
      {#each jobs as { app, job, label, attestation } (app.id)}
        <li data-update-item={app.id} tabindex="-1" class:focused={focus === app.id}>
          <span class="name">{app.name}</span>
          <span class="state {label.kind}" role={label.kind === 'active' ? 'status' : undefined}>{label.text}</span>
          {#if job.parts.length > 0}
            <ul class="parts">
              {#each job.parts as part (part.part)}<li>{partText(part)}</li>{/each}
            </ul>
          {/if}
          {#if attestation}
            <p class="attestation" data-attestation={job.attestation}>{attestation}</p>
          {/if}
          {#if job.lines.length > 0}
            <details>
              <summary>{t('Output of {name}', { name: app.name })}</summary>
              <pre>{job.lines.join('\n')}</pre>
            </details>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{:else if typed.length === 0}
  <section class="updates" aria-labelledby="updates-heading">
    <h2 id="updates-heading">{t('Updates')}</h2>
    <p class="note">{t('No update has run since the hub started. Update an app from its tile, or press "Update all".')}</p>
  </section>
{/if}

{#if typed.length > 0}
  <section class="updates" aria-labelledby="update-pages-heading">
    <h2 id="update-pages-heading">{t('Update the pages')}</h2>
    <p class="note">{t('This page runs nothing. In the folder of the clones, type:')}</p>
    <ul>
      {#each typed as { app, lines } (app.id)}
        <li>
          <span class="name">{app.name}</span>
          <pre>{lines.join('\n')}</pre>
        </li>
      {/each}
    </ul>
    <p class="note">{t('Then reload the open pages.')}</p>
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
  .updates > ul > li.focused {
    border-color: var(--accent);
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
  .attestation {
    margin: 0.25rem 0 0;
    color: var(--muted);
    font-size: 0.9em;
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
