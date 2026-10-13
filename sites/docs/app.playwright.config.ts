import { defineConfig, devices } from '@playwright/test';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const port = process.env.DOCS_APP_E2E_PORT ?? '43519';
const repoRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const url = `http://127.0.0.1:${port}`;

// Docs window checks need the real app shell but no API/render server.
export default defineConfig({
  testDir: '../../e2e',
  testMatch: 'app.spec.ts',
  reporter: 'list',
  use: { baseURL: url, ...devices['Desktop Chrome'] },
  webServer: {
    command: `npm run dev:web -- --host 127.0.0.1 --port ${port} --strictPort`,
    cwd: repoRoot,
    url,
    reuseExistingServer: false,
  },
});
