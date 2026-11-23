// What the page says about updates: plain functions, no DOM. The hub decides what can be updated and runs it; the
// page only words its codes.

import type { HubApp, UpdateInfo, UpdateJob, UpdatePart } from './api';
import { msg, t } from './i18n.svelte';

/** The codes of the hub, in words. An unknown code is shown as it is (with spaces), never dropped. */
const REASONS: Record<string, string> = {
  workstation: msg('not on the workstation, where nothing unsigned is installed'),
  role_unclear: msg('the machine role is unclear (NVIM_MACHINE_ROLE)'),
  windows_only: msg('programs are only updated on Windows'),
  no_release: msg('the app has no release to install from'),
  no_release_file: msg('its release names no file the hub installs'),
  no_gh: msg('the GitHub CLI (gh) was not found'),
  no_installer: msg('the hub does not know how to install its release'),
  no_git: msg('git was not found'),
  not_installed: msg('nothing of it is installed'),
  not_a_clone: msg('its folder is not a git clone'),
  not_on_main: msg('the clone is not on main'),
  dirty: msg('the clone has local changes'),
  git_failed: msg('git failed'),
  pull_failed: msg('the pull failed (offline, or no fast-forward)'),
  sums_mismatch: msg('the page does not match SHA256SUMS'),
  download_failed: msg('the release could not be downloaded'),
  checksum_failed: msg('a file of the release does not match SHA256SUMS'),
  install_failed: msg('the install failed'),
  attestation_failed: msg('its attestation does not verify (not built by the workflow of its repository)'),
  attestation_required: msg('its attestation could not be checked, and one is required (update.require_attestation)'),
  close_first: msg('it is running: close it first'),
  busy: msg('a start or stop of it is on its way'),
  no_work_folder: msg('there is no folder for the download'),
  not_ready: msg('it is not ready'),
  not_found: msg('it is not in the catalog'),
};

export function reasonText(code: string): string {
  const known = REASONS[code];
  return known ? t(known) : code.replaceAll('_', ' ');
}

/** Whether a part of the app can be updated here. */
export function updatable(info: UpdateInfo | undefined): boolean {
  return info?.native === 'ready' || info?.web === 'ready';
}

/** Whether an update of the app is queued or running. */
export function active(job: UpdateJob | null | undefined): boolean {
  return job?.state === 'queued' || job?.state === 'running';
}

/** Why nothing of the app can be updated here, for the hint of a disabled entry. */
export function whyNot(info: UpdateInfo | undefined): string {
  const code = [info?.native, info?.web].find((c): c is string => typeof c === 'string' && c !== 'ready');
  return t('Cannot be updated here: {why}', { why: reasonText(code ?? 'not_installed') });
}

const STEPS: Record<string, string> = {
  looking: msg('Looking…'),
  checking: msg('Checking the clone…'),
  pulling: msg('Pulling…'),
  downloading: msg('Downloading…'),
  verifying: msg('Checking the sums…'),
  attesting: msg('Checking the attestation…'),
  installing: msg('Installing…'),
};

export type JobKind = 'active' | 'ok' | 'failed' | 'skipped';

/** One line for the tile and the list: where the update is, or what came of it. */
export function jobLabel(job: UpdateJob): { kind: JobKind; text: string } {
  switch (job.state) {
    case 'queued':
      return { kind: 'active', text: t('Update queued') };
    case 'running':
      return { kind: 'active', text: (job.step && STEPS[job.step] && t(STEPS[job.step] as string)) || t('Updating…') };
    case 'failed': {
      const failed = job.parts.find((p) => p.outcome === 'failed');
      return { kind: 'failed', text: t('Update failed: {why}', { why: reasonText(failed?.result ?? 'not_ready') }) };
    }
    case 'skipped': {
      const first = job.parts[0];
      return { kind: 'skipped', text: t('Not updated: {why}', { why: reasonText(first?.result ?? 'not_installed') }) };
    }
    default: {
      const ok = job.parts.filter((p) => p.outcome === 'ok');
      const changed = ok.some((p) => p.result !== 'up_to_date');
      if (job.reload) return { kind: 'ok', text: t('Updated: reload its open page') };
      return { kind: 'ok', text: changed ? t('Updated') : t('Up to date') };
    }
  }
}

/** What one part came to, in words. */
export function partText(part: UpdatePart): string {
  const what = part.part === 'native' ? t('Program') : t('Web page');
  if (part.outcome === 'ok') {
    const word =
      part.result === 'up_to_date' ? t('up to date') : part.result === 'installed' ? t('installed') : t('updated');
    return `${what}: ${word}`;
  }
  return `${what}: ${part.outcome === 'skipped' ? t('not updated') : t('failed')}, ${reasonText(part.result)}`;
}

/**
 * What the check of the attestation of the program came to, in words; `null` when the job did not get that far.
 * An unknown word of a newer hub is shown as it is, never dropped.
 */
export function attestationText(job: UpdateJob): string | null {
  switch (job.attestation ?? null) {
    case null:
      return null;
    case 'attested':
      return t('Attestation: verified, built by the workflow of its repository');
    case 'not_checked':
      return t('Attestation: not checked (no attestation, or gh could not ask); see the output');
    case 'failed':
      return t('Attestation: does not verify, nothing was installed');
    default:
      return t('Attestation: {what}', { what: String(job.attestation).replaceAll('_', ' ') });
  }
}

/** The message of a finished round of updates. */
export function roundText(jobs: readonly { name: string; job: UpdateJob }[]): string {
  const failed = jobs.filter((j) => j.job.state === 'failed');
  if (jobs.length === 1 && jobs[0]) return `${jobs[0].name}: ${jobLabel(jobs[0].job).text}.`;
  if (failed.length === 0) return t('{n} apps updated.', { n: jobs.length });
  const names = failed.map((j) => j.name).join(', ');
  return t('{done} of {n} apps updated; failed: {names}.', { done: jobs.length - failed.length, n: jobs.length, names });
}

/**
 * The web hub cannot run anything: the lines a person types instead, in the folder of the clones. Only for a web
 * variant on this machine (its link `../<dir>/<page>` names the clone); `null` for anything else.
 */
export function webUpdateLines(app: HubApp): string[] | null {
  const dir = /^\.\.\/([A-Za-z0-9._-]+)\/[^/]+$/.exec(app.href ?? '')?.[1];
  if (!app.installed || !dir) return null;
  return [`git -C ${dir} pull --ff-only`, `git -C ${dir} status --short`];
}
