import { defineConfig, devices } from '@playwright/test';
const port=process.env.HUB_TEST_PORT ?? '5198';
export default defineConfig({testDir:'./e2e',workers:2,use:{baseURL:`http://127.0.0.1:${port}`},projects:[{name:'chromium',use:{...devices['Desktop Chrome']}}],webServer:{command:`npx vite --config sites/hub/vite.config.ts --host 127.0.0.1 --port ${port} --strictPort`,cwd:'../..',url:`http://127.0.0.1:${port}`,reuseExistingServer:false}});
