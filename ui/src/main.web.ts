// The page of the web variant: one HTML file opened from disk. No server and no pairing; the hub is `web/hub.ts`, which
// answers the calls of the Rust hub in the page. Nothing is fetched (the policy forbids it).

import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { App as AppState } from './lib/app.svelte';
import { installTooltips } from 'appkit-ui/tooltip';
import catalog from './web/catalog.json';
import { type CatalogEntry, indexedDbMissing, WebHub } from './web/hub';

const hub = new WebHub(catalog as CatalogEntry[], indexedDbMissing(window.indexedDB), __WEB_VERSION__);
// The marks of a tab are the browser's, not the tab's: no session storage is needed (there is no session).
const none = { getItem: () => null, setItem: () => undefined, removeItem: () => undefined };
const state = new AppState(hub, none, undefined, hub);
installTooltips(document, window, { regionLabel: () => 'Hints' });
mount(App, { target: document.getElementById('app') as HTMLElement, props: { app: state, code: 'web' } });
