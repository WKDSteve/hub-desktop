// The icons of the page (rule APP-11). The hub's own lies in assets/icons/<id>/; an app's on a tile comes from the
// repository of that app, as a pinned copy in assets/icons/_borrowed (tools/sync-app-icons.py writes there). The hub
// build gets a file for each (img-src 'self'), the web build a data: URL (img-src data:, the page requests nothing).

import type { HubApp } from './api';
import hubDesktop from '../../../assets/icons/hub-desktop/hub-desktop-small.svg?url';
import hubWeb from '../../../assets/icons/hub-web/hub-web-small.svg?url';

const borrowed = import.meta.glob<string>('../../../assets/icons/_borrowed/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
});

const byId = new Map(Object.entries(borrowed).map(([file, url]) => [file.replace(/^.*\/|\.svg$/g, ''), url]));

export const logo: string = import.meta.env.MODE === 'web' ? hubWeb : hubDesktop;

/**
 * The icon of a tile: the variant the tile opens (`tasks-web`), else the other one (a program in the web hub has no web
 * variant), else the app's one (`streamtap`); null when there is none.
 */
export function iconOf(app: Pick<HubApp, 'id' | 'href' | 'default_mode'>): string | null {
  const [first, second] = app.href || app.default_mode === 'web' ? ['web', 'desktop'] : ['desktop', 'web'];
  return byId.get(`${app.id}-${first}`) ?? byId.get(`${app.id}-${second}`) ?? byId.get(app.id) ?? null;
}
