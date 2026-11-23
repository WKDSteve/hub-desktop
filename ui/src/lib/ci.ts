// The CI watcher of the hub (`ciwatch.rs`): it writes the verdict of the hosted CI into the finished tasks and
// keeps what a person may want to look at as hints. `GET /api/hub/ci` tells what it is at; this file turns that
// into the lines of the section "Testing". Only repository names, task ids and fixed words reach it.

import { t } from './i18n.svelte';

export type CiHint =
  | { kind: 'queued'; repo: string; minutes: number }
  | { kind: 'behind'; repo: string }
  | { kind: 'no_checkout'; repo: string }
  | { kind: 'no_github'; repo: string }
  | { kind: 'no_answer'; repo: string; why: string }
  | { kind: 'awaits_trust'; repo: string };

export interface CiVerdict {
  id: string;
  ci: 'green' | 'red';
}

/** The settings of the watcher (`config.json`) and the bounds of the field. */
export interface CiSettings {
  on: boolean;
  interval_min: number;
  min: number;
  max: number;
  default: number;
}

export type CiView =
  /** The web hub: there is no watcher. */
  | { state: 'none' }
  | { state: 'off'; why: string; settings?: CiSettings }
  | { state: 'waiting'; settings?: CiSettings }
  | {
      state: 'ran';
      settings?: CiSettings;
      /** When the last round ended, seconds since 1970. */
      at: number;
      written: CiVerdict[];
      not_written: CiVerdict[];
      hints: CiHint[];
      problem: string | null;
    };

export const CI_NONE: CiView = { state: 'none' };

/** What a hint says, in words. */
export function hintText(hint: CiHint): string {
  switch (hint.kind) {
    case 'queued':
      return t('{repo}: the CI run has waited {minutes} min for a runner', { repo: hint.repo, minutes: hint.minutes });
    case 'behind':
      return t('{repo}: the checkout does not know the head of origin/main yet (not fetched)', { repo: hint.repo });
    case 'no_checkout':
      return t('{repo}: no checkout below REPOS_DIR', { repo: hint.repo });
    case 'no_github':
      return t('{repo}: no hosted CI (its origin is not on GitHub)', { repo: hint.repo });
    case 'no_answer':
      return t('{repo}: no usable answer ({why})', { repo: hint.repo, why: hint.why });
    case 'awaits_trust':
      return t('{repo}: the runner guard turned the run away, it waits for your release (testing ci trust)', {
        repo: hint.repo,
      });
  }
}

/** The things of the last round a person may want to look at, one line each (the icon counts the same). */
export function ciLines(view: CiView): string[] {
  if (view.state !== 'ran') return [];
  return [
    ...(view.problem ? [t('The round did not get to the tasks: {why}', { why: view.problem })] : []),
    ...view.not_written.map((v) => t('{id}: the tasks command did not take ci={ci}', { id: v.id, ci: v.ci })),
    ...view.hints.map(hintText),
  ];
}

/** The line that says what the watcher is at; `null`: nothing to say (the web hub). */
export function ciStatus(view: CiView, now: Date = new Date()): string | null {
  switch (view.state) {
    case 'none':
      return null;
    case 'off':
      return t('The CI watcher does not run: {why}.', { why: view.why });
    case 'waiting':
      return t('The CI watcher has not finished a round yet.');
    case 'ran': {
      const minutes = Math.max(0, Math.floor((now.getTime() / 1000 - view.at) / 60));
      const when = minutes === 0 ? t('less than a minute ago') : t('{n} min ago', { n: minutes });
      const written = view.written.length === 1 ? t('1 verdict') : t('{n} verdicts', { n: view.written.length });
      return t('CI watcher: last round {when}, {written} written.', { when, written });
    }
  }
}

/** The settings of a view, when the hub sent them whole. */
export function settingsOf(view: CiView): CiSettings | null {
  return view.state !== 'none' && view.settings ? view.settings : null;
}

/** What is wrong with the text of the interval field; `null`: it is a whole number within the bounds. */
export function intervalProblem(text: string, settings: CiSettings): string | null {
  const bounds = t('{min} to {max} minutes', { min: settings.min, max: settings.max });
  if (!/^\d+$/.test(text.trim())) return t('A whole number of minutes, {bounds}.', { bounds });
  const minutes = Number(text.trim());
  if (minutes < settings.min || minutes > settings.max) return t('The interval is {bounds}.', { bounds });
  return null;
}

function isSettings(s: unknown): s is CiSettings {
  if (typeof s !== 'object' || s === null) return false;
  const d = s as Record<string, unknown>;
  return (
    typeof d.on === 'boolean' &&
    ['interval_min', 'min', 'max', 'default'].every((k) => Number.isInteger(d[k]))
  );
}

/** A document of the hub that is a view of the watcher (anything else is no answer). */
export function isCiView(doc: unknown): doc is CiView {
  if (typeof doc !== 'object' || doc === null) return false;
  const d = doc as Record<string, unknown>;
  if (d.settings !== undefined && !isSettings(d.settings)) return false;
  switch (d.state) {
    case 'off':
      return typeof d.why === 'string';
    case 'waiting':
      return true;
    case 'ran':
      return (
        typeof d.at === 'number' &&
        Array.isArray(d.written) &&
        Array.isArray(d.not_written) &&
        Array.isArray(d.hints) &&
        (d.problem === null || typeof d.problem === 'string')
      );
    default:
      return false;
  }
}
