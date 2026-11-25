// The options form can be sent while it is wrong: the field says what is wrong and the focus goes there, instead of a
// button that is locked for no visible reason. A page with an unsaved change asks before the tab is left. This test
// reads the components, so a form added later does not forget it.

import { describe, expect, it } from 'vitest';

const COMPONENTS = import.meta.glob('./**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('forms', () => {
  it('has components to read', () => {
    expect(Object.keys(COMPONENTS).length).toBeGreaterThan(4);
  });

  it('no submit button is locked for a wrong field or for a request on its way', () => {
    const locked = Object.entries(COMPONENTS).flatMap(([file, source]) =>
      [...source.matchAll(/<button\b[^>]*type="submit"[^>]*>/g)]
        .filter((m) => /\bdisabled=\{[^}]*(problem|writing|saving|trim\(\))/.test(m[0]))
        .map((m) => `${file}: ${m[0]}`),
    );
    expect(locked).toEqual([]);
  });

  it('every form names its checks itself (novalidate)', () => {
    const open = Object.entries(COMPONENTS).flatMap(([file, source]) =>
      [...source.matchAll(/<form\b[^>]*>/g)].filter((m) => !/novalidate/.test(m[0])).map((m) => `${file}: ${m[0]}`),
    );
    expect(open).toEqual([]);
  });

  it('the options ask before the tab is left while a change is not saved', () => {
    expect(COMPONENTS['./Options.svelte']).toMatch(/warnBeforeUnload\(\)/);
  });

  it('the form helpers are the ones of the ui kit, not a copy of the app', () => {
    const options = COMPONENTS['./Options.svelte'];
    expect(options).toMatch(/import \{ focusFirstInvalid \} from 'appkit-ui\/formErrors'/);
    expect(options).toMatch(/import \{ warnBeforeUnload \} from 'appkit-ui\/unsaved'/);
  });
});
