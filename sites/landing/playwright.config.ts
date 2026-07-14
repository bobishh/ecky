import { defineConfig, devices } from '@playwright/test';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// Landing e2e — a separate static Vite project from the Tauri app.
// Run from repo root:  npx playwright test --config=sites/landing/playwright.config.ts
// Per root AGENTS.md the local agent port is 4243.
const port = process.env.PLAYWRIGHT_LANDING_PORT ?? '4243';
const url = `http://localhost:${port}`;
const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: 'list',
  use: {
    baseURL: url,
    ...devices['Desktop Chrome'],
  },
  projects: [{ name: 'chromium' }],
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url,
    cwd: __dirname,
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
