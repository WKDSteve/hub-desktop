import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assemble as assemblePage } from 'appkit-web/pack';
import { assemble, CSP, verifyShell } from './pack.mjs';

const page = () => assemble({ css: 'body{margin:0}', js: 'console.log(1)', build: 'abc1234' });

test('the page is one file with the strict policy and nothing it loads', () => {
  const html = page();
  assert.deepEqual(verifyShell(html), []);
  assert.match(html, /connect-src 'none'/);
  assert.doesNotMatch(CSP, /wasm|worker/);
});

test('a script in the script is not the end of the file', () => {
  const html = assemble({ css: '', js: 'const s = "</script><b>"', build: 'x' });
  assert.deepEqual(verifyShell(html), []);
});

test('a shell that loads something is refused', () => {
  const html = page();
  assert.ok(verifyShell(html.replace('<body>', '<body><img src="x.png">')).length > 0);
  assert.ok(verifyShell(html.replace('<body>', '<body><script src="x.js"></script>')).length > 0);
  assert.ok(verifyShell(html.replace('<body>', '<body><a href="https://example.org/">x</a>')).length > 0);
  assert.ok(verifyShell(html.replace("connect-src 'none'", "connect-src 'self'")).length > 0);
});

test('a policy that allows more than the strict one is refused when packing', () => {
  assert.throws(() => assemblePage({ css: '', js: '', csp: "default-src 'none'", title: 'x', comment: 'x' }));
});
