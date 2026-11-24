// A browser's page translation rewrites text it takes for prose. App names, repo names, file names, commands and the
// output of a test run must stay as they are. Every `<code>`, `<kbd>` and `<pre>` of the page, and every element with the
// class `name` or `where`, therefore says `translate="no"`. This test reads every component and names the element that
// forgot it. The search field also says `spellcheck="false"`: what is typed there is a name, not a sentence.

import { describe, expect, it } from 'vitest';

const COMPONENTS = import.meta.glob('./**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

/** The opening tags that must not be translated. */
function tagsOf(source: string): string[] {
  const tags = [...source.matchAll(/<(?:code|kbd|pre)\b[^>]*>/g)].map((m) => m[0]);
  const named = [...source.matchAll(/<span\b[^>]*class="(?:name|where)"[^>]*>/g)].map((m) => m[0]);
  return [...tags, ...named];
}

describe('names and code are not translated', () => {
  it('finds the components', () => {
    expect(Object.keys(COMPONENTS).length).toBeGreaterThan(3);
  });

  for (const [file, source] of Object.entries(COMPONENTS)) {
    const tags = tagsOf(source);
    if (tags.length === 0) continue;
    it(`${file}: every name, code and pre says translate="no"`, () => {
      expect(tags.filter((tag) => !/\btranslate="no"/.test(tag))).toEqual([]);
    });
  }

  it('the filter field is not spell checked', () => {
    const app = COMPONENTS['./App.svelte'] ?? '';
    const search = [...app.matchAll(/<input\b[^>]*type="search"[^>]*>/g)].map((m) => m[0]);
    expect(search.length).toBeGreaterThan(0);
    expect(search.filter((tag) => !/\bspellcheck="false"/.test(tag))).toEqual([]);
  });
});
