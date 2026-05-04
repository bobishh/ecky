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
  previewStlPath: '/mock/cache/preview.stl',
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
  runtimeDelayMs?: number;
  messagesPageMode?: 'full' | 'skinny-active' | 'omits-active';
  runtimeFilesExist?: boolean;
  allowBootRebuild?: boolean;
  renderDelayMs?: number;
  lastSnapshotMode?: 'full' | 'missing-manifest' | 'missing-design';
  pointedMessageMode?: 'full' | 'missing';
};

function installBaseBootMock(page: Page, options: BootMockOptions = {}) {
  return page.addInitScript(({ runtimeCapabilities, config, artifactBundle, modelManifest, design, runtimeDelayMs, messagesPageMode, runtimeFilesExist, allowBootRebuild, renderDelayMs, lastSnapshotMode, pointedMessageMode }) => {
    (window as any).__BOOT_CALLS__ = [];
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      (window as any).__BOOT_CALLS__.push({ cmd, args });
      if (cmd === 'get_config') return config;
      if (cmd === 'save_config') return null;
      if (cmd === 'get_runtime_capabilities') {
        if (runtimeDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, runtimeDelayMs));
        }
        return runtimeCapabilities;
      }
      if (cmd === 'get_history') {
        return [
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
        return {
          design: lastSnapshotMode === 'missing-design' ? null : design,
          threadId: 'thread-boot',
          messageId: 'msg-cached',
          artifactBundle,
          modelManifest: lastSnapshotMode === 'missing-manifest' ? null : modelManifest,
          selectedPartId: null,
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
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'plugin:fs|exists') return runtimeFilesExist;
      if (cmd === 'plugin:fs|size') return 1024;
      if (cmd === 'render_model' && allowBootRebuild) {
        if (renderDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, renderDelayMs));
        }
        return {
          ...artifactBundle,
          modelId: 'cached-model-rebuilt',
          contentHash: 'cached-hash-rebuilt',
          previewStlPath: '/mock/cache/rebuilt-preview.stl',
        };
      }
      if (cmd === 'get_model_manifest') return {
        ...modelManifest,
        modelId: args?.modelId ?? 'cached-model-rebuilt',
      };
      if (cmd === 'save_model_manifest') return null;
      if (cmd === 'update_version_runtime') return null;
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
    runtimeDelayMs: options.runtimeDelayMs ?? 0,
    messagesPageMode: options.messagesPageMode ?? 'full',
    runtimeFilesExist: options.runtimeFilesExist ?? true,
    allowBootRebuild: options.allowBootRebuild ?? false,
    renderDelayMs: options.renderDelayMs ?? 0,
    lastSnapshotMode: options.lastSnapshotMode ?? 'full',
    pointedMessageMode: options.pointedMessageMode ?? 'full',
  });
}

test.describe('Boot restore', () => {
  test('Given runtime restore is slow When app starts Then boot overlay stays visible instead of dropping to blank viewport', async ({ page }) => {
    await installBaseBootMock(page, { runtimeDelayMs: 4000 });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toBeVisible();
    await page.waitForTimeout(1700);
    await expect(page.locator('.boot-overlay')).toBeVisible();
    await expect(page.locator('.boot-overlay__status')).toHaveText('Restoring environment...');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 8000 });
  });

  test('Given boot rebuild is slow When app starts Then rebuild stays inside boot overlay without render splash', async ({ page }) => {
    await installBaseBootMock(page, {
      runtimeFilesExist: false,
      allowBootRebuild: true,
      renderDelayMs: 1200,
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toBeVisible();
    await page.waitForTimeout(300);
    await expect(page.locator('.boot-overlay')).toBeVisible();
    await expect(page.locator('.viewport-transmutation')).toHaveCount(0);
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  });

  test('Given last snapshot points to a cached artifact When app boots Then it restores the pointed DB version without full thread load or rerender', async ({ page }) => {
    await installBaseBootMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_last_design');
    expect(calls).toContain('get_thread_message_version');
    expect(calls).toContain('get_thread_messages_page');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given restored runtime files are missing When app boots Then it rebuilds preview instead of leaving blank model', async ({ page }) => {
    await installBaseBootMock(page, { runtimeFilesExist: false, allowBootRebuild: true });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_thread_message_version');
    expect(calls).toContain('render_model');
    expect(calls).not.toContain('get_thread');
  });

  test('Given cached snapshot has no source and runtime files are missing When app boots Then pointed DB version is loaded and rebuilt', async ({ page }) => {
    await installBaseBootMock(page, {
      lastSnapshotMode: 'missing-design',
      runtimeFilesExist: false,
      allowBootRebuild: true,
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /EXPORT/ })).toBeEnabled();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_thread_message_version');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).toContain('render_model');
    expect(calls).not.toContain('get_thread');
  });

  test('Given pointed message is missing When app boots Then latest full version hydrates model runtime', async ({ page }) => {
    await installBaseBootMock(page, { pointedMessageMode: 'missing' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_thread_message_version');
    expect(calls).toContain('get_thread_latest_version');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given last snapshot is missing manifest When app boots Then pointed full version hydrates model runtime', async ({ page }) => {
    await installBaseBootMock(page, { lastSnapshotMode: 'missing-manifest' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).toContain('get_thread_message_version');
    expect(calls).not.toContain('get_thread_latest_version');
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given restored active message is skinny in first page When app boots Then active cached runtime stays selectable', async ({ page }) => {
    await installBaseBootMock(page, { messagesPageMode: 'skinny-active' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /CODE/ })).toBeEnabled();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });

  test('Given first thread page omits restored active message When app boots Then active cached runtime remains first version', async ({ page }) => {
    await installBaseBootMock(page, { messagesPageMode: 'omits-active' });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByRole('button', { name: /CODE/ })).toBeEnabled();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await expect(page.locator('.version-title').filter({ hasText: 'Cached Boot Model' }).first()).toBeVisible();

    const calls = await page.evaluate(() => (window as any).__BOOT_CALLS__.map((entry: { cmd: string }) => entry.cmd));
    expect(calls).not.toContain('get_thread');
    expect(calls).not.toContain('render_model');
  });
});
