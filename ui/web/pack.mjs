// Packs the page of the web variant into one file.
//
//   pnpm web:build            (= vite build --mode web, then this)
//   node web/pack.mjs [--dist=<folder of vite build --mode web>] [--out=<folder>]    (from ui/)
//
// In:  dist-web/index.web.html with its one script and its one style (vite build --mode web).
// Out: <out>/hub.html and <out>/SHA256SUMS. Everything is in the file: script and style inline, and a
//      Content-Security-Policy as a meta tag that forbids every request (CSP_STRICT: no WASM, no worker). The page is
//      opened from disk (file://) and needs no server.
//
// The packing and the check are the kit's (appkit-web/pack). The build refuses a file whose shell loads anything: an
// element with a src or a href, a script that is not inline, a URL, a refresh or a missing policy. What the compiled
// script does at run time is checked by the browser test (e2e-web/), which opens the file and counts the requests.

import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assemble as assemblePage, CSP_STRICT, readDist as readKitDist, verifyShell as verifyKitShell, writePage } from 'appkit-web/pack';

const ui = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** The page may not request anything and starts no WASM and no worker. */
export const CSP = CSP_STRICT;

/** The single file. */
export function assemble({ css, js, build }) {
  return assemblePage({ css, js, csp: CSP, title: 'Hub', comment: `hub-desktop web variant, build ${build}` });
}

/** What the page without the contents of its scripts and styles may not hold; empty when it is fine. */
export function verifyShell(html) {
  return verifyKitShell(html);
}

/** The script and the style `vite build --mode web` made, found through its index. */
export function readDist(dist) {
  return readKitDist(dist, 'index.web.html');
}

function commit() {
  try {
    return execFileSync('git', ['-C', ui, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

function main() {
  const arg = (name, fallback) => {
    const given = process.argv.find((a) => a.startsWith(`--${name}=`));
    return resolve(given ? given.slice(name.length + 3) : fallback);
  };
  const dist = arg('dist', join(ui, 'dist-web'));
  const out = arg('out', dist);
  const { js, css } = readDist(dist);
  const html = assemble({ css, js, build: commit() });
  const problems = verifyShell(html);
  if (problems.length) {
    for (const p of problems) console.error('pack: ' + p);
    process.exit(1);
  }
  const sha = writePage(out, 'hub.html', html);
  console.log(`${sha}  ${join(out, 'hub.html')} (${html.length} bytes: script ${js.length}, style ${css.length})`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
