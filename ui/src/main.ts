import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { Hub } from './lib/api';
import { App as AppState, realClock } from './lib/app.svelte';
import { UpdateChecker } from './lib/updates';
import { takePairingCode } from 'appkit-ui/pair';
import { installTooltips } from 'appkit-ui/tooltip';

/** `localStorage` for the once-a-day limit of the update check; a profile that blocks it checks without the limit. */
function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

// The code leaves the address bar before anything else happens.
const code = takePairingCode(window.location, window.history);

const state = new AppState(
  new Hub((input, init) => window.fetch(input, init)),
  window.sessionStorage,
  realClock,
  null,
  new UpdateChecker((input, init) => window.fetch(input, init), storage()),
);
installTooltips(document, window, { regionLabel: () => 'Hints' });
mount(App, { target: document.getElementById('app') as HTMLElement, props: { app: state, code } });

// A link from `hub-desktop open` pasted into a tab that is already open changes only the fragment: no reload.
window.addEventListener('hashchange', () => {
  const later = takePairingCode(window.location, window.history);
  if (later) void state.start(later);
});
