// The page of the web variant: one HTML file opened from disk. No server and no pairing; the hub is `web/hub.ts`, which
// answers the calls of the Rust hub in the page. Nothing is fetched (the policy forbids it).

import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { App as AppState } from './lib/app.svelte';
import { logo } from './lib/icons';
import { installTooltips } from 'appkit-ui/tooltip';
import { pickerSupport, pickFolder } from 'appkit-web/folder';
import catalog from './web/catalog.json';
import { type CatalogEntry, type FolderPicker, indexedDbMissing, WebHub } from './web/hub';

// The shell may name no file, so the icon of the tab comes in here, as a data: URL.
document.head.append(Object.assign(document.createElement('link'), { rel: 'icon', href: logo }));

// The folder of the clones is looked at once, read only; a browser without the dialog (Firefox, Safari) keeps the marks
// of the menu.
const picker: FolderPicker =
  pickerSupport(window) === 'ok' ? () => pickFolder(window, { id: 'hub-web-clones', mode: 'read' }) : null;
const hub = new WebHub(catalog as CatalogEntry[], indexedDbMissing(window.indexedDB), __WEB_VERSION__, picker);
// The marks of a tab are the browser's, not the tab's: no session storage is needed (there is no session).
const none = { getItem: () => null, setItem: () => undefined, removeItem: () => undefined };
const state = new AppState(hub, none, undefined, hub);
installTooltips(document, window, { regionLabel: () => 'Hints' });
mount(App, { target: document.getElementById('app') as HTMLElement, props: { app: state, code: 'web' } });
