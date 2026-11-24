// The fixed sentences the hub itself sends as a reason (`problem`, `why`): they are formulated in the Rust code, as
// `&'static str`, so the set is finite. The page knows them here and shows them in its language; a sentence it does
// not know (an older or newer hub) is shown as the hub sent it. `hubtext.test.ts` reads the Rust sources and fails
// when a sentence of them is not listed here.

import { msg, t } from './i18n.svelte';

const KNOWN: string[] = [
  // The export folder of testing.nvim (monitor.rs).
  msg('the export folder has no testing-export.json'),
  msg('a document of the export is bigger than the hub reads'),
  msg('the export cannot be read'),
  msg('a document of the export is not JSON'),
  msg('testing-export.json is not a manifest of testing-export'),
  msg('the export is of a newer schema than this hub reads'),
  msg('the manifest does not list fleet.json'),
  msg('fleet.json is not the size the manifest says (an export in the middle of a write?)'),
  msg('fleet.json is not a fleet of the export form'),
  msg('fleet.json was made without asking the data classes'),
  msg('the export has no document for this repository'),
  msg('the document of the repository is not the size the manifest says (an export in the middle of a write?)'),
  msg('the document of the repository is not one of the export form'),
  // The CI watcher (ciwatch.rs).
  msg('config.json turns it off (ci_watch: false)'),
  msg('the hub runs for a test'),
  msg('the state of the watcher is lost'),
  msg('REPOS_DIR names no folder'),
  msg('TASKS_VAULT names no folder'),
  msg('the document names no repository'),
  msg('nvim is not on the PATH'),
  msg('git is not on the PATH'),
  msg('no checkout of tasks.nvim in REPOS_DIR'),
  msg('no checkout of testing.nvim in REPOS_DIR'),
  msg('the tasks command gave no JSON'),
  msg('the tasks command gave no tasks.finished document'),
  msg('testing gave no JSON'),
  msg('testing gave no testing-ci-remote/1 document'),
  msg('the document names no commit of origin/main'),
  msg('the tasks command failed'),
  msg('the tasks command did not start'),
  msg('testing did not start'),
  msg('the round broke off'),
  msg('testing gave no run of the commit itself'),
  // The runners (runners.rs).
  msg('the runners are looked at on the private machine only'),
  msg('pwsh is not on the PATH'),
  msg('no scripts/runners.ps1 in NVIM_CONFIG_DIR'),
  msg('the runners are locked'),
  msg('runners.ps1 did not end in time'),
  msg('runners.ps1 gave no status'),
  msg('pwsh did not start'),
  msg('no runner of that name is stopped here'),
  msg('a start of that runner is on its way'),
];

const SET = new Set(KNOWN);

/** The sentences of the hub the page translates. */
export const HUB_SENTENCES: readonly string[] = KNOWN;

/** A reason of the hub in the language of the page; one the page does not know stays as the hub sent it. */
export function hubText(text: string): string {
  return SET.has(text) ? t(text) : text;
}
