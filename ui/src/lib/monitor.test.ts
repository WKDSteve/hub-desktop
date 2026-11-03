import { describe, expect, it } from 'vitest';
import { hasCases, LIGHTS, omitted, parts, type RedCase, type RepoLight, stamped, title, where } from './monitor';

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

describe('the red cases', () => {
  const c = (over: Partial<RedCase> = {}): RedCase => ({
    file: 'TESTS/x_spec.lua',
    line: 12,
    status: 'fail',
    name: 'reads a file',
    describe: ['parser', 'open'],
    message: null,
    expected: null,
    actual: null,
    flaky: false,
    ...over,
  });

  it('says where a case is and what it is called', () => {
    expect(where(c())).toBe('TESTS/x_spec.lua:12');
    expect(where(c({ line: null }))).toBe('TESTS/x_spec.lua');
    expect(title(c())).toBe('parser › open › reads a file');
    expect(title(c({ describe: [], name: '' }))).toBe('(no name)');
  });

  it('offers the cases of a row whose last local run was red, and says how many are not shown', () => {
    const row = { key: 'k', name: 'n', light: 'red', local: 'red', fail: 1, stale: 0, ci_local: null, ci_remote: null, ts: null } as const;
    expect(hasCases(row)).toBe(true);
    expect(hasCases({ ...row, local: 'green', light: 'green' })).toBe(false);
    expect(hasCases({ ...row, local: null })).toBe(false);
    for (const key of ['a b', '../a', '.a', '', 'k'.repeat(121)]) expect(hasCases({ ...row, key }), key).toBe(false);
    const ready = { state: 'ready' as const, key: 'k', local_read: true, cases_red: 3, cases: [] as RedCase[] };
    expect(omitted({ ...ready, cases_omitted: 0 })).toBeNull();
    expect(omitted({ ...ready, cases_omitted: null })).toBeNull();
    expect(omitted({ ...ready, cases_omitted: 1 })).toBe('1 more red case is not shown.');
    expect(omitted({ ...ready, cases_omitted: 4 })).toBe('4 more red cases are not shown.');
  });
});
