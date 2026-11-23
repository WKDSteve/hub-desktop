<script lang="ts">
  import {
    type CasesView,
    hasCases,
    LIGHTS,
    type MonitorReading,
    omitted,
    parts,
    STATUS_WORDS,
    stamped,
    title,
    where,
  } from './lib/monitor';
  import { type CiView, ciLines, ciStatus } from './lib/ci';
  import { t } from './lib/i18n.svelte';

  let {
    monitor,
    ci = { state: 'none' },
    open = null,
    cases = { state: 'loading' },
    ontoggle = () => {},
  }: {
    monitor: MonitorReading;
    /** What the CI watcher of the hub is at; its hints show the section even without an export folder. */
    ci?: CiView;
    /** The key of the repository whose red cases are shown. */
    open?: string | null;
    cases?: CasesView;
    ontoggle?: (key: string) => void;
  } = $props();

  const hints = $derived(ciLines(ci));
  const status = $derived(ciStatus(ci));
</script>

{#if monitor.state !== 'not_configured' || hints.length > 0}
  <section aria-labelledby="testing-heading" data-monitor>
    <h2 id="testing-heading">{t('Testing')}</h2>
    {#if status}
      <div class="ci" data-ci={ci.state}>
        <p class="stamp" role="status">{status}</p>
        {#if hints.length > 0}
          <ul class="ci-hints" aria-label={t('Hints of the CI watcher')}>
            {#each hints as hint, i (i)}
              <li class="ci-hint"><span aria-hidden="true">!</span> {hint}</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
    {#if monitor.state === 'not_configured'}
      <!-- Only the hints of the CI watcher: there is no export folder. -->
    {:else if monitor.state === 'problem'}
      <p class="none" role="status">{t('The testing export cannot be shown: {why}.', { why: monitor.problem })}</p>
    {:else}
      <p class="stamp">{stamped(monitor.generated_at)}</p>
      {#if monitor.repos.length > 0}
        <ul class="rows">
          {#each monitor.repos as repo (repo.key)}
            <li class="row" data-light={repo.light} data-repo={repo.name}>
              <span class="light" data-light={repo.light}>
                <span aria-hidden="true">{LIGHTS[repo.light].mark}</span>
                {t(LIGHTS[repo.light].word)}
              </span>
              <span class="name">{repo.name}</span>
              <span class="what">{parts(repo, monitor.local_read).join(' · ')}</span>
              {#if hasCases(repo)}
                <button
                  type="button"
                  class="toggle"
                  aria-expanded={open === repo.key}
                  aria-controls={`cases-${repo.key}`}
                  onclick={() => ontoggle(repo.key)}
                >
                  {t('Red cases')}
                </button>
              {/if}
              {#if open === repo.key}
                <div class="cases" id={`cases-${repo.key}`} role="region" aria-label={t('Red cases of {name}', { name: repo.name })}>
                  {#if cases.state === 'loading'}
                    <p class="none" role="status">{t('Reading the cases…')}</p>
                  {:else if cases.state === 'failed'}
                    <p class="none" role="status">{t('The hub did not answer.')}</p>
                  {:else if cases.state === 'not_configured'}
                    <p class="none">{t('No export folder is named.')}</p>
                  {:else if cases.state === 'problem'}
                    <p class="none" role="status">{t('The cases cannot be shown: {why}.', { why: cases.problem })}</p>
                  {:else if !cases.local_read}
                    <p class="none">{t('The export was made where nothing runs: no case is recorded.')}</p>
                  {:else if cases.cases.length === 0}
                    <p class="none">{t('No red case is recorded for the last run.')}</p>
                  {:else}
                    <ul class="case-list">
                      {#each cases.cases as c, i (i)}
                        <li class="case" data-case={where(c)}>
                          <div class="case-head">
                            <span class="where">{where(c)}</span>
                            <span class="status">{t(STATUS_WORDS[c.status])}{c.flaky ? t(', flaky') : ''}</span>
                          </div>
                          <div class="case-title">{title(c)}</div>
                          {#if c.message}<pre class="text">{c.message}</pre>{/if}
                          {#if c.expected !== null || c.actual !== null}
                            <dl class="diff">
                              {#if c.expected !== null}<dt>{t('Expected')}</dt><dd><pre class="text">{c.expected}</pre></dd>{/if}
                              {#if c.actual !== null}<dt>{t('Actual')}</dt><dd><pre class="text">{c.actual}</pre></dd>{/if}
                            </dl>
                          {/if}
                        </li>
                      {/each}
                    </ul>
                    {#if omitted(cases)}<p class="none">{omitted(cases)}</p>{/if}
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="none">{t('The export holds no repository (none is released for it on this device).')}</p>
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
  .ci-hints {
    list-style: none;
    padding: 0;
    margin: 0 0 0.75rem;
    display: grid;
    gap: 0.25rem;
  }
  .ci-hint {
    padding: 0.3rem 0.6rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-left: 3px solid var(--warn);
    border-radius: var(--radius);
    overflow-wrap: anywhere;
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
    grid-template-columns: 7rem minmax(8rem, 14rem) 1fr auto;
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
  .toggle {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.1rem 0.5rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .toggle:hover {
    background: var(--line);
  }
  .cases {
    grid-column: 1 / -1;
    border-top: 1px solid var(--line);
    padding-top: 0.4rem;
  }
  .case-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 0.5rem;
  }
  .case-head {
    display: flex;
    gap: 0.75rem;
    flex-wrap: wrap;
  }
  .where {
    font-family: var(--mono, monospace);
    overflow-wrap: anywhere;
  }
  .status {
    color: var(--bad);
    font-weight: 600;
  }
  .text {
    margin: 0.25rem 0 0;
    padding: 0.3rem 0.5rem;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 16rem;
    overflow: auto;
  }
  .diff {
    margin: 0.25rem 0 0;
  }
  .diff dt {
    color: var(--muted);
    margin-top: 0.25rem;
  }
  .diff dd {
    margin: 0;
  }
  @media (max-width: 40rem) {
    .row {
      grid-template-columns: 1fr;
      gap: 0.1rem;
    }
  }
</style>
