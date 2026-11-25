// Type declarations for the build: the Vite client types and the web variant's `__WEB_VERSION__` constant.

/// <reference types="vite/client" />

/** The version the page of the web variant tells (set by `vite build --mode web`). */
declare const __WEB_VERSION__: string;
