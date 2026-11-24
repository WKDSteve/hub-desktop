<script lang="ts">
  // The options of the hub that a person may change on the page: the CI watcher (on or off, the minutes between two
  // of its rounds). The hub writes them into `config.json`; the watcher reads them before its next round, without a
  // new start. A page of a file (the web hub) has no watcher and shows nothing.
  import { untrack } from 'svelte';
  import { type CiSettings, intervalProblem } from './lib/ci';
  import { type Theme, THEMES } from './lib/theme';
  import { LANG_NAMES, LANGS, type LangChoice, msg, t } from './lib/i18n.svelte';

  const THEME_WORDS: Record<Theme, string> = {
    system: msg('Follow the system'),
    light: msg('Light'),
    dark: msg('Dark'),
  };
  const LANG_WORDS: Record<LangChoice, string> = { system: msg('Follow the browser'), en: LANG_NAMES.en, de: LANG_NAMES.de };

  let {
    settings,
    onsave = () => {},
    theme = 'system',
    onTheme = () => {},
    version = '',
    langChoice = 'system',
    onLang = () => {},
  }: {
    /** The look of the page, and what happens when the person picks another. */
    theme?: Theme;
    onTheme?: (theme: Theme) => void;
    /** The version of the hub, as it said hello. */
    version?: string;
    /** The language the person picked (`system`: the browser's), and what happens when they pick another. */
    langChoice?: LangChoice;
    onLang?: (choice: LangChoice) => void;
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

<section aria-labelledby="options-heading" data-options>
  <h2 id="options-heading">{t('Options')}</h2>
  <fieldset class="group" data-theme-options>
    <legend>{t('Appearance')}</legend>
    {#each THEMES as th (th)}
      <label class="choice">
        <input type="radio" name="theme" value={th} checked={theme === th} onchange={() => onTheme(th)} />
        {t(THEME_WORDS[th])}
      </label>
    {/each}
  </fieldset>
  <fieldset class="group" data-lang-options>
    <legend>{t('Language')}</legend>
    {#each ['system', ...LANGS] as choice (choice)}
      <label class="choice">
        <input
          type="radio"
          name="lang"
          value={choice}
          checked={langChoice === choice}
          onchange={() => onLang(choice as LangChoice)}
        />
        {t(LANG_WORDS[choice as LangChoice])}
      </label>
    {/each}
  </fieldset>
  {#if settings}
    <form class="ci-options" onsubmit={save} aria-label={t('CI watcher')}>
      <label class="switch">
        <input type="checkbox" bind:checked={on} />
        {t('CI watcher: write the verdict of the hosted CI into finished tasks')}
      </label>
      <label class="interval">
        {t('Minutes between two rounds')}
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
        {problem ??
          t('{min} to {max}, default {default}. It counts from the next round.', {
            min: settings.min,
            max: settings.max,
            default: settings.default,
          })}
      </p>
      <button type="submit" class="save" disabled={!changed || problem !== null}>{t('Save')}</button>
    </form>
  {/if}
  <div class="group" data-about>
    <h3>{t('About')}</h3>
    <p class="help">{t('Hub')}{version ? ` ${version}` : ''}</p>
  </div>
</section>

<style>
  h2 {
    font-size: 1.1em;
    margin: 1rem 0 0.5rem;
  }
  .group {
    display: grid;
    gap: 0.4rem;
    justify-items: start;
    margin: 0 0 0.75rem;
    padding: 0.6rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .group h3,
  legend {
    margin: 0;
    padding: 0 0.25rem;
  }
  .ci-options {
    margin-bottom: 0.75rem;
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
    min-height: 1.5rem;
    cursor: pointer;
  }
  .save:hover:not(:disabled) {
    background: var(--line);
  }
  .save:disabled {
    cursor: default;
    color: var(--muted);
  }
  @media (pointer: coarse) {
    .save {
      min-height: 2.75rem;
    }
  }
</style>
