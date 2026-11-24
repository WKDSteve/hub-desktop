// The messages of a refusal of the hub (`ApiError.message`): the kit (`appkit-hub`) and the hub itself formulate them in
// Rust, in English, next to a stable `code`. The page shows the ones it knows in its language; one it does not know (a
// newer kit) is shown as the hub sent it. The sources are in other repositories (the kit is a dependency), so the test
// cannot read them: it keeps this list whole in German, and the list follows the kit when it is raised.
// Left out: the three messages that quote a request body (`the body is not {"mode": ...}` and its kind), which a person
// never meets, and the one with an apostrophe (`the list of devices could not be written; the device's sessions…`).

import { msg, t } from './i18n.svelte';

const KNOWN: string[] = [
  // Routes and requests (appkit-hub: access.rs, guard.rs).
  msg('no such app'),
  msg('no such file'),
  msg('no such route'),
  msg('too many attempts'),
  msg('too many requests'),
  msg('invalid request body'),
  msg('the code is not valid'),
  msg('unknown host'),
  msg('unknown origin'),
  msg('an Origin is required'),
  msg('invalid Content-Length'),
  msg('invalid Host header'),
  msg('invalid Origin header'),
  msg('invalid Sec-Fetch-Site header'),
  msg('body too large'),
  msg('cross-site request'),
  msg('method not allowed'),
  msg('application/json required'),
  msg('this route takes no query'),
  msg('the request took too long'),
  msg('this session has too many open event streams'),
  // Sessions and devices.
  msg('credential required'),
  msg('session required'),
  msg('this route is for the PC only'),
  msg('this route is not released for devices'),
  msg('push is for paired devices'),
  msg('through the tailnet only a paired device has a session'),
  msg('the device could not be verified'),
  msg('the name or the key is not valid'),
  msg('this key is paired already'),
  msg('the list of devices is full: revoke one on the PC'),
  msg('this device may not do this; the PC can raise its level'),
  msg('read, create, change or trigger'),
  msg('confirm this on the device'),
  msg('adding a vault must be confirmed'),
  msg('changing the folder of a vault must be confirmed'),
  msg('this hub has no list of vaults'),
  msg('the subscription is not one this hub sends to'),
  // What went wrong inside.
  msg('no challenge could be made'),
  msg('no code could be made'),
  msg('no session could be made'),
  msg('the device could not be kept'),
  msg('the hub cannot be asked to end'),
  msg('the list of devices could not be written; the level is as it was'),
  msg('the subscription could not be kept'),
  msg('the subscription could not be removed'),
  // The apps (hub-desktop: server.rs).
  msg('the app is not installed'),
  msg('something else answers on the port of the app'),
  msg('the web variants are not served (the port is taken)'),
  msg('the app could not be started'),
  msg('the app could not be asked to stop'),
  msg('the hub cannot stop this app'),
  msg('the start with the login of this app cannot be switched here'),
  msg('the start with the login could not be switched'),
  msg('the settings could not be written'),
];

const SET = new Set(KNOWN);

/** The messages of the hub the page translates. */
export const KIT_SENTENCES: readonly string[] = KNOWN;

/** A message of a refusal in the language of the page; one the page does not know stays as the hub sent it. */
export function kitText(message: string): string {
  return SET.has(message) ? t(message) : message;
}
