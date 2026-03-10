import { test, expect, type Page } from '@playwright/test';

test.describe('Q&A and Design Flow (Mocked)', () => {
  function boxStl(name: string, min: [number, number, number], max: [number, number, number]) {
    const [x0, y0, z0] = min;
    const [x1, y1, z1] = max;
    const vertices = [
      [x0, y0, z0],
      [x1, y0, z0],
      [x1, y1, z0],
      [x0, y1, z0],
      [x0, y0, z1],
      [x1, y0, z1],
      [x1, y1, z1],
      [x0, y1, z1],
    ];
    const faces = [
      [0, 1, 2], [0, 2, 3],
      [4, 6, 5], [4, 7, 6],
      [0, 4, 5], [0, 5, 1],
      [1, 5, 6], [1, 6, 2],
      [2, 6, 7], [2, 7, 3],
      [3, 7, 4], [3, 4, 0],
    ];

    const lines = [`solid ${name}`];
    for (const [a, b, c] of faces) {
      const va = vertices[a];
      const vb = vertices[b];
      const vc = vertices[c];
      lines.push('  facet normal 0 0 0');
      lines.push('    outer loop');
      lines.push(`      vertex ${va[0]} ${va[1]} ${va[2]}`);
      lines.push(`      vertex ${vb[0]} ${vb[1]} ${vb[2]}`);
      lines.push(`      vertex ${vc[0]} ${vc[1]} ${vc[2]}`);
      lines.push('    endloop');
      lines.push('  endfacet');
    }
    lines.push(`endsolid ${name}`);
    return lines.join('\n');
  }

  async function setupMocks(page: Page) {
    const stlFixtures: Record<string, string> = {
      '/mock/output.stl': boxStl('output', [-35, 0, -20], [35, 28, 20]),
      '/mock/parts/shell.stl': boxStl('shell', [-120, 0, -42], [-10, 60, 42]),
      '/mock/parts/lid.stl': boxStl('lid', [18, 24, -24], [120, 46, 24]),
      '/mock/imported-preview.stl': boxStl('imported_preview', [-28, 0, -18], [28, 30, 18]),
      '/mock/parts/outer-shell.stl': boxStl('outer_shell', [-34, 0, -22], [34, 30, 22]),
      '/mock/mess.stl': boxStl('mess', [-12, 0, -12], [12, 10, 12]),
    };

    await page.route(/\/mock\/.*\.stl(\?.*)?$/, async (route) => {
      const url = new URL(route.request().url());
      const body = stlFixtures[url.pathname];
      if (!body) {
        await route.fallback();
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body,
      });
    });

    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__MOCK_THREADS__ = {};
      window.__MOCK_HISTORY__ = [];
      window.__MOCK_LAST_DESIGN__ = null;
      window.__MOCK_MODEL_MANIFESTS__ = {};
      window.__MOCK_BUNDLES__ = {};
      (window as any).__MOCK_CALLS__ = [];

      const generatedBundle = () => ({
        modelId: 'generated-box',
        sourceKind: 'generated',
        contentHash: 'abc123',
        fcstdPath: '/mock/output.FCStd',
        manifestPath: '/mock/manifest.json',
        previewStlPath: '/mock/output.stl',
        viewerAssets: [
          {
            partId: 'part-shell',
            nodeId: 'Shell',
            objectName: 'Shell',
            label: 'Shell',
            path: '/mock/parts/shell.stl',
            format: 'stl'
          },
          {
            partId: 'part-lid',
            nodeId: 'Lid',
            objectName: 'Lid',
            label: 'Lid',
            path: '/mock/parts/lid.stl',
            format: 'stl'
          }
        ]
      });

      const generatedManifest = () => ({
        modelId: 'generated-box',
        sourceKind: 'generated',
        document: {
          documentName: 'Box',
          documentLabel: 'Box',
          objectCount: 2,
          warnings: []
        },
        parts: [
          {
            partId: 'part-shell',
            freecadObjectName: 'Shell',
            label: 'Shell',
            kind: 'Part::Feature',
            viewerAssetPath: '/mock/parts/shell.stl',
            viewerNodeIds: ['Shell'],
            parameterKeys: ['size', 'height'],
            editable: true
          },
          {
            partId: 'part-lid',
            freecadObjectName: 'Lid',
            label: 'Lid',
            kind: 'Part::Feature',
            viewerAssetPath: '/mock/parts/lid.stl',
            viewerNodeIds: ['Lid'],
            parameterKeys: ['size'],
            editable: true
          }
        ],
        parameterGroups: [
          {
            groupId: 'group-shell',
            label: 'Shell',
            parameterKeys: ['size', 'height'],
            partIds: ['part-shell'],
            editable: true
          },
          {
            groupId: 'group-lid',
            label: 'Lid',
            parameterKeys: ['size'],
            partIds: ['part-lid'],
            editable: true
          }
        ],
        selectionTargets: [
          { partId: 'part-shell', viewerNodeId: 'Shell', label: 'Shell', kind: 'part', editable: true },
          { partId: 'part-lid', viewerNodeId: 'Lid', label: 'Lid', kind: 'part', editable: true }
        ],
        warnings: [],
        enrichmentState: { status: 'none', proposals: [] }
      });

      const importedBundle = () => ({
        modelId: 'imported-fcstd-1',
        sourceKind: 'importedFcstd',
        contentHash: 'import-123',
        artifactVersion: 1,
        fcstdPath: '/mock/imported.FCStd',
        manifestPath: '/mock/imported-manifest.json',
        previewStlPath: '/mock/imported-preview.stl',
        viewerAssets: [
          {
            partId: 'part-outer-shell',
            nodeId: 'OuterShell001',
            objectName: 'OuterShell001',
            label: 'Outer Shell',
            path: '/mock/parts/outer-shell.stl',
            format: 'stl'
          }
        ]
      });

      const importedManifest = () => ({
        modelId: 'imported-fcstd-1',
        sourceKind: 'importedFcstd',
        document: {
          documentName: 'Imported Shell',
          documentLabel: 'Imported Shell',
          objectCount: 1,
          warnings: []
        },
        parts: [
          {
            partId: 'part-outer-shell',
            freecadObjectName: 'OuterShell001',
            label: 'Outer Shell',
            kind: 'Part::Feature',
            viewerAssetPath: '/mock/parts/outer-shell.stl',
            viewerNodeIds: ['OuterShell001'],
            parameterKeys: [],
            editable: false
          }
        ],
        parameterGroups: [],
        selectionTargets: [
          {
            partId: 'part-outer-shell',
            viewerNodeId: 'OuterShell001',
            label: 'Outer Shell',
            kind: 'part',
            editable: false
          }
        ],
        warnings: ['Imported FCStd models are inspect-only until bindings are confirmed.'],
        enrichmentState: {
          status: 'pending',
          proposals: [
            {
              proposalId: 'proposal-outershell',
              label: 'Expose Outer Shell dimensions',
              partIds: ['part-outer-shell'],
              parameterKeys: ['outer_shell_width', 'outer_shell_depth', 'outer_shell_height'],
              confidence: 0.42,
              status: 'pending',
              provenance: 'heuristic.import.bounds'
            }
          ]
        }
      });

      const humanizeParameterKey = (key: string) =>
        key
          .split(/[_\-.]+/)
          .filter(Boolean)
          .map((token: string) => token.charAt(0).toUpperCase() + token.slice(1))
          .join(' ');

      const inferImportedDimensionValue = (
        key: string,
        bounds:
          | {
              xMax: number;
              xMin: number;
              yMax: number;
              yMin: number;
              zMax: number;
              zMin: number;
            }
          | null
          | undefined,
      ) => {
        if (!bounds) return 0;
        if (key.endsWith('_height')) return Math.max(0, bounds.zMax - bounds.zMin);
        if (key.endsWith('_depth')) return Math.max(0, bounds.yMax - bounds.yMin);
        return Math.max(0, bounds.xMax - bounds.xMin);
      };

      const buildImportedUiSpec = (manifest: any) => {
        const keys = new Set<string>();
        for (const group of manifest.parameterGroups || []) {
          if (!group.editable) continue;
          for (const key of group.parameterKeys || []) {
            keys.add(key);
          }
        }
        for (const part of manifest.parts || []) {
          if (!part.editable) continue;
          for (const key of part.parameterKeys || []) {
            keys.add(key);
          }
        }
        return {
          fields: [...keys].sort().map((key: string) => ({
            type: 'range',
            key,
            label: humanizeParameterKey(key),
            min: 0,
            step: 1,
            frozen: false,
          })),
        };
      };

      const buildImportedOutput = (manifest: any, existingOutput: any) => {
        const uiSpec = buildImportedUiSpec(manifest);
        const initialParams: Record<string, number> = {};
        for (const field of uiSpec.fields || []) {
          if (existingOutput?.initialParams?.[field.key] !== undefined) {
            initialParams[field.key] = existingOutput.initialParams[field.key];
            continue;
          }
          const sourcePart =
            (manifest.parts || []).find((part: any) => (part.parameterKeys || []).includes(field.key)) ?? null;
          initialParams[field.key] = inferImportedDimensionValue(field.key, sourcePart?.bounds);
        }
        return {
          title:
            manifest.document.documentLabel ||
            manifest.document.documentName ||
            'Imported FreeCAD Model',
          versionName: existingOutput?.versionName || 'Imported',
          response: 'Imported FreeCAD model.',
          interactionMode: 'design',
          macroCode: '',
          uiSpec,
          initialParams,
        };
      };

      const applyImportedParamsToManifest = (manifest: any, parameters: Record<string, number>) => {
        const nextManifest = structuredClone(manifest);
        nextManifest.parts = (nextManifest.parts || []).map((part: any) => {
          if (!part.bounds) return part;
          const bounds = { ...part.bounds };
          for (const key of part.parameterKeys || []) {
            const numericValue = Number(parameters[key]);
            if (!Number.isFinite(numericValue)) continue;
            if (key.endsWith('_height')) {
              bounds.zMax = bounds.zMin + numericValue;
            } else if (key.endsWith('_depth')) {
              bounds.yMax = bounds.yMin + numericValue;
            } else {
              bounds.xMax = bounds.xMin + numericValue;
            }
          }
          return {
            ...part,
            bounds,
          };
        });
        return nextManifest;
      };

      const updateMessageById = (messageId: string, updater: (message: any) => void) => {
        if (!messageId) return null;
        for (const thread of Object.values(
          window.__MOCK_THREADS__ as Record<string, { messages: any[] }>,
        )) {
          const message = thread.messages.find((entry: any) => entry.id === messageId);
          if (message) {
            updater(message);
            return message;
          }
        }
        return null;
      };

      const upsertHistoryThread = (threadId: string, title: string) => {
        const existing = window.__MOCK_HISTORY__.find((thread) => thread.id === threadId);
        if (existing) {
          existing.title = title;
          existing.updatedAt = Date.now() / 1000;
          return existing;
        }
        const thread = {
          id: threadId,
          title,
          updatedAt: Date.now() / 1000,
          versionCount: 0,
          pendingCount: 0,
          errorCount: 0
        };
        window.__MOCK_HISTORY__.unshift(thread);
        return thread;
      };

      window.__MOCK_MODEL_MANIFESTS__['generated-box'] = generatedManifest();
      window.__MOCK_MODEL_MANIFESTS__['imported-fcstd-1'] = importedManifest();
      window.__MOCK_BUNDLES__['generated-box'] = generatedBundle();
      window.__MOCK_BUNDLES__['imported-fcstd-1'] = importedBundle();

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        (window as any).__MOCK_CALLS__.push({ cmd, args });
        console.log('[MOCK] Invoke:', cmd, args);
        if (cmd === 'get_config') return { engines: [{ id: 'mock', name: 'Mock' }], selectedEngineId: 'mock' };
        if (cmd === 'get_history') return window.__MOCK_HISTORY__;
        if (cmd === 'get_last_design') return window.__MOCK_LAST_DESIGN__;
        if (cmd === 'save_last_design') {
          window.__MOCK_LAST_DESIGN__ = args.snapshot ?? null;
          return null;
        }
        if (cmd === 'get_default_macro') return '# macro';
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        if (cmd === 'plugin:dialog|open') return '/mock/imported.FCStd';
        
        if (cmd === 'init_generation_attempt') {
          const assistantId = 'msg-' + Math.random();
          const threadId = args.threadId;
          
          if (!window.__MOCK_THREADS__[threadId]) {
            const historyThread = upsertHistoryThread(threadId, args.prompt.slice(0, 20));
            historyThread.pendingCount = 1;
            window.__MOCK_THREADS__[threadId] = { 
              id: threadId,
              messages: [
                { id: 'user-1', role: 'user', content: args.prompt, status: 'success', timestamp: Date.now() },
                { id: assistantId, role: 'assistant', content: 'Generating...', status: 'pending', timestamp: Date.now() + 100 }
              ] 
            };
          }
          return assistantId;
        }

        if (cmd === 'classify_intent') {
          const isQuestion = args.prompt.includes('?');
          return { 
            intentMode: isQuestion ? 'question' : 'design', 
            confidence: 1.0, 
            response: isQuestion ? 'I am a helpful assistant.' : 'Creating design.',
            usage: {
              inputTokens: 120,
              outputTokens: 18,
              totalTokens: 138,
              cachedInputTokens: 0,
              reasoningTokens: 0,
              estimatedCostUsd: 0.0005,
              segments: [
                {
                  stage: 'classify',
                  provider: 'gemini',
                  model: 'gemini-2.0-flash-lite',
                  inputTokens: 120,
                  outputTokens: 18,
                  totalTokens: 138,
                  cachedInputTokens: 0,
                  reasoningTokens: 0,
                  estimatedCostUsd: 0.0005
                }
              ]
            }
          };
        }

        if (cmd === 'generate_design') {
          return {
            threadId: args.threadId,
            messageId: 'msg-final',
            design: {
              title: 'A Box',
              macroCode: 'create_box()',
              initialParams: { size: 10, height: 14 },
              uiSpec: {
                fields: [
                  { type: 'range', key: 'size', label: 'Size', min: 1, max: 40, step: 1, frozen: false },
                  { type: 'number', key: 'height', label: 'Height', min: 1, max: 80, step: 1, frozen: false }
                ]
              },
              response: 'Box created.',
              interactionMode: 'design'
            },
            usage: {
              inputTokens: 480,
              outputTokens: 220,
              totalTokens: 700,
              cachedInputTokens: 0,
              reasoningTokens: 0,
              estimatedCostUsd: 0.0012,
              segments: [
                {
                  stage: 'generate',
                  provider: 'gemini',
                  model: 'gemini-2.0-flash',
                  inputTokens: 480,
                  outputTokens: 220,
                  totalTokens: 700,
                  cachedInputTokens: 0,
                  reasoningTokens: 0,
                  estimatedCostUsd: 0.0012
                }
              ]
            }
          };
        }

        if (cmd === 'render_model') {
          return window.__MOCK_BUNDLES__['generated-box'];
        }

        if (cmd === 'import_fcstd') {
          return window.__MOCK_BUNDLES__['imported-fcstd-1'];
        }

        if (cmd === 'get_model_manifest') {
          if (args.modelId === 'imported-fcstd-1') {
            return window.__MOCK_MODEL_MANIFESTS__['imported-fcstd-1'];
          }
          return window.__MOCK_MODEL_MANIFESTS__['generated-box'];
        }

        if (cmd === 'apply_imported_model') {
          const nextManifest = applyImportedParamsToManifest(args.manifest, args.parameters);
          const nextBundle = {
            ...args.artifactBundle,
            artifactVersion: (args.artifactBundle?.artifactVersion || 1) + 1,
            contentHash: `import-${Math.random()}`,
          };
          window.__MOCK_MODEL_MANIFESTS__[nextBundle.modelId] = nextManifest;
          window.__MOCK_BUNDLES__[nextBundle.modelId] = nextBundle;

          const message = args.messageId
            ? updateMessageById(args.messageId, (entry) => {
                entry.artifactBundle = nextBundle;
                entry.modelManifest = nextManifest;
                entry.output = {
                  ...buildImportedOutput(nextManifest, entry.output),
                  initialParams: args.parameters,
                };
              })
            : null;

          if (window.__MOCK_LAST_DESIGN__?.artifactBundle?.modelId === nextBundle.modelId) {
            window.__MOCK_LAST_DESIGN__ = {
              ...window.__MOCK_LAST_DESIGN__,
              artifactBundle: nextBundle,
              modelManifest: nextManifest,
              design:
                message?.output ??
                {
                  ...buildImportedOutput(nextManifest, window.__MOCK_LAST_DESIGN__?.design),
                  initialParams: args.parameters,
                },
            };
          }
          return nextBundle;
        }

        if (cmd === 'finalize_generation_attempt') {
          const threadId = Object.keys(window.__MOCK_THREADS__)[0];
          const thread = window.__MOCK_THREADS__[threadId];
          if (thread) {
            const assistantMsg = thread.messages.find((message) => message.role === 'assistant');
            if (assistantMsg) {
              assistantMsg.status = args.status;
              assistantMsg.content = args.responseText || (args.status === 'success' ? 'Success' : 'Error');
              if (args.design) assistantMsg.output = args.design;
              if (args.usage) assistantMsg.usage = args.usage;
            }
          }
          if (args.status === 'success') {
            window.__MOCK_HISTORY__[0].pendingCount = 0;
            if (args.design) {
              window.__MOCK_HISTORY__[0].versionCount = 1;
              window.__MOCK_HISTORY__[0].title = args.design.title;
            }
          }
          return {};
        }

        if (cmd === 'add_manual_version') {
          const messageId = 'manual-' + Math.random();
          const threadId = args.threadId;
          const historyThread = upsertHistoryThread(threadId, args.title);
          historyThread.versionCount = (historyThread.versionCount || 0) + 1;
          historyThread.updatedAt = Date.now() / 1000;
          historyThread.title = args.title;

          const thread = window.__MOCK_THREADS__[threadId] || {
            id: threadId,
            messages: [],
          };
          thread.messages.push({
            id: messageId,
            role: 'assistant',
            content: 'Manual edit committed as new version.',
            status: 'success',
            timestamp: Date.now(),
            output: {
              title: args.title,
              versionName: args.versionName,
              macroCode: args.macroCode,
              uiSpec: args.uiSpec,
              initialParams: args.parameters,
              response: 'Manual edit committed as new version.',
              interactionMode: 'design',
            },
            artifactBundle: args.artifactBundle,
            modelManifest: args.modelManifest,
          });
          window.__MOCK_THREADS__[threadId] = thread;
          return messageId;
        }

        if (cmd === 'add_imported_model_version') {
          const messageId = 'imported-' + Math.random();
          const threadId = args.threadId;
          const historyThread = upsertHistoryThread(threadId, args.title);
          historyThread.versionCount = 1;
          historyThread.pendingCount = 0;
          historyThread.errorCount = 0;
          window.__MOCK_MODEL_MANIFESTS__[args.artifactBundle.modelId] = args.modelManifest;
          window.__MOCK_BUNDLES__[args.artifactBundle.modelId] = args.artifactBundle;

          window.__MOCK_THREADS__[threadId] = {
            id: threadId,
            title: args.title,
            messages: [
              {
                id: messageId,
                role: 'assistant',
                content: 'Imported FCStd model.',
                status: 'success',
                timestamp: Date.now(),
                output: null,
                artifactBundle: args.artifactBundle,
                modelManifest: args.modelManifest,
              }
            ],
          };
          return messageId;
        }

        if (cmd === 'save_model_manifest') {
          const messageId = args.messageId;
          window.__MOCK_MODEL_MANIFESTS__[args.modelId] = args.manifest;
          if (messageId) {
            updateMessageById(messageId, (message) => {
              message.modelManifest = args.manifest;
              if (args.manifest?.sourceKind === 'importedFcstd') {
                message.output = buildImportedOutput(args.manifest, message.output);
              }
            });
          }
          if (window.__MOCK_LAST_DESIGN__?.modelId === args.modelId || window.__MOCK_LAST_DESIGN__?.artifactBundle?.modelId === args.modelId) {
            window.__MOCK_LAST_DESIGN__ = {
              ...window.__MOCK_LAST_DESIGN__,
              design:
                args.manifest?.sourceKind === 'importedFcstd'
                  ? buildImportedOutput(args.manifest, window.__MOCK_LAST_DESIGN__?.design)
                  : window.__MOCK_LAST_DESIGN__?.design ?? null,
              modelManifest: args.manifest,
              messageId: messageId ?? window.__MOCK_LAST_DESIGN__?.messageId ?? null,
            };
          }
          return null;
        }

        if (cmd === 'update_parameters') {
          const message = updateMessageById(args.messageId, (entry) => {
            if (!entry.output && entry.modelManifest?.sourceKind === 'importedFcstd') {
              entry.output = buildImportedOutput(entry.modelManifest, entry.output);
            }
            if (entry.output) {
              entry.output = {
                ...entry.output,
                initialParams: args.parameters,
              };
            }
          });
          if (window.__MOCK_LAST_DESIGN__?.messageId === args.messageId) {
            window.__MOCK_LAST_DESIGN__ = {
              ...window.__MOCK_LAST_DESIGN__,
              design: message?.output ?? window.__MOCK_LAST_DESIGN__?.design ?? null,
            };
          }
          return null;
        }

        if (cmd === 'get_thread') {
          return window.__MOCK_THREADS__[args.id] || { id: args.id, messages: [] };
        }
        return {};
      };
    });
  }

  test('asking a question should show Ecky response without creating design', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    const textarea = page.locator('.prompt-input');
    await textarea.fill('How does this work?');
    
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await sendBtn.click();

    await page.waitForSelector('.microwave-unit', { timeout: 5000 });
    await page.waitForSelector('.mw-thinking-result', { timeout: 5000 });

    const bubbleText = page.locator('.bubble-text');
    await expect(bubbleText).toBeVisible();
    await expect(bubbleText).toContainText('I am a helpful assistant');
    await expect(page.locator('.usage-strip')).toContainText('TOK');
  });

  test('requesting a design should trigger rendering and model update', async ({ page }) => {
    page.on('console', msg => console.log(`[PAGE] ${msg.type()}: ${msg.text()}`));
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    const textarea = page.locator('.prompt-input');
    await textarea.fill('Create a box');
    await page.click('button:has-text("PROCESS")');

    await page.waitForSelector('.microwave-unit', { timeout: 5000 });
    await page.waitForSelector('.part-chip', { timeout: 10000 });
    await expect(page.locator('.part-chip')).toContainText(['Shell', 'Lid']);
    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_model')).toBeTruthy();
  });

  test('design flow should use the model runtime commands', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');

    await page.waitForSelector('.part-chip', { timeout: 10000 });
    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_model')).toBeTruthy();
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'get_model_manifest')).toBeTruthy();
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_stl')).toBeFalsy();
  });

  test('selected parts expose editable controls in the main viewer overlay', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    await page.locator('.part-chip').filter({ hasText: 'Shell' }).click();

    const overlay = page.locator('.viewer-part-overlay');
    await expect(overlay).toContainText('Shell');
    await expect(overlay).toContainText('Size');

    await page.locator('.live-toggle').click();

    const beforeRenderCount = await page.evaluate(
      () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    await overlay.locator('input[type="range"]').first().evaluate((input) => {
      const element = input as HTMLInputElement;
      element.value = '12';
      element.dispatchEvent(new Event('input', { bubbles: true }));
    });

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBeGreaterThan(beforeRenderCount);
  });

  test('viewer overlay edits stage values when live is disabled', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    await page.locator('.part-chip').filter({ hasText: 'Shell' }).click();

    const overlay = page.locator('.viewer-part-overlay');
    const beforeRenderCount = await page.evaluate(
      () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    await overlay.locator('input[type="range"]').first().evaluate((input) => {
      const element = input as HTMLInputElement;
      element.value = '16';
      element.dispatchEvent(new Event('input', { bubbles: true }));
    });

    await page.waitForTimeout(150);
    await expect(page.locator('.viewer-overlay-readout').first()).toContainText('16');
    await expect(page.locator('.apply-btn')).toBeEnabled();

    const stagedRenderCount = await page.evaluate(
      () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    expect(stagedRenderCount).toBe(beforeRenderCount);

    await page.locator('.apply-btn').click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBeGreaterThan(beforeRenderCount);
  });

  test('manual semantic knobs can be created from raw params and attached to a custom context', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    await page.locator('.part-chip').filter({ hasText: 'Shell' }).click();
    await page.getByRole('button', { name: '+ KNOB' }).click();

    await page.locator('#composer-primitive-label').fill('Shell Pair');
    await page.locator('.composer-list .primitive-picker', { hasText: 'Size' }).click();
    await page.locator('.composer-list .primitive-picker', { hasText: 'Height' }).click();
    await page.getByRole('button', { name: 'CREATE KNOB' }).click();

    const manifest = await page.evaluate(() => (window as any).__MOCK_MODEL_MANIFESTS__['generated-box']);
    const primitive = manifest.controlPrimitives.find((entry: { label: string }) => entry.label === 'Shell Pair');
    expect(Boolean(primitive)).toBeTruthy();
    const manualView = manifest.controlViews.find((entry: { primitiveIds?: string[]; source: string }) =>
      entry.source === 'manual' && entry.primitiveIds?.includes(primitive.primitiveId),
    );
    expect(Boolean(manualView)).toBeTruthy();
    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    const saveManifestCall = [...calls].reverse().find((entry: { cmd: string }) => entry.cmd === 'save_model_manifest');
    expect(saveManifestCall?.args?.manifest?.controlPrimitives?.some((entry: { label: string }) => entry.label === 'Shell Pair')).toBeTruthy();
  });

  test('linked semantic knobs propagate related param updates', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    await page.locator('.part-chip').filter({ hasText: 'Shell' }).click();
    await page.getByRole('button', { name: '+ LINK' }).click();
    await page.locator('.view-composer').getByRole('button', { name: 'CREATE LINK' }).click();

    const saveCalls = await page.evaluate(() =>
      (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'save_model_manifest'),
    );
    const relationManifest = [...saveCalls].reverse()[0]?.args?.manifest;
    expect(relationManifest?.controlRelations?.length).toBeGreaterThan(0);

    await page.locator('.live-toggle').click();
    const beforeRenderCount = await page.evaluate(
      () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    await page.locator('.viewer-part-overlay input[type="range"]').first().evaluate((input) => {
      const element = input as HTMLInputElement;
      element.value = '18';
      element.dispatchEvent(new Event('input', { bubbles: true }));
    });

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBeGreaterThan(beforeRenderCount);

    const lastRenderCall = await page.evaluate(() => {
      const calls = (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model');
      return calls[calls.length - 1];
    });
    expect(lastRenderCall?.args?.parameters?.size).toBe(18);
    expect(lastRenderCall?.args?.parameters?.height).toBe(18);
  });

  test('clicking the model selects a part and opens the in-view overlay', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    const viewer = page.locator('.viewer-host');
    const bounds = await viewer.boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) throw new Error('viewer bounds missing');

    await page.waitForTimeout(250);
    for (const ratio of [0.18, 0.24, 0.3]) {
      await page.mouse.click(bounds.x + bounds.width * ratio, bounds.y + bounds.height * 0.54);
      const overlayText = (await page.locator('.viewer-part-overlay').allTextContents())[0] ?? null;
      if (overlayText?.includes('Shell')) break;
    }

    await expect(page.locator('.viewer-part-overlay')).toContainText('Shell');
  });

  test('importing a macro should create a manual version via the manual commit path', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: '📜' }).click();
    await page.getByPlaceholder('Paste FreeCAD macro (Python) here...').fill('print(\"manual\")');
    await page.getByRole('button', { name: /CREATE THREAD/i }).click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) => entry.cmd === 'add_manual_version').length;
      })
      .toBeGreaterThan(0);
  });

  test('imported FCStd proposals persist after review and version reload', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: '📦' }).click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__.map((entry: { cmd: string }) => entry.cmd));
        return calls;
      })
      .toContain('import_fcstd');
    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__.map((entry: { cmd: string }) => entry.cmd));
        return calls;
      })
      .toContain('add_imported_model_version');

    await expect(page.locator('.model-status-card')).toContainText('IMPORTED FCSTD');
    await expect(page.locator('.proposal-card')).toContainText('Expose Outer Shell dimensions');
    await expect(page.locator('.proposal-status')).toContainText('PENDING');

    await page.locator('.proposal-card button:has-text("ACCEPT")').click();
    await expect(page.locator('.proposal-status')).toContainText('ACCEPTED');

    await page.locator('.history-card', { hasText: 'Imported Shell' }).click();
    await expect(page.locator('.proposal-status')).toContainText('ACCEPTED');

    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    const saveManifestCall = calls.find((entry: { cmd: string }) => entry.cmd === 'save_model_manifest');
    expect(saveManifestCall?.args?.messageId).toBeTruthy();
  });

  test('accepted imported bindings become editable and persist control values', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: '📦' }).click();
    await expect(page.locator('.proposal-card')).toContainText('Expose Outer Shell dimensions');

    await page.locator('.proposal-card button:has-text("ACCEPT")').click();
    await expect(page.locator('.proposal-status')).toContainText('ACCEPTED');

    await page.locator('.part-chip', { hasText: 'Outer Shell' }).click();
    await expect(page.locator('.param-field', { hasText: 'Outer Shell Width' })).toBeVisible();
    await expect(page.locator('.viewer-part-overlay')).toContainText('Outer Shell');
    await expect(page.locator('.viewer-part-overlay')).toContainText('EDIT');
    await expect(page.locator('.viewer-part-overlay')).toContainText('Outer Shell Width');
    await expect(page.locator('.viewer-dimension-layer')).toContainText('Outer Shell');

    await page.locator('.viewer-part-overlay input[type="range"]').first().evaluate((element) => {
      const input = element as HTMLInputElement;
      input.value = '48';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });

    await expect(page.locator('.viewer-overlay-readout').first()).toContainText('48');
    await expect(page.locator('.apply-btn')).toBeEnabled();
    await page.locator('.apply-btn').click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) => entry.cmd === 'apply_imported_model').length;
      })
      .toBeGreaterThan(0);

    await page.locator('.history-card', { hasText: 'Imported Shell' }).click();
    await page.locator('.part-chip', { hasText: 'Outer Shell' }).click();
    await expect(page.locator('.viewer-part-overlay .viewer-overlay-readout').first()).toContainText('48');
  });
});
