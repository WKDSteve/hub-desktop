import { afterEach, describe, expect, it } from 'vitest';
import { ciLines, ciStatus } from './ci';
import { HUB_SENTENCES, hubText } from './hubtext';
import { setLang } from './i18n.svelte';

// The Rust sources that formulate the reasons the page shows (the sentences are `&'static str`).
const RUST = import.meta.glob<string>(
  [
    '../../../crates/hub-desktop/src/monitor.rs',
    '../../../crates/hub-desktop/src/ciwatch.rs',
    '../../../crates/hub-desktop/src/runners.rs',
  ],
  { query: '?raw', import: 'default', eager: true },
);

/** The sentences of the hub in its sources: a literal with a space in the places that make a reason. */
function rustSentences(): Set<string> {
  const found = new Set<string>();
  const patterns = [
    /pub const [A-Z_]+: &str =\s*"([^"]*)"/g,
    /\bErr\(\s*"([^"]*)"/g,
    /\bSome\(\s*"([^"]*)"/g,
    /\bproblem(?: =|:)\s*"([^"]*)"/g,
    /\bwhy:\s*"([^"]*)"/g,
    /\bok_or\(\s*"([^"]*)"/g,
    /\bmap_err\(\|_\|\s*"([^"]*)"/g,
    /\bOff\(\s*"([^"]*)"/g,
  ];
  for (const source of Object.values(RUST)) {
    // The tests of the file speak of other things (fixtures).
    const code = source.split('#[cfg(test)]')[0] as string;
    for (const pattern of patterns) {
      for (const m of code.matchAll(pattern))
        if ((m[1] as string).includes(' ')) found.add(m[1] as string);
    }
  }
  return found;
}

afterEach(() => setLang('en'));

describe('the sentences of the hub', () => {
  it('finds the sentences of the sources (a scan that finds nothing proves nothing)', () => {
    const found = rustSentences();
    expect(found.size).toBeGreaterThan(25);
    expect(found.has('the tasks command failed')).toBe(true);
    expect(found.has('the export cannot be read')).toBe(true);
  });

  it('knows every sentence the Rust sources send as a reason', () => {
    expect(
      [...rustSentences()].filter((s) => !HUB_SENTENCES.includes(s)),
    ).toEqual([]);
  });

  it('lists no sentence the sources no longer send', () => {
    const found = rustSentences();
    expect(HUB_SENTENCES.filter((s) => !found.has(s))).toEqual([]);
  });
});

describe('hubText', () => {
  it('is the sentence in English, and German after setLang', () => {
    expect(hubText('the tasks command failed')).toBe(
      'the tasks command failed',
    );
    setLang('de');
    expect(hubText('the tasks command failed')).toBe(
      'der tasks-Befehl ist fehlgeschlagen',
    );
  });

  it('shows a sentence it does not know as the hub sent it', () => {
    setLang('de');
    expect(hubText('something a newer hub says')).toBe(
      'something a newer hub says',
    );
  });

  it('is what the CI lines and the status show of a reason', () => {
    setLang('de');
    const ran = {
      state: 'ran',
      at: 0,
      written: [],
      not_written: [],
      hints: [{ kind: 'no_answer', repo: 'y', why: 'testing gave no JSON' }],
      problem: 'the tasks command failed',
    } as unknown as Parameters<typeof ciLines>[0];
    expect(ciLines(ran).join('\n')).toContain(
      'der tasks-Befehl ist fehlgeschlagen',
    );
    expect(ciLines(ran).join('\n')).toContain('testing lieferte kein JSON');
    expect(
      ciStatus({
        state: 'off',
        why: 'the hub runs for a test',
      } as unknown as Parameters<typeof ciStatus>[0]),
    ).toContain('der Hub läuft für einen Test');
  });
});
