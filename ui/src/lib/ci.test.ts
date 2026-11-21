import { describe, expect, it } from 'vitest';
import { CI_NONE, type CiSettings, type CiView, ciLines, ciStatus, hintText, intervalProblem, isCiView, settingsOf } from './ci';

const SETTINGS: CiSettings = { on: true, interval_min: 5, min: 1, max: 60, default: 5 };

const ran = (over: Partial<Extract<CiView, { state: 'ran' }>> = {}): CiView => ({
  state: 'ran',
  at: 1_000_000,
  written: [],
  not_written: [],
  hints: [],
  problem: null,
  ...over,
});

describe('hints', () => {
  it('says each hint in words, with its repository', () => {
    expect(hintText({ kind: 'queued', repo: 'tasks.nvim', minutes: 42 })).toBe(
      'tasks.nvim: the CI run has waited 42 min for a runner',
    );
    expect(hintText({ kind: 'behind', repo: 'a' })).toContain('not fetched');
    expect(hintText({ kind: 'no_checkout', repo: 'a' })).toBe('a: no checkout below REPOS_DIR');
    expect(hintText({ kind: 'no_github', repo: 'a' })).toContain('no hosted CI');
    expect(hintText({ kind: 'no_answer', repo: 'a', why: 'testing gave no JSON' })).toBe(
      'a: no usable answer (testing gave no JSON)',
    );
  });

  it('lists the problem, the refused verdicts and the hints of the last round, as many as the icon counts', () => {
    const view = ran({
      problem: 'the tasks command failed',
      not_written: [{ id: 'a/b', ci: 'red' }],
      written: [{ id: 'a/c', ci: 'green' }],
      hints: [{ kind: 'no_github', repo: 'x' }],
    });
    expect(ciLines(view)).toEqual([
      'The round did not get to the tasks: the tasks command failed',
      'a/b: the tasks command did not take ci=red',
      'x: no hosted CI (its origin is not on GitHub)',
    ]);
    for (const quiet of [CI_NONE, { state: 'waiting' } as CiView, { state: 'off', why: 'x' } as CiView, ran()]) {
      expect(ciLines(quiet)).toEqual([]);
    }
  });
});

describe('status', () => {
  it('says what the watcher is at', () => {
    expect(ciStatus(CI_NONE)).toBeNull();
    expect(ciStatus({ state: 'off', why: 'the hub runs for a test' })).toBe(
      'The CI watcher does not run: the hub runs for a test.',
    );
    expect(ciStatus({ state: 'waiting' })).toContain('not finished a round');
    const now = new Date(1_000_000 * 1000 + 5 * 60 * 1000);
    expect(ciStatus(ran({ written: [{ id: 'a/b', ci: 'green' }] }), now)).toBe(
      'CI watcher: last round 5 min ago, 1 verdict written.',
    );
    expect(ciStatus(ran(), new Date(1_000_000 * 1000))).toBe(
      'CI watcher: last round less than a minute ago, 0 verdicts written.',
    );
  });

  it('takes only a view of the watcher from the hub', () => {
    expect(isCiView(ran())).toBe(true);
    expect(isCiView({ state: 'off', why: 'x' })).toBe(true);
    expect(isCiView({ state: 'waiting' })).toBe(true);
    expect(isCiView({ state: 'waiting', settings: SETTINGS })).toBe(true);
    for (const bad of [
      null,
      'off',
      {},
      { state: 'off' },
      { state: 'ran', at: 1 },
      { state: 'none' },
      { state: 'waiting', settings: { ...SETTINGS, interval_min: '5' } },
      { state: 'waiting', settings: { ...SETTINGS, on: 'yes' } },
      { state: 'waiting', settings: { ...SETTINGS, max: 2.5 } },
    ]) {
      expect(isCiView(bad)).toBe(false);
    }
  });
});

describe('options', () => {
  it('takes a whole number of minutes within the bounds only', () => {
    for (const ok of ['1', '5', ' 60 ']) expect(intervalProblem(ok, SETTINGS)).toBeNull();
    for (const out of ['0', '61', '600']) expect(intervalProblem(out, SETTINGS)).toBe('The interval is 1 to 60 minutes.');
    for (const bad of ['', 'five', '2.5', '-3', '1e2']) {
      expect(intervalProblem(bad, SETTINGS)).toBe('A whole number of minutes, 1 to 60 minutes.');
    }
  });

  it('has settings only from a hub that sent them', () => {
    expect(settingsOf(CI_NONE)).toBeNull();
    expect(settingsOf({ state: 'waiting' })).toBeNull();
    expect(settingsOf({ state: 'off', why: 'x', settings: SETTINGS })).toEqual(SETTINGS);
  });
});
