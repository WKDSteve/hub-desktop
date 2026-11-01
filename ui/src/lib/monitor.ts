// The monitor of the page: the light of each repository of the testing export, in words. The hub reduces the document
// to a few words of its own (`monitor.rs`); this file turns them into the text of a row. Colour is never the only
// signal: every light has a word and a shape.

export type Light = 'red' | 'amber' | 'green' | 'unknown';

/** One repository as `GET /api/hub/monitor` tells it. */
export interface RepoLight {
  key: string;
  name: string;
  light: Light;
  /** The last local result: `green`, `green_partial`, `red`; `null`: none (or not read on this machine). */
  local: 'green' | 'green_partial' | 'red' | null;
  fail: number | null;
  stale: number | null;
  ci_local: 'green' | 'green_partial' | 'red' | 'error' | 'infra' | null;
  /** A state word of the contract; `null`: the CI was not asked. */
  ci_remote: string | null;
  ts: string | null;
}

export type MonitorReading =
  | { state: 'not_configured' }
  | { state: 'problem'; problem: string }
  | { state: 'ready'; generated_at: string | null; local_read: boolean; repos: RepoLight[] };

export const NOT_CONFIGURED: MonitorReading = { state: 'not_configured' };

/** What the word of a light says, and the mark drawn before it. */
export const LIGHTS: Record<Light, { word: string; mark: string }> = {
  red: { word: 'Red', mark: '●' },
  amber: { word: 'Check', mark: '◐' },
  green: { word: 'Green', mark: '○' },
  unknown: { word: 'No result', mark: '–' },
};

const LOCAL = { green: 'green', green_partial: 'partly run', red: 'red' } as const;
const PIPELINE = { green: 'green', green_partial: 'partly run', red: 'red', error: 'error', infra: 'infrastructure' } as const;

/** The CI state as a person says it. */
function remote(state: string): string {
  return state.replace(/_/g, ' ');
}

/** The parts of the line under a repository: what each source says. */
export function parts(repo: RepoLight, localRead: boolean): string[] {
  const out: string[] = [];
  if (repo.local) {
    const fails = repo.local === 'red' && repo.fail ? ` (${repo.fail} failing)` : '';
    out.push(`local ${LOCAL[repo.local]}${fails}`);
    if (repo.stale) out.push(`${repo.stale} outdated`);
  } else if (!localRead) {
    out.push('local not read here');
  }
  if (repo.ci_local) out.push(`pipeline ${PIPELINE[repo.ci_local]}`);
  if (repo.ci_remote) out.push(`CI ${remote(repo.ci_remote)}`);
  if (out.length === 0) out.push('nothing recorded');
  return out;
}

/** The sentence over the rows. */
export function stamped(generated: string | null): string {
  return generated ? `Export of ${generated.replace('T', ' ').replace('Z', ' UTC')}` : 'Export of an unknown time';
}
