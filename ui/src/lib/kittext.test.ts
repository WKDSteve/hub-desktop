import { afterEach, describe, expect, it } from 'vitest';
import { Hub } from './api';
import { App, type TokenStore } from './app.svelte';
import { setLang } from './i18n.svelte';
import { KIT_SENTENCES, kitText } from './kittext';

afterEach(() => setLang('en'));

const memory = (): TokenStore => {
  const data: Record<string, string> = {};
  return {
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => void (data[key] = value),
    removeItem: (key) => void delete data[key],
  };
};

describe('kitText', () => {
  it('lists a message once', () => {
    expect(new Set(KIT_SENTENCES).size).toBe(KIT_SENTENCES.length);
  });

  it('is the message in English, and German after setLang', () => {
    expect(kitText('too many attempts')).toBe('too many attempts');
    setLang('de');
    expect(kitText('too many attempts')).toBe('zu viele Versuche');
  });

  it('shows a message it does not know as the hub sent it', () => {
    setLang('de');
    expect(kitText('something a newer kit says')).toBe('something a newer kit says');
    expect(kitText('HTTP 502')).toBe('HTTP 502');
  });
});

describe('the problem of a hub that refuses at the start', () => {
  const refusing = (message: string): Hub =>
    new Hub((async () =>
      new Response(JSON.stringify({ error: { code: 'internal', message } }), {
        status: 500,
        headers: { 'content-type': 'application/json' },
      })) as typeof fetch);

  it('is said in the language of the page', async () => {
    setLang('de');
    const app = new App(refusing('no session could be made'), memory());
    await app.start('c0de');
    expect(app.phase).toBe('error');
    expect(app.problem).toBe('es ließ sich keine Sitzung erzeugen');
  });

  it('keeps the words of a message it does not know', async () => {
    setLang('de');
    const unknown = new App(refusing('a new kind of trouble'), memory());
    await unknown.start('c0de');
    expect(unknown.problem).toBe('a new kind of trouble');
  });
});
