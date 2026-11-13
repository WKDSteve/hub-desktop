// What the page says about updates: plain functions, no DOM. The hub decides what can be updated and runs it; the
// page only words its codes.

import type { HubApp, UpdateInfo, UpdateJob, UpdatePart } from './api';

/** The codes of the hub, in words. An unknown code is shown as it is (with spaces), never dropped. */
const REASONS: Record<string, string> = {
  workstation: 'not on the workstation, where nothing unsigned is installed',
  role_unclear: 'the machine role is unclear (NVIM_MACHINE_ROLE)',
  windows_only: 'programs are only updated on Windows',
  no_release: 'the app has no release to install from',
  no_release_file: 'its release names no file the hub installs',
  no_gh: 'the GitHub CLI (gh) was not found',
  no_powershell: 'Windows PowerShell was not found',
  no_git: 'git was not found',
  not_installed: 'nothing of it is installed',
  not_a_clone: 'its folder is not a git clone',
  not_on_main: 'the clone is not on main',
  dirty: 'the clone has local changes',
  git_failed: 'git failed',
  pull_failed: 'the pull failed (offline, or no fast-forward)',
  sums_mismatch: 'the page does not match SHA256SUMS',
  download_failed: 'the release could not be downloaded',
  checksum_failed: 'a file of the release does not match SHA256SUMS',
  install_failed: 'the install script failed',
  attestation_failed: 'its attestation does not verify (not built by the workflow of its repository)',
  attestation_required: 'its attestation could not be checked, and one is required (update.require_attestation)',
  close_first: 'it is running: close it first',
  busy: 'a start or stop of it is on its way',
  no_work_folder: 'there is no folder for the download',
  not_ready: 'it is not ready',
  not_found: 'it is not in the catalog',
};

export function reasonText(code: string): string {
  return REASONS[code] ?? code.replaceAll('_', ' ');
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
  return `Cannot be updated here: ${reasonText(code ?? 'not_installed')}`;
}

const STEPS: Record<string, string> = {
  looking: 'Looking…',
  checking: 'Checking the clone…',
  pulling: 'Pulling…',
  downloading: 'Downloading…',
  verifying: 'Checking the sums…',
  attesting: 'Checking the attestation…',
  installing: 'Installing…',
};

export type JobKind = 'active' | 'ok' | 'failed' | 'skipped';

/** One line for the tile and the list: where the update is, or what came of it. */
export function jobLabel(job: UpdateJob): { kind: JobKind; text: string } {
  switch (job.state) {
    case 'queued':
      return { kind: 'active', text: 'Update queued' };
    case 'running':
      return { kind: 'active', text: (job.step && STEPS[job.step]) || 'Updating…' };
    case 'failed': {
      const failed = job.parts.find((p) => p.outcome === 'failed');
      return { kind: 'failed', text: `Update failed: ${reasonText(failed?.result ?? 'not_ready')}` };
    }
    case 'skipped': {
      const first = job.parts[0];
      return { kind: 'skipped', text: `Not updated: ${reasonText(first?.result ?? 'not_installed')}` };
    }
    default: {
      const ok = job.parts.filter((p) => p.outcome === 'ok');
      const changed = ok.some((p) => p.result !== 'up_to_date');
      if (job.reload) return { kind: 'ok', text: 'Updated: reload its open page' };
      return { kind: 'ok', text: changed ? 'Updated' : 'Up to date' };
    }
  }
}

/** What one part came to, in words. */
export function partText(part: UpdatePart): string {
  const what = part.part === 'native' ? 'Program' : 'Web page';
  if (part.outcome === 'ok') {
    const word = part.result === 'up_to_date' ? 'up to date' : part.result === 'installed' ? 'installed' : 'updated';
    return `${what}: ${word}`;
  }
  return `${what}: ${part.outcome === 'skipped' ? 'not updated' : 'failed'}, ${reasonText(part.result)}`;
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
      return 'Attestation: verified, built by the workflow of its repository';
    case 'not_checked':
      return 'Attestation: not checked (no attestation, or gh could not ask); see the output';
    case 'failed':
      return 'Attestation: does not verify, nothing was installed';
    default:
      return `Attestation: ${String(job.attestation).replaceAll('_', ' ')}`;
  }
}

/** The message of a finished round of updates. */
export function roundText(jobs: readonly { name: string; job: UpdateJob }[]): string {
  const failed = jobs.filter((j) => j.job.state === 'failed');
  if (jobs.length === 1 && jobs[0]) return `${jobs[0].name}: ${jobLabel(jobs[0].job).text}.`;
  if (failed.length === 0) return `${jobs.length} apps updated.`;
  const names = failed.map((j) => j.name).join(', ');
  return `${jobs.length - failed.length} of ${jobs.length} apps updated; failed: ${names}.`;
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
