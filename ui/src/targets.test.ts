// A small button is hit with a finger or a shaky hand: its box must be at least 24 CSS px high (and 44 px on a touch
// screen). These buttons get their height from a thin padding only, which is about 21 px, so each of them also sets
// `min-height` (and the touch rule 2.75rem). This test reads the components and names a button that lost it.

import { describe, expect, it } from 'vitest';

const COMPONENTS = import.meta.glob('./**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

/** The small buttons by component: the selector of their rule. */
const SMALL: Record<string, string[]> = {
  './Tile.svelte': ['.do-update', '.more'],
  './Monitor.svelte': ['.toggle'],
  './Options.svelte': ['.save'],
};

/** The body of the first rule of `selector` (a plain `selector {`), or `''`. */
function ruleBody(source: string, selector: string): string {
  const start = source.indexOf(`\n  ${selector} {`);
  if (start < 0) return '';
  return source.slice(start, source.indexOf('}', start));
}

describe('small buttons have a hit area', () => {
  for (const [file, selectors] of Object.entries(SMALL)) {
    it(`${file}: ${selectors.join(', ')} have min-height 1.5rem and a touch rule of 2.75rem`, () => {
      const source = COMPONENTS[file] ?? '';
      expect(source).not.toBe('');
      for (const selector of selectors) expect(ruleBody(source, selector), selector).toMatch(/min-height:\s*1\.5rem/);
      expect(source).toMatch(/@media \(pointer: coarse\)[^]*min-height:\s*2\.75rem/);
    });
  }
});
