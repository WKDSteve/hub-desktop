import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { COMPONENTS, NOTICES_FILE, noticesMarkdown, noticesText, readLock, versionIn } from './notices.mjs';
import { assemble } from './pack.mjs';

test('the notices file next to the page is what the lockfile gives', () => {
  assert.equal(readFileSync(NOTICES_FILE, 'utf8').replaceAll('\r\n', '\n'), noticesMarkdown(readLock()));
});

test('every bundled component is named with its version from the lockfile', () => {
  const lock = readLock();
  const text = noticesText(lock);
  for (const c of COMPONENTS) assert.ok(text.includes(`${c.name} ${versionIn(lock, c.pkg)}`), c.name);
  assert.match(text, /Permission is hereby granted/);
});

test('the page carries the notices as a comment that does not end early', () => {
  const html = assemble({ css: '', js: '', build: 'abc1234' });
  assert.ok(html.includes(noticesText(readLock())));
  assert.doesNotMatch(noticesText(readLock()), /--/);
});

test('a package that is missing or there twice stops the build', () => {
  const lock = "\npackages:\n\n  a@1.0.0:\n  a@2.0.0:\n\nsnapshots:\n";
  assert.throws(() => versionIn(lock, 'a'), /2 times/);
  assert.throws(() => versionIn(lock, 'b'), /0 times/);
});
