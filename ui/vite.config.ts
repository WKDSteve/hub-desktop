import { readFileSync } from 'node:fs';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

const test = {
  environment: 'node' as const,
  include: ['src/**/*.test.ts'],
};

// `vite build --mode web` builds the page of the web variant: one module with everything in it, out of `dist-web/`.
// `web/pack.mjs` turns that into the single `hub.html` (script and style inline, the policy). Nothing in it may fetch:
// no preload, no split chunk, no file next to the page.
const web = defineConfig({
  plugins: [svelte()],
  base: './',
  define: {
    __WEB_VERSION__: JSON.stringify(process.env['WEB_BUILD'] ?? (JSON.parse(readFileSync('package.json', 'utf8')) as { version: string }).version),
  },
  build: {
    outDir: 'dist-web',
    target: 'es2022',
    assetsInlineLimit: 0,
    modulePreload: false,
    cssCodeSplit: false,
    sourcemap: false,
    rollupOptions: {
      input: 'index.web.html',
      output: { codeSplitting: false },
    },
  },
  test,
});

// The page is served by the hub on a fixed origin. Nothing is inlined: no data: URL, no inline script (the CSP of
// the shell allows 'self' only).
const hub = defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    outDir: 'dist',
    target: 'es2022',
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    sourcemap: false,
  },
  test,
});

export default ({ mode }: { mode: string }) => (mode === 'web' ? web : hub);
