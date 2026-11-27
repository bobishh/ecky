import { test, expect, type Page } from '@playwright/test';

const runtimeCapabilities = {
  freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
  build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
  mesh: { available: true, detail: 'bundled', path: null },
  recommendedAuthoringContext: {
    engineKind: 'freecad',
    sourceLanguage: 'legacyPython',
    geometryBackend: 'freecad',
  },
};

const config = {
  engines: [{ id: 'mock', name: 'Mock', provider: 'mock', apiKey: '', baseUrl: '' }],
  selectedEngineId: 'mock',
  freecadCmd: '',
  assets: [],
  microwave: { muted: true },
  voice: { sttLanguageCode: 'en-US' },
  mcp: { port: null, maxSessions: null, mode: 'passive', primaryAgentId: null, promptTimeoutSecs: 1800, autoAgents: [] },
  hasSeenOnboarding: true,
  defaultEngineKind: 'freecad',
  defaultSourceLanguage: 'legacyPython',
  defaultGeometryBackend: 'freecad',
  maxGenerationAttempts: 3,
  maxVerifyAttempts: 1,
};

const artifactBundle = {
  modelId: 'cached-model',
  sourceKind: 'generated',
  engineKind: 'freecad',
  sourceLanguage: 'legacyPython',
  geometryBackend: 'freecad',
  contentHash: 'cached-hash',
  artifactVersion: 1,
  fcstdPath: '/mock/cache/model.FCStd',
  manifestPath: '/mock/cache/manifest.json',
  macroPath: '/mock/cache/source.FCMacro',
  modelStlPath: '/mock/cache/model.stl',
  viewerAssets: [],
};

const modelManifest = {
  modelId: 'cached-model',
  sourceKind: 'generated',
  document: {
    documentName: 'Cached Boot Model',
    documentLabel: 'Cached Boot Model',
    objectCount: 1,
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

const design = {
  title: 'Cached Boot Model',
  versionName: 'Cached',
  response: '',
  interactionMode: 'design',
  macroCode: '# cached macro',
  sourceLanguage: 'legacyPython',
  geometryBackend: 'freecad',
  uiSpec: { fields: [] },
  initialParams: {},
  postProcessing: null,
};

type BootMockOptions = {
  history?: Array<Record<string, unknown>>;
  runtimeDelayMs?: number;
  invalidPrintSetup?: boolean;
  messagesPageMode?: 'full' | 'skinny-active' | 'omits-active';
  runtimeFilesExist?: boolean;
  runtimeSizeBytes?: number;
  runtimeStlFailsOnce?: boolean;
  allowBootRebuild?: boolean;
  rebuildSameArtifact?: boolean;
  renderDelayMs?: number;
  rebuildError?: string;
  lastSnapshotMode?: 'full' | 'missing-manifest' | 'missing-design' | 'none';
  pointedMessageMode?: 'full' | 'missing';
  threadWindowLayout?: Record<string, unknown> | null;
  recoveryState?: {
    terminationCount: number;
    automaticReloadUsed: boolean;
    blocked: boolean;
    rawError: string | null;
    occurredAt: number | null;
  };
};

const MOCK_STL = `solid mock
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid mock
`;

async function installBaseBootMock(page: Page, options: BootMockOptions = {}) {
  let modelStlRequests = 0;
  await page.route(/\/mock\/.*\.stl(?:\?.*)?$/, async (route) => {
    const url = route.request().url();
    const runtimeFilesExist = options.runtimeFilesExist ?? true;
    const allowBootRebuild = options.allowBootRebuild ?? false;
    if (options.runtimeStlFailsOnce && url.includes('/mock/cache/model.stl')) {
      modelStlRequests += 1;
      if (modelStlRequests === 1) {
        await route.fulfill({ status: 404, contentType: 'text/plain', body: 'missing runtime' });
        return;
      }
    }
    if (url.includes('/mock/cache/rebuilt-model.stl')) {
      await route.fulfill({ status: allowBootRebuild ? 200 : 404, contentType: 'model/stl', body: MOCK_STL });
      return;
    }
    if (!runtimeFilesExist) {
      await route.fulfill({ status: 404, contentType: 'text/plain', body: 'missing runtime' });
      return;
    }
    await route.fulfill({ status: 200, contentType: 'model/stl', body: MOCK_STL });
  });

  return page.addInitScript(({ runtimeCapabilities, config, artifactBundle, modelManifest, design, history, runtimeDelayMs, messagesPageMode, runtimeFilesExist, runtimeSizeBytes, allowBootRebuild, rebuildSameArtifact, renderDelayMs, rebuildError, lastSnapshotMode, pointedMessageMode, threadWindowLayout, recoveryState, invalidPrintSetup }) => {
    (window as any).__BOOT_CALLS__ = [];
    (window as any).__BOOT_CAPABILITIES_RESOLVED__ = false;
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    let nextCallbackId = 1;
    window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
      const callbackId = nextCallbackId++;
      (window as unknown as Record<string, unknown>)[`_${callbackId}`] = callback;
      return callbackId;
    };
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      (window as any).__BOOT_CALLS__.push({ cmd, args });
      if (cmd === 'plugin:event|listen') return Number(args?.handler ?? 0);
      if (cmd === 'plugin:event|unlisten') return null;
      if (cmd === 'get_boot_runtime_projection') {
        if (runtimeDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, runtimeDelayMs));
        }
        (window as any).__BOOT_CAPABILITIES_RESOLVED__ = true;
        return { config, capabilities: runtimeCapabilities };
      }
      if (cmd === 'get_boot_projection') {
        const bootHistory = history ?? [
          {
            id: 'thread-boot',
            title: 'Cached Thread',
            summary: 'cached summary',
            messages: [],
            updatedAt: 100,
            versionCount: 1,
            pendingCount: 0,
            queuedCount: 0,
            errorCount: 0,
            isBlank: false,
            status: 'active',
            engineKind: 'freecad',
            sourceLanguage: 'legacyPython',
            geometryBackend: 'freecad',
          },
        ];
        const selectedVersion = {
          id: 'msg-cached',
          role: 'assistant',
          content: 'Cached Boot Model',
          status: 'success',
          output: design,
          artifactBundle,
          modelManifest,
          timestamp: 100,
        };
        const pageMessage = messagesPageMode === 'skinny-active'
          ? {
              ...selectedVersion,
              content: 'Cached Boot Model skinny',
              output: null,
              artifactBundle: null,
              modelManifest: null,
            }
          : messagesPageMode === 'omits-active'
            ? {
                ...selectedVersion,
                id: 'msg-older',
                content: 'Older Boot Model',
                output: null,
                artifactBundle: null,
                modelManifest: null,
                timestamp: 90,
              }
            : selectedVersion;
        const thread = bootHistory.find((entry: any) => entry.id === 'thread-boot') ?? bootHistory[0];
        const workspace = thread
          ? {
              thread: { ...thread, messages: [] },
              messagesPage: {
                messages: [pageMessage],
                nextBefore: null,
                hasMore: false,
                observedBytes: 0,
                truncatedFields: [],
              },
              selectedVersion,
              requestedMessageFound: pointedMessageMode !== 'missing',
            }
          : null;
        return {
          config,
          history: bootHistory,
          workspace,
          selectedPartId: null,
        };
      }
      if (cmd === 'get_config') return config;
      if (cmd === 'save_config') return null;
      if (cmd === 'get_runtime_capabilities') {
        if (runtimeDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, runtimeDelayMs));
        }
        (window as any).__BOOT_CAPABILITIES_RESOLVED__ = true;
        return runtimeCapabilities;
      }
      if (cmd === 'get_history') {
        return history ?? [
          {
            id: 'thread-boot',
            title: 'Cached Thread',
            summary: 'cached summary',
            messages: [],
            updatedAt: 100,
            versionCount: 1,
            pendingCount: 0,
            queuedCount: 0,
            errorCount: 0,
            status: 'active',
            engineKind: 'freecad',
            sourceLanguage: 'legacyPython',
            geometryBackend: 'freecad',
          },
        ];
      }
      if (cmd === 'get_last_design') {
        if (lastSnapshotMode === 'none') return null;
        return {
          design: lastSnapshotMode === 'missing-design' ? null : design,
          threadId: 'thread-boot',
          messageId: 'msg-cached',
          artifactBundle,
          modelManifest: lastSnapshotMode === 'missing-manifest' ? null : modelManifest,
          selectedPartId: null,
        };
      }
      if (cmd === 'get_workspace_projection') {
        const selectedVersion = {
          id: 'msg-cached',
          role: 'assistant',
          content: 'Cached Boot Model',
          status: 'success',
          output: design,
          artifactBundle,
          modelManifest,
          timestamp: 100,
        };
        const pageMessage = messagesPageMode === 'skinny-active'
          ? {
              ...selectedVersion,
              content: 'Cached Boot Model skinny',
              output: null,
              artifactBundle: null,
              modelManifest: null,
            }
          : messagesPageMode === 'omits-active'
            ? {
                ...selectedVersion,
                id: 'msg-older',
                content: 'Older Boot Model',
                output: null,
                artifactBundle: null,
                modelManifest: null,
                timestamp: 90,
              }
            : selectedVersion;
        const thread = (history ?? []).find((entry: any) => entry.id === args?.threadId) ?? {
          id: 'thread-boot',
          title: 'Cached Thread',
          summary: 'cached summary',
          messages: [],
          updatedAt: 100,
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          engineKind: 'freecad',
          sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad',
        };
        return {
          thread: { ...thread, messages: [] },
          messagesPage: {
            messages: [pageMessage],
            nextBefore: null,
            hasMore: false,
            truncatedFields: [],
          },
          selectedVersion,
          requestedMessageFound: pointedMessageMode !== 'missing',
        };
      }
      if (cmd === 'get_thread_latest_version') {
        return {
          id: 'msg-cached',
          role: 'assistant',
          content: 'Cached Boot Model',
          status: 'success',
          output: design,
          artifactBundle,
          modelManifest,
          timestamp: 100,
        };
      }
      if (cmd === 'get_thread_message_version') {
        if (pointedMessageMode === 'missing') return null;
        if (args?.threadId !== 'thread-boot' || args?.messageId !== 'msg-cached') return null;
        return {
          id: 'msg-cached',
          role: 'assistant',
          content: 'Cached Boot Model',
          status: 'success',
          output: design,
          artifactBundle,
          modelManifest,
          timestamp: 100,
        };
      }
      if (cmd === 'get_thread_messages_page') {
        if (messagesPageMode === 'skinny-active') {
          return {
            messages: [
              {
                id: 'msg-cached',
                role: 'assistant',
                content: 'Cached Boot Model skinny',
                status: 'success',
                output: null,
                artifactBundle: null,
                modelManifest: null,
                timestamp: 100,
              },
            ],
            nextBefore: null,
            hasMore: false,
          };
        }
        if (messagesPageMode === 'omits-active') {
          return {
            messages: [
              {
                id: 'msg-older',
                role: 'assistant',
                content: 'Older Boot Model',
                status: 'success',
                output: null,
                artifactBundle: null,
                modelManifest: null,
                timestamp: 90,
              },
            ],
            nextBefore: null,
            hasMore: false,
          };
        }
        return {
          messages: [
            {
              id: 'msg-cached',
              role: 'assistant',
              content: 'Cached Boot Model',
              status: 'success',
              output: design,
              artifactBundle,
              modelManifest,
              timestamp: 100,
            },
          ],
          nextBefore: null,
          hasMore: false,
        };
      }
      if (cmd === 'get_default_macro') return '# default macro';
      if (cmd === 'get_thread_agent_state') {
        return { connectionState: 'disconnected', agentLabel: null, phase: null, statusText: '', busy: false, waitingOnPrompt: false, updatedAt: null };
      }
      if (cmd === 'get_thread_window_layout') return threadWindowLayout;
      if (cmd === 'save_thread_window_layout') return null;
      if (cmd === 'get_web_content_recovery_state') return recoveryState;
      if (cmd === 'acknowledge_web_content_recovery') return null;
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_agent_activity') return { events: [], latestCursor: 0 };
      if (cmd === 'plugin:fs|exists') return runtimeFilesExist;
      if (cmd === 'plugin:fs|size') return runtimeSizeBytes;
      if (cmd === 'repair_version_runtime' && allowBootRebuild) {
        if (renderDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, renderDelayMs));
        }
        if (rebuildError) throw new Error(rebuildError);
        const repairedBundle = rebuildSameArtifact ? artifactBundle : {
          ...artifactBundle,
          modelId: 'cached-model-rebuilt',
          contentHash: 'cached-hash-rebuilt',
          modelStlPath: '/mock/cache/rebuilt-model.stl',
        };
        const repairedManifest = { ...modelManifest, modelId: repairedBundle.modelId };
        const selectedVersion = {
          id: 'msg-cached', role: 'assistant', content: 'Cached Boot Model', status: 'success',
          output: design, artifactBundle: repairedBundle, modelManifest: repairedManifest, timestamp: 100,
        };
        return {
          snapshotId: 'snapshot-repaired',
          artifactIdentity: repairedBundle.contentHash,
          workspace: {
            thread: {
              id: 'thread-boot', title: 'Cached Thread', summary: 'cached summary', messages: [], updatedAt: 100,
              versionCount: 1, pendingCount: 0, queuedCount: 0, errorCount: 0, status: 'active',
              engineKind: 'freecad', sourceLanguage: 'legacyPython', geometryBackend: 'freecad',
            },
            messagesPage: { messages: [selectedVersion], nextBefore: null, hasMore: false, truncatedFields: [] },
            selectedVersion,
            requestedMessageFound: true,
          },
        };
      }
      if (cmd === 'get_model_manifest') return {
        ...modelManifest,
        modelId: args?.modelId ?? 'cached-model-rebuilt',
      };
      if (cmd === 'safe_save_dialog') return '/mock/print.3mf';
      if (cmd === 'export_3mf') {
        if (invalidPrintSetup) {
          throw { code: 'validation', message: 'Invalid print settings in print.orca.json.' };
        }
        return null;
      }
      if (cmd === 'save_model_manifest') return null;
      if (cmd === 'render_model') throw new Error('render_model must not run during cached boot restore');
      if (cmd === 'get_thread') throw new Error('full get_thread must not run during cached boot restore');
      return null;
    };
  }, {
    runtimeCapabilities,
    config,
    artifactBundle,
    modelManifest,
    design,
    history: options.history ?? null,
    runtimeDelayMs: options.runtimeDelayMs ?? 0,
    messagesPageMode: options.messagesPageMode ?? 'full',
    invalidPrintSetup: options.invalidPrintSetup ?? false,
    runtimeFilesExist: options.runtimeFilesExist ?? true,
    runtimeSizeBytes: options.runtimeSizeBytes ?? 1024,
    allowBootRebuild: options.allowBootRebuild ?? false,
    rebuildSameArtifact: options.rebuildSameArtifact ?? false,
    renderDelayMs: options.renderDelayMs ?? 0,
    rebuildError: options.rebuildError ?? '',
    lastSnapshotMode: options.lastSnapshotMode ?? 'full',
    pointedMessageMode: options.pointedMessageMode ?? 'full',
    threadWindowLayout: options.threadWindowLayout ?? null,
    recoveryState: options.recoveryState ?? {
      terminationCount: 0,
      automaticReloadUsed: false,
      blocked: false,
      rawError: null,
      occurredAt: null,
    },
  });
}

test.describe('3MF export', () => {
  test('one 3MF option sends the active project and geometry to export', async ({ page }) => {
    await installBaseBootMock(page);
    await page.goto('/');
    await page.getByRole('button', { name: 'EXPORT', exact: true }).click();
    await expect(page.getByRole('button', { name: /^(?:Orca )?3MF\b/ })).toHaveCount(1);
    await page.getByRole('button', { name: /^3MF\b/ }).click();

    await expect.poll(() => page.evaluate(() =>
      (window as any).__BOOT_CALLS__.find((entry: { cmd: string }) => entry.cmd === 'export_3mf')?.args,
    )).toEqual({
      threadId: 'thread-boot',
      targetPath: '/mock/print.3mf',
      parts: [{
        label: 'Cached Boot Model', partId: 'model', path: '/mock/cache/model.stl',
        objectName: null, displayColor: null,
      }],
    });
  });

  test('reports an invalid print setup', async ({ page }) => {
    await installBaseBootMock(page, { invalidPrintSetup: true });
    await page.goto('/');
    await page.getByRole('button', { name: 'EXPORT', exact: true }).click();
    await expect(page.getByRole('button', { name: /^(?:Orca )?3MF\b/ })).toHaveCount(1);
    await page.getByRole('button', { name: /^3MF\b/ }).click();

    await expect(page.getByText(/Invalid print settings in print.orca.json/).first()).toBeVisible();
  });
});

test.describe('Boot restore', () => {
  test('Given WebContent terminated after memory pressure When recovery reload boots Then durable state restores without duplicate work', async ({ page }) => {
    await installBaseBootMock(page, {
      recoveryState: {
        terminationCount: 1,
        automaticReloadUsed: true,
        blocked: false,
        rawError: 'WKWebView web content process terminated',
        occurredAt: 123,
      },
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
    await expect(page.getByRole('alert')).toContainText(
      'WebContent recovered: WKWebView web content process terminated',
    );

    const calls = await page.evaluate(() =>
      (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd),
    );
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_last_design');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
    expect(calls).not.toContain('queue_agent_prompt');
    expect(calls).not.toContain('send_codex_provider_message');
    expect(calls).not.toContain('send_agy_provider_message');
  });

  test('Given no saved snapshot and a newer reusable blank thread When app boots Then it opens the latest authored thread', async ({ page }) => {
    await installBaseBootMock(page, {
      lastSnapshotMode: 'none',
      history: [
        {
          id: 'blank-thread',
          title: 'Untitled design',
          summary: 'Thread: Untitled design',
          messages: [],
          updatedAt: 200,
          versionCount: 0,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          isBlank: true,
        },
        {
          id: 'thread-boot',
          title: 'Cached Thread',
          summary: 'cached summary',
          messages: [],
          updatedAt: 100,
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          isBlank: false,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.viewer-shell canvas')).toBeVisible({ timeout: 5000 });
    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_workspace_projection');
  });

  test('Given a campaign was active before restart When app boots Then campaign is not auto-restored', async ({ page }) => {
    await installBaseBootMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).not.toContain('get_active_project_navigation');
    await expect(page.locator('.campaign-project-page')).toHaveCount(0);
  });

  test('Given Docs was visible in saved layout When app restarts Then campaign docs stays closed', async ({ page }) => {
    await installBaseBootMock(page, {
      threadWindowLayout: {
        schemaVersion: 1,
        rememberLayout: true,
        windows: {
          docs: { visible: true, minimized: false, x: 170, y: 100, width: 1000, height: 700, z: 8 },
        },
      },
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('[data-window-id="docs"]')).toBeHidden();
    await expect(page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'Ecky IR docs' })).toBeVisible();
  });

  test('Given runtime capability probe is slow When cached version exists Then boot restores before probe finishes', async ({ page }) => {
    await installBaseBootMock(page, { runtimeDelayMs: 8000 });

    await page.goto('/');
    await expect(page.locator('.viewer-shell canvas')).toBeVisible({ timeout: 1500 });
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await expect
      .poll(() => page.evaluate(() => (window as any).__BOOT_CAPABILITIES_RESOLVED__))
      .toBe(false);
  });

  test('Given saved preview is missing When app starts Then its saved source rebuilds the runtime', async ({ page }) => {
    await installBaseBootMock(page, {
      runtimeFilesExist: false,
      allowBootRebuild: true,
      renderDelayMs: 1200,
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Dismiss error' })).toHaveCount(0);

    await expect.poll(() => page.evaluate(() =>
      (window as any).__BOOT_CALLS__.some((entry: { cmd: string }) => entry.cmd === 'repair_version_runtime'),
    )).toBe(true);
    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('repair_version_runtime');
    expect(calls).not.toContain('render_model');
    expect(calls).not.toContain('repair_missing_version_runtime');
  });

  test('Given last snapshot points to a cached artifact When app boots Then it restores the pointed DB version without full thread load or rerender', async ({ page }) => {
    await installBaseBootMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_last_design');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).not.toContain('get_thread_message_version');
    expect(calls).not.toContain('get_thread_messages_page');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given a saved version is restored When boot persists restart state Then it keeps a saved-version target pointer', async ({ page }) => {
    await installBaseBootMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });

    const persistedSnapshot = await page.evaluate(() => {
      const calls = (window as any).__BOOT_CALLS__ as Array<{
        cmd: string;
        args?: { snapshot?: { targetRef?: unknown } };
      }>;
      return calls
        .filter((entry) => entry.cmd === 'save_last_design' && entry.args?.snapshot)
        .at(-1)?.args?.snapshot ?? null;
    });

    expect(persistedSnapshot).toMatchObject({
      threadId: 'thread-boot',
      messageId: 'msg-cached',
      targetRef: {
        kind: 'savedVersion',
        threadId: 'thread-boot',
        messageId: 'msg-cached',
      },
    });
  });

  test('Given a saved preview has a large reported size When app boots Then frontend uses it without a size gate', async ({ page }) => {
    let previewRequests = 0;
    page.on('request', (request) => {
      if (request.url().includes('/mock/cache/model.stl')) previewRequests += 1;
    });
    await installBaseBootMock(page, { runtimeSizeBytes: 1024 * 1024 * 1024 });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).not.toContain('plugin:fs|size');
    expect(calls).not.toContain('render_model');
    expect(previewRequests).toBeGreaterThan(0);
  });

  test('Given saved preview is missing When source rebuild fails Then raw render error stays visible', async ({ page }) => {
    await installBaseBootMock(page, {
      runtimeFilesExist: false,
      allowBootRebuild: true,
      rebuildError: 'FreeCAD exited 1: missing Part workbench',
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();
    await expect(page.getByRole('alert')).toContainText('Runtime Rebuild Error:');
    await expect(page.getByRole('alert')).toContainText('FreeCAD exited 1: missing Part workbench');

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).toContain('repair_version_runtime');
    expect(calls).not.toContain('render_model');
    expect(calls).not.toContain('repair_missing_version_runtime');
    expect(calls).not.toContain('get_thread');
  });

  test('Given saved preview fetch fails once When app boots Then it rebuilds and reloads the same artifact URL', async ({ page }) => {
    let previewRequests = 0;
    page.on('request', (request) => {
      if (request.url().includes('/mock/cache/model.stl')) previewRequests += 1;
    });
    await installBaseBootMock(page, {
      runtimeStlFailsOnce: true,
      allowBootRebuild: true,
      rebuildSameArtifact: true,
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect.poll(() => page.evaluate(() =>
      (window as any).__BOOT_CALLS__.some((entry: { cmd: string }) => entry.cmd === 'repair_version_runtime'),
    )).toBe(true);
    await expect(page.getByRole('button', { name: 'Dismiss error' })).toHaveCount(0);

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('repair_version_runtime');
    expect(calls).not.toContain('render_model');
    expect(calls).not.toContain('repair_missing_version_runtime');
    expect(previewRequests).toBeGreaterThan(1);
  });

  test('Given cached snapshot has no source and preview is missing When app boots Then pointed DB source rebuilds it', async ({ page }) => {
    await installBaseBootMock(page, {
      lastSnapshotMode: 'missing-design',
      runtimeFilesExist: false,
      allowBootRebuild: true,
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();
    await expect.poll(() => page.evaluate(() =>
      (window as any).__BOOT_CALLS__.some((entry: { cmd: string }) => entry.cmd === 'repair_version_runtime'),
    )).toBe(true);
    await expect(page.getByRole('button', { name: 'Dismiss error' })).toHaveCount(0);

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).toContain('repair_version_runtime');
    expect(calls).not.toContain('render_model');
    expect(calls).not.toContain('repair_missing_version_runtime');
    expect(calls).not.toContain('get_thread');
  });

  test('Given pointed message is missing When app boots Then latest full version hydrates model runtime', async ({ page }) => {
    await installBaseBootMock(page, { pointedMessageMode: 'missing' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).not.toContain('get_thread_message_version');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given last snapshot is missing manifest When app boots Then pointed full version hydrates model runtime', async ({ page }) => {
    await installBaseBootMock(page, { lastSnapshotMode: 'missing-manifest' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given restored active message is skinny in first page When app boots Then active cached runtime stays selectable', async ({ page }) => {
    await installBaseBootMock(page, { messagesPageMode: 'skinny-active' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given first thread page omits restored active message When app boots Then active cached runtime remains first version', async ({ page }) => {
    await installBaseBootMock(page, { messagesPageMode: 'omits-active' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await expect(page.locator('.version-title').filter({ hasText: 'Cached Boot Model' }).first()).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given a thread latest preview is missing When thread opens Then source rebuilds its durable STL', async ({ page }) => {
    await installBaseBootMock(page, {
      lastSnapshotMode: 'none',
      runtimeFilesExist: false,
      allowBootRebuild: true,
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const card = page.locator('.project-card').filter({ hasText: 'Cached Thread' });
    await card.getByRole('button', { name: 'OPEN' }).click();

    await expect(page.getByRole('button', { name: /Code inspector/i })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Dismiss error' })).toHaveCount(0);

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_boot_projection');
    expect(calls).not.toContain('get_workspace_projection');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).toContain('repair_version_runtime');
    expect(calls).not.toContain('render_model');
    expect(calls).not.toContain('repair_missing_version_runtime');
  });
});
