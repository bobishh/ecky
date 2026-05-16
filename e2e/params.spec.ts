import { test, expect } from '@playwright/test';

test.describe('ParamPanel Persistence', () => {
  test.beforeEach(async ({ page }) => {
    await page.route(/\/mock\.stl(?:\?.*)?$/, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body: `solid mock
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid mock
`,
      });
    });
    await page.addInitScript(() => {
      (window as any).__PARAM_CALLS__ = [];
      const nativeFind = Array.prototype.find;
      Array.prototype.find = function (...findArgs: any[]) {
        if (((window as any).__SLOW_PARAM_FIND__ || (window as any).__COUNT_PARAM_FIND__) && this.length > 1000) {
          (window as any).__SLOW_PARAM_FIND_COUNT__ = ((window as any).__SLOW_PARAM_FIND_COUNT__ || 0) + 1;
        }
        if ((window as any).__SLOW_PARAM_FIND__ && this.length > 1000) {
          const end = performance.now() + 40;
          while (performance.now() < end) {
            // Force old synchronous input handlers to expose UI-thread blocking.
          }
        }
        return nativeFind.apply(this, findArgs as [never, unknown]);
      };
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        (window as any).__PARAM_CALLS__.push({ cmd, args });
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
          if (`${args?.prompt ?? ''}`.includes('heavy param box')) {
            const fields = Array.from({ length: 1200 }, (_, index) => ({
              type: 'number',
              key: `p${index}`,
              label: `P${index}`,
            }));
            const initialParams = Object.fromEntries(fields.map((field, index) => [field.key, index]));
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Heavy Param Box',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode: 'print("heavy")',
                uiSpec: { fields },
                initialParams,
                postProcessing: null,
              },
            };
          }
          if (`${args?.prompt ?? ''}`.includes('param box')) {
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Param Box',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode: 'print("box")',
                uiSpec: {
                  fields: [
                    {
                      type: 'number',
                      key: 'width',
                      label: 'Width',
                    },
                  ],
                },
                initialParams: { width: 10 },
                postProcessing: null,
              },
            };
          }
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
        if (cmd === 'add_manual_version') return 'mock-param-version-1';
        if (cmd === 'update_version_runtime') return null;
        if (cmd === 'update_parameters') return null;
        if (cmd === 'update_post_processing') return null;
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
    await expect(page.locator('.panel-code-btn')).toBeVisible();
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

  test('Given live apply When number changes rapidly Then only latest value renders after idle', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.locator('.live-toggle').click();

    const beforeRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    const width = page.locator('.param-panel input.param-input').first();
    await width.evaluate((input) => {
      const element = input as HTMLInputElement;
      for (const value of ['21', '22', '23']) {
        element.value = value;
        element.dispatchEvent(new Event('input', { bubbles: true }));
      }
    });

    await page.waitForTimeout(100);
    const immediateRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    expect(immediateRenderCount).toBe(beforeRenderCount);

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBe(beforeRenderCount + 1);

    const lastRenderCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model');
      return calls[calls.length - 1];
    });
    expect(lastRenderCall?.args?.parameters?.width).toBe(23);
  });

  test('Given non-live heavy params When typing number Then input handler stays fast and does not render', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a heavy param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'RAW' }).click();
    await expect(page.locator('#p600')).toBeVisible();

    const beforeRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    const inputDurationMs = await page.locator('#p600').evaluate((input) => {
      const element = input as HTMLInputElement;
      (window as any).__SLOW_PARAM_FIND__ = true;
      const start = performance.now();
      element.value = '987';
      element.dispatchEvent(new Event('input', { bubbles: true }));
      const duration = performance.now() - start;
      (window as any).__SLOW_PARAM_FIND__ = false;
      return duration;
    });

    expect(inputDurationMs).toBeLessThan(16);
    await expect(page.locator('#p600')).toHaveValue('987');
    const afterRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    expect(afterRenderCount).toBe(beforeRenderCount);
  });

  test('Given non-live heavy params When typing number Then parent param tree does not recompute while field is focused', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a heavy param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'RAW' }).click();
    await expect(page.locator('#p600')).toBeVisible();

    const parentFindsBeforeDebounce = await page.locator('#p600').evaluate(async (input) => {
      const element = input as HTMLInputElement;
      (window as any).__SLOW_PARAM_FIND_COUNT__ = 0;
      (window as any).__COUNT_PARAM_FIND__ = true;
      element.value = '987';
      element.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 180));
      (window as any).__COUNT_PARAM_FIND__ = false;
      return (window as any).__SLOW_PARAM_FIND_COUNT__;
    });

    expect(parentFindsBeforeDebounce).toBe(0);
    await expect(page.locator('#p600')).toHaveValue('987');
  });

  test('Given non-live heavy params When Apply is clicked from a focused number Then latest local value renders', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a heavy param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'RAW' }).click();
    await expect(page.locator('#p600')).toBeVisible();

    const beforeRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    await page.locator('#p600').fill('987');
    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBe(beforeRenderCount + 1);
    const lastRenderCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model');
      return calls[calls.length - 1];
    });
    expect(lastRenderCall?.args?.parameters?.p600).toBe(987);
  });

  test('Given non-live params When Apply renders Then draft changes but history does not grow', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.locator('.param-panel input.param-input').first().fill('42');
    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBeGreaterThan(0);

    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const renderCall = calls.filter((entry: { cmd: string }) => entry.cmd === 'render_model').at(-1);
    expect(renderCall?.args?.parameters?.width).toBe(42);
    expect(calls.map((entry: { cmd: string }) => entry.cmd)).not.toContain('add_manual_version');
    expect(calls.map((entry: { cmd: string }) => entry.cmd)).not.toContain('update_parameters');
  });

  test('Given staged params When Commit is clicked Then one immutable version is saved', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.locator('.param-panel input.param-input').first().fill('42');
    await page.getByRole('button', { name: 'COMMIT' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.some((entry: { cmd: string }) => entry.cmd === 'add_manual_version'),
        ),
      )
      .toBe(true);

    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const addVersionCall = calls.find((entry: { cmd: string }) => entry.cmd === 'add_manual_version');
    expect(addVersionCall?.args?.input?.macroCode).toBe('print("box")');
    expect(addVersionCall?.args?.input?.parameters?.width).toBe(42);
    expect(addVersionCall?.args?.input?.artifactBundle?.previewStlPath).toBe('/mock.stl');
  });
});
