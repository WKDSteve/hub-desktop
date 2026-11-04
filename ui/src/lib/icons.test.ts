import { describe, expect, it } from 'vitest';
import { iconOf, logo, logoFor } from './icons';

const app = (id: string, default_mode: 'native' | 'web' = 'native', href: string | null = null) => ({ id, default_mode, href });
const named = (url: string | null) => url?.replace(/^.*\//, '');

describe('iconOf', () => {
  it('has an icon for every app of the catalog, in the way its tile opens it', () => {
    expect(named(iconOf(app('tasks')))).toBe('tasks-desktop.svg');
    expect(named(iconOf(app('rules')))).toBe('rules-desktop.svg');
    expect(named(iconOf(app('docmap')))).toBe('docmap-desktop.svg');
    expect(named(iconOf(app('ai')))).toBe('ai-desktop.svg');
    expect(named(iconOf(app('testing')))).toBe('testing-desktop.svg');
    expect(named(iconOf(app('testing', 'web')))).toBe('testing-web.svg');
    expect(named(iconOf(app('streamtap')))).toBe('streamtap.svg');
  });

  it('shows the web variant for a link of the web hub', () => {
    expect(named(iconOf(app('tasks', 'native', '../tasks-web/tasks.html')))).toBe('tasks-web.svg');
  });

  it('falls back to the other variant: a program in the web hub has no web variant of its own', () => {
    expect(named(iconOf(app('rules', 'web')))).toBe('rules-desktop.svg');
    expect(named(iconOf(app('docmap', 'web')))).toBe('docmap-desktop.svg');
    expect(named(iconOf(app('streamtap', 'web')))).toBe('streamtap.svg');
  });

  it('has nothing for an app it does not know, and the tile falls back to its letter', () => {
    expect(iconOf(app('nothing'))).toBeNull();
  });

  it('takes the own mark from the own folder', () => {
    expect(logo).toMatch(/hub-(desktop|web)-small\.svg$/);
  });
});

describe('logoFor', () => {
  it('has one spoke per installed app', () => {
    for (let n = 1; n <= 10; n += 1) expect(named(logoFor(n))).toBe(`hub-desktop-${n}-small.svg`);
  });

  it('stays at ten with more apps than that', () => {
    expect(named(logoFor(11))).toBe('hub-desktop-10-small.svg');
  });

  it('is the plain mark with no app', () => {
    expect(logoFor(0)).toBe(logo);
  });
});
