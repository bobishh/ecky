import { test, expect } from '@playwright/test';

test.describe('Configuration Panel', () => {
  test('Given workbench When settings toggles Then config window opens and closes', async ({ page }) => {
    await page.goto('/');

    await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();

    await page.getByRole('button', { name: 'Settings' }).click();
    const settingsWindow = page.locator('[data-window-id="settings"]');
    await expect(settingsWindow).toBeVisible();
    await expect(settingsWindow.getByText('CONNECTION TYPE')).toBeVisible();

    await settingsWindow.locator('.window-close').click();
    await expect(settingsWindow).toBeHidden();
    await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();
  });

  test('Given settings opens When default section renders Then connection type is visible', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    await expect(page.locator('[data-window-id="settings"]').getByText('CONNECTION TYPE')).toBeVisible();
  });

  test('Given legacy config missing verify attempts When settings opens Then screenshot verify defaults to 2 with updated help copy', async ({ page }) => {
    await page.addInitScript(() => {
      const config = {
        engines: [],
        selectedEngineId: '',
        freecadCmd: '',
        assets: [],
        microwave: null,
        voice: { sttLanguageCode: 'en-US' },
        mcp: {
          port: null,
          maxSessions: null,
          mode: 'passive',
          primaryAgentId: null,
          promptTimeoutSecs: 1800,
          eckyAstAuthoring: false,
          autoAgents: [],
        },
        hasSeenOnboarding: true,
        connectionType: null,
        defaultEngineKind: 'freecad',
        defaultSourceLanguage: 'legacyPython',
        defaultGeometryBackend: 'freecad',
        maxGenerationAttempts: 3,
      };

      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') return structuredClone(config);
        if (cmd === 'save_config') return null;
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
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
        if (cmd === 'get_mcp_server_status') {
          return { running: true, endpointUrl: 'http://127.0.0.1:39249/mcp', lastStartupError: null };
        }
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        return null;
      };
    });

    await page.goto('/');
    await page.waitForSelector('.workbench');
    await page.locator('button[title="Settings"], button[title="Configuration"]').click();
    await page.getByRole('button', { name: 'APP' }).click();

    const verifyAttemptsField = page.locator('.field').filter({ has: page.locator('#max-verify-attempts') });
    await expect(page.locator('#max-verify-attempts')).toHaveValue('2');
    await expect(verifyAttemptsField).toContainText('Default: 2');
    await expect(verifyAttemptsField).toContainText('structural verification still runs');
  });

  test('Given workbench When settings opens Then gear opens floating settings window', async ({ page }) => {
    await page.goto('/');

    await expect(page.getByRole('button', { name: 'Settings' })).toBeVisible();
    await page.getByRole('button', { name: 'Settings' }).click();
    await expect(page.locator('[data-window-id="settings"] .window-close')).toContainText('×');
  });

  test('Given Settings MCP When AST authoring toggled and saved Then config persists eckyAstAuthoring', async ({ page }) => {
    await page.addInitScript(() => {
      const saveCalls: unknown[] = [];
      const config = {
        engines: [],
        selectedEngineId: '',
        freecadCmd: '',
        assets: [],
        microwave: null,
        voice: { sttLanguageCode: 'en-US' },
        mcp: {
          port: null,
          maxSessions: null,
          mode: 'passive',
          primaryAgentId: null,
          promptTimeoutSecs: 1800,
          eckyAstAuthoring: false,
          autoAgents: [],
        },
        hasSeenOnboarding: true,
        connectionType: null,
        defaultEngineKind: 'freecad',
        defaultSourceLanguage: 'legacyPython',
        defaultGeometryBackend: 'freecad',
        maxGenerationAttempts: 3,
        maxVerifyAttempts: 0,
      };

      (window as Window & typeof globalThis & { __SAVE_CONFIG_CALLS__?: unknown[] }).__SAVE_CONFIG_CALLS__ = saveCalls;
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') return structuredClone(config);
        if (cmd === 'save_config') {
          saveCalls.push(args?.config ?? null);
          Object.assign(config, args?.config ?? {});
          if ((window as any).__DELAY_SAVE_CONFIG__) {
            await new Promise<void>((resolve) => {
              (window as any).__RESOLVE_SAVE_CONFIG__ = resolve;
            });
          }
          return null;
        }
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
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
        if (cmd === 'get_mcp_server_status') {
          return { running: true, endpointUrl: 'http://127.0.0.1:39249/mcp', lastStartupError: null };
        }
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        return null;
      };
    });

    await page.goto('/');
    await page.waitForSelector('.workbench');
    await page.locator('button[title="Settings"], button[title="Configuration"]').click();
    await page.getByRole('button', { name: 'MCP' }).click();

    const astToggleLabel = page.locator('.mcp-ast-authoring-toggle').filter({ hasText: 'ECKY AST AUTHORING' });
    const astToggle = page.getByRole('checkbox', { name: 'ECKY AST AUTHORING' });
    await expect(astToggleLabel).toBeVisible();

    await astToggleLabel.click();
    await expect(astToggle).toBeChecked();
    await page.evaluate(() => {
      (window as any).__DELAY_SAVE_CONFIG__ = true;
    });
    await page.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await expect(page.getByRole('button', { name: 'SAVING...' })).toBeDisabled();
    await page.evaluate(() => {
      (window as any).__RESOLVE_SAVE_CONFIG__?.();
    });
    await expect(page.locator('.status-msg')).toContainText('Registry saved successfully.');

    const saveCalls = await page.evaluate(() =>
      (window as Window & typeof globalThis & { __SAVE_CONFIG_CALLS__?: unknown[] }).__SAVE_CONFIG_CALLS__ ?? [],
    );
    expect(saveCalls.length).toBeGreaterThan(0);
    expect(saveCalls[saveCalls.length - 1]).toMatchObject({
      connectionType: 'mcp',
      mcp: {
        eckyAstAuthoring: true,
      },
    });
  });

  test('Given Settings MCP When Ecky MCP skill exported Then zip save command receives selected path', async ({ page }) => {
    await page.addInitScript(() => {
      const exportCalls: unknown[] = [];
      const config = {
        engines: [],
        selectedEngineId: '',
        freecadCmd: '',
        assets: [],
        microwave: null,
        voice: { sttLanguageCode: 'en-US' },
        mcp: {
          port: null,
          maxSessions: null,
          mode: 'passive',
          primaryAgentId: null,
          promptTimeoutSecs: 1800,
          eckyAstAuthoring: false,
          autoAgents: [],
        },
        hasSeenOnboarding: true,
        connectionType: 'mcp',
        defaultEngineKind: 'freecad',
        defaultSourceLanguage: 'ecky',
        defaultGeometryBackend: 'freecad',
        maxGenerationAttempts: 3,
        maxVerifyAttempts: 0,
      };

      (window as Window & typeof globalThis & { __ECKY_SKILL_EXPORT_CALLS__?: unknown[] }).__ECKY_SKILL_EXPORT_CALLS__ = exportCalls;
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') return structuredClone(config);
        if (cmd === 'save_config') return null;
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
            mesh: { available: true, detail: 'bundled', path: null },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'ecky',
              geometryBackend: 'freecad',
            },
          };
        }
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
        if (cmd === 'get_mcp_server_status') {
          return { running: true, endpointUrl: 'http://127.0.0.1:39249/mcp', lastStartupError: null };
        }
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        if (cmd === 'plugin:dialog|save') return '/mock/ecky-mcp-skill.zip';
        if (cmd === 'export_ecky_mcp_skill_zip') {
          exportCalls.push(args);
          if ((window as any).__DELAY_SKILL_EXPORT__) {
            await new Promise<void>((resolve) => {
              (window as any).__RESOLVE_SKILL_EXPORT__ = resolve;
            });
          }
          return null;
        }
        return null;
      };
    });

    await page.goto('/');
    await page.waitForSelector('.workbench');
    await page.locator('button[title="Settings"], button[title="Configuration"]').click();
    await page.getByRole('button', { name: 'MCP' }).click();

    await page.evaluate(() => {
      (window as any).__DELAY_SKILL_EXPORT__ = true;
    });
    await page.getByRole('button', { name: 'EXPORT SKILL ZIP' }).click();
    await expect(page.getByRole('button', { name: 'EXPORTING...' })).toBeDisabled();
    await page.evaluate(() => {
      (window as any).__RESOLVE_SKILL_EXPORT__?.();
    });
    await expect(page.locator('.status-msg')).toContainText('Ecky MCP skill exported.');

    const exportCalls = await page.evaluate(() =>
      (window as Window & typeof globalThis & { __ECKY_SKILL_EXPORT_CALLS__?: unknown[] }).__ECKY_SKILL_EXPORT_CALLS__ ?? [],
    );
    expect(exportCalls).toEqual([{ targetPath: '/mock/ecky-mcp-skill.zip' }]);
  });
});
