// The third-party notices of hub.html. MIT asks for the notice in all copies, and hub.html is handed on as one file,
// so the page carries it as a comment; the same text as a file goes next to it in hub-web. The versions come from the
// lockfile, so they cannot go stale:
//
//   node web/notices.mjs           (from ui/) write web/THIRD-PARTY-NOTICES.md
//   node web/notices.mjs --check   fail when that file is not what the lockfile gives (a test does the same)

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
export const NOTICES_FILE = join(here, 'THIRD-PARTY-NOTICES.md');

/** What the page bundles. `pkg` is the name in the lockfile, which gives the version. */
export const COMPONENTS = [
  { name: 'Svelte (client runtime)', pkg: 'svelte', license: 'MIT', copyright: 'Copyright (c) 2016-2025 Svelte Contributors.' },
  { name: 'clsx (dependency of the Svelte runtime)', pkg: 'clsx', license: 'MIT', copyright: 'Copyright (c) Luke Edwards (lukeed.com).' },
];

const MIT = `Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;

const literal = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * The version a package has in `pnpm-lock.yaml`. A package that is not there, or there twice, stops the build: which of
 * two versions the page bundles cannot be guessed.
 */
export function versionIn(lock, pkg) {
  const packages = lock.slice(lock.indexOf('\npackages:'), lock.indexOf('\nsnapshots:'));
  const found = [...packages.matchAll(new RegExp(`^  '?${literal(pkg)}@(\\d[^:'(]*)'?:`, 'gm'))].map((m) => m[1]);
  if (found.length !== 1) throw new Error(`notices: ${pkg} is ${found.length} times in the lockfile, expected once`);
  return found[0];
}

/** The components with their versions. */
export function resolved(lock) {
  return COMPONENTS.map((c) => ({ ...c, version: versionIn(lock, c.pkg) }));
}

/** The notices as plain text, without markup and without `--`: this is what goes into the comment of the page. */
export function noticesText(lock) {
  return [
    'Third-party notices. This page bundles:',
    ...resolved(lock).map((c) => `  ${c.name} ${c.version}, ${c.license}. ${c.copyright}`),
    '',
    'First-party code in the page, all MIT, Copyright (c) 2026 Stefan Bartl: hub-desktop, appkit.',
    '',
    'MIT License (all components above)',
    '',
    MIT,
  ].join('\n');
}

/** The notices as the file next to the page. */
export function noticesMarkdown(lock) {
  const all = resolved(lock);
  return `${[
    '# Third-party notices',
    '',
    '`hub.html` is built by [hub-desktop](https://github.com/WKDSteve/hub-desktop) and bundles the code below; the page',
    'also carries this text as a comment. The versions come from the lockfile of the build (`ui/web/notices.mjs`).',
    '',
    '| Component | Version | License |',
    '| --- | --- | --- |',
    ...all.map((c) => `| ${c.name} | ${c.version} | ${c.license} |`),
    '',
    'First-party code in the page, all MIT, Copyright (c) 2026 Stefan Bartl: hub-desktop (UI) and appkit (`appkit-ui`,',
    '`appkit-web`).',
    '',
    '## Copyright notices',
    '',
    ...all.map((c) => `- ${c.name}: ${c.copyright}`),
    '',
    '## MIT License (all components above)',
    '',
    MIT,
  ].join('\n')}\n`;
}

export const readLock = () => readFileSync(join(here, '..', 'pnpm-lock.yaml'), 'utf8').replaceAll('\r\n', '\n');

function main() {
  const text = noticesMarkdown(readLock());
  if (process.argv.includes('--check')) {
    if (readFileSync(NOTICES_FILE, 'utf8').replaceAll('\r\n', '\n') !== text) {
      console.error('notices: web/THIRD-PARTY-NOTICES.md is not what the lockfile gives: run node web/notices.mjs');
      process.exit(1);
    }
    return;
  }
  writeFileSync(NOTICES_FILE, text);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
