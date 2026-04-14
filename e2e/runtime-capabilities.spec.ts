import { test, expect, type Page } from '@playwright/test';

type MockConfig = {
  engines: Array<{ id: string; name: string; enabled?: boolean }>;
  selectedEngineId: string;
  hasSeenOnboarding: boolean;
  freecadCmd: string;
  assets: unknown[];
  microwave: null;
  mcp: {
    port: null;
    maxSessions: null;
    mode: 'passive';
    primaryAgentId: null;
    promptTimeoutSecs: number;
    autoAgents: unknown[];
  };
  connectionType: null;
  defaultEngineKind: 'freecad' | 'eckyIrV0';
  defaultSourceLanguage: 'legacyPython' | 'eckyIrV0';
  defaultGeometryBackend: 'freecad' | 'build123d';
  maxGenerationAttempts: number;
  maxVerifyAttempts: number;
};

function buildConfig(overrides: Partial<MockConfig> = {}): MockConfig {
  return {
    engines: [],
    selectedEngineId: '',
    hasSeenOnboarding: true,
    freecadCmd: '',
    assets: [],
    microwave: null,
    mcp: {
      port: null,
      maxSessions: null,
      mode: 'passive',
      primaryAgentId: null,
      promptTimeoutSecs: 1800,
      autoAgents: [],
    },
    connectionType: null,
    defaultEngineKind: 'freecad',
    defaultSourceLanguage: 'legacyPython',
    defaultGeometryBackend: 'freecad',
    maxGenerationAttempts: 3,
    maxVerifyAttempts: 0,
    ...overrides,
  };
}

async function installCapabilityMock(page: Page, configOverrides: Partial<MockConfig> = {}) {
  await page.addInitScript((mockConfig) => {
    const saveCalls: unknown[] = [];
    const config = { ...mockConfig };

    (window as Window & typeof globalThis & {
      __SAVE_CONFIG_CALLS__?: unknown[];
      __CURRENT_CONFIG__?: typeof config;
    }).__SAVE_CONFIG_CALLS__ = saveCalls;
    (window as Window & typeof globalThis & {
      __CURRENT_CONFIG__?: typeof config;
    }).__CURRENT_CONFIG__ = config;

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') return config;
      if (cmd === 'save_config') {
        saveCalls.push(args.config);
        Object.assign(config, args.config);
        return null;
      }
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: {
            available: false,
            detail: "FreeCAD executable not found at '/missing/freecadcmd'.",
            path: null,
          },
          build123d: {
            available: true,
            detail: 'Ready at /mock/python3',
            path: '/mock/python3',
          },
          eckyRust: {
            available: true,
            detail: 'bundled',
            path: null,
          },
          recommendedAuthoringContext: {
            engineKind: 'eckyIrV0',
            sourceLanguage: 'eckyIrV0',
            geometryBackend: 'build123d',
          },
        };
      }
      if (cmd === 'check_freecad') return false;
      if (cmd === 'get_history') return [];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: args?.threadId ?? null,
          connectionState: 'disconnected',
          sessions: [],
          primaryAgentLabel: null,
          statusText: '',
        };
      }
      if (cmd === 'list_models') return [];
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      return null;
    };
  }, buildConfig(configOverrides));
}

test.describe('Runtime capability boot repair', () => {
  test('Given FreeCAD absent When app boots Then prompt stays enabled without runtime banner spam', async ({ page }) => {
    await installCapabilityMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.locator('.prompt-input').fill('build test cup');

    await expect(page.locator('.freecad-missing-banner')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'PROCESS' })).toBeEnabled();
  });

  test('Given all runtimes available When app boots Then runtime banner stays hidden', async ({ page }) => {
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd) => {
        if (cmd === 'get_config') {
          return {
            engines: [],
            selectedEngineId: '',
            hasSeenOnboarding: true,
            freecadCmd: '/Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd',
            assets: [],
            microwave: null,
            mcp: {
              port: null,
              maxSessions: null,
              mode: 'passive',
              primaryAgentId: null,
              promptTimeoutSecs: 1800,
              autoAgents: [],
            },
            connectionType: null,
            defaultEngineKind: 'freecad',
            defaultSourceLanguage: 'legacyPython',
            defaultGeometryBackend: 'freecad',
            maxGenerationAttempts: 3,
            maxVerifyAttempts: 0,
          };
        }
        if (cmd === 'save_config') return null;
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: {
              available: true,
              detail: 'Ready at /mock/freecadcmd',
              path: '/mock/freecadcmd',
            },
            build123d: {
              available: true,
              detail: 'Ready at /mock/python3',
              path: '/mock/python3',
            },
            eckyRust: {
              available: true,
              detail: 'bundled',
              path: null,
            },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
            },
          };
        }
        if (cmd === 'check_freecad') return true;
        if (cmd === 'get_history') return [];
        if (cmd === 'get_last_design') return null;
        if (cmd === 'get_active_agent_sessions') return [];
        if (cmd === 'get_agent_terminal_snapshots') return [];
        if (cmd === 'get_thread_agent_state') {
          return {
            threadId: null,
            connectionState: 'disconnected',
            sessions: [],
            primaryAgentLabel: null,
            statusText: '',
          };
        }
        if (cmd === 'list_models') return [];
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        return null;
      };
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await expect(page.locator('.freecad-missing-banner')).toHaveCount(0);
  });

  test('Given persisted FreeCAD default When boot repairs Then repaired config persists to build123d-backed IR', async ({ page }) => {
    await installCapabilityMock(page, {
      defaultEngineKind: 'freecad',
      defaultSourceLanguage: 'legacyPython',
      defaultGeometryBackend: 'freecad',
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
    await page.locator('button[title="Settings"]').click();

    const saveCalls = await page.evaluate(() =>
      (window as Window & typeof globalThis & { __SAVE_CONFIG_CALLS__?: unknown[] }).__SAVE_CONFIG_CALLS__ ?? [],
    );
    expect(saveCalls.length).toBeGreaterThan(0);
    const repairedCall = saveCalls.find(
      (call) =>
        typeof call === 'object' &&
        call !== null &&
        'defaultSourceLanguage' in call &&
        (call as { defaultSourceLanguage?: string }).defaultSourceLanguage === 'eckyIrV0',
    );
    expect(repairedCall).toBeTruthy();
    expect(repairedCall).toMatchObject({
      defaultEngineKind: 'eckyIrV0',
      defaultSourceLanguage: 'eckyIrV0',
      defaultGeometryBackend: 'build123d',
    });

    await expect(page.getByRole('button', { name: 'ECKY IR' })).toHaveClass(/active/);
    await expect(page.locator('button.conn-type-btn.active', { hasText: 'BUILD123D' }).first()).toBeVisible();
  });

  test('Given FreeCAD absent When user opens import and settings Then FreeCAD actions stay visible but disabled with reason', async ({ page }) => {
    await installCapabilityMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: '+' }).click();
    const importButton = page.getByRole('button', { name: 'Import FreeCAD' }).last();
    await expect(importButton).toBeDisabled();
    await expect(importButton).toHaveAttribute('title', /FreeCAD executable not found/);
    await page.keyboard.press('Escape');

    await page.locator('button[title="Settings"]').click();
    const freecadDefault = page.getByRole('button', { name: 'FREECAD (PY)' });
    await expect(freecadDefault).toBeDisabled();
    await expect(freecadDefault).toHaveAttribute('title', /FreeCAD executable not found/);
  });
});
