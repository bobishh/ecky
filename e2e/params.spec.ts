import { test, expect } from '@playwright/test';

test.describe('ParamPanel Persistence', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') {
          return {
            engines: [{ id: 'mock', name: 'Mock' }],
            selectedEngineId: 'mock',
            hasSeenOnboarding: true,
          };
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
        if (cmd === 'check_freecad') return true;
        if (cmd === 'get_history') return [];
        if (cmd === 'get_last_design') return null;
        if (cmd === 'get_default_macro') return '# macro';
        if (cmd === 'init_generation_attempt') return 'mock-msg-1';
        if (cmd === 'classify_intent') {
          return {
            intentMode: 'design',
            response: 'Routing request...',
            finalResponse: '',
            confidence: 0.9,
            usage: null,
          };
        }
        if (cmd === 'generate_design') {
          return {
            threadId: args.threadId || 'mock-thread-1',
            messageId: 'mock-msg-1',
            usage: null,
            design: {
              title: 'Lithophane Mock',
              versionName: 'V1',
              interactionMode: 'design',
              macroCode: 'print("litho")',
              uiSpec: {
                fields: [
                  {
                    type: 'image',
                    key: 'source_image',
                    label: 'Upload Lithophane Photo',
                  },
                ],
              },
              initialParams: {},
              postProcessing: {
                displacement: {
                  imageParam: 'source_image',
                  projection: 'cylindrical',
                  depthMm: 3.0,
                  invert: false,
                },
              },
            },
          };
        }
        if (cmd === 'render_model') {
          return {
            modelId: 'litho-model',
            sourceKind: 'generated',
            contentHash: 'mock-hash',
            fcstdPath: '/mock.FCStd',
            manifestPath: '/mock/manifest.json',
            previewStlPath: '/mock.stl',
            viewerAssets: [],
            calloutAnchors: [],
            measurementGuides: [],
            edgeTargets: [],
          };
        }
        if (cmd === 'get_model_manifest') {
          return {
            modelId: 'litho-model',
            sourceKind: 'generated',
            document: {
              documentName: 'Lithophane Mock',
              documentLabel: 'Lithophane Mock',
              objectCount: 0,
              warnings: [],
            },
            parts: [],
            parameterGroups: [],
            controlPrimitives: [],
            controlRelations: [],
            controlViews: [],
            selectionTargets: [],
            advisories: [],
            measurementAnnotations: [],
            warnings: [],
            enrichmentState: { status: 'none', proposals: [] },
          };
        }
        if (cmd === 'verify_generated_model') {
          return {
            passed: true,
            summary: 'Structural checks passed.',
            issues: [],
            metrics: {
              partCount: 1,
              previewStlSizeBytes: 1024,
              totalVolume: 1000,
              totalArea: 500,
              bbox: { xMin: 0, yMin: 0, zMin: 0, xMax: 10, yMax: 10, zMax: 10 },
            },
            verifierStatus: 'ok',
            verifierSource: 'mock',
          };
        }
        if (cmd === 'get_thread') {
          return {
            id: args.id,
            title: 'New Session',
            updatedAt: Date.now() / 1000,
            versionCount: 1,
            pendingCount: 0,
            errorCount: 0,
            summary: '',
            messages: [],
          };
        }
        if (cmd === 'save_model_manifest') return null;
        if (cmd === 'finalize_generation_attempt') return null;
        if (cmd === 'save_last_design') return null;
        if (cmd === 'save_config') return null;
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
        if (cmd === 'plugin:dialog|open') {
          return '/Users/test/Desktop/cool_photo.jpg';
        }
        return null;
      };
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
  });

  test('toolbar and mode tabs stay wired after the split', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a lithophane (mock)');
    await page.locator('textarea.prompt-input').press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await expect(page.getByPlaceholder('Search controls...')).toBeVisible();

    await page.getByRole('button', { name: /EDIT CONTROLS/i }).click();
    await expect(page.getByRole('button', { name: /READ FROM MACRO/i })).toBeVisible();
    await expect(page.getByRole('button', { name: /CANCEL/i })).toBeVisible();

    await page.getByRole('button', { name: /CANCEL/i }).click();
    await page.getByRole('button', { name: 'RAW' }).click();
    await expect(page.getByRole('button', { name: 'CODE' })).toBeVisible();
  });

  test('views tab keeps context actions and empty state after the split', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a lithophane (mock)');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });

    await page.getByRole('button', { name: 'VIEWS' }).click();
    await expect(page.getByText('CONTEXTS')).toBeVisible();
    await expect(page.getByRole('button', { name: '+ VIEW' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ KNOB' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ RULE' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ LINK' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Model' })).toBeVisible();
    await expect(page.getByText('Main')).toBeVisible();
  });
});
