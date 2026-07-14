import { defineConfig, devices } from '@playwright/test';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// Docs-site e2e — server-rendered Ecky IR Field Guide.
// Mirrors the production nginx layout under target/www/ and serves it.
// Run from repo root:  npx playwright test --config=sites/docs/playwright.config.ts
// Per root AGENTS.md the local agent port is 4245.
const port = process.env.DOCS_E2E_PORT ?? '4245';
const url = `http://localhost:${port}`;
const thisDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(dirname(thisDir)); // sites/docs → sites → ecky (repo root)

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
    // Build both artifacts (HTML + EPUB), then serve the staged directory.
    // Must run from the repo root: the npm scripts + serve.mjs resolve paths
    // relative to cwd.
    command: `npm run build:docs-site && npm run build:book && node sites/docs/serve.mjs`,
    url: `${url}/docs/`,
    cwd: repoRoot,
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
