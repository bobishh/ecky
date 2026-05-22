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
          if (`${args?.prompt ?? ''}`.includes('seeded macro')) {
            (window as any).__PARAM_SCENARIO__ = 'seeded-macro';
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Seeded Macro',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode:
                  '(model\n' +
                  '  (part/region shell\n' +
                  '    (input port inlet)\n' +
                  '    (inline param anchor width))\n' +
                  ')\n',
                uiSpec: {
                  fields: [
                    {
                      type: 'number',
                      key: 'model_size_mm',
                      label: 'Model Size',
                    },
                    {
                      type: 'number',
                      key: 'part_region_mm',
                      label: 'Part Region',
                    },
                    {
                      type: 'number',
                      key: 'input_port_diameter_mm',
                      label: 'Input Port Diameter',
                    },
                    {
                      type: 'number',
                      key: 'inline_anchor_width_mm',
                      label: 'Inline Anchor Width',
                    },
                  ],
                },
                initialParams: {
                  model_size_mm: 40,
                  part_region_mm: 12,
                  input_port_diameter_mm: 6,
                  inline_anchor_width_mm: 3,
                },
                postProcessing: null,
              },
            };
          }
          if (`${args?.prompt ?? ''}`.includes('editable macro')) {
            (window as any).__PARAM_SCENARIO__ = 'editable-macro';
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Editable Macro',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode: '(model\n  (part body (box 10 20 5)))',
                uiSpec: {
                  fields: [
                    {
                      type: 'number',
                      key: 'model_size_mm',
                      label: 'Model Size',
                    },
                  ],
                },
                initialParams: { model_size_mm: 10 },
                postProcessing: null,
              },
            };
          }
          if (`${args?.prompt ?? ''}`.includes('narrow layout box')) {
            (window as any).__PARAM_SCENARIO__ = 'narrow-layout-box';
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Narrow Layout Box',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode: 'print(\"narrow\")',
                uiSpec: {
                  fields: [
                    {
                      type: 'number',
                      key: 'top_lid_side_shutter_clearance',
                      label: 'Top Lid Side Shutter Clearance',
                    },
                    {
                      type: 'number',
                      key: 'raised_shutter_front_overlap',
                      label: 'Raised Shutter Front Overlap',
                    },
                    {
                      type: 'number',
                      key: 'rear_adapter_mount_offset',
                      label: 'Rear Adapter Mount Offset',
                    },
                    {
                      type: 'number',
                      key: 'left_panel_capture_depth',
                      label: 'Left Panel Capture Depth',
                    },
                    {
                      type: 'number',
                      key: 'right_panel_capture_depth',
                      label: 'Right Panel Capture Depth',
                    },
                  ],
                },
                initialParams: {
                  top_lid_side_shutter_clearance: 3.4,
                  raised_shutter_front_overlap: 1.2,
                  rear_adapter_mount_offset: 0.7,
                  left_panel_capture_depth: 2.1,
                  right_panel_capture_depth: 2.1,
                },
                postProcessing: null,
              },
            };
          }
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
        if (cmd === 'macro_ast_source_map') {
          const src = String(args?.macroCode ?? '');
          const balanced = (start: number) => {
            let depth = 0;
            for (let i = start; i < src.length; i += 1) {
              const ch = src[i];
              if (ch === '(') depth += 1;
              else if (ch === ')') {
                depth -= 1;
                if (depth === 0) return i + 1;
              }
            }
            return -1;
          };
          const nodes: any[] = [];
          const modelStart = src.indexOf('(model');
          if (modelStart >= 0) {
            const modelEnd = balanced(modelStart);
            if (modelEnd > 0) {
              nodes.push({ id: 'model', kind: 'model', label: 'model', startByte: modelStart, endByte: modelEnd });
            }
            const partRe = /\((part|feature)\s+([A-Za-z0-9_-]+)/g;
            let match: RegExpExecArray | null;
            while ((match = partRe.exec(src))) {
              const end = balanced(match.index);
              if (end > 0) {
                nodes.push({
                  id: `${match[1]}:${match[2]}`,
                  kind: match[1],
                  label: match[2],
                  startByte: match.index,
                  endByte: end,
                });
              }
            }
          }
          return nodes;
        }
        if (cmd === 'render_model' && String(args?.macroCode ?? '').includes('boom')) {
          throw { code: 'validation', message: 'mock render exploded: boom op unsupported' };
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
          if ((window as any).__PARAM_SCENARIO__ === 'editable-macro') {
            return {
              modelId: 'editable-macro-model',
              sourceKind: 'generated',
              document: {
                documentName: 'Editable Macro',
                documentLabel: 'Editable Macro',
                objectCount: 1,
                warnings: [],
              },
              parts: [
                {
                  partId: 'body',
                  freecadObjectName: 'body',
                  label: 'Body',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['model_size_mm'],
                },
              ],
            };
          }
          if ((window as any).__PARAM_SCENARIO__ === 'seeded-macro') {
            return {
              modelId: 'seeded-macro-model',
              sourceKind: 'generated',
              document: {
                documentName: 'Seeded Macro',
                documentLabel: 'Seeded Macro',
                objectCount: 1,
                warnings: [],
              },
              parts: [
                {
                  partId: 'part-model',
                  freecadObjectName: 'model_body',
                  label: 'Model',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['model_size_mm'],
                },
                {
                  partId: 'part-region',
                  freecadObjectName: 'part_region_shell',
                  label: 'Part/Region',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['part_region_mm'],
                },
                {
                  partId: 'input-port',
                  freecadObjectName: 'input_port_inlet',
                  label: 'Input Port',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['input_port_diameter_mm'],
                },
                {
                  partId: 'inline-anchor',
                  freecadObjectName: 'inline_param_anchor',
                  label: 'Inline Param Anchor',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['inline_anchor_width_mm'],
                },
              ],
              parameterGroups: [],
              controlPrimitives: [
                {
                  primitiveId: 'primitive-model-size',
                  label: 'Model Size',
                  kind: 'number',
                  source: 'generated',
                  partIds: ['part-model'],
                  bindings: [{ parameterKey: 'model_size_mm', scale: 1, offset: 0, min: null, max: null }],
                  editable: true,
                  order: 0,
                },
                {
                  primitiveId: 'primitive-part-region',
                  label: 'Part Region',
                  kind: 'number',
                  source: 'generated',
                  partIds: ['part-region'],
                  bindings: [{ parameterKey: 'part_region_mm', scale: 1, offset: 0, min: null, max: null }],
                  editable: true,
                  order: 1,
                },
                {
                  primitiveId: 'primitive-input-port',
                  label: 'Input Port Diameter',
                  kind: 'number',
                  source: 'generated',
                  partIds: ['input-port'],
                  bindings: [{ parameterKey: 'input_port_diameter_mm', scale: 1, offset: 0, min: null, max: null }],
                  editable: true,
                  order: 2,
                },
                {
                  primitiveId: 'primitive-inline-anchor',
                  label: 'Inline Param Anchor',
                  kind: 'number',
                  source: 'generated',
                  partIds: ['inline-anchor'],
                  bindings: [{ parameterKey: 'inline_anchor_width_mm', scale: 1, offset: 0, min: null, max: null }],
                  editable: true,
                  order: 3,
                },
              ],
              controlRelations: [],
              controlViews: [
                {
                  viewId: 'view-model',
                  label: 'Model',
                  scope: 'global',
                  partIds: [],
                  primitiveIds: ['primitive-model-size'],
                  sections: [
                    {
                      sectionId: 'model-main',
                      label: 'Model',
                      primitiveIds: ['primitive-model-size'],
                      collapsed: false,
                    },
                  ],
                  default: true,
                  source: 'generated',
                  status: 'accepted',
                  order: 0,
                },
                {
                  viewId: 'view-part-region',
                  label: 'Part/Region',
                  scope: 'part',
                  partIds: ['part-region'],
                  primitiveIds: ['primitive-part-region'],
                  sections: [
                    {
                      sectionId: 'part-region-main',
                      label: 'Part/Region',
                      primitiveIds: ['primitive-part-region'],
                      collapsed: false,
                    },
                  ],
                  default: false,
                  source: 'generated',
                  status: 'accepted',
                  order: 1,
                },
                {
                  viewId: 'view-input-port',
                  label: 'Input Port',
                  scope: 'part',
                  partIds: ['input-port'],
                  primitiveIds: ['primitive-input-port'],
                  sections: [
                    {
                      sectionId: 'input-port-main',
                      label: 'Input Port',
                      primitiveIds: ['primitive-input-port'],
                      collapsed: false,
                    },
                  ],
                  default: false,
                  source: 'generated',
                  status: 'accepted',
                  order: 2,
                },
                {
                  viewId: 'view-inline-anchor',
                  label: 'Inline Param Anchor',
                  scope: 'part',
                  partIds: ['inline-anchor'],
                  primitiveIds: ['primitive-inline-anchor'],
                  sections: [
                    {
                      sectionId: 'inline-anchor-main',
                      label: 'Inline Param Anchor',
                      primitiveIds: ['primitive-inline-anchor'],
                      collapsed: false,
                    },
                  ],
                  default: false,
                  source: 'generated',
                  status: 'accepted',
                  order: 3,
                },
              ],
              selectionTargets: [],
              advisories: [],
              measurementAnnotations: [],
              warnings: [],
              enrichmentState: { status: 'none', proposals: [] },
            };
          }
          if ((window as any).__PARAM_SCENARIO__ === 'narrow-layout-box') {
            return {
              modelId: 'narrow-layout-box',
              sourceKind: 'generated',
              document: {
                documentName: 'Narrow Layout Box',
                documentLabel: 'Narrow Layout Box',
                objectCount: 2,
                warnings: [],
              },
              parts: [
                {
                  partId: 'part-top-lid',
                  freecadObjectName: 'top_lid_cover_module',
                  label: 'Top Lid Cover Module',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['top_lid_side_shutter_clearance', 'raised_shutter_front_overlap'],
                },
                {
                  partId: 'part-side-panel',
                  freecadObjectName: 'side_panel_capture_module',
                  label: 'Side Panel Capture Module',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['left_panel_capture_depth', 'right_panel_capture_depth'],
                },
              ],
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
    await page.getByRole('button', { name: 'RAW', exact: true }).click();
    await expect(page.locator('.panel-code-btn:not(.panel-file-btn)')).toBeVisible();
    await expect(page.locator('.panel-file-btn')).toBeVisible();
  });

  test('Given seeded macro When New Params opens Then syntax markers reflect block types', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a seeded macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    await expect(page.locator('.macro-ast-node-root .macro-ast-node__shape')).toBeVisible();
    await expect.soft(page.locator('.macro-ast-node-root .macro-ast-syntax-badge')).toContainText('MODEL');
    await expect.soft(page.locator('.macro-ast-node-part .macro-ast-syntax-badge').first()).toContainText('SOLID');
    await expect.soft(page.locator('.macro-ast-node-param .macro-ast-syntax-badge').first()).toContainText('NUMBER');
    // Ports are dots on param modules now, not nested blocks.
    await expect(page.locator('.macro-ast-node-port')).toHaveCount(0);
  });

  test('Given seeded macro When New Params opens Then connector layer and overlay anchors exist', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a seeded macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    await expect(page.locator('.macro-ast-scene__svg')).toBeVisible();
    await expect
      .poll(async () => page.locator('.macro-ast-connector').count())
      .toBeGreaterThan(0);
    await expect(page.locator('.macro-ast-control-anchor').first()).toBeVisible();
  });

  test('Given seeded macro When a param blob is clicked Then the embedded control gets focus', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a seeded macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    await page.locator('.macro-ast-node-param .macro-ast-node__header').first().click();
    await expect(page.locator('.macro-ast-node-param input.param-input').first()).toBeFocused();
  });

  test('Given seeded macro When New Params opens Then the old PARAMS entrypoint remains and semantic views stay named', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a seeded macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'VIEWS' }).click();

    await expect.soft(page.getByRole('button', { name: 'PARAMS', exact: true })).toBeVisible();
    await expect.soft(page.getByRole('button', { name: 'new params', exact: true })).toBeVisible();
    await expect.soft(page.locator('.param-panel .context-strip-head + .part-strip-list .view-chip')).toContainText([
      'model',
      'part/region',
      'input port',
      'inline param anchor',
    ]);
  });

  test('Given seeded macro When New Params edits a value Then Apply rerenders the draft', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a seeded macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const beforeRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    // Zoomed out the map shows dense chips; clicking a module flies the
    // camera in and reveals the live control.
    await page.locator('.macro-ast-node-param .macro-ast-node__header').first().click();
    const firstParam = page.locator('.macro-ast-map-shell .param-field input.param-input').first();
    await expect(firstParam).toBeVisible();
    await firstParam.fill('42');
    await expect(page.getByRole('button', { name: 'APPLY' })).toBeEnabled();

    const pendingRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    expect(pendingRenderCount).toBe(beforeRenderCount);

    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBe(beforeRenderCount + 1);
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

  test('Given narrow panel When params stay visible Then tabs wrap and long labels do not collapse to ellipsis', async ({ page }) => {
    await page.setViewportSize({ width: 820, height: 900 });
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a narrow layout box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });

    const tabsFitPanel = await page.locator('.panel-mode-tabs').evaluate((node) => {
      const element = node as HTMLElement;
      return element.scrollWidth <= element.clientWidth + 1;
    });
    expect(tabsFitPanel).toBe(true);

    await expect(page.getByRole('button', { name: '+ VIEW' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ KNOB' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ RULE' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ LINK' })).toBeVisible();

    await page.getByRole('button', { name: 'RAW', exact: true }).click();
    const longLabel = page.locator('[data-param-key=\"top_lid_side_shutter_clearance\"] .param-label');
    await expect(longLabel).toContainText('Top Lid Side Shutter Clearance');

    const labelLayout = await longLabel.evaluate((node) => {
      const element = node as HTMLElement;
      const style = window.getComputedStyle(element);
      return {
        clientHeight: element.clientHeight,
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
        textOverflow: style.textOverflow,
        whiteSpace: style.whiteSpace,
      };
    });
    expect(labelLayout.textOverflow).toBe('clip');
    expect(labelLayout.whiteSpace).not.toBe('nowrap');
    expect(labelLayout.scrollWidth).toBeLessThanOrEqual(labelLayout.clientWidth + 1);
    expect(labelLayout.clientHeight).toBeGreaterThan(0);
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
    await page.getByRole('button', { name: 'RAW', exact: true }).click();
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
    await page.getByRole('button', { name: 'RAW', exact: true }).click();
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
    await page.getByRole('button', { name: 'RAW', exact: true }).click();
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

  test('Given applied params When Undo is clicked Then previous params rerender', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBeGreaterThan(0);

    const width = page.locator('.param-panel input.param-input').first();
    const beforeRenderCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );

    await width.fill('42');
    await page.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBe(beforeRenderCount + 1);
    const appliedRenderCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model');
      return calls[calls.length - 1];
    });
    expect(appliedRenderCall?.args?.parameters?.width).toBe(42);

    await page.getByRole('button', { name: 'UNDO' }).click();
    await expect(width).toHaveValue('10');
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBe(beforeRenderCount + 2);

    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const renderCall = calls.filter((entry: { cmd: string }) => entry.cmd === 'render_model').at(-1);
    expect(renderCall?.args?.parameters?.width).toBe(10);
    await expect(page.getByRole('button', { name: 'UNDO' })).toBeDisabled();
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

  test('Given params were applied When Commit is clicked Then saved version reuses rendered draft', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    const width = page.locator('.param-panel input.param-input').first();
    await width.fill('42');

    const renderCountBeforeApply = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    await page.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBe(renderCountBeforeApply + 1);

    await page.getByRole('button', { name: 'COMMIT' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.some((entry: { cmd: string }) => entry.cmd === 'add_manual_version'),
        ),
      )
      .toBe(true);

    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    expect(calls.filter((entry: { cmd: string }) => entry.cmd === 'render_model')).toHaveLength(
      renderCountBeforeApply + 1,
    );
    const addVersionCall = calls.find((entry: { cmd: string }) => entry.cmd === 'add_manual_version');
    expect(addVersionCall?.args?.input?.parameters?.width).toBe(42);
    expect(addVersionCall?.args?.input?.artifactBundle?.previewStlPath).toBe('/mock.stl');
  });

  test('Given editable macro When part node source is edited in place Then the edit renders', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make an editable macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const partNode = page.locator('.macro-ast-node-part[data-node-id="part:body"]');
    await expect(partNode).toBeVisible();
    await partNode.locator('.macro-ast-node__header').first().dblclick();

    // Split pane: full document with the node scope highlighted.
    const pane = page.getByTestId('macro-source-pane');
    await expect(pane).toBeVisible();
    await expect(pane).toContainText('EDIT SOURCE / BODY');
    await expect(pane.locator('.cm-content')).toContainText('(part body (box 10 20 5))');
    await expect(pane.locator('.cm-ecky-scope').first()).toBeVisible();

    await pane.locator('.cm-content').fill('(model\n  (part body (box 12 20 5)))');
    await pane.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () =>
            (window as any).__PARAM_CALLS__.filter(
              (entry: { cmd: string; args?: any }) =>
                entry.cmd === 'render_model' &&
                `${entry.args?.macroCode ?? ''}`.includes('box 12 20 5'),
            ).length,
        ),
      )
      .toBeGreaterThan(0);
    await expect(pane).toHaveCount(0);
  });

  test('Given editable macro When an in-place edit fails to render Then the error stays at the source pane', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make an editable macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const partNode = page.locator('.macro-ast-node-part[data-node-id="part:body"]');
    await partNode.locator('.macro-ast-node__header').first().dblclick();
    const pane = page.getByTestId('macro-source-pane');
    await pane.locator('.cm-content').fill('(model\n  (part body (boom 12 20 5)))');
    await pane.getByRole('button', { name: 'APPLY' }).click();

    await expect(pane.locator('.macro-source-pane__error')).toBeVisible();
    await expect(pane.locator('.macro-source-pane__error')).toContainText('boom');
    await expect(pane).toBeVisible();
  });

  test('Given editable macro When ADD PART opens the pane Then the template scope applies as a new part', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make an editable macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();
    await expect(page.getByTestId('macro-ast-minimap')).toBeVisible();

    await page.locator('.macro-ast-insert-trigger').click();
    const pane = page.getByTestId('macro-source-pane');
    await expect(pane).toBeVisible();
    await expect(pane).toContainText('EDIT SOURCE / NEW PART PART_2');
    await expect(pane.locator('.cm-content')).toContainText('(part part_2 (box 10 10 10))');
    await expect(pane.locator('.cm-ecky-scope').first()).toBeVisible();

    await pane.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () =>
            (window as any).__PARAM_CALLS__.filter(
              (entry: { cmd: string; args?: any }) =>
                entry.cmd === 'render_model' &&
                `${entry.args?.macroCode ?? ''}`.includes('(part part_2 (box 10 10 10))'),
            ).length,
        ),
      )
      .toBeGreaterThan(0);
    await expect(pane).toHaveCount(0);
  });
});
