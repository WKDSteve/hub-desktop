import { describe, expect, it } from 'vitest';
import { LIGHTS, parts, type RepoLight, stamped } from './monitor';

const repo = (over: Partial<RepoLight> = {}): RepoLight => ({
  key: 'a-1',
  name: 'a',
  light: 'unknown',
  local: null,
  fail: null,
  stale: null,
  ci_local: null,
  ci_remote: null,
  ts: null,
  ...over,
});

describe('parts', () => {
  it('says what each source says', () => {
    expect(parts(repo({ local: 'red', fail: 3, stale: 2, ci_local: 'infra', ci_remote: 'waiting_runner' }), true)).toEqual([
      'local red (3 failing)',
      '2 outdated',
      'pipeline infrastructure',
      'CI waiting runner',
    ]);
  });

  it('does not say 0 failing for a result that is green, nor for one that is unknown', () => {
    expect(parts(repo({ local: 'green', fail: 0 }), true)).toEqual(['local green']);
    expect(parts(repo({ local: 'red', fail: null }), true)).toEqual(['local red']);
  });

  it('says an export from a machine without local runs does not know the local result', () => {
    expect(parts(repo({ ci_remote: 'success' }), false)).toEqual(['local not read here', 'CI success']);
    expect(parts(repo(), true)).toEqual(['nothing recorded']);
  });
});

describe('lights', () => {
  it('has a word and a mark for each, so colour is never the only signal', () => {
    for (const { word, mark } of Object.values(LIGHTS)) expect(word && mark).toBeTruthy();
    expect(new Set(Object.values(LIGHTS).map((l) => l.mark)).size).toBe(4);
  });

  it('writes the time of the export', () => {
    expect(stamped('2026-11-01T10:00:00Z')).toBe('Export of 2026-11-01 10:00:00 UTC');
    expect(stamped(null)).toBe('Export of an unknown time');
  });
});
