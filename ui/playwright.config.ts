import { defineConfig } from '@playwright/test';

// The system's Chrome is used (`channel`), so nothing is downloaded. Every test starts a hub of its own against a
// machine that is made for it (e2e/fixture.ts), so the tests do not share state; one worker keeps the load low on the
// runner, which is also somebody's computer.
export default defineConfig({
  testDir: './e2e',
  testMatch: /.*\.spec\.ts/,
  workers: 1,
  fullyParallel: false,
  timeout: 30_000,
  reporter: 'list',
  use: {
    channel: 'chrome',
    trace: 'retain-on-failure',
    screenshot: 'off',
    video: 'off',
  },
});
