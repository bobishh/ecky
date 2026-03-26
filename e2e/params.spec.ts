import { test, expect } from '@playwright/test';

test.describe('ParamPanel Persistence', () => {
  test.beforeEach(async ({ page }) => {
    // Setup a mock to intercept Tauri commands
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__MOCK_HISTORY__ = [
        {
          id: 'thread-1',
          title: 'Test Thread',
          updatedAt: Date.now() / 1000,
          versionCount: 1,
          messages: [
            {
              id: 'msg-1',
              role: 'assistant',
              status: 'success',
              output: {
                title: 'Test Design',
                versionName: 'V1',
                macroCode: 'params = {"x": 10}',
                uiSpec: { fields: [] },
                initialParams: { x: 10 }
              }
            }
          ]
        }
      ];
      
      let currentUiSpec = { fields: [] };
      let currentParams = { x: 10 };

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        console.log('[MOCK] invoke', cmd, args);
        if (cmd === 'get_config') return { engines: [], selectedEngineId: '', hasSeenOnboarding: true };
        if (cmd === 'save_config') return null;
        if (cmd === 'check_freecad') return true;
        if (cmd === 'get_history') return window.__MOCK_HISTORY__;
        if (cmd === 'get_last_design') return [window.__MOCK_HISTORY__[0].messages[0].output, 'thread-1'];
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
        if (cmd === 'get_thread') return window.__MOCK_HISTORY__[0];
        
        if (cmd === 'parse_macro_params') {
          return {
            fields: [{ key: 'x', label: 'x', type: 'number', freezed: false }],
            params: { x: 10 }
          };
        }
        
        if (cmd === 'update_ui_spec') {
          currentUiSpec = args.uiSpec;
          return;
        }
        
        if (cmd === 'update_parameters') {
          currentParams = args.parameters;
          return;
        }

        if (cmd === 'render_model') {
          return {
            modelId: 'test-model',
            sourceKind: 'generated',
            contentHash: 'mock-hash',
            fcstdPath: '/mock.FCStd',
            manifestPath: '/mock/manifest.json',
            previewStlPath: '/mock.stl',
            viewerAssets: [],
          };
        }

        if (cmd === 'get_model_manifest') {
          return {
            modelId: 'test-model',
            sourceKind: 'generated',
            document: {
              documentName: 'Test Design',
              documentLabel: 'Test Design',
              objectCount: 0,
              warnings: [],
            },
            parts: [],
            parameterGroups: [],
            selectionTargets: [],
            warnings: [],
            enrichmentState: { status: 'none', proposals: [] },
          };
        }

        if (cmd === 'save_last_design') return null;

        return null;
      };
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
  });

  test('read from macro, save, switch thread, and return should keep params', async ({ page }) => {
    await page.locator('.history-card').first().click();

    // 1. Enter edit mode
    await page.getByRole('button', { name: /EDIT CONTROLS/i }).click();

    // 2. Read from macro
    await page.getByRole('button', { name: /READ FROM MACRO/i }).click();
    
    // Check if field x appeared in edit list
    const fieldInput = page.locator('.edit-field input[placeholder="key"]');
    await expect(fieldInput).toHaveValue('x');

    // 3. Save
    await page.getByRole('button', { name: /SAVE/i }).filter({ hasText: '💾 SAVE' }).click();
    await page.getByRole('button', { name: 'RAW' }).click();

    // Verify it's in the UI
    await expect(page.locator('.param-label').filter({ hasText: 'x' })).toBeVisible();

    // 4. Switch to a "new session" (simulates leaving the thread)
    await page.locator('button[title="Create New Thread"]').click();
    await page.getByRole('button', { name: /Blank Thread/i }).click();
    await expect(page.locator('.param-label')).toHaveCount(0); // Should be empty

    // 5. Go back to the original thread (mock thread)
    // Note: since we mock, clicking the thread in history should reload it
    await page.locator('.history-card').first().click();
    await page.getByRole('button', { name: 'RAW' }).click();

    // The param should STILL be there because workingCopy should have patched it
    await expect(page.locator('.param-label').filter({ hasText: 'x' })).toBeVisible();
  });
});
