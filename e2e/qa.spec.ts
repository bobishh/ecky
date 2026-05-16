import { test, expect, type Locator, type Page } from '@playwright/test';

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

  async function setupMocks(
    page: Page,
    options: {
      failCanonicalCup?: boolean;
      stepArtifact?: boolean;
      directOcctAvailable?: boolean;
      directOcctDetail?: string;
      forkConfirmResult?: boolean;
      bootWithGeneratedDesign?: boolean;
      renderModelDelayMs?: number;
    } = {},
  ) {
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

    await page.addInitScript((mockOptions) => {
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
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
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
        ],
        exportArtifacts: mockOptions.stepArtifact
          ? [{ label: 'STEP', format: 'step', path: '/mock/output.step', role: 'primary' }]
          : [],
        edgeTargets: [],
        calloutAnchors: [],
        measurementGuides: []
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
        ],
        exportArtifacts: mockOptions.stepArtifact
          ? [{ label: 'STEP', format: 'step', path: '/mock/imported.step', role: 'primary' }]
          : [],
        edgeTargets: [],
        calloutAnchors: [],
        measurementGuides: []
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
          errorCount: 0,
          summary: '',
          messages: [],
        };
        window.__MOCK_HISTORY__.unshift(thread);
        return thread;
      };

      window.__MOCK_MODEL_MANIFESTS__['generated-box'] = generatedManifest();
      window.__MOCK_MODEL_MANIFESTS__['imported-fcstd-1'] = importedManifest();
      window.__MOCK_BUNDLES__['generated-box'] = generatedBundle();
      window.__MOCK_BUNDLES__['imported-fcstd-1'] = importedBundle();
      if (mockOptions.bootWithGeneratedDesign) {
        window.__MOCK_LAST_DESIGN__ = {
          design: {
            title: 'A Box',
            versionName: 'V1',
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
          threadId: 'thread-restored',
          messageId: 'msg-restored',
          artifactBundle: generatedBundle(),
          modelManifest: generatedManifest(),
          selectedPartId: null,
        };
      }

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        (window as any).__MOCK_CALLS__.push({ cmd, args });
        console.log('[MOCK] Invoke:', cmd, args);
        if (cmd === 'get_config') {
          return {
            engines: [{ id: 'mock', name: 'Mock' }],
            selectedEngineId: 'mock',
            hasSeenOnboarding: true,
          };
        }
        if (cmd === 'save_config') return null;
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
            directOcct: {
              available: mockOptions.directOcctAvailable ?? false,
              detail: mockOptions.directOcctDetail ?? 'Direct OCCT unavailable: missing TKDESTEP',
              path: mockOptions.directOcctAvailable ? '/mock/occt' : null,
            },
            mesh: { available: true, detail: 'bundled', path: null },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
            },
          };
        }
        if (cmd === 'check_freecad') return true;
        if (cmd === 'get_history') return window.__MOCK_HISTORY__;
        if (cmd === 'get_last_design') return window.__MOCK_LAST_DESIGN__;
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
        if (cmd === 'save_last_design') {
          window.__MOCK_LAST_DESIGN__ = args.snapshot ?? null;
          return null;
        }
        if (cmd === 'get_default_macro') return '# macro';
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        if (cmd === 'plugin:dialog|open') return '/mock/imported.FCStd';
        if (cmd === 'plugin:dialog|save') return '/mock/exported.step';
        if (cmd === 'plugin:dialog|confirm') return mockOptions.forkConfirmResult ?? true;
        if (cmd === 'export_file') return null;
        
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
              versionName: 'V1',
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
          const delayMs = Number(mockOptions.renderModelDelayMs ?? 0);
          if (delayMs > 0) {
            await new Promise((resolve) => window.setTimeout(resolve, delayMs));
          }
          return window.__MOCK_BUNDLES__['generated-box'];
        }

        if (cmd === 'verify_generated_model') {
          return {
            passed: true,
            summary: 'Mock structural verification passed.',
            issues: [],
            metrics: {
              partCount: 2,
              previewStlSizeBytes: 1024,
              totalVolume: null,
              totalArea: null,
              bbox: null,
            },
            verifierStatus: 'ok',
            verifierSource: 'rust_structural',
          };
        }

        if (cmd === 'verify_render') {
          return {
            passed: true,
            summary: 'Mock visual verification passed.',
            issues: [],
            usage: null,
          };
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
              if (args.artifactBundle) assistantMsg.artifactBundle = args.artifactBundle;
              if (args.modelManifest) assistantMsg.modelManifest = args.modelManifest;
            }
          }
          if (args.status === 'success') {
            window.__MOCK_HISTORY__[0].pendingCount = 0;
            if (args.design) {
              window.__MOCK_HISTORY__[0].versionCount = 1;
              window.__MOCK_HISTORY__[0].title = args.design.title;
              window.__MOCK_LAST_DESIGN__ = {
                design: args.design,
                threadId,
                messageId: args.messageId,
                artifactBundle: args.artifactBundle ?? null,
                modelManifest: args.modelManifest ?? null,
                selectedPartId: null,
              };
            }
          }
          return null;
        }

        if (cmd === 'get_thread_latest_version') {
          const thread = window.__MOCK_THREADS__[args.threadId];
          if (!thread) return null;
          return [...thread.messages].reverse().find((message) =>
            message.role === 'assistant' && (message.output || message.artifactBundle || message.modelManifest),
          ) ?? null;
        }

        if (cmd === 'get_thread_messages_page') {
          const thread = window.__MOCK_THREADS__[args.threadId];
          return {
            messages: thread?.messages ?? [],
            nextBefore: null,
            hasMore: false,
          };
        }

        if (cmd === 'add_manual_version') {
          const input = args.input ?? args;
          const messageId = 'manual-' + Math.random();
          const threadId = input.threadId;
          const historyThread = upsertHistoryThread(threadId, input.title);
          historyThread.versionCount = (historyThread.versionCount || 0) + 1;
          historyThread.updatedAt = Date.now() / 1000;
          historyThread.title = input.title;

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
              title: input.title,
              versionName: input.versionName,
              macroCode: input.macroCode,
              uiSpec: input.uiSpec,
              initialParams: input.parameters,
              response: 'Manual edit committed as new version.',
              interactionMode: 'design',
            },
            artifactBundle: input.artifactBundle,
            modelManifest: input.modelManifest,
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
          const thread = window.__MOCK_THREADS__[args.id];
          const historyThread = window.__MOCK_HISTORY__.find((entry) => entry.id === args.id) ?? null;
          if (!thread) {
            return {
              id: args.id,
              title: historyThread?.title ?? 'Mock Thread',
              updatedAt: historyThread?.updatedAt ?? Date.now() / 1000,
              versionCount: historyThread?.versionCount ?? 0,
              pendingCount: historyThread?.pendingCount ?? 0,
              errorCount: historyThread?.errorCount ?? 0,
              summary: historyThread?.summary ?? '',
              messages: [],
            };
          }
          return {
            id: args.id,
            title: historyThread?.title ?? thread.title ?? 'Mock Thread',
            updatedAt: historyThread?.updatedAt ?? Date.now() / 1000,
            versionCount: historyThread?.versionCount ?? 0,
            pendingCount: historyThread?.pendingCount ?? 0,
            errorCount: historyThread?.errorCount ?? 0,
            summary: historyThread?.summary ?? '',
            messages: thread.messages,
          };
        }
        return {};
      };
    }, options);
  }

  async function gotoWorkbench(page: Page) {
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
  }

  async function openDialogue(page: Page) {
    const dialogueWindow = page.locator('[data-window-id="dialogue"]');
    if (!(await dialogueWindow.isVisible().catch(() => false))) {
      await page.getByRole('button', { name: 'DIALOGUE' }).click();
    }
    await expect(dialogueWindow).toBeVisible();
  }

  async function openParams(page: Page) {
    const paramsWindow = page.locator('[data-window-id="params"]');
    if (!(await paramsWindow.isVisible().catch(() => false))) {
      await page.getByRole('button', { name: 'PARAMS' }).click();
    }
    await expect(paramsWindow).toBeVisible();
  }

  async function numericZIndex(target: Locator) {
    return target.evaluate((element) => Number.parseInt(window.getComputedStyle(element).zIndex || '0', 10));
  }

  test('asking a question should show Ecky response without creating design', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await openDialogue(page);

    const textarea = page.locator('.prompt-input');
    await textarea.fill('How does this work?');
    
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await sendBtn.click();

    await page.waitForSelector('.microwave-unit', { timeout: 5000 });
    await page.waitForSelector('.mw-thinking-result', { timeout: 5000 });

    await expect(page.locator('.mw-thinking-result')).toContainText('ADVICE');
    await expect(page.locator('.microwave-unit').filter({ hasText: 'How does this work?' })).toBeVisible();
    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_model')).toBeFalsy();
  });

  test('requesting a design should trigger rendering and model update', async ({ page }) => {
    page.on('console', msg => console.log(`[PAGE] ${msg.type()}: ${msg.text()}`));
    await setupMocks(page);
    await gotoWorkbench(page);

    await openDialogue(page);

    const textarea = page.locator('.prompt-input');
    await textarea.fill('Create a box');
    await page.click('button:has-text("PROCESS")');

    await page.waitForSelector('.microwave-unit', { timeout: 5000 });
    await openParams(page);
    await page.waitForSelector('.part-chip', { timeout: 10000 });
    await expect(page.locator('.part-chip')).toContainText(['Shell', 'Lid']);
    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_model')).toBeTruthy();
  });

  test('normal model render shows viewport transmutation outside boot initialization', async ({ page }) => {
    await setupMocks(page, { renderModelDelayMs: 1200 });
    await gotoWorkbench(page);

    await openDialogue(page);
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });

    await expect(page.locator('.viewport-transmutation')).toBeVisible();
    await expect(page.locator('.viewport-transmutation')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  });

  test('design flow should use the model runtime commands', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await openDialogue(page);
    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');

    await openParams(page);
    await page.waitForSelector('.part-chip', { timeout: 10000 });
    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_model')).toBeTruthy();
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'get_model_manifest')).toBeTruthy();
    expect(calls.some((entry: { cmd: string }) => entry.cmd === 'render_stl')).toBeFalsy();
  });

  test('Given fork confirmation is cancelled When viewer fork is clicked Then no new thread is created', async ({
    page,
  }) => {
    await setupMocks(page, { forkConfirmResult: false });
    await gotoWorkbench(page);

    await openDialogue(page);
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });
    const forkButton = page.getByRole('button', { name: /FORK/i });
    await expect(forkButton).toBeEnabled();

    const before = await page.evaluate(() => ({
      threadIds: Object.keys((window as any).__MOCK_THREADS__),
      addVersionCalls: (window as any).__MOCK_CALLS__.filter((entry: { cmd: string }) =>
        ['add_manual_version', 'add_imported_model_version'].includes(entry.cmd),
      ).length,
    }));

    await forkButton.click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) => entry.cmd === 'plugin:dialog|confirm').length;
      })
      .toBe(1);
    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) =>
          ['add_manual_version', 'add_imported_model_version'].includes(entry.cmd),
        ).length;
      })
      .toBe(before.addVersionCalls);

    const afterThreadIds = await page.evaluate(() => Object.keys((window as any).__MOCK_THREADS__));
    expect(afterThreadIds).toEqual(before.threadIds);
  });

  test('Given fork confirmation is accepted When viewer fork is clicked Then version copies into a new thread', async ({
    page,
  }) => {
    await setupMocks(page, { forkConfirmResult: true });
    await gotoWorkbench(page);

    await openDialogue(page);
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });
    const forkButton = page.getByRole('button', { name: /FORK/i });
    await expect(forkButton).toBeEnabled();

    const beforeThreadCount = await page.evaluate(() => Object.keys((window as any).__MOCK_THREADS__).length);

    await forkButton.click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) => entry.cmd === 'plugin:dialog|confirm').length;
      })
      .toBe(1);
    await expect
      .poll(async () => {
        return page.evaluate(() => Object.keys((window as any).__MOCK_THREADS__).length);
      })
      .toBe(beforeThreadCount + 1);
    const forkWrite = await page.evaluate(() =>
      (window as any).__MOCK_CALLS__.find((entry: { cmd: string }) =>
        ['add_manual_version', 'add_imported_model_version'].includes(entry.cmd),
      ),
    );
    expect(forkWrite?.cmd).toBeTruthy();
  });

  test('Given code inspector forks a version When new thread becomes active Then code remains clickable', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1400, height: 900 });
    await setupMocks(page, { forkConfirmResult: true });
    await gotoWorkbench(page);

    await openDialogue(page);
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });

    const viewportCodeButton = page.getByRole('button', { name: /CODE/i }).first();
    await expect(viewportCodeButton).toBeEnabled();
    await viewportCodeButton.click();
    await expect(page.locator('.cm-content')).toContainText('create_box()');

    await page.getByRole('button', { name: /FORK TO NEW THREAD/i }).click();
    await expect(page.locator('.code-modal-content')).toHaveCount(0);

    const restoredCodeButton = page.getByRole('button', { name: /CODE/i }).first();
    await expect(restoredCodeButton).toBeEnabled();
    await restoredCodeButton.click();
    await expect(page.locator('.cm-content')).toContainText('create_box()');
  });

  test('Given floating window header is inconvenient When dragging visible content and double-clicking body Then window moves and fits viewport', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1180, height: 680 });
    await setupMocks(page);
    await gotoWorkbench(page);

    await openDialogue(page);
    const dialogueWindow = page.locator('[data-window-id="dialogue"]');
    await expect(dialogueWindow).toBeVisible();

    const toolbar = dialogueWindow.locator('.dialogue-toolbar');
    await expect(toolbar).toBeVisible();
    const dragPoint = await toolbar.evaluate((node) => {
      const toolbarRect = node.getBoundingClientRect();
      const labelRect = (node.querySelector('.dialogue-toolbar__remember') as HTMLElement | null)?.getBoundingClientRect() ?? toolbarRect;
      const gapLeft = labelRect.right + 16;
      const gapRight = toolbarRect.right - 16;
      const x = gapLeft < gapRight ? (gapLeft + gapRight) / 2 : toolbarRect.left + toolbarRect.width * 0.8;
      return {
        x,
        y: toolbarRect.top + toolbarRect.height / 2,
      };
    });

    const beforeBox = await dialogueWindow.boundingBox();
    expect(beforeBox).not.toBeNull();

    const viewportSize = page.viewportSize();
    expect(viewportSize).not.toBeNull();
    expect((beforeBox?.x ?? 0) + (beforeBox?.width ?? 0)).toBeLessThanOrEqual((viewportSize?.width ?? 0) + 1);
    expect((beforeBox?.y ?? 0) + (beforeBox?.height ?? 0)).toBeLessThanOrEqual((viewportSize?.height ?? 0) + 1);

    await page.mouse.dblclick(dragPoint.x, dragPoint.y);

    await expect
      .poll(async () => {
        const box = await dialogueWindow.boundingBox();
        if (!box || !viewportSize) return null;
        return {
          withinViewport:
            box.x >= 0 &&
            box.y >= 0 &&
            box.x + box.width <= (viewportSize?.width ?? 0) + 1 &&
            box.y + box.height <= (viewportSize?.height ?? 0) + 1,
        };
      })
      .toEqual({ withinViewport: true });
  });

  test('Given plain text inside floating window When pointer drags across text Then window stays put', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1180, height: 680 });
    await setupMocks(page);
    await gotoWorkbench(page);

    await openDialogue(page);
    await page.evaluate(() => {
      const host = document.querySelector('[data-window-id="dialogue"] .dialogue-content');
      if (!host) return;
      const existing = document.getElementById('window-selection-proof');
      existing?.remove();
      const probe = document.createElement('div');
      probe.id = 'window-selection-proof';
      probe.textContent = 'Selection proof text stays selectable inside floating window.';
      probe.style.padding = '12px';
      probe.style.fontFamily = 'var(--font-mono)';
      probe.style.fontSize = '14px';
      probe.style.lineHeight = '1.4';
      probe.style.userSelect = 'text';
      host.appendChild(probe);
    });
    const bubbleText = page.locator('#window-selection-proof');
    await expect(bubbleText).toBeVisible();

    const dialogueWindow = page.locator('[data-window-id="dialogue"]');
    const beforeBox = await dialogueWindow.boundingBox();
    const textBox = await bubbleText.boundingBox();
    expect(beforeBox).not.toBeNull();
    expect(textBox).not.toBeNull();

    const startX = (textBox?.x ?? 0) + Math.max(12, (textBox?.width ?? 0) * 0.2);
    const endX = (textBox?.x ?? 0) + Math.max(48, (textBox?.width ?? 0) * 0.7);
    const y = (textBox?.y ?? 0) + Math.max(10, (textBox?.height ?? 0) * 0.5);

    await page.mouse.move(startX, y);
    await page.mouse.down();
    await page.mouse.move(endX, y, { steps: 12 });
    await page.mouse.up();

    const afterBox = await dialogueWindow.boundingBox();
    expect(afterBox).not.toBeNull();
    expect(Math.abs((afterBox?.x ?? 0) - (beforeBox?.x ?? 0))).toBeLessThanOrEqual(2);
    expect(Math.abs((afterBox?.y ?? 0) - (beforeBox?.y ?? 0))).toBeLessThanOrEqual(2);
  });

  test('Given mesh model lacks STEP artifact When export chooser opens Then direct OCCT blocker is shown', async ({
    page,
  }) => {
    await setupMocks(page, { directOcctDetail: 'Direct OCCT unavailable: missing TKDESTEP' });
    await gotoWorkbench(page);

    await openDialogue(page);
    await expect(page.locator('.prompt-input')).toBeVisible();
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });
    await expect(page.getByRole('button', { name: /EXPORT/i })).toBeVisible();
    await page.getByRole('button', { name: /EXPORT/i }).click();

    const stepButton = page.locator('.export-chooser__action', { hasText: 'STEP' });
    await expect(stepButton).toBeVisible();
    await expect(stepButton).toBeDisabled();
    await expect(stepButton).toContainText(
      'STEP unavailable for mesh/EckyRust render: Direct OCCT unavailable: missing TKDESTEP',
    );
  });

  test('Given mesh model lacks STEP artifact When model renders Then direct OCCT STEP status shows blocker', async ({
    page,
  }) => {
    await setupMocks(page, { directOcctDetail: 'Direct OCCT unavailable: missing TKDESTEP' });
    await gotoWorkbench(page);

    await openDialogue(page);
    await expect(page.locator('.prompt-input')).toBeVisible();
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });

    const status = page.getByLabel('Direct OCCT STEP status');
    await expect(status).toBeVisible();
    await expect(status).toContainText('DIRECT OCCT STEP FAST PATH');
    await expect(status).toContainText('BLOCKED');
    await expect(status).toContainText('Direct OCCT unavailable: missing TKDESTEP');
  });

  test('Given direct OCCT is ready but mesh bundle has no STEP When export chooser opens Then BRep artifact absence is shown', async ({
    page,
  }) => {
    await setupMocks(page, { directOcctAvailable: true, directOcctDetail: 'Direct OCCT ready' });
    await gotoWorkbench(page);

    await openDialogue(page);
    await expect(page.locator('.prompt-input')).toBeVisible();
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });
    await expect(page.getByRole('button', { name: /EXPORT/i })).toBeVisible();
    await page.getByRole('button', { name: /EXPORT/i }).click();

    const stepButton = page.locator('.export-chooser__action', { hasText: 'STEP' });
    await expect(stepButton).toBeVisible();
    await expect(stepButton).toBeDisabled();
    await expect(stepButton).toContainText(
      'STEP unavailable for mesh/EckyRust render: no BRep STEP artifact was produced.',
    );
  });

  test('Given direct OCCT is ready but mesh bundle has no STEP When model renders Then direct OCCT status explains no BRep artifact', async ({
    page,
  }) => {
    await setupMocks(page, { directOcctAvailable: true, directOcctDetail: 'Direct OCCT ready' });
    await gotoWorkbench(page);

    await openDialogue(page);
    await expect(page.locator('.prompt-input')).toBeVisible();
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });

    const status = page.getByLabel('Direct OCCT STEP status');
    await expect(status).toBeVisible();
    await expect(status).toContainText('DIRECT OCCT STEP FAST PATH');
    await expect(status).toContainText('READY / NO STEP');
    await expect(status).toContainText('no BRep STEP artifact was produced');
  });

  test('Given rendered model has STEP artifact When STEP export selected Then source artifact is copied', async ({
    page,
  }) => {
    await setupMocks(page, { stepArtifact: true });
    await gotoWorkbench(page);

    await openDialogue(page);
    await expect(page.locator('.prompt-input')).toBeVisible();
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });
    await expect(page.getByRole('button', { name: /EXPORT/i })).toBeVisible();
    await page.getByRole('button', { name: /EXPORT/i }).click();

    const stepButton = page.locator('.export-chooser__action', { hasText: 'STEP' });
    await expect(stepButton).toBeEnabled();
    await stepButton.click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.find((entry: { cmd: string }) => entry.cmd === 'export_file')?.args ?? null;
      })
      .toEqual({ sourcePath: '/mock/output.step', targetPath: '/mock/exported.step' });
    await expect(page.locator('.export-chooser')).toHaveCount(0);
  });

  test('Given rendered model has STEP artifact When model renders Then direct OCCT status shows STEP ready', async ({
    page,
  }) => {
    await setupMocks(page, { stepArtifact: true, directOcctAvailable: true, directOcctDetail: 'Direct OCCT ready' });
    await gotoWorkbench(page);

    await openDialogue(page);
    await expect(page.locator('.prompt-input')).toBeVisible();
    await page.locator('.prompt-input').fill('Create a box');
    await page.locator('button:has-text("PROCESS")').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });

    const status = page.getByLabel('Direct OCCT STEP status');
    await expect(status).toBeVisible();
    await expect(status).toContainText('DIRECT OCCT STEP FAST PATH');
    await expect(status).toContainText('STEP READY');
    await expect(status).toContainText('output.step');
  });

  test('selected parts expose editable controls in the main viewer overlay', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await openDialogue(page);

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await openParams(page);
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    await page.locator('.part-chip').filter({ hasText: 'Shell' }).click();
    await expect(page.locator('.part-chip.part-chip-active')).toContainText('Shell');

    const overlay = page.locator('.viewer-part-overlay');
    await expect(overlay).toContainText('Size');
    await expect(overlay).toContainText('Height');

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
    await gotoWorkbench(page);

    await openDialogue(page);

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await openParams(page);
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
    await expect(page.locator('.viewer-overlay-readout').first()).toHaveValue('16');
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
    await gotoWorkbench(page);

    await openDialogue(page);

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await openParams(page);
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
    await gotoWorkbench(page);

    await openDialogue(page);

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await openParams(page);
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
    await gotoWorkbench(page);

    await openDialogue(page);

    await page.locator('.prompt-input').fill('Create a box');
    await page.click('button:has-text("PROCESS")');
    await openParams(page);
    await page.waitForSelector('.part-chip', { timeout: 10000 });

    const viewer = page.locator('.viewer-host').first();
    const bounds = await viewer.boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) throw new Error('viewer bounds missing');

    await page.waitForTimeout(250);
    for (const ratio of [0.18, 0.24, 0.3]) {
      await page.mouse.click(bounds.x + bounds.width * ratio, bounds.y + bounds.height * 0.54);
      const overlayText = (await page.locator('.viewer-part-overlay').allTextContents())[0] ?? null;
      if (overlayText?.includes('Size')) break;
    }

    await expect(page.locator('.part-chip.part-chip-active')).toContainText('Shell');
    await expect(page.locator('.viewer-part-overlay')).toContainText('Size');
  });

  test('importing a macro should create a manual version via the manual commit path', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await page.locator('button[title="New project"]').click();
    await page.getByRole('button', { name: /Import Macro/i }).click();
    await page.getByPlaceholder('Paste FreeCAD macro (Python) here...').fill('print(\"manual\")');
    await page.getByRole('button', { name: /CREATE THREAD/i }).click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) => entry.cmd === 'add_manual_version').length;
      })
      .toBeGreaterThan(0);
  });

  test('project choosers should not expose canonical cup entry', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await page.locator('button[title="New project"]').click();
    await expect(page.getByRole('button', { name: /Canonical Cup/i })).toHaveCount(0);
    await page.keyboard.press('Escape');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: /\+ NEW/i }).click();
    await expect(page.getByRole('button', { name: /Canonical Cup/i })).toHaveCount(0);
  });

  test('Given visible projects window When new project chooser opens from both entry points Then chooser stacks above floating windows', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow).toBeVisible();

    const assertChooserAboveProjects = async () => {
      const modalBackdrop = page.locator('.modal-backdrop');
      await expect(modalBackdrop).toBeVisible();
      await expect(modalBackdrop).toContainText('Start New Project');
      expect(await numericZIndex(modalBackdrop)).toBeGreaterThan(await numericZIndex(projectsWindow));
      await page.keyboard.press('Escape');
      await expect(modalBackdrop).toHaveCount(0);
    };

    await page.locator('button[title="New project"]').click();
    await assertChooserAboveProjects();

    await projectsWindow.click({ position: { x: 20, y: 20 } });
    await page.getByRole('button', { name: /\+ NEW/i }).click();
    await assertChooserAboveProjects();
  });

  test('imported FCStd proposals persist after review and version reload', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await page.locator('button[title="New project"]').click();
    await page.getByRole('button', { name: /Import FreeCAD|Import FCStd/i }).click();

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

    const importDialog = page.getByRole('dialog');
    await expect(importDialog).toContainText('IMPORT ENRICHMENT');
    await expect(importDialog.locator('.proposal-card').last()).toContainText('Expose Outer Shell dimensions');
    await expect(importDialog.locator('.proposal-status').last()).toContainText('PENDING');

    await importDialog.getByRole('button', { name: 'ACCEPT ALL' }).click();
    await expect(importDialog.locator('.proposal-status').last()).toContainText('ACCEPTED');
    await importDialog.getByRole('button', { name: 'APPLY' }).click();

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('.project-card', { hasText: 'Imported Shell' }).getByRole('button', { name: 'OPEN' }).click();
    await openParams(page);
    await page.locator('.part-chip', { hasText: 'Outer Shell' }).click();
    await expect(page.locator('.param-field', { hasText: 'Outer Shell Width' })).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
    const saveManifestCall = calls.find((entry: { cmd: string }) => entry.cmd === 'save_model_manifest');
    expect(saveManifestCall?.args?.messageId).toBeTruthy();
  });

  test('accepted imported bindings become editable and persist control values', async ({ page }) => {
    await setupMocks(page);
    await gotoWorkbench(page);

    await page.locator('button[title="New project"]').click();
    await page.getByRole('button', { name: /Import FreeCAD|Import FCStd/i }).click();
    const importDialog = page.getByRole('dialog');
    await expect(importDialog.locator('.proposal-card').last()).toContainText('Expose Outer Shell dimensions');

    await importDialog.getByRole('button', { name: 'ACCEPT ALL' }).click();
    await expect(importDialog.locator('.proposal-status').last()).toContainText('ACCEPTED');
    await importDialog.getByRole('button', { name: 'APPLY' }).click();

    await openParams(page);
    await page.locator('.part-chip', { hasText: 'Outer Shell' }).click();
    await expect(page.locator('.param-field', { hasText: 'Outer Shell Width' })).toBeVisible();
    await expect(page.locator('.viewer-part-overlay')).toContainText('Outer Shell');
    await expect(page.locator('.viewer-part-overlay')).toContainText('Outer Shell Width');
    await expect(page.getByPlaceholder('Filter controls...')).toBeVisible();

    await page.locator('.param-field', { hasText: 'Outer Shell Width' }).hover();
    await expect(page.locator('.viewer-dimension-layer')).toContainText('Outer Shell');

    await page.locator('.viewer-part-overlay input[type="number"]').first().evaluate((element) => {
      const input = element as HTMLInputElement;
      input.value = '48';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });

    await expect(page.locator('.viewer-part-overlay input[type="number"]').first()).toHaveValue('48');
    await expect(page.locator('.apply-btn')).toBeEnabled();
    await page.locator('.apply-btn').click();

    await expect
      .poll(async () => {
        const calls = await page.evaluate(() => (window as any).__MOCK_CALLS__);
        return calls.filter((entry: { cmd: string }) => entry.cmd === 'apply_imported_model').length;
      })
      .toBeGreaterThan(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('.project-card', { hasText: 'Imported Shell' }).getByRole('button', { name: 'OPEN' }).click();
    await openParams(page);
    await page.locator('.part-chip', { hasText: 'Outer Shell' }).click();
    await expect(page.locator('.viewer-part-overlay input[type="number"]').first()).toHaveValue('48');
  });
});
