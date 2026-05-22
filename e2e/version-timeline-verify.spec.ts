import { expect, test } from '@playwright/test';

function installVersionTimelineMocks() {
  const thread = {
    id: 'thread-verify',
    title: 'Verify Timeline Thread',
    summary: '',
    updatedAt: Date.UTC(2026, 5, 13),
    versionCount: 1,
    pendingCount: 0,
    queuedCount: 0,
    errorCount: 0,
    status: 'finalized',
    finalizedAt: Date.UTC(2026, 5, 13),
    pendingConfirm: null,
    genieTraits: null,
    messages: [
      {
        id: 'msg-verify',
        role: 'assistant',
        content: 'Bracket ready.',
        status: 'success',
        timestamp: Date.UTC(2026, 5, 13) / 1000,
        output: {
          title: 'Bracket',
          versionName: 'V1',
          response: 'Bracket ready.',
          interactionMode: 'design',
          macroCode: `(model
  (verify rib_clearance (> 1 2))
  (part rib
    (box 10 4 20)))`,
          sourceLanguage: 'ecky',
          geometryBackend: 'build123d',
          uiSpec: { fields: [] },
          initialParams: {},
          postProcessing: null,
        },
        structuralVerification: {
          passed: false,
          summary: 'Authored verify found clearance issue.',
          issues: [],
          authoredVerifyChecks: [
            {
              tag: 'rib_clearance',
              status: 'failed',
              message: 'Gap below minimum.',
              stableNodeId: 'verify:0',
              metricSource: 'clearance',
              metricKey: 'min-distance',
              comparator: '>=',
              expected: { kind: 'number', value: 0.3 },
              actual: { kind: 'number', value: 0.12 },
            },
            {
              tag: 'step_export',
              status: 'passed',
              message: 'STEP export present.',
              stableNodeId: null,
              metricSource: 'manifest',
              metricKey: 'has-step',
              comparator: '==',
              expected: { kind: 'boolean', value: true },
              actual: { kind: 'boolean', value: true },
            },
          ],
          metrics: {
            partCount: 2,
            totalVolume: 10,
            totalArea: 8,
            bbox: null,
            previewStlSizeBytes: 128,
            previewStlTriangleCount: 64,
            previewStlComponentCount: 1,
            previewStlNonManifoldEdgeCount: 0,
            previewStlOverhangTriangleCount: 0,
            previewStlOverhangRatio: 0,
          },
          verifierStatus: 'ok',
          verifierSource: 'native',
        },
        artifactBundle: {
          modelId: 'model-verify',
          sourceKind: 'generated',
          sourceLanguage: 'ecky',
          geometryBackend: 'build123d',
          contentHash: 'hash-verify',
          fcstdPath: '/mock/model.FCStd',
          manifestPath: '/mock/manifest.json',
          previewStlPath: '/mock/model.stl',
          viewerAssets: [],
          exportArtifacts: [],
        },
        modelManifest: {
          modelId: 'model-verify',
          sourceKind: 'generated',
          sourceLanguage: 'ecky',
          geometryBackend: 'build123d',
          document: {
            documentName: 'Bracket',
            documentLabel: 'Bracket',
            objectCount: 2,
            warnings: [],
          },
          parts: [
            {
              partId: 'base',
              label: 'Base',
              editable: true,
              parameterKeys: [],
              viewerNodeIds: ['Base001'],
            },
            {
              partId: 'rib',
              label: 'Rib',
              editable: true,
              parameterKeys: [],
              viewerNodeIds: ['Rib001'],
            },
          ],
          parameterGroups: [],
          selectionTargets: [
            {
              targetId: 'part:base',
              partId: 'base',
              viewerNodeId: 'Base001',
              label: 'Base',
              kind: 'part',
              editable: true,
              parameterKeys: [],
              primitiveIds: [],
              viewIds: [],
              aliasIds: [],
            },
            {
              targetId: 'part:rib',
              partId: 'rib',
              viewerNodeId: 'Rib001',
              label: 'Rib',
              kind: 'part',
              editable: true,
              parameterKeys: [],
              primitiveIds: [],
              viewIds: [],
              aliasIds: [],
            },
          ],
          warnings: [],
          enrichmentState: { status: 'none', proposals: [] },
        },
      },
    ],
  };

  return async ({ page }: { page: import('@playwright/test').Page }) => {
    await page.addInitScript(({ thread }) => {
      const mockWindow = window as any;
      localStorage.clear();

      const config = {
        engines: [{ id: 'mock', name: 'Mock', provider: 'openai', apiKey: '', model: 'mock', baseUrl: '', enabled: true }],
        selectedEngineId: 'mock',
        freecadCmd: '',
        assets: [],
        microwave: { humId: null, dingId: null, muted: true },
        voice: { sttLanguageCode: 'en-US' },
        mcp: { mode: 'passive', autoAgents: [] },
        hasSeenOnboarding: true,
        connectionType: null,
        defaultEngineKind: 'freecad',
        defaultSourceLanguage: 'legacyPython',
        defaultGeometryBackend: 'freecad',
        maxGenerationAttempts: 3,
        maxVerifyAttempts: 0,
      };

      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.metadata = {};
      window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
        const id = Math.floor(Math.random() * 1_000_000_000);
        (window as any)[`_${id}`] = callback;
        return id;
      };
      window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
        if (cmd === 'get_config') return structuredClone(config);
        if (cmd === 'save_config') return null;
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
            directOcct: { available: true, detail: 'Ready', path: '/mock/occt' },
            mesh: { available: true, detail: 'Ready', path: '/mock/mesh' },
            recommendedAuthoringContext: {
              engineKind: 'build123d',
              sourceLanguage: 'ecky',
              geometryBackend: 'build123d',
            },
          };
        }
        if (cmd === 'get_history') {
          return [
            {
              ...thread,
              messages: [],
            },
          ];
        }
        if (cmd === 'get_inventory') {
          return [structuredClone(thread)];
        }
        if (cmd === 'get_thread') return structuredClone(thread);
        if (cmd === 'get_thread_latest_version') return structuredClone(thread.messages[0]);
        if (cmd === 'get_thread_messages_page') {
          return {
            messages: structuredClone(thread.messages),
            hasMore: false,
            nextBefore: null,
          };
        }
        if (cmd === 'macro_ast_source_map') {
          return [
            { id: 'model', kind: 'model', label: 'model', startByte: 0, endByte: 66 },
            { id: 'verify:0', kind: 'verify', label: 'rib_clearance', startByte: 9, endByte: 38 },
            { id: 'part:rib', kind: 'part', label: 'rib', startByte: 41, endByte: 64 },
          ];
        }
        if (cmd === 'get_deleted_messages') return [];
        if (cmd === 'get_last_design') return null;
        if (cmd === 'get_active_agent_sessions') return [];
        if (cmd === 'get_agent_terminal_snapshots') return [];
        if (cmd === 'get_mcp_server_status') return [];
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        if (cmd === 'get_default_macro') return '# mock macro';
        return null;
      };
    }, { thread });
  };
}

test('Given persisted authored verify chips When opening version thread Then chips render and stable node click focuses authored source', async ({ page }) => {
  await installVersionTimelineMocks()({ page });

  await page.goto('/');
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'ARCHIVED' }).click();
  await page.locator('.project-card', { hasText: 'Verify Timeline Thread' }).getByTitle('Open', { exact: true }).click();
  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.getByRole('button', { name: 'PARAMS' }).click();

  const failedChip = page.getByRole(
    'button',
    { name: /Authored verify rib_clearance: clearance min-distance expected >= 0\.3; actual 0\.12/i },
  );
  const passedChip = page.getByRole(
    'button',
    { name: /Authored verify step_export: manifest has-step expected == true; actual true/i },
  );

  await expect(failedChip).toBeVisible();
  await expect(passedChip).toBeDisabled();

  await failedChip.click();

  await expect(page.getByTestId('macro-source-pane')).toBeVisible();
  await expect(page.getByText(/EDIT SOURCE \/ RIB_CLEARANCE/i)).toBeVisible();
});
