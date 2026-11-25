// Svelte's compiler settings for the hub's page: preprocessing (TypeScript in components) through Vite.

import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
};
