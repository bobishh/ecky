import { test, expect, type Page } from '@playwright/test';

async function openSettings(page: Page) {
  await page.addInitScript(() => {
    const stored = sessionStorage.getItem('jev-settings-fixture');
    let config = stored ? JSON.parse(stored) : {
      engines: [], selectedEngineId: '', assets: [], hasSeenOnboarding: true,
      connectionType: 'provider:codex', providerModels: { codex: '', agy: '' },
      defaultEngineKind: 'ecky', defaultSourceLanguage: 'ecky', defaultGeometryBackend: 'mesh',
      maxGenerationAttempts: 3, maxVerifyAttempts: 0,
    };
    const state = window as any;
    state.__JEV_CALLS__ = 0;
    state.__SAVED_JEV_CONFIG__ = structuredClone(config);
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') return structuredClone(config);
      if (cmd === 'save_config') {
        const next = args?.config;
        if (next?.jevClassifier?.enabled && !next.jevClassifier.apiKey.trim()) {
          throw { message: 'Jev API token is required when classifier is enabled' };
        }
        if (state.__DELAY_JEV_SAVE__) {
          await new Promise<void>(resolve => { state.__FINISH_JEV_SAVE__ = resolve; });
        }
        config = structuredClone(next);
        state.__SAVED_JEV_CONFIG__ = structuredClone(config);
        sessionStorage.setItem('jev-settings-fixture', JSON.stringify(config));
        return null;
      }
      if (cmd === 'list_provider_models') return { models: ['gpt-5.6-sol'], isLive: true };
      if (cmd === 'get_history' || cmd === 'get_active_agent_sessions' || cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_runtime_capabilities') return {
        freecad: { available: false }, build123d: { available: false }, mesh: { available: true },
        recommendedAuthoringContext: { engineKind: 'ecky', sourceLanguage: 'ecky', geometryBackend: 'mesh' },
      };
      if (cmd === 'classify_jev') state.__JEV_CALLS__++;
      return null;
    };
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  return page.locator('[data-window-id="settings"]');
}

test('Given disabled Jev When token and toggle saved Then reload retains masked settings without classification', async ({ page }) => {
  let settings = await openSettings(page);
  const toggle = settings.getByRole('checkbox', { name: 'Experimental Jev classifier' });
  await expect(toggle).not.toBeChecked();
  const token = settings.getByLabel('TypeSafe API token');
  await expect(token).toHaveAttribute('type', 'password');
  await token.fill('fixture-jev-token');
  await toggle.check();
  await page.evaluate(() => { (window as any).__DELAY_JEV_SAVE__ = true; });
  await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
  await expect(settings.getByRole('button', { name: 'SAVING...' })).toBeDisabled();
  await page.evaluate(() => { (window as any).__FINISH_JEV_SAVE__(); });
  await expect(settings.locator('.status-msg')).toContainText('Registry saved successfully.');
  await page.reload();
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  settings = page.locator('[data-window-id="settings"]');
  await expect(settings.getByRole('checkbox', { name: 'Experimental Jev classifier' })).toBeChecked();
  await expect(settings.getByLabel('TypeSafe API token')).toHaveValue('fixture-jev-token');
  await expect(settings.getByLabel('TypeSafe API token')).toHaveAttribute('type', 'password');
  expect(await page.evaluate(() => (window as any).__JEV_CALLS__)).toBe(0);
});

test('Given one saved Jev setting When provider and connection mode change Then same setting stays available', async ({ page }) => {
  const settings = await openSettings(page);
  const toggle = settings.getByRole('checkbox', { name: 'Experimental Jev classifier' });
  const token = settings.getByLabel('TypeSafe API token');
  await token.fill('shared-global-token');
  await toggle.check();

  const provider = settings.getByRole('button', { name: 'PROVIDER', exact: true });
  await provider.click();
  await expect(toggle).toBeChecked();
  await expect(token).toHaveValue('shared-global-token');
  await settings.getByRole('button', { name: 'AGY', exact: true }).click();
  await expect(toggle).toBeChecked();
  await expect(token).toHaveValue('shared-global-token');

  await settings.getByRole('button', { name: 'MCP', exact: true }).click();
  await expect(toggle).toBeChecked();
  await expect(token).toHaveValue('shared-global-token');
  await settings.getByRole('button', { name: 'API KEY', exact: true }).click();
  await expect(toggle).toBeChecked();
  await expect(token).toHaveValue('shared-global-token');
});

test('Given no token When Jev enabled and saved Then error shown and saved configuration unchanged', async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole('checkbox', { name: 'Experimental Jev classifier' }).check();
  await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
  await expect(settings.locator('.status-msg')).toContainText('Jev API token is required');
  const saved = await page.evaluate(() => (window as any).__SAVED_JEV_CONFIG__);
  expect(saved.jevClassifier?.enabled ?? false).toBe(false);
});
