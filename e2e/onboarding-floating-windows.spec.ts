import { test, expect } from '@playwright/test';

test.describe('First-run onboarding with floating windows', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      (window as any).__SAVE_CONFIG_CALLS__ = [];
      Object.defineProperty(navigator, 'webdriver', {
        configurable: true,
        get: () => false,
      });

      const config = {
        engines: [],
        selectedEngineId: '',
        hasSeenOnboarding: false,
        microwave: {
          humId: null,
          dingId: null,
          muted: false,
        },
        freecadCmd: '',
        assets: [],
        connectionType: null,
        defaultEngineKind: 'freecad',
        defaultGeometryBackend: 'freecad',
        defaultSourceLanguage: 'legacyPython',
        maxGenerationAttempts: 3,
        maxVerifyAttempts: 0,
        mcp: {
          port: null,
          maxSessions: null,
          mode: 'passive',
          primaryAgentId: null,
          promptTimeoutSecs: 1800,
          autoAgents: [],
        },
      };

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') return structuredClone(config);
        if (cmd === 'save_config') {
          (window as any).__SAVE_CONFIG_CALLS__.push(structuredClone(args?.config ?? null));
          return null;
        }
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
            mesh: { available: true, detail: 'bundled', path: null },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
            },
          };
        }
        if (cmd === 'get_history') return [];
        if (cmd === 'get_last_design') return null;
        if (cmd === 'get_active_agent_sessions') return [];
        if (cmd === 'get_agent_terminal_snapshots') return [];
        if (cmd === 'list_models') return [];
        if (cmd === 'get_default_macro') return '';
        if (cmd === 'get_thread_window_layout') return null;
        if (cmd === 'save_thread_window_layout') return null;
        return null;
      };
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
  });

  test('opens and spotlights guided floating windows on first run', async ({ page }) => {
    await expect(page.locator('.onboarding-backdrop')).toBeVisible();
    await expect(page.getByRole('button', { name: 'NEXT' })).toBeVisible();

    await page.getByRole('button', { name: 'NEXT' }).click();
    await expect(page.locator('[data-onboarding-target="dialogue"].onboarding-highlight')).toBeVisible();
    await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();
    await expect(page.locator('[data-window-id="dialogue"].window--highlighted')).toBeVisible();

    await page.getByRole('button', { name: 'NEXT' }).click();
    await expect(page.locator('[data-onboarding-target="viewport"].onboarding-highlight')).toBeVisible();
    await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();

    await page.getByRole('button', { name: 'NEXT' }).click();
    await expect(page.locator('[data-onboarding-target="params"].onboarding-highlight')).toBeVisible();
    await expect(page.locator('[data-window-id="params"]')).toBeVisible();
    await expect(page.locator('[data-window-id="params"].window--highlighted')).toBeVisible();
    await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();

    await page.getByRole('button', { name: 'NEXT' }).click();
    await expect(page.locator('[data-onboarding-target="projects"].onboarding-highlight')).toBeVisible();
    await expect(page.locator('[data-window-id="projects"]')).toBeVisible();
    await expect(page.locator('[data-window-id="projects"].window--highlighted')).toBeVisible();
    await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();
    await expect(page.locator('[data-window-id="params"]')).toBeVisible();

    await page.getByRole('button', { name: 'SKIP' }).click();
    await expect(page.locator('.onboarding-backdrop')).toHaveCount(0);

    const saveCalls = await page.evaluate(() => (window as any).__SAVE_CONFIG_CALLS__);
    expect(saveCalls.at(-1)?.hasSeenOnboarding).toBe(true);
  });
});
