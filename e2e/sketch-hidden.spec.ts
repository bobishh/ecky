import { expect, test } from '@playwright/test';

test.describe('Detached Sketch Workspace visibility', () => {
  test.beforeEach(async ({ page }) => {
    await page.route(/\/mock\/preview\.stl(?:\?.*)?$/, async (route) => {
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
    await page.route(/\/mock\/saved-sketch-preview\.stl(?:\?.*)?$/, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body: 'solid stale-sketch\nendsolid stale-sketch\n',
      });
    });
    await page.addInitScript(() => {
      (window as any).__LAYOUT_REQUESTS__ = 0;
      const design = {
        title: 'Persisted layout model',
        versionName: 'V1',
        interactionMode: 'design',
        macroCode: '(model)',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      };
      const artifactBundle = {
        modelId: 'persisted-layout-model',
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        contentHash: 'persisted-layout-hash',
        artifactVersion: 1,
        fcstdPath: null,
        manifestPath: '/mock/manifest.json',
        macroPath: '/mock/model.ecky',
        modelStlPath: '/mock/model.stl',
        viewerAssets: [],
      };
      const modelManifest = {
        modelId: 'persisted-layout-model',
        sourceKind: 'generated',
        document: {
          documentName: 'Persisted layout model',
          documentLabel: 'Persisted layout model',
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
      const versionMessage = {
        id: 'version-layout',
        role: 'assistant',
        content: 'Persisted layout model',
        status: 'success',
        output: design,
        artifactBundle,
        modelManifest,
        timestamp: 100,
      };
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') {
          return {
            engines: [],
            selectedEngineId: '',
            hasSeenOnboarding: true,
            microwave: { humId: null, dingId: null, muted: true },
            voice: { sttLanguageCode: 'en-US' },
            freecadCmd: '',
            cadTextFontPath: '',
            freecadLibraryRoots: [],
            assets: [],
            connectionType: null,
            defaultEngineKind: 'freecad',
            defaultGeometryBackend: 'freecad',
            defaultSourceLanguage: 'legacyPython',
            maxGenerationAttempts: 3,
            maxVerifyAttempts: 0,
            mcp: {
              port: null,
              maxSessions: null,
              mode: 'passive',
              primaryAgentId: null,
              promptTimeoutSecs: 1800,
              eckyAstAuthoring: false,
              autoAgents: [],
            },
          };
        }
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
            mesh: { available: true, detail: 'bundled', path: null },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
            },
          };
        }
        if (cmd === 'get_history') {
          return [
            {
              id: 'thread-layout',
              title: 'Persisted layout',
              summary: '',
              messages: [],
              updatedAt: 100,
              versionCount: 1,
              pendingCount: 0,
              queuedCount: 0,
              errorCount: 0,
              status: 'active',
              engineKind: 'ecky',
              sourceLanguage: 'ecky',
              geometryBackend: 'mesh',
            },
          ];
        }
        if (cmd === 'get_last_design') {
          return {
            design,
            threadId: 'thread-layout',
            messageId: 'version-layout',
            artifactBundle,
            modelManifest,
            selectedPartId: null,
          };
        }
        if (cmd === 'get_thread_message_version') {
          return args?.threadId === 'thread-layout' && args?.messageId === 'version-layout'
            ? versionMessage
            : null;
        }
        if (cmd === 'get_thread_messages_page') {
          return { messages: [versionMessage], nextBefore: null, hasMore: false };
        }
        if (cmd === 'get_active_agent_sessions') return [];
        if (cmd === 'get_agent_terminal_snapshots') return [];
        if (cmd === 'get_default_macro') return '';
        if (cmd === 'load_sketch_preview_draft') {
          return {
            scopeId: 'stale-sketch-draft',
            updatedAt: 100,
            draftSource: {
              sourceLanguage: 'ecky',
              geometryBackend: 'mesh',
              macroDialect: 'ecky',
              source: '(model (part stale_sketch (box 100 50 5)))',
              warnings: ['diagnostic sketch preview'],
            },
            artifactBundle: {
              ...artifactBundle,
              modelId: 'stale-sketch-preview',
              contentHash: 'stale-sketch-preview-hash',
              modelStlPath: '/mock/saved-sketch-model.stl',
            },
            sketchDocument: null,
          };
        }
        if (cmd === 'plugin:fs|exists') return true;
        if (cmd === 'plugin:fs|size') return 1024;
        if (cmd === 'get_thread_agent_state') {
          return {
            connectionState: 'disconnected',
            agentLabel: null,
            phase: null,
            statusText: '',
            busy: false,
            waitingOnPrompt: false,
            updatedAt: null,
          };
        }
        if (cmd === 'get_thread_window_layout') {
          (window as any).__LAYOUT_REQUESTS__ += 1;
          return {
            schemaVersion: 1,
            rememberLayout: true,
            windows: {
              params: { visible: true, minimized: false, x: 520, y: 80, width: 360, height: 480, z: 4 },
              sketch: { visible: true, minimized: false, x: 180, y: 120, width: 760, height: 520, z: 9 },
            },
          };
        }
        if (cmd === 'save_thread_window_layout') return null;
        return null;
      };
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await expect(page.locator('.workbench')).toBeVisible();
  });

  test('Given workbench loads When dock renders Then Sketch is available without detached preview', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Sketch', exact: true })).toBeVisible();
    await expect(page.getByTestId('profile-workspace')).toBeVisible();
    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
  });

  test('Given stale sketch preview data When thread layout restores Then only profile editor appears', async ({
    page,
  }) => {
    await page.waitForFunction(() => (window as any).__LAYOUT_REQUESTS__ > 0);

    await expect(page.locator('[data-window-id="params"]')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Sketch', exact: true })).toBeVisible();
    await expect(page.getByTestId('profile-workspace')).toBeVisible();
    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
    await expect(page.getByText('diagnostic sketch preview')).toHaveCount(0);
  });
});
