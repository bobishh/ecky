import { test, expect, type Page } from '@playwright/test';

async function openSeededMacroMap(page: Page) {
  await page.getByRole('button', { name: 'Dialogue', exact: true }).click();
  await page.fill('textarea.prompt-input', 'make a seeded macro');
  await page.getByRole('button', { name: 'PROCESS', exact: true }).click();
  await expect(page.getByText('Generated: Seeded Macro', { exact: false }).first()).toBeVisible({
    timeout: 15000,
  });
  await page.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true }).click();
  await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
  await page.getByRole('button', { name: 'new params', exact: true }).click();
  await expect(page.locator('.macro-ast-map-shell')).toBeVisible();
}

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
      let nextCallbackId = 1;
      window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
        const callbackId = nextCallbackId++;
        (window as unknown as Record<string, unknown>)[`_${callbackId}`] = callback;
        return callbackId;
      };
      const storedParamThread = () => {
        const storedSnapshot = JSON.parse(sessionStorage.getItem('param-last-design') || 'null')
          ?? (window as any).__PARAM_LAST_DESIGN__
          ?? null;
        if (!storedSnapshot) return null;
        return {
          id: storedSnapshot.threadId ?? 'mock-thread-1',
          title: storedSnapshot.design?.title ?? 'Stored Param Design',
          summary: '',
          messages: [{
            id: storedSnapshot.messageId ?? 'mock-msg-1',
            role: 'assistant',
            content: '',
            status: 'success',
            output: storedSnapshot.design,
            artifactBundle: storedSnapshot.artifactBundle,
            modelManifest: storedSnapshot.modelManifest,
            timestamp: Date.now() / 1000,
            deletedAt: null,
          }],
          updatedAt: Date.now() / 1000,
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          engineKind: storedSnapshot.modelManifest?.engineKind ?? 'freecad',
          sourceLanguage: storedSnapshot.modelManifest?.sourceLanguage ?? 'legacyPython',
          geometryBackend: storedSnapshot.modelManifest?.geometryBackend ?? 'freecad',
        };
      };

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        (window as any).__PARAM_CALLS__.push({ cmd, args });
        if (cmd === 'plugin:event|listen') return Number(args?.handler ?? 0);
        if (cmd === 'plugin:event|unlisten') return null;
        if (cmd === 'get_agent_activity') return { events: [], latestCursor: 0 };
        if (cmd === 'get_authoring_graph') {
          return {
            sourceDigest: 'sha256:typed-source',
            coreDigest: 'sha256:typed-core',
            astNodes: [
              {
                path: '/parts/input-port/root/let/bindings/wall',
                stableNodeKey: 'stable:wall',
                kind: 'Literal',
                valueKind: 'Number',
                partId: 'input-port',
                sourceAddressable: true,
                editableOps: ['replace'],
                childPaths: [],
                inputPorts: [],
              },
              {
                path: '/parts/input-port/root/let/bindings/holes',
                stableNodeKey: 'stable:holes',
                kind: 'Call',
                valueKind: 'Solid',
                operation: 'repeat-union',
                partId: 'input-port',
                sourceAddressable: true,
                editableOps: ['replace'],
                childPaths: ['/parts/input-port/root/let/bindings/holes/args/0'],
                inputPorts: [
                  {
                    role: 'tools',
                    valueKind: 'Solid',
                    cardinality: 'many',
                    childPath: '/parts/input-port/root/let/bindings/holes/args/0',
                  },
                ],
              },
              {
                path: '/parts/input-port/root',
                stableNodeKey: 'stable:difference',
                kind: 'Call',
                valueKind: 'Solid',
                operation: 'difference',
                partId: 'input-port',
                sourceAddressable: true,
                editableOps: ['replace'],
                childPaths: [
                  '/parts/input-port/root/args/0',
                  '/parts/input-port/root/args/1',
                ],
                inputPorts: [
                  {
                    role: 'base',
                    valueKind: 'Solid',
                    cardinality: 'one',
                    childPath: '/parts/input-port/root/args/0',
                  },
                  {
                    role: 'tools',
                    valueKind: 'Solid',
                    cardinality: 'many',
                    childPath: '/parts/input-port/root/args/1',
                  },
                ],
              },
              {
                path: '/parts/input-port/root/expanded/0',
                stableNodeKey: 'stable:expanded',
                kind: 'Call',
                valueKind: 'Solid',
                operation: 'union',
                partId: 'input-port',
                sourceAddressable: false,
                editableOps: [],
                nonEditableReason: 'Macro-expanded node has no exact authored source target.',
                childPaths: [],
                inputPorts: [],
              },
            ],
            features: [],
            dependencies: [],
            constraints: [],
            targets: [],
            handles: [],
          };
        }
        if (cmd === 'create_design_thread') {
          return {
            threadId: 'mock-thread-1',
            sourceDocument: { folder: '/mock/param-thread-1', file: '/mock/param-thread-1/model.ecky', source: '(model)' },
            initialVersionId: null, snapshotId: null, parserMatched: null, initialVersionError: null,
            workspace: {
              thread: { id: 'mock-thread-1', title: 'Untitled design', summary: '', updatedAt: 1, versionCount: 0, pendingCount: 0, queuedCount: 0, errorCount: 0, status: 'active', engineKind: 'ecky', sourceLanguage: 'ecky', geometryBackend: 'mesh' },
              messagesPage: { messages: [], nextBefore: null, hasMore: false }, selectedVersion: null, requestedMessageFound: false,
            },
          };
        }
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
        if (cmd === 'get_history') {
          const thread = storedParamThread();
          return thread ? [thread] : [];
        }
        if (cmd === 'get_last_design') {
          const storedSnapshot = sessionStorage.getItem('param-last-design');
          return storedSnapshot ? JSON.parse(storedSnapshot) : null;
        }
        if (cmd === 'get_default_macro') return '# macro';
        if (cmd === 'init_generation_attempt') return 'mock-msg-1';
        if (cmd === 'start_exploration_run') {
          const generated = await window.__TAURI_INTERNALS__.invoke('generate_design', {
            prompt: args?.input?.prompt,
            threadId: args?.input?.threadId,
          });
          const artifactBundle = await window.__TAURI_INTERNALS__.invoke('render_model', {
            macroCode: generated.design.macroCode,
            parameters: generated.design.initialParams,
          });
          const modelManifest = await window.__TAURI_INTERNALS__.invoke('get_model_manifest', {
            modelId: artifactBundle.modelId,
          });
          const storedSnapshot = {
            design: generated.design,
            threadId: generated.threadId,
            messageId: generated.messageId,
            artifactBundle,
            modelManifest,
            selectedPartId: null,
          };
          (window as any).__PARAM_LAST_DESIGN__ = storedSnapshot;
          sessionStorage.setItem('param-last-design', JSON.stringify(storedSnapshot));
          return {
            run: {
              requestId: args?.input?.requestId,
              threadId: generated.threadId,
              phase: 'completed',
              messageId: generated.messageId,
              design: generated.design,
              artifactBundle,
              modelManifest,
              structuralVerification: null,
              usage: null,
              responseText: generated.design.response ?? 'Design synthesized successfully.',
              rawError: null,
              publicationAllowed: true,
            },
            message: {
              id: generated.messageId, role: 'assistant', content: generated.design.response ?? '', status: 'success',
              timestamp: Date.now(), output: generated.design, artifactBundle, modelManifest,
            },
            snapshotId: `snapshot-${generated.messageId}`,
          };
        }
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
            const response = {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Seeded Macro',
                versionName: 'V1',
                interactionMode: 'design',
                macroDialect: 'ecky',
                engineKind: 'ecky',
                sourceLanguage: 'ecky',
                geometryBackend: 'mesh',
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
            (window as any).__PARAM_SEEDED_DESIGN__ = response.design;
            return response;
          }
          if (`${args?.prompt ?? ''}`.includes('macro with two editable parts')) {
            (window as any).__PARAM_SCENARIO__ = 'editable-macro-pair';
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Editable Macro Pair',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode: '(model\n  (part alpha (box 10 20 5))\n  (part beta (box 7 7 7)))',
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
          if (`${args?.prompt ?? ''}`.includes('dense macro')) {
            (window as any).__PARAM_SCENARIO__ = 'dense-macro';
            const denseKeys = Array.from({ length: 8 }, (_, index) => `dense_param_${index}_mm`);
            return {
              threadId: args.threadId || 'mock-thread-1',
              messageId: 'mock-msg-1',
              usage: null,
              design: {
                title: 'Dense Macro',
                versionName: 'V1',
                interactionMode: 'design',
                macroCode: '(model\n  (part dense (box 10 20 5)))',
                uiSpec: {
                  fields: denseKeys.map((key, index) => ({
                    type: 'number',
                    key,
                    label: `Dense Param ${index}`,
                  })),
                },
                initialParams: Object.fromEntries(denseKeys.map((key, index) => [key, index])),
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
                    {
                      type: 'select',
                      key: 'font',
                      label: 'Font',
                      options: [
                        { label: 'Arial', value: 'Arial' },
                        { label: 'Impact', value: 'Impact' },
                      ],
                    },
                  ],
                },
                initialParams: { width: 10, font: 'Arial' },
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
        if (cmd === 'apply_manual_code') {
          const storedSnapshot = JSON.parse(sessionStorage.getItem('param-last-design') || 'null');
          const source = String(args?.input?.source ?? '');
          const messageId = args?.input?.persist ? 'mock-code-version-1' : null;
          const designOutput = {
            ...storedSnapshot.design,
            macroCode: source,
            response: source.includes('boom')
              ? 'Manual code failed validation.'
              : 'Manual code version appended.',
          };
          if (source.includes('boom')) {
            return {
              threadId: args?.input?.threadId,
              baseMessageId: args?.input?.baseMessageId,
              messageId,
              status: 'error',
              designOutput,
              artifactBundle: null,
              modelManifest: null,
              snapshotId: null,
              parserMatched: true,
              error: {
                code: 'validation',
                message: 'mock render exploded: boom op unsupported',
                details: 'mock kernel body mismatch\npart=body op=boom width=12 depth=20',
                stableNodeKey: 'part:body',
                startLine: 2,
                endLine: 2,
                operation: 'boom',
              },
            };
          }
          sessionStorage.setItem('param-last-design', JSON.stringify({
            ...storedSnapshot,
            design: designOutput,
            messageId: messageId ?? args?.input?.baseMessageId,
          }));
          const artifactBundle = storedSnapshot.artifactBundle ?? (window as any).__PARAM_LAST_BUNDLE__;
          const modelManifest = storedSnapshot.modelManifest ?? await window.__TAURI_INTERNALS__.invoke(
            'get_model_manifest',
            { modelId: artifactBundle?.modelId },
          );
          return {
            threadId: args?.input?.threadId,
            baseMessageId: args?.input?.baseMessageId,
            messageId,
            status: 'success',
            designOutput,
            artifactBundle,
            modelManifest,
            snapshotId: `snapshot-${messageId ?? args?.input?.baseMessageId}`,
            parserMatched: true,
            error: null,
          };
        }
        if (cmd === 'apply_manual_parameters') {
          if ((window as any).__PARAM_DELAY_APPLY__ && !args?.input?.persist) {
            await new Promise((resolve) => setTimeout(resolve, 250));
          }
          const storedSnapshot = JSON.parse(sessionStorage.getItem('param-last-design') || 'null');
          const designOutput = {
            ...storedSnapshot.design,
            response: args?.input?.persist
              ? 'Parameter version appended.'
              : 'Parameters applied.',
            initialParams: args?.input?.parameters ?? {},
          };
          const messageId = args?.input?.persist ? 'mock-param-version-1' : null;
          if (storedSnapshot) {
            sessionStorage.setItem('param-last-design', JSON.stringify({
              ...storedSnapshot,
              design: designOutput,
              messageId: messageId ?? args?.input?.targetMessageId,
            }));
          }
          return {
            threadId: args?.input?.threadId,
            baseMessageId: args?.input?.targetMessageId,
            messageId,
            status: (window as any).__PARAM_DELAY_APPLY__ && args?.input?.persist
              ? 'working'
              : 'success',
            designOutput,
            artifactBundle: (window as any).__PARAM_DELAY_APPLY__ && args?.input?.persist
              ? null
              : storedSnapshot?.artifactBundle ?? (window as any).__PARAM_LAST_BUNDLE__,
            modelManifest: (window as any).__PARAM_DELAY_APPLY__ && args?.input?.persist
              ? null
              : storedSnapshot?.modelManifest,
            snapshotId: (window as any).__PARAM_DELAY_APPLY__ && args?.input?.persist
              ? null
              : `snapshot-${messageId ?? args?.input?.targetMessageId}`,
            error: null,
          };
        }
        if (cmd === 'render_model' && String(args?.macroCode ?? '').includes('boom')) {
          throw {
            code: 'validation',
            message: 'mock render exploded: boom op unsupported',
            details: 'mock kernel body mismatch\npart=body op=boom width=12 depth=20',
            stableNodeKey: 'part:body',
            startLine: 2,
            endLine: 2,
            operation: 'boom',
          };
        }
        if (cmd === 'render_model') {
          const scenarioModelIds: Record<string, string> = {
            'seeded-macro': 'seeded-macro-model',
            'editable-macro-pair': 'editable-macro-pair-model',
            'dense-macro': 'dense-macro-model',
            'editable-macro': 'editable-macro-model',
            'narrow-layout-box': 'narrow-layout-box',
          };
          const scenario = (window as any).__PARAM_SCENARIO__;
          const modelId = scenarioModelIds[scenario] ?? 'litho-model';
          const isEcky = scenario === 'seeded-macro';
          const bundle = {
            modelId,
            sourceKind: 'generated',
            engineKind: isEcky ? 'ecky' : 'freecad',
            sourceLanguage: isEcky ? 'ecky' : 'legacyPython',
            geometryBackend: isEcky ? 'mesh' : 'freecad',
            contentHash: 'mock-hash',
            fcstdPath: '/mock.FCStd',
            manifestPath: '/mock/manifest.json',
            modelStlPath: '/mock.stl',
            viewerAssets: [],
            calloutAnchors: [],
            measurementGuides: [],
            edgeTargets: [],
          };
          (window as any).__PARAM_LAST_BUNDLE__ = bundle;
          return bundle;
        }
        if (cmd === 'get_model_manifest') {
          if ((window as any).__PARAM_SCENARIO__ === 'editable-macro-pair') {
            return {
              modelId: 'editable-macro-pair-model',
              sourceKind: 'generated',
              document: {
                documentName: 'Editable Macro Pair',
                documentLabel: 'Editable Macro Pair',
                objectCount: 2,
                warnings: [],
              },
              parts: [
                {
                  partId: 'alpha',
                  freecadObjectName: 'alpha',
                  label: 'Alpha',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: ['model_size_mm'],
                },
                {
                  partId: 'beta',
                  freecadObjectName: 'beta',
                  label: 'Beta',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: [],
                },
              ],
            };
          }
          if ((window as any).__PARAM_SCENARIO__ === 'dense-macro') {
            const denseKeys = Array.from({ length: 8 }, (_, index) => `dense_param_${index}_mm`);
            return {
              modelId: 'dense-macro-model',
              sourceKind: 'generated',
              document: {
                documentName: 'Dense Macro',
                documentLabel: 'Dense Macro',
                objectCount: 1,
                warnings: [],
              },
              parts: [
                {
                  partId: 'dense',
                  freecadObjectName: 'dense',
                  label: 'Dense',
                  kind: 'solid',
                  editable: true,
                  parameterKeys: denseKeys,
                },
              ],
            };
          }
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
              engineKind: 'ecky',
              sourceLanguage: 'ecky',
              geometryBackend: 'mesh',
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
              warnings: [
                'Feature graph was not carried forward because rendered topology no longer validates old feature bindings.',
                'Manufacturing clearance could not be verified.',
              ],
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
              modelStlSizeBytes: 1024,
              totalVolume: 1000,
              totalArea: 500,
              bbox: { xMin: 0, yMin: 0, zMin: 0, xMax: 10, yMax: 10, zMax: 10 },
            },
            verifierStatus: 'ok',
            verifierSource: 'mock',
          };
        }
        if (cmd === 'verify_render') {
          return {
            passed: true,
            summary: 'Visual checks passed.',
            issues: [],
            usage: null,
          };
        }
        if (cmd === 'get_thread') {
          const storedThread = storedParamThread();
          if (storedThread) return storedThread;
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
        if (cmd === 'save_model_manifest') {
          if (args?.manifest?.sourceLanguage === 'ecky') {
            sessionStorage.setItem('param-last-design', JSON.stringify({
              design: (window as any).__PARAM_SEEDED_DESIGN__,
              threadId: 'mock-thread-1',
              messageId: 'mock-msg-1',
              artifactBundle: (window as any).__PARAM_LAST_BUNDLE__,
              modelManifest: args.manifest,
              selectedPartId: null,
            }));
          }
          return null;
        }
        if (cmd === 'add_manual_version') return 'mock-param-version-1';
        if (cmd === 'update_version_runtime') return null;
        if (cmd === 'persist_control_defaults') {
          const input = args?.input as Record<string, any>;
          const storedThread = storedParamThread();
          const selectedVersion = storedThread?.messages?.[0] ?? null;
          const parameters = input.mutation.parameters
            ?? selectedVersion?.output?.initialParams
            ?? {};
          const uiSpec = input.mutation.action === 'saveSchema'
            ? input.mutation.uiSpec
            : selectedVersion?.output?.uiSpec ?? { fields: [] };
          if (selectedVersion?.output) {
            selectedVersion.output = { ...selectedVersion.output, uiSpec, initialParams: parameters };
          }
          return {
            uiSpec,
            parameters,
            workspace: {
              thread: storedThread,
              messagesPage: {
                threadId: storedThread?.id ?? 'mock-thread-1',
                messages: storedThread?.messages ?? [],
                hasMore: false,
                nextBefore: null,
              },
              selectedVersion,
              requestedMessageFound: selectedVersion !== null,
            },
          };
        }
        if (cmd === 'update_parameters') return null;
        if (cmd === 'update_post_processing') return null;
        if (cmd === 'finalize_generation_attempt') {
          if (args?.status === 'success' && args?.design && args?.artifactBundle && args?.modelManifest) {
            const storedSnapshot = JSON.parse(sessionStorage.getItem('param-last-design') || 'null');
            const protectsEckySnapshot =
              storedSnapshot?.modelManifest?.sourceLanguage === 'ecky' &&
              (args.modelManifest.sourceLanguage !== 'ecky' || args.design.sourceLanguage !== 'ecky');
            if (!protectsEckySnapshot) {
              sessionStorage.setItem('param-last-design', JSON.stringify({
                design: args.design,
                threadId: 'mock-thread-1',
                messageId: args.messageId ?? 'mock-msg-1',
                artifactBundle: args.artifactBundle,
                modelManifest: args.modelManifest,
                selectedPartId: null,
              }));
            }
          }
          return null;
        }
        if (cmd === 'save_last_design') {
          if (args?.snapshot) {
            const storedSnapshot = JSON.parse(sessionStorage.getItem('param-last-design') || 'null');
            const protectsEckySnapshot =
              storedSnapshot?.modelManifest?.sourceLanguage === 'ecky' &&
              (args.snapshot.modelManifest?.sourceLanguage !== 'ecky' ||
                args.snapshot.design?.sourceLanguage !== 'ecky');
            if (!protectsEckySnapshot) {
              sessionStorage.setItem('param-last-design', JSON.stringify(args.snapshot));
            }
          } else {
            sessionStorage.removeItem('param-last-design');
          }
          return null;
        }
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
    await page.locator('textarea.prompt-input').press('Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await expect(page.getByPlaceholder('Search controls...')).toBeVisible();

    await page.getByRole('button', { name: /EDIT CONTROLS/i }).click();
    await expect(page.getByRole('button', { name: /READ FROM MACRO/i })).toBeVisible();
    await expect(page.getByRole('button', { name: /CANCEL/i })).toBeVisible();

    await page.getByRole('button', { name: /CANCEL/i }).click();
    await page.getByRole('button', { name: 'PARAMETERS', exact: true }).click();
    await expect(page.locator('.panel-code-btn')).toBeVisible();
    await expect(page.getByRole('button', { name: 'OPEN FILE', exact: true })).toHaveCount(0);
  });

  test('Given parameters open When switching work surfaces Then Views is a separate tab', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a narrow layout box');
    await page.getByRole('button', { name: 'PROCESS', exact: true }).click();
    await expect
      .poll(() => page.evaluate(() => (window as any).__PARAM_SCENARIO__), { timeout: 15000 })
      .toBe('narrow-layout-box');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    const panel = page.locator('.param-panel');
    await expect(panel).toBeVisible({ timeout: 10000 });

    const parametersTab = page.getByRole('button', { name: 'PARAMETERS', exact: true });
    const meshTab = page.getByRole('button', { name: 'MESH', exact: true });
    const viewsTab = page.getByRole('button', { name: 'VIEWS', exact: true });
    const firstParameter = panel.locator('.param-field').first();
    await expect(parametersTab).toHaveAttribute('aria-pressed', 'true');
    await expect(firstParameter).toBeVisible();
    const parameterLayout = await panel.evaluate((element) => {
      const body = element.querySelector('.param-panel-body') as HTMLElement;
      const field = element.querySelector('.param-field') as HTMLElement;
      return {
        scrollable: body.scrollHeight > body.clientHeight,
        fieldHeight: field.getBoundingClientRect().height,
      };
    });
    expect(parameterLayout.scrollable).toBe(true);
    expect(parameterLayout.fieldHeight).toBeGreaterThan(48);
    await expect(viewsTab).toHaveAttribute('aria-pressed', 'false');
    await expect(page.getByRole('button', { name: '+ VIEW' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'OUTLINE', exact: true })).toHaveCount(0);

    const positions = await Promise.all([
      page.getByPlaceholder('Search controls...').boundingBox(),
      firstParameter.boundingBox(),
      viewsTab.boundingBox(),
    ]);
    expect(positions.every(Boolean)).toBe(true);
    expect(positions[0]!.y).toBeLessThan(positions[1]!.y);
    expect(positions[2]!.y).toBeLessThan(positions[0]!.y);

    await meshTab.click();
    await expect(meshTab).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByRole('button', { name: 'OUTLINE', exact: true })).toBeVisible();
    await expect(firstParameter).toHaveCount(0);

    await viewsTab.click();
    await expect(viewsTab).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByRole('button', { name: '+ VIEW' })).toBeVisible();

    await page.getByRole('button', { name: /EDIT CONTROLS/i }).click();
    await expect(page.getByRole('button', { name: /READ FROM MACRO/i })).toBeVisible();
    await expect(panel.locator('.edit-list')).toBeVisible();
  });

  test('Given empty session When parameters open Then default surface remains usable with inactive workspace tabs', async ({ page }) => {
    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    const panel = page.locator('.param-panel');

    await expect(panel).toBeVisible();
    await expect(page.getByRole('button', { name: 'PARAMETERS', exact: true })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await expect(panel.getByText('No raw controls match your search.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'VIEWS', exact: true })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
  });

  test('Given manifest diagnostics When parameters open Then diagnostic warning block is hidden', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a narrow layout box');
    await page.getByRole('button', { name: 'PROCESS', exact: true }).click();
    await expect
      .poll(() => page.evaluate(() => (window as any).__PARAM_SCENARIO__), { timeout: 15000 })
      .toBe('narrow-layout-box');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    const panel = page.locator('.param-panel');
    await expect(panel.locator('.warning-stack')).toHaveCount(0);
    await expect(panel.getByText('Manufacturing clearance could not be verified.')).toHaveCount(0);
    await expect(panel.getByText(/Feature graph was not carried forward/i)).toHaveCount(0);
  });

  test('Given seeded macro When New Params opens Then syntax markers reflect block types', async ({
    page,
  }) => {
    await openSeededMacroMap(page);

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
    await openSeededMacroMap(page);

    await expect(page.locator('.macro-ast-scene__svg')).toBeVisible();
    await expect
      .poll(async () => page.locator('.macro-ast-connector').count())
      .toBeGreaterThan(0);
    await expect(page.locator('.macro-ast-control-anchor').first()).toBeVisible();
  });

  test('Given seeded macro When a param blob is clicked Then the embedded control gets focus', async ({
    page,
  }) => {
    await openSeededMacroMap(page);

    await page.locator('.macro-ast-node-param .macro-ast-node__header').first().click();
    await expect(page.locator('.macro-ast-node-param input.param-input').first()).toBeFocused();
  });

  test('Given Ecky macro with control views When Parameters reopens Then the views persistence surface remains available', async ({
    page,
  }) => {
    await openSeededMacroMap(page);
    await expect
      .poll(() => page.evaluate(() => sessionStorage.getItem('param-last-design') !== null))
      .toBe(true);
    await page.reload();
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'Parameters', exact: true }).click();

    await expect.soft(
      page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'Parameters', exact: true }),
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'VIEWS', exact: true })).toBeVisible();

    const persistedControlViews = await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('param-last-design') || 'null')
        ?.modelManifest?.controlViews ?? null,
    );
    expect(persistedControlViews).toHaveLength(4);
  });

  test('Given seeded macro When New Params edits a value Then Apply rerenders the draft', async ({
    page,
  }) => {
    await openSeededMacroMap(page);

    const beforeApplyCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );

    // Zoomed out the map shows dense chips; clicking a module flies the
    // camera in and reveals the live control.
    await page.locator('.macro-ast-node-param .macro-ast-node__header').first().click();
    const firstParam = page.locator('.macro-ast-map-shell .param-field input.param-input').first();
    await expect(firstParam).toBeVisible();
    await firstParam.fill('42');
    await expect(page.getByRole('button', { name: 'APPLY' })).toBeEnabled();

    const pendingApplyCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );
    expect(pendingApplyCount).toBe(beforeApplyCount);

    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
        ),
      )
      .toBe(beforeApplyCount + 1);
  });

  test('Given several map params When search result is chosen Then owning region frames, pulses, and inline apply keeps identity', async ({
    page,
  }, testInfo) => {
    await openSeededMacroMap(page);

    const camera = page.locator('.macro-ast-camera');
    const cameraBefore = await camera.getAttribute('style');
    const search = page.getByPlaceholder('Search controls...');
    await search.fill('Input Port Diameter');

    const results = page.getByRole('listbox', { name: 'Map search results' });
    const result = results.getByRole('option', { name: /Input Port Diameter/ });
    await expect(result).toBeVisible();
    await result.click();

    const target = page.locator(
      '.macro-ast-node[data-node-id="part:input-port/param:input_port_diameter_mm"]',
    );
    const owner = page.locator('.macro-ast-node[data-node-id="part:input-port"]');
    await expect(target).toHaveAttribute('data-search-selected', 'true');
    await expect(owner).toHaveAttribute('data-search-owner', 'true');
    await expect
      .poll(async () => camera.getAttribute('style'))
      .not.toBe(cameraBefore);
    await expect(target.locator('.macro-ast-node__shape')).toHaveCSS(
      'animation-name',
      'macro-search-pulse',
    );
    await expect(target.locator('.macro-ast-node__shape path')).toHaveAttribute('d', /C/);
    await expect(results.locator('.macro-ast-camera')).toHaveCount(0);
    const visualPath = testInfo.outputPath('macro-map-search-focus.png');
    await page.locator('.macro-ast-map-shell').screenshot({
      path: visualPath,
      animations: 'disabled',
    });
    await testInfo.attach('macro map search focus', {
      path: visualPath,
      contentType: 'image/png',
    });

    const input = page.locator('#macro-input_port_diameter_mm');
    await expect(input).toBeFocused();
    const beforeApplyCount = await page.evaluate(
      () =>
        (window as any).__PARAM_CALLS__.filter(
          (entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters',
        ).length,
    );
    await input.fill('17');
    await page.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () =>
            (window as any).__PARAM_CALLS__.filter(
              (entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters',
            ).length,
        ),
      )
      .toBe(beforeApplyCount + 1);
    const appliedParameters = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter(
        (entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters',
      );
      return calls.at(-1)?.args?.input?.parameters;
    });
    expect(appliedParameters?.input_port_diameter_mm).toBe(17);
    await expect(target).toHaveAttribute('data-search-selected', 'true');
  });

  test('Given typed Core projection When New Params opens Then scalar, geometry ports, and read-only nodes stay distinct', async ({
    page,
  }) => {
    await openSeededMacroMap(page);

    const scalar = page.locator('.macro-ast-node[data-node-id="stable:wall"]');
    await expect(scalar).toBeVisible();
    await expect(scalar).toHaveAttribute('data-node-kind', 'expression');

    const holes = page.locator('.macro-ast-node[data-node-id="stable:holes"]');
    await expect(holes).toBeVisible();
    await expect(holes).toHaveAttribute('data-node-kind', 'operation');
    await expect(holes.locator('.macro-ast-port[data-port-role="tools"]')).toBeVisible();

    const difference = page.locator('.macro-ast-node[data-node-id="stable:difference"]');
    await expect(difference.locator('.macro-ast-port[data-port-role="base"]')).toBeVisible();
    await expect(difference.locator('.macro-ast-port[data-port-role="tools"]')).toBeVisible();

    const expanded = page.locator('.macro-ast-node[data-node-id="stable:expanded"]');
    await expect(expanded).toHaveAttribute('data-node-kind', 'readonly');
    await expect(expanded).toHaveAttribute(
      'title',
      'Macro-expanded node has no exact authored source target.',
    );
  });

  test('Given a selected map param When search has no match Then source and selection stay unchanged', async ({
    page,
  }) => {
    await openSeededMacroMap(page);

    const search = page.getByPlaceholder('Search controls...');
    await search.fill('Inline Anchor Width');
    await page
      .getByRole('listbox', { name: 'Map search results' })
      .getByRole('option', { name: /Inline Anchor Width/ })
      .click();
    const selected = page.locator(
      '.macro-ast-node[data-node-id="part:inline-anchor/param:inline_anchor_width_mm"]',
    );
    await expect(selected).toHaveAttribute('data-search-selected', 'true');
    const before = await page.evaluate(() => {
      const renders = (window as any).__PARAM_CALLS__.filter(
        (entry: { cmd: string }) => entry.cmd === 'render_model',
      );
      return { count: renders.length, macroCode: renders.at(-1)?.args?.macroCode };
    });

    await search.fill('no_such_fastener_zzz');
    await expect(page.getByRole('status', { name: 'Map search status' })).toHaveText(
      'NO MAP MATCHES',
    );
    await expect(selected).toHaveAttribute('data-search-selected', 'true');
    const after = await page.evaluate(() => {
      const renders = (window as any).__PARAM_CALLS__.filter(
        (entry: { cmd: string }) => entry.cmd === 'render_model',
      );
      return { count: renders.length, macroCode: renders.at(-1)?.args?.macroCode };
    });
    expect(after).toEqual(before);
  });

  test('views tab keeps context actions and empty state after the split', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a lithophane (mock)');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
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

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });

    const tabsFitPanel = await page.locator('.panel-mode-tabs').evaluate((node) => {
      const element = node as HTMLElement;
      return element.scrollWidth <= element.clientWidth + 1;
    });
    expect(tabsFitPanel).toBe(true);

    await page.getByRole('button', { name: 'VIEWS', exact: true }).click();
    await expect(page.getByRole('button', { name: '+ VIEW' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ KNOB' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ RULE' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+ LINK' })).toBeVisible();

    await page.getByRole('button', { name: 'PARAMETERS', exact: true }).click();
    const longLabel = page.locator('[data-param-key=\"top_lid_side_shutter_clearance\"] .param-label').first();
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

  test('Given parameter workspace When opened Then tabs lead, staged controls follow, and live apply is absent', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await expect(page.locator('.live-toggle')).toHaveCount(0);

    const panelOrder = await page.locator('.param-panel').evaluate((panel) =>
      [...panel.children].map((child) => child.className),
    );
    expect(panelOrder[0]).toContain('panel-mode-tabs');
    expect(panelOrder[1]).toContain('param-panel-body');

    const footerPosition = await page.locator('.param-panel').evaluate((panel) => {
      const body = panel.querySelector('.param-panel-body') as HTMLElement;
      const footer = panel.querySelector('.param-panel-footer') as HTMLElement;
      const before = footer.getBoundingClientRect().top;
      body.scrollTop = body.scrollHeight;
      const panelBounds = panel.getBoundingClientRect();
      const footerBounds = footer.getBoundingClientRect();
      return {
        moved: footerBounds.top - before,
        withinPanel: footerBounds.bottom <= panelBounds.bottom + 1,
        bodyHeight: body.clientHeight,
        scrollMode: window.getComputedStyle(body).overflowY,
      };
    });
    expect(footerPosition.moved).toBe(0);
    expect(footerPosition.withinPanel).toBe(true);
    expect(footerPosition.bodyHeight).toBeGreaterThan(0);
    expect(footerPosition.scrollMode).toBe('auto');

    const beforeApplyCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );

    const width = page.locator('.param-panel input.param-input').first();
    await width.evaluate((input) => {
      const element = input as HTMLInputElement;
      for (const value of ['21', '22', '23']) {
        element.value = value;
        element.dispatchEvent(new Event('input', { bubbles: true }));
      }
    });

    await page.waitForTimeout(250);
    const applyCountBeforeClick = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );
    expect(applyCountBeforeClick).toBe(beforeApplyCount);

    await page.getByRole('button', { name: 'APPLY', exact: true }).click();
    await expect.poll(() => page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    )).toBe(beforeApplyCount + 1);

    const lastApplyCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters');
      return calls[calls.length - 1];
    });
    expect(lastApplyCall?.args?.input?.parameters?.width).toBe(23);
  });

  test('Given non-live heavy params When typing number Then input handler stays fast and does not render', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a heavy param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'PARAMETERS', exact: true }).click();
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

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'PARAMETERS', exact: true }).click();
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

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'PARAMETERS', exact: true }).click();
    await expect(page.locator('#p600')).toBeVisible();

    const beforeApplyCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );
    await page.locator('#p600').fill('987');
    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
        ),
      )
      .toBe(beforeApplyCount + 1);
    const lastApplyCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters');
      return calls[calls.length - 1];
    });
    expect(lastApplyCall?.args?.input?.parameters?.p600).toBe(987);
  });

  test('Given edited params When Apply runs Then one Rust intent appends an immutable version', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.locator('.param-panel input.param-input').first().fill('42');
    const renderCountBeforeApply = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
    );
    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter(
            (entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters',
          ).length,
        ),
      )
      .toBeGreaterThan(0);

    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const applyCall = calls.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').at(-1);
    expect(applyCall?.args?.input?.parameters?.width).toBe(42);
    expect(applyCall?.args?.input?.persist).toBe(true);
    expect(calls.filter((entry: { cmd: string }) => entry.cmd === 'render_model')).toHaveLength(
      renderCountBeforeApply,
    );
    expect(calls.map((entry: { cmd: string }) => entry.cmd)).not.toContain('add_manual_version');
    expect(calls.map((entry: { cmd: string }) => entry.cmd)).not.toContain('update_parameters');
  });

  test('Given edited defaults When Save Values runs Then one Rust intent persists and returns canonical projection', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page.locator('textarea.prompt-input').press(
      process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter',
    );
    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    const input = page.locator('.param-panel input.param-input').first();
    await expect(input).toBeVisible({ timeout: 10000 });
    await input.fill('73');
    const historyReadsBefore = await page.evaluate(() => (window as any).__PARAM_CALLS__
      .filter((entry: { cmd: string }) => entry.cmd === 'get_history').length);

    await page.getByRole('button', { name: 'SAVE VALUES' }).click();

    await expect(page.getByRole('button', { name: 'SAVED' })).toBeVisible();
    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const intents = calls.filter((entry: { cmd: string }) => entry.cmd === 'persist_control_defaults');
    expect(intents).toHaveLength(1);
    expect(intents[0].args.input).toMatchObject({
      messageId: 'mock-msg-1',
      mutation: { action: 'saveValues', parameters: { width: 73 } },
    });
    expect(calls.filter((entry: { cmd: string }) => entry.cmd === 'get_history')).toHaveLength(historyReadsBefore);
    expect(calls.map((entry: { cmd: string }) => entry.cmd)).not.toContain('update_parameters');
  });

  test('Given canonical macro When Read From Macro runs Then Rust derives merges and persists controls', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page.locator('textarea.prompt-input').press(
      process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter',
    );
    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await page.getByRole('button', { name: /EDIT CONTROLS/i }).click();
    const legacyCallsBefore = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__ as Array<{ cmd: string }>;
      return {
        parse: calls.filter(entry => entry.cmd === 'parse_macro_params').length,
        spec: calls.filter(entry => entry.cmd === 'update_ui_spec').length,
        values: calls.filter(entry => entry.cmd === 'update_parameters').length,
      };
    });

    await page.getByRole('button', { name: /READ FROM MACRO/i }).click();

    await expect.poll(() => page.evaluate(() => (window as any).__PARAM_CALLS__
      .filter((entry: { cmd: string }) => entry.cmd === 'persist_control_defaults').length)).toBe(1);
    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const intent = calls.find((entry: { cmd: string }) => entry.cmd === 'persist_control_defaults');
    expect(intent.args.input).toEqual({
      messageId: 'mock-msg-1',
      mutation: { action: 'readFromMacro' },
    });
    expect(calls.filter((entry: { cmd: string }) => entry.cmd === 'parse_macro_params')).toHaveLength(legacyCallsBefore.parse);
    expect(calls.filter((entry: { cmd: string }) => entry.cmd === 'update_ui_spec')).toHaveLength(legacyCallsBefore.spec);
    expect(calls.filter((entry: { cmd: string }) => entry.cmd === 'update_parameters')).toHaveLength(legacyCallsBefore.values);
  });

  test('Given parameter rendering is pending When Apply is clicked Then version append returns without render delay', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page.locator('textarea.prompt-input').press(
      process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter',
    );
    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel input.param-input').first()).toBeVisible();
    await page.evaluate(() => ((window as any).__PARAM_DELAY_APPLY__ = true));
    await page.locator('.param-panel input.param-input').first().fill('42');
    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect.poll(() => page.evaluate(() => (window as any).__PARAM_CALLS__.some(
      (entry: { cmd: string; args?: { input?: { persist?: boolean } } }) =>
        entry.cmd === 'apply_manual_parameters' && entry.args?.input?.persist === true,
    ))).toBe(true);
    await expect(page.getByText(/APPLY QUEUED/)).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'APPLY' })).toBeEnabled({ timeout: 150 });
  });

  test('Given text font select When font changes Then UI stages it and Apply rerenders with the family', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    const fontField = page.locator('.param-field[data-param-key="font"]');
    await expect(fontField).toContainText('Font');
    await expect(fontField.locator('.select-label')).toHaveText('Arial');

    const applyCountBeforeChoice = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );
    await fontField.locator('button.select-trigger').click();
    await fontField.getByRole('button', { name: 'Impact', exact: true }).click();
    await expect(fontField.locator('.select-label')).toHaveText('Impact');
    await expect
      .poll(() => page.evaluate(
        () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
      ))
      .toBe(applyCountBeforeChoice);

    await page.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(() => page.evaluate(
        () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
      ))
      .toBe(applyCountBeforeChoice + 1);
    const lastApplyCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters');
      return calls.at(-1);
    });
    expect(lastApplyCall?.args?.input?.parameters?.font).toBe('Impact');
  });

  test('Given applied params When Undo is clicked Then previous params rerender', async ({ page }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a param box');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'render_model').length,
        ),
      )
      .toBeGreaterThan(0);

    const width = page.locator('.param-panel input.param-input').first();
    const beforeApplyCount = await page.evaluate(
      () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
    );

    await width.fill('42');
    await page.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
        ),
      )
      .toBe(beforeApplyCount + 1);
    const appliedCall = await page.evaluate(() => {
      const calls = (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters');
      return calls[calls.length - 1];
    });
    expect(appliedCall?.args?.input?.parameters?.width).toBe(42);

    await page.getByRole('button', { name: 'UNDO' }).click();
    await expect(width).toHaveValue('10');
    await expect
      .poll(async () =>
        page.evaluate(
          () => (window as any).__PARAM_CALLS__.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').length,
        ),
      )
      .toBe(beforeApplyCount + 2);

    const calls = await page.evaluate(() => (window as any).__PARAM_CALLS__);
    const undoCall = calls.filter((entry: { cmd: string }) => entry.cmd === 'apply_manual_parameters').at(-1);
    expect(undoCall?.args?.input?.parameters?.width).toBe(10);
    await expect(page.getByRole('button', { name: 'UNDO' })).toBeDisabled();
  });

  test('Given editable macro When part node source is edited in place Then the edit renders', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a macro with two editable parts');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const partNode = page.locator('.macro-ast-node-part[data-node-id="part:alpha"]');
    await expect(partNode).toBeVisible();
    await partNode.locator('.macro-ast-node__header').first().dblclick();

    // Slice pane: only the selected node's source, not the whole document.
    const pane = page.getByTestId('macro-source-pane');
    await expect(pane).toBeVisible();
    await expect(pane).toContainText('EDIT SOURCE / ALPHA');
    await expect(pane.locator('.cm-content')).toContainText('(part alpha (box 10 20 5))');
    await expect(pane.locator('.cm-content')).not.toContainText('(part beta (box 7 7 7))');

    await pane.locator('.cm-content').fill('(part alpha (box 12 20 5))');
    await pane.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(
          () =>
            (window as any).__PARAM_CALLS__.filter(
              (entry: { cmd: string; args?: any }) =>
                entry.cmd === 'apply_manual_code' &&
                `${entry.args?.input?.source ?? ''}`.includes('box 12 20 5') &&
                `${entry.args?.input?.source ?? ''}`.includes('box 7 7 7'),
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

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const partNode = page.locator('.macro-ast-node-part[data-node-id="part:body"]');
    await partNode.locator('.macro-ast-node__header').first().dblclick();
    const pane = page.getByTestId('macro-source-pane');
    await pane.locator('.cm-content').fill('(part body (boom 12 20 5))');
    await pane.getByRole('button', { name: 'APPLY' }).click();

    await expect(pane.locator('.macro-source-pane__error')).toBeVisible();
    await expect(pane.locator('.macro-source-pane__error')).toContainText('boom');
    await expect(pane.locator('.macro-source-pane__error')).toContainText(
      'Context: part=body | op=boom | width=12 | depth=20 | lines=2',
    );
    await expect(pane).toBeVisible();
  });

  test('Given model-scope source edit When backend returns responsible node Then pane retargets that node and keeps structured context', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make an editable macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const modelNode = page.locator('.macro-ast-node-root[data-node-id="macro-root"]');
    await expect(modelNode).toBeVisible();
    await modelNode.locator('.macro-ast-node__header').first().dblclick();

    const pane = page.getByTestId('macro-source-pane');
    await expect(pane).toContainText('EDIT SOURCE / MACRO ROOT');

    await pane.locator('.cm-content').fill('(model\n  (part body (boom 12 20 5)))');
    await pane.getByRole('button', { name: 'APPLY' }).click();

    await expect(pane.locator('.macro-source-pane__error')).toBeVisible();
    await expect(pane).toContainText('EDIT SOURCE / BODY');
    await expect(pane.locator('.macro-source-pane__error')).toContainText(
      'Context: part=body | op=boom | width=12 | depth=20 | lines=2',
    );
  });

  test('Given editable macro When ADD PART opens the pane Then the template scope applies as a new part', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make an editable macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();
    await expect(page.getByTestId('macro-ast-minimap')).toBeVisible();

    await page.locator('.macro-ast-insert-trigger').click();
    const pane = page.getByTestId('macro-source-pane');
    await expect(pane).toBeVisible();
    await expect(pane).toContainText('EDIT SOURCE / NEW PART PART_2');
    // Slice pane: only the inserted template, not the surrounding document.
    await expect(pane.locator('.cm-content')).toHaveText('(part part_2 (box 10 10 10))');

    await pane.getByRole('button', { name: 'APPLY' }).click();
    await expect
      .poll(async () =>
        page.evaluate(
          () =>
            (window as any).__PARAM_CALLS__.filter(
              (entry: { cmd: string; args?: any }) =>
                entry.cmd === 'apply_manual_code' &&
                `${entry.args?.input?.source ?? ''}`.includes('(part part_2 (box 10 10 10))'),
            ).length,
        ),
      )
      .toBeGreaterThan(0);
    await expect(pane).toHaveCount(0);
  });

  test('Given a dirty source pane When another node is double-clicked Then the switch is refused with an inline notice', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a macro with two editable parts');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const alphaNode = page.locator('.macro-ast-node-part[data-node-id="part:alpha"]');
    const betaNode = page.locator('.macro-ast-node-part[data-node-id="part:beta"]');
    await alphaNode.locator('.macro-ast-node__header').first().dblclick();

    const pane = page.getByTestId('macro-source-pane');
    await expect(pane).toContainText('EDIT SOURCE / ALPHA');
    await pane.locator('.cm-content').fill('(part alpha (box 99 20 5))');

    // Dirty draft: switching to another node is refused, draft is kept.
    // dispatchEvent: with the pane open the squeezed map can slide nodes under
    // the bottom dock overlay, which blocks Playwright's pointer actionability.
    await betaNode.locator('.macro-ast-node__header').first().dispatchEvent('dblclick');
    await expect(pane).toContainText('EDIT SOURCE / ALPHA');
    await expect(pane.locator('.cm-content')).toContainText('box 99 20 5');
    await expect(pane).toContainText(/unsaved/i);

    // Clean pane: closing and reopening lets the switch through.
    await pane.getByRole('button', { name: 'CLOSE' }).click();
    await expect(pane).toHaveCount(0);
    await betaNode.locator('.macro-ast-node__header').first().dispatchEvent('dblclick');
    await expect(pane).toContainText('EDIT SOURCE / BETA');
    await expect(pane.locator('.cm-content')).toContainText('(part beta (box 7 7 7))');

    // Clean pane (no edits yet): dblclicking another node swaps freely.
    await alphaNode.locator('.macro-ast-node__header').first().dispatchEvent('dblclick');
    await expect(pane).toContainText('EDIT SOURCE / ALPHA');
    await expect(pane.locator('.cm-content')).toContainText('(part alpha (box 10 20 5))');
  });

  test('Given a dense part When New Params opens Then it renders collapsed with a count chip until expanded', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.fill('textarea.prompt-input', 'make a dense macro');
    await page
      .locator('textarea.prompt-input')
      .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.getByRole('button', { name: 'new params', exact: true }).click();
    await expect(page.locator('.macro-ast-map-shell')).toBeVisible();

    const denseNode = page.locator('.macro-ast-node-part[data-node-id="part:dense"]');
    await expect(denseNode).toBeVisible();

    // Scene nodes are flat siblings in the DOM (not nested under the part),
    // so param controls are asserted against the whole map shell.
    const overlays = page.locator('.macro-ast-map-shell .macro-ast-node__overlay');

    const chip = denseNode.getByTestId('macro-ast-part-collapse-chip');
    await expect(chip).toBeVisible();
    await expect(chip).toContainText('8 PARAMS');
    await expect(overlays).toHaveCount(0);
    await expect(page.locator('.macro-ast-node-param')).toHaveCount(0);

    await chip.click();
    await expect(page.locator('.macro-ast-node-param')).toHaveCount(8);
    await expect(overlays).not.toHaveCount(0);

    // Zoomed out the map shows dense chips; clicking a module flies the
    // camera in and reveals the live control (same pattern as the
    // "New Params edits a value" spec).
    await page.locator('.macro-ast-node-param .macro-ast-node__header').first().click();
    const firstDenseParam = page.locator('.macro-ast-map-shell .param-field input.param-input').first();
    await expect(firstDenseParam).toBeVisible();
    await firstDenseParam.fill('42');
    await expect(page.getByRole('button', { name: 'APPLY' })).toBeEnabled();

    // Collapsing again removes the controls and restores the compact node.
    // dispatchEvent: after the camera flies in, a param node can visually
    // overlap the part header/chip, which blocks Playwright's pointer
    // actionability check (same workaround as the dirty-switch guard spec).
    const collapseChip = denseNode.getByTestId('macro-ast-part-collapse-chip');
    await collapseChip.dispatchEvent('click');
    await expect(page.locator('.macro-ast-node-param')).toHaveCount(0);
    await expect(overlays).toHaveCount(0);
    await expect(collapseChip).toContainText('8 PARAMS');
  });

});
