// The hub's routes, as typed calls. The transport (the bearer token, the refusal, the pairing code, hello) is the
// kit's; this file is the routes of the hub.

import { ApiError, HubClient, type Hello } from 'appkit-ui/client';

export { ApiError, type Hello };

/** One app of the catalog as `GET /api/hub/apps` tells it. The list is empty until the catalog is part of the hub. */
export interface HubApp {
  id: string;
  name: string;
}

export interface Apps {
  apps: HubApp[];
}

export class Hub extends HubClient {
  constructor(fetchFn: typeof fetch, base = '') {
    super(fetchFn, '/api/hub', base);
  }

  apps(): Promise<Apps> {
    return this.json<Apps>(this.route('apps'));
  }
}
