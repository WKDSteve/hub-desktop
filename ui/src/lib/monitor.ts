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

export type CaseStatus = 'fail' | 'error' | 'timeout' | 'crash' | 'xpass';

/** One red case of the last local run, as `GET /api/hub/monitor/cases` tells it (texts redacted by testing.nvim). */
export interface RedCase {
  file: string;
  line: number | null;
  status: CaseStatus;
  name: string;
  describe: string[];
  message: string | null;
  expected: string | null;
  actual: string | null;
  flaky: boolean;
}

export type CasesReading =
  | { state: 'not_configured' }
  | { state: 'problem'; problem: string }
  | {
      state: 'ready';
      key: string;
      local_read: boolean;
      cases_red: number | null;
      cases_omitted: number | null;
      cases: RedCase[];
    };

/** What the page shows for the cases of a repository: the reading, or that it is under way or failed. */
export type CasesView = CasesReading | { state: 'loading' } | { state: 'failed' };

/** What a status says, in words. */
export const STATUS_WORDS: Record<CaseStatus, string> = {
  fail: 'failed',
  error: 'error',
  timeout: 'timed out',
  crash: 'crashed',
  xpass: 'passed unexpectedly',
};

/** A key the hub reads a document for (`plain_key` of `monitor.rs`); it is also the id of the list of cases. */
const PLAIN_KEY = /^[A-Za-z0-9_-][A-Za-z0-9._-]{0,119}$/;

/** A row whose last local run was red has cases to show, if its key is one the hub reads. */
export function hasCases(repo: RepoLight): boolean {
  return repo.local === 'red' && PLAIN_KEY.test(repo.key);
}

/** Where a case is: `file:line`, or the file alone. */
export function where(c: RedCase): string {
  return c.line ? `${c.file}:${c.line}` : c.file;
}

/** The describe titles and the name of a case, outermost first. */
export function title(c: RedCase): string {
  return [...c.describe, c.name].filter((s) => s !== '').join(' › ') || '(no name)';
}

/** The sentence under the cases when some are not shown; `null` when all are. */
export function omitted(reading: Extract<CasesReading, { state: 'ready' }>): string | null {
  const n = reading.cases_omitted;
  if (!n) return null;
  return n === 1 ? '1 more red case is not shown.' : `${n} more red cases are not shown.`;
}

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
