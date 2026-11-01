import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { Hub } from './lib/api';
import { App as AppState } from './lib/app.svelte';
import { takePairingCode } from 'appkit-ui/pair';
import { installTooltips } from 'appkit-ui/tooltip';

// The code leaves the address bar before anything else happens.
const code = takePairingCode(window.location, window.history);

const state = new AppState(
  new Hub((input, init) => window.fetch(input, init)),
  window.sessionStorage,
);
installTooltips(document, window, { regionLabel: () => 'Hints' });
mount(App, { target: document.getElementById('app') as HTMLElement, props: { app: state, code } });

// A link from `hub-desktop open` pasted into a tab that is already open changes only the fragment: no reload.
window.addEventListener('hashchange', () => {
  const later = takePairingCode(window.location, window.history);
  if (later) void state.start(later);
});
