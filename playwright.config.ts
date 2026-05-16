import { defineConfig, devices } from '@playwright/test';

const webPort = process.env.PLAYWRIGHT_WEB_PORT ?? '5173';
const webUrl = `http://localhost:${webPort}`;

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  workers: process.env.CI ? 2 : undefined,
  retries: process.env.CI ? 1 : 0,
  reporter: 'list',
  use: {
    baseURL: webUrl,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: [
    {
      command: `npm run dev:web -- --port ${webPort} --strictPort`,
      url: webUrl,
      reuseExistingServer: true,
    },
    {
      command: 'npm run dev:server',
      url: 'http://localhost:8787/api/health',
      reuseExistingServer: true,
    },
  ],
});
