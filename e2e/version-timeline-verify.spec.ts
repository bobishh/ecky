import { expect, test } from '@playwright/test';

function installVersionTimelineMocks(options?: { includeFailedHead?: boolean; compactSelectedVersion?: boolean }) {
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
              severity: 'error',
              message: 'Gap below minimum.',
              stableNodeId: 'verify:rib_clearance',
              metricSource: 'clearance',
              metricKey: 'min-distance',
              comparator: '>=',
              expected: { kind: 'number', value: 0.3 },
              actual: { kind: 'number', value: 0.12 },
            },
            {
              tag: 'step_export',
              status: 'passed',
              severity: 'error',
              message: 'STEP export present.',
              stableNodeId: null,
              metricSource: 'manifest',
              metricKey: 'has-step',
              comparator: '==',
              expected: { kind: 'boolean', value: true },
              actual: { kind: 'boolean', value: true },
            },
            {
              tag: 'triangle_budget',
              status: 'failed',
              severity: 'warning',
              intent: 'Keep preview responsive',
              message: 'Triangle budget exceeded.',
              stableNodeId: null,
            },
            {
              tag: 'assembly_connected',
              status: 'skipped',
              severity: 'error',
              intent: 'Assembly must remain connected',
              condition: 'assembly-preview',
              conditionResult: false,
              skipReason: 'Authored `when` condition resolved false.',
              message: 'Skipped.',
              stableNodeId: null,
            },
          ],
          metrics: {
            partCount: 2,
            totalVolume: 10,
            totalArea: 8,
            bbox: null,
            modelStlSizeBytes: 128,
            modelStlTriangleCount: 64,
            modelStlComponentCount: 1,
            modelStlNonManifoldEdgeCount: 0,
            modelStlOverhangTriangleCount: 0,
            modelStlOverhangRatio: 0,
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
          fcstdPath: '/mock/model-runtime/model.FCStd',
          manifestPath: '/mock/model-runtime/manifest.json',
          modelStlPath: '/mock/model-runtime/model.stl',
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
    ] as Array<Record<string, unknown>>,
  };

  if (options?.includeFailedHead) {
    thread.messages.push({
      id: 'msg-failed-head',
      role: 'assistant',
      content: 'line 17: unexpected closing parenthesis',
      status: 'error',
      timestamp: Date.UTC(2026, 5, 13) / 1000 + 1,
      output: {
        title: 'Broken bracket draft',
        versionName: 'V2',
        response: 'line 17: unexpected closing parenthesis',
        interactionMode: 'tune',
        macroCode: '(model (part broken (box 10 4 20))))',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      },
      structuralVerification: null,
      artifactBundle: null,
      modelManifest: null,
      agentOrigin: {
        hostLabel: 'Codex MCP Client',
        clientKind: 'mcp-http',
        agentLabel: 'Ecky',
        llmModelId: 'mock-model',
        llmModelLabel: 'Mock Model',
        sessionId: 'session-failed-draft',
        createdAt: Date.UTC(2026, 5, 13) / 1000 + 1,
      },
    });
    thread.versionCount = 2;
    thread.errorCount = 1;
    thread.updatedAt += 1;
  }

  // Real workspace pages retain lightweight identity after full version payloads are evicted.
  for (const message of thread.messages) {
    const output = message.output as { title: string; versionName: string };
    const bundle = message.artifactBundle as { modelId: string } | null;
    message.versionSummary = {
      title: output.title, versionName: output.versionName, modelId: bundle?.modelId ?? null,
      hasOutput: true, hasRuntime: Boolean(bundle), hasManifest: Boolean(message.modelManifest),
    };
  }

  return async ({ page }: { page: import('@playwright/test').Page }) => {
    await page.route(/\/model-runtime\/model\.stl(?:\?.*)?$/, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body: `solid mock
facet normal 0 0 1
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
    await page.addInitScript(({ thread, compactSelectedVersion }) => {
      const mockWindow = window as any;
      localStorage.clear();
      mockWindow.__versionProjectionCalls = [];

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
        mockWindow.__versionProjectionCalls.push({ cmd, args: structuredClone(args ?? {}) });
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
        if (cmd === 'open_inventory_thread_intent') {
          return {
            thread: { ...structuredClone(thread), messages: [] },
            selectedVersion: compactSelectedVersion
              ? { ...structuredClone(thread.messages.at(-1)), output: null, artifactBundle: null, modelManifest: null }
              : structuredClone(thread.messages.at(-1)),
            messagesPage: { messages: structuredClone(thread.messages), hasMore: false, nextBefore: null, observedBytes: 0, truncatedFields: [] },
            requestedMessageFound: false,
          };
        }
        if (cmd === 'get_thread') return structuredClone(thread);
        if (cmd === 'get_thread_latest_version') {
          return structuredClone(thread.messages.at(-1));
        }
        if (cmd === 'get_thread_message_version') {
          return structuredClone(
            thread.messages.find((message) => message.id === args?.messageId) ?? null,
          );
        }
        if (cmd === 'get_version_detail') {
          const message = thread.messages.find((candidate) => candidate.id === args?.messageId) ?? null;
          if (!message) return null;
          const projected = structuredClone(message) as any;
          const edgeTargets = projected.artifactBundle?.edgeTargets ?? [];
          const faceTargets = projected.artifactBundle?.faceTargets ?? [];
          const selectionTargets = projected.modelManifest?.selectionTargets ?? [];
          if (projected.artifactBundle) {
            projected.artifactBundle.edgeTargets = [];
            projected.artifactBundle.faceTargets = [];
          }
          if (projected.modelManifest) projected.modelManifest.selectionTargets = [];
          return {
            message: projected,
            denseTopologyRef: 'version-topology:msg-verify',
            edgeCount: edgeTargets.length,
            faceCount: faceTargets.length,
            selectionTargetCount: selectionTargets.length,
            observedBytes: 4096,
            truncatedFields: [],
            availableSections: ['core', 'denseTopology'],
          };
        }
        if (cmd === 'get_dense_topology_page') {
          const message = thread.messages.find((candidate) => candidate.id === args?.messageId) as any;
          const kind = args?.kind;
          const values = kind === 'edge'
            ? message?.artifactBundle?.edgeTargets ?? []
            : kind === 'face'
              ? message?.artifactBundle?.faceTargets ?? []
              : message?.modelManifest?.selectionTargets ?? [];
          return {
            snapshotRef: 'version-topology:msg-verify',
            kind,
            items: values.map((value: unknown) => ({ kind, value })),
            nextCursor: null,
            totalCount: values.length,
            observedBytes: 1024,
          };
        }
        if (cmd === 'get_thread_messages_page') {
          return {
            messages: structuredClone(thread.messages),
            hasMore: false,
            nextBefore: null,
          };
        }
        if (cmd === 'get_project_source') {
          const latestOutput = thread.messages.at(-1)?.output as { macroCode?: string } | undefined;
          return {
            threadId: thread.id,
            folder: '/mock/projects/thread-verify',
            file: '/mock/projects/thread-verify/model.ecky',
            source: latestOutput?.macroCode ?? '',
            sourceDigest: 'sha256:verify-source',
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
        if (cmd === 'get_agent_activity') {
          return { events: [], latestCursor: 0, oldestCursor: 0, droppedCount: 0 };
        }
        if (cmd === 'get_agent_terminal_snapshots') return [];
        if (cmd === 'get_mcp_server_status') return [];
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        if (cmd === 'get_default_macro') return '# mock macro';
        return null;
      };
    }, { thread, compactSelectedVersion: options?.compactSelectedVersion ?? false });
  };
}

test('Given persisted authored verify chips When opening version thread Then chips render and stable node click focuses authored source', async ({ page }) => {
  await installVersionTimelineMocks()({ page });

  await page.goto('/');
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'COMPLETED' }).click();
  await page.locator('.project-card', { hasText: 'Verify Timeline Thread' }).getByRole('button', { name: 'VIEW' }).click();
  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.getByRole('button', { name: 'Parameters' }).click();

  const failedChip = page.getByRole(
    'button',
    { name: /Authored verify rib_clearance: clearance min-distance expected >= 0\.3; actual 0\.12/i },
  );
  const passedChip = page.getByRole(
    'button',
    { name: /Authored verify step_export: manifest has-step expected == true; actual true/i },
  );
  const warningChip = page.getByRole('button', {
    name: /Authored verify triangle_budget: Keep preview responsive — Triangle budget exceeded\./i,
  });
  const skippedChip = page.getByRole('button', {
    name: /Authored verify assembly_connected: Assembly must remain connected — when assembly-preview: false/i,
  });

  await expect(page.getByText('IMMUTABLE VERSION', { exact: true })).toHaveCount(0);
  await expect(page.getByText('TUNING NOTE', { exact: true })).toHaveCount(0);
  expect(await page.locator('.trail-content').first().evaluate((el) => parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThanOrEqual(12);
  expect(await failedChip.evaluate((el) => parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThanOrEqual(10);
  await expect(failedChip).toBeVisible();
  await expect(passedChip).toBeDisabled();
  await expect(warningChip).toHaveClass(/trail-authored-verify__chip--amber/);
  await expect(skippedChip).toHaveClass(/trail-authored-verify__chip--neutral/);
  await expect(warningChip).toBeDisabled();
  await expect(skippedChip).toBeDisabled();

  await failedChip.click();

  await expect(page.getByTestId('macro-source-pane')).toBeVisible();
  await expect(page.getByText(/EDIT SOURCE \/ RIB_CLEARANCE/i)).toBeVisible();
});

test('Given compact version metadata When opening a version Then core loads first and topology hydrates by bounded pages', async ({ page }) => {
  await installVersionTimelineMocks({ compactSelectedVersion: true })({ page });

  await page.goto('/');
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'COMPLETED' }).click();
  await page.locator('.project-card', { hasText: 'Verify Timeline Thread' }).getByRole('button', { name: 'VIEW' }).click();

  await expect.poll(async () => page.evaluate(() => (
    (window as any).__versionProjectionCalls as Array<{ cmd: string }>
  ).some((call) => call.cmd === 'get_version_detail'))).toBe(true);
  const calls = await page.evaluate(() => (window as any).__versionProjectionCalls as Array<{ cmd: string }>);
  expect(calls.some((call) => call.cmd === 'get_thread')).toBe(false);
  expect(calls.some((call) => call.cmd === 'get_dense_topology_page')).toBe(true);
});

test('Given a failed artifact-less draft is newest When opening its thread Then it remains head and visible version history', async ({ page }) => {
  await installVersionTimelineMocks({ includeFailedHead: true })({ page });

  await page.goto('/');
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'COMPLETED' }).click();
  await page.locator('.project-card', { hasText: 'Verify Timeline Thread' }).getByRole('button', { name: 'VIEW' }).click();
  await page.getByRole('button', { name: 'DIALOGUE' }).click();

  await expect(page.locator('.version-counter')).toHaveText(/V 2 OF 2/);
  await expect(page.locator('.version-title')).toHaveText('Broken bracket draft');
  await expect(page.locator('.trail-active-version')).toContainText('line 17: unexpected closing parenthesis');
  await expect(page.locator('.trail-active-version .trail-status')).toHaveText('ERROR');
  expect(await page.locator('.trail-active-version .trail-status').evaluate((el) => parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThanOrEqual(10);

});

test('Given version authoring controls When editing code or parameters Then Apply is the only version action', async ({ page }) => {
  await installVersionTimelineMocks()({ page });

  await page.goto('/');
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'COMPLETED' }).click();
  await page.locator('.project-card', { hasText: 'Verify Timeline Thread' }).getByRole('button', { name: 'VIEW' }).click();

  await page.getByRole('button', { name: 'Code inspector' }).click();
  await expect(page.getByRole('button', { name: 'APPLY', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /COMMIT VERSION/i })).toHaveCount(0);

  await page.getByRole('button', { name: 'Parameters' }).click();
  await expect(page.getByRole('button', { name: 'COMMIT', exact: true })).toHaveCount(0);
});
