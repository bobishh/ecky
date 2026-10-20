import { expect, test, type Page } from '@playwright/test';

type FreecadLibraryMockMode = 'ok' | 'paged' | 'unconfigured' | 'mesh' | 'searchError' | 'importError' | 'pending' | 'sourcePending' | 'pickerError';

const importedBundle = {
  modelId: 'imported-step-freecad-library-608',
  sourceKind: 'importedStep',
  contentHash: 'freecad-library-608',
  artifactVersion: 1,
  fcstdPath: '/mock/runtime/model.FCStd',
  manifestPath: '/mock/runtime/manifest.json',
  modelStlPath: '/mock/runtime/model.stl',
  exportArtifacts: [{ label: 'STEP', format: 'step', path: '/mock/runtime/model.step', role: 'primary' }],
};

const importedManifest = {
  schemaVersion: 1,
  modelId: 'imported-step-freecad-library-608',
  sourceKind: 'importedStep',
  engineKind: 'freecad',
  sourceLanguage: 'legacyPython',
  geometryBackend: 'freecad',
  document: {
    documentName: '608 Bearing',
    documentLabel: '608 Bearing',
    sourcePath: '/mock/freecad-library/Mechanical Parts/Bearings/608.step',
    objectCount: 1,
    warnings: [],
  },
  parts: [],
  parameterGroups: [],
  controlPrimitives: [],
  controlRelations: [],
  controlViews: [],
  advisories: [],
  selectionTargets: [],
  measurementAnnotations: [],
  warnings: [],
  enrichmentState: { status: 'none', proposals: [] },
};

const importedMeshBundle = {
  modelId: 'imported-mesh-freecad-library-fan-guard',
  sourceKind: 'importedMesh',
  contentHash: 'freecad-library-fan-guard',
  artifactVersion: 1,
  fcstdPath: '',
  manifestPath: '/mock/runtime/mesh-manifest.json',
  modelStlPath: '/mock/freecad-library/Printable/Fan Guard.stl',
  viewerAssets: [
    {
      partId: 'mesh-body',
      nodeId: 'mesh-body',
      objectName: 'Fan Guard',
      label: 'Fan Guard',
      path: '/mock/freecad-library/Printable/Fan Guard.stl',
      format: 'stl',
    },
  ],
  exportArtifacts: [
    { label: 'Source mesh', format: 'stl', path: '/mock/freecad-library/Printable/Fan Guard.stl', role: 'source' },
  ],
  geometryBackend: 'mesh',
  sourceLanguage: 'ecky',
  engineKind: 'ecky',
};

const importedMeshManifest = {
  schemaVersion: 1,
  modelId: 'imported-mesh-freecad-library-fan-guard',
  sourceKind: 'importedMesh',
  engineKind: 'ecky',
  sourceLanguage: 'ecky',
  geometryBackend: 'mesh',
  document: {
    documentName: 'Fan Guard',
    documentLabel: 'Fan Guard',
    sourcePath: '/mock/freecad-library/Printable/Fan Guard.stl',
    objectCount: 1,
    warnings: ['Imported mesh models are reference-only; CAD booleans and topology selectors are unavailable.'],
  },
  parts: [
    {
      partId: 'mesh-body',
      freecadObjectName: 'Fan Guard',
      label: 'Fan Guard',
      kind: 'mesh',
      semanticRole: 'mesh-reference',
      viewerAssetPath: '/mock/freecad-library/Printable/Fan Guard.stl',
      viewerNodeIds: ['mesh-body'],
      parameterKeys: [],
      editable: false,
    },
  ],
  parameterGroups: [],
  controlPrimitives: [],
  controlRelations: [],
  controlViews: [],
  advisories: [],
  selectionTargets: [],
  measurementAnnotations: [],
  warnings: ['Imported mesh models are reference-only; CAD booleans and topology selectors are unavailable.'],
  enrichmentState: { status: 'none', proposals: [] },
};

async function installFreecadLibraryMocks(page: Page, mode: FreecadLibraryMockMode) {
  await page.addInitScript(({ mockMode, bundle, manifest, meshBundle, meshManifest }) => {
    const mockWindow = window as any;
    mockWindow.__PERSISTED_LIBRARY_ROOTS__ = null;
    mockWindow.__LIBRARY_PANEL_INTENTS__ = [];
    mockWindow.__IMPORT_CALLS__ = [];
    mockWindow.__ADDED_IMPORTED__ = null;
    mockWindow.__RENDER_CALLS__ = [];
    mockWindow.__IMPORTED_APPLY_CALLS__ = [];
    mockWindow.__PACKAGE_HEADERS__ = [];
    mockWindow.__CONFIG__ = {
      engines: [],
      selectedEngineId: '',
      freecadCmd: '',
      freecadLibraryRoots: mockMode === 'ok' || mockMode === 'paged' || mockMode === 'mesh' || mockMode === 'pending' || mockMode === 'sourcePending'
        ? ['/mock/freecad-library']
        : [],
      assets: [],
      microwave: { humId: null, dingId: null, muted: true },
      mcp: {
        port: null,
        maxSessions: null,
        mode: 'passive',
        primaryAgentId: null,
        promptTimeoutSecs: 1800,
        autoAgents: [],
      },
      hasSeenOnboarding: true,
      connectionType: 'api_key',
      defaultEngineKind: 'ecky',
      defaultSourceLanguage: 'ecky',
      defaultGeometryBackend: 'freecad',
      maxGenerationAttempts: 1,
      maxVerifyAttempts: 0,
    };

    const item = {
      id: 'Mechanical Parts/Bearings/608',
      name: '608 Bearing',
      categoryPath: 'Mechanical Parts / Bearings',
      rootPath: '/mock/freecad-library',
      relativePath: 'Mechanical Parts/Bearings/608.step',
      formats: ['fcstd', 'step', 'stl'],
      preferredFormat: 'step',
      importPath: '/mock/freecad-library/Mechanical Parts/Bearings/608.step',
      previewPath: '/mock/freecad-library/thumbnails/608.png',
      tags: ['mechanical', 'hardware', 'reference', 'printableCandidate'],
    };

    const meshItem = {
      id: 'Printable/Fan Guard',
      name: 'Fan Guard',
      categoryPath: 'Printable',
      rootPath: '/mock/freecad-library',
      relativePath: 'Printable/Fan Guard.stl',
      formats: ['stl'],
      preferredFormat: 'stl',
      importPath: '/mock/freecad-library/Printable/Fan Guard.stl',
      previewPath: null,
      tags: ['meshOnly', 'printableCandidate'],
    };

    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') return mockWindow.__CONFIG__;
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: {
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
          },
        };
      }
      if (cmd === 'get_history') return [];
      if (cmd === 'get_project_source') {
        if (mockMode === 'sourcePending') await delay(600);
        return {
          threadId: args?.threadId,
          folder: `/mock/projects/${args?.threadId}`,
          sourcePath: `/mock/projects/${args?.threadId}/model.ecky`,
          source: '',
          sourceDigest: 'empty',
        };
      }
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_default_macro') return '';
      if (cmd === 'check_freecad') return true;
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: null,
          connectionState: 'disconnected',
          sessions: [],
          primaryAgentLabel: null,
          statusText: '',
          phase: null,
          busy: false,
          agentLabel: null,
          activityLabel: '',
          sessionId: null,
        };
      }
      if (cmd === 'library_panel_intent') {
        const intent = args?.intent;
        mockWindow.__LIBRARY_PANEL_INTENTS__.push(intent);
        if (intent?.kind === 'loadComponents') {
          return { kind: 'componentPackages', packageHeaders: mockWindow.__PACKAGE_HEADERS__ };
        }
        if (intent?.kind !== 'loadFreecad' && intent?.kind !== 'setFreecadRoot') {
          throw new Error(`unexpected library panel intent: ${intent?.kind}`);
        }
        if (mockMode === 'searchError') {
          throw {
            code: 'persistence',
            message: 'FreeCAD library scan failed',
            details: 'raw root missing: /mock/freecad-library',
          };
        }
        if (intent.kind === 'setFreecadRoot') {
          mockWindow.__CONFIG__.freecadLibraryRoots = [intent.root];
          mockWindow.__PERSISTED_LIBRARY_ROOTS__ = [intent.root];
        }
        const roots = mockWindow.__CONFIG__.freecadLibraryRoots;
        const page = intent.kind === 'loadFreecad' ? intent.page : 0;
        if (mockMode === 'paged') {
          const offset = page * 100;
          const count = page === 0 ? 100 : 1;
          const items = Array.from({ length: count }, (_, index) => {
            const number = offset + index + 1;
            return {
              ...item,
              id: `Mechanical Parts/Part-${number}`,
              name: `Nested Part ${number}`,
              relativePath: `Mechanical Parts/Deep/Part-${number}.step`,
              importPath: `/mock/freecad-library/Mechanical Parts/Deep/Part-${number}.step`,
            };
          });
          return {
            kind: 'freecadLibrary',
            freecadLibraryRoots: roots,
            items,
            page,
            hasMore: page === 0,
          };
        }
        const items = roots.length === 0 ? [] : mockMode === 'mesh' ? [meshItem] : [item];
        return {
          kind: 'freecadLibrary',
          freecadLibraryRoots: roots,
          items,
          page,
          hasMore: false,
        };
      }
      if (cmd === 'plugin:dialog|open') {
        if (mockMode === 'pickerError') throw new Error('raw folder picker failure');
        return '/mock/freecad-library';
      }
      if (cmd === 'import_model_intent') {
        const item = args?.input?.source?.item;
        mockWindow.__IMPORT_CALLS__.push(item?.id ?? null);
        if (mockMode === 'pending') await delay(600);
        if (mockMode === 'importError') {
          throw {
            code: 'render',
            message: 'FreeCAD library import failed',
            details: 'raw FreeCAD import body',
          };
        }
        const artifactBundle = mockMode === 'mesh' ? meshBundle : bundle;
        const modelManifest = mockMode === 'mesh' ? meshManifest : manifest;
        const title = item?.name ?? 'Imported model';
        const designOutput = {
          title,
          versionName: 'Imported model',
          response: `Imported ${title}.`,
          interactionMode: 'design',
          macroCode: '',
          macroDialect: 'ecky',
          engineKind: modelManifest.engineKind,
          sourceLanguage: modelManifest.sourceLanguage,
          geometryBackend: modelManifest.geometryBackend,
          uiSpec: { fields: [] },
          initialParams: {},
          postProcessing: null,
        };
        const messageId = 'msg-imported-608';
        const message = {
          id: messageId,
          role: 'assistant',
          content: designOutput.response,
          status: 'success',
          output: designOutput,
          artifactBundle,
          modelManifest,
          usage: null,
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: 1,
        };
        const projection = {
          threadId: 'thread-imported-608',
          messageId,
          title,
          message,
          designOutput,
          artifactBundle,
          modelManifest,
          snapshotId: 'snapshot-imported-608',
        };
        mockWindow.__ADDED_IMPORTED__ = projection;
        return projection;
      }
      if (cmd === 'import_freecad_library_part') {
        mockWindow.__IMPORT_CALLS__.push(args?.request?.item?.id ?? null);
        if (mockMode === 'pending') await delay(600);
        if (mockMode === 'importError') {
          throw {
            code: 'render',
            message: 'FreeCAD library import failed',
            details: 'raw FreeCAD import body',
          };
        }
        return mockMode === 'mesh' ? meshBundle : bundle;
      }
      if (cmd === 'get_model_manifest') return mockMode === 'mesh' ? meshManifest : manifest;
      if (cmd === 'add_imported_model_version') {
        mockWindow.__ADDED_IMPORTED__ = args;
        return 'msg-imported-608';
      }
      if (cmd === 'save_model_manifest') return null;
      if (cmd === 'save_last_design') return null;
      if (cmd === 'apply_imported_parameters' || cmd === 'apply_manual_parameters') {
        mockWindow.__IMPORTED_APPLY_CALLS__.push(args);
        const input = args?.input;
        const designOutput = {
          title: '608 Bearing',
          versionName: 'Imported model',
          response: 'Parameter version appended.',
          interactionMode: 'tune',
          macroCode: '',
          macroDialect: 'ecky',
          engineKind: 'freecad',
          sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad',
          uiSpec: { fields: [] },
          initialParams: input?.parameters ?? {},
          postProcessing: null,
        };
        return {
          threadId: input?.threadId,
          baseMessageId: input?.targetMessageId,
          messageId: 'msg-imported-608-params',
          status: 'success',
          designOutput,
          artifactBundle: bundle,
          modelManifest: manifest,
          snapshotId: 'snapshot-imported-608-params',
          error: null,
        };
      }
      if (cmd === 'apply_manual_code') {
        mockWindow.__IMPORTED_APPLY_CALLS__.push(args);
        const input = args?.input;
        const designOutput = {
          title: input?.title ?? '608 Bearing',
          versionName: input?.versionName ?? 'Imported model',
          response: 'Code draft applied.',
          interactionMode: 'design',
          macroCode: input?.source ?? '',
          macroDialect: 'ecky',
          engineKind: 'ecky',
          sourceLanguage: input?.sourceLanguage ?? 'ecky',
          geometryBackend: input?.geometryBackend ?? 'mesh',
          uiSpec: input?.uiSpec ?? { fields: [] },
          initialParams: input?.parameters ?? {},
          postProcessing: input?.postProcessing ?? null,
        };
        return {
          threadId: input?.threadId,
          baseMessageId: input?.baseMessageId,
          messageId: null,
          status: 'success',
          designOutput,
          artifactBundle: bundle,
          modelManifest: manifest,
          snapshotId: 'snapshot-manual-component-608',
          parserMatched: false,
          error: null,
        };
      }
      if (cmd === 'render_model') {
        mockWindow.__RENDER_CALLS__.push(args);
        return bundle;
      }
      if (cmd === 'apply_imported_model') {
        mockWindow.__IMPORTED_APPLY_CALLS__.push(args);
        return bundle;
      }
      if (cmd === 'open_project_in_editor') {
        mockWindow.__OPENED_SOURCE__ = args;
        return {
          slug: '608-bearing',
          folder: '/mock/projects/608-bearing',
          file: '/mock/projects/608-bearing/model.ecky',
        };
      }
      if (cmd === 'open_imported_cad_source') {
        mockWindow.__OPENED_CAD__ = args;
        return {
          slug: '608-bearing',
          folder: '/mock/projects/608-bearing',
          file: '/mock/projects/608-bearing/608.step',
        };
      }
      return null;
    };
  }, {
    mockMode: mode,
    bundle: importedBundle,
    manifest: importedManifest,
    meshBundle: importedMeshBundle,
    meshManifest: importedMeshManifest,
  });
}

test.describe('FreeCAD library catalog', () => {
  test('Given configured local library When user searches and imports Then imported version opens', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();

    await expect(page.getByText('LOCAL SOURCE')).toBeVisible();
    await expect(page.getByText('608 Bearing')).toBeVisible();
    await page.getByPlaceholder('Search library...').fill('608 bearing');
    await page.getByRole('button', { name: 'SEARCH', exact: true }).click();

    await expect(page.getByText('608 Bearing')).toBeVisible();
    await expect(page.getByText('Mechanical Parts / Bearings')).toBeVisible();
    await page.getByRole('button', { name: 'IMPORT 608 Bearing' }).click();

    await expect(page.evaluate(() => (window as any).__IMPORT_CALLS__)).resolves.toEqual([
      'Mechanical Parts/Bearings/608',
    ]);
    await expect
      .poll(() => page.evaluate(() => (window as any).__ADDED_IMPORTED__?.title))
      .toBe('608 Bearing');

    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'CODE' }).click();
    const codeModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(codeModal.getByRole('tab', { name: 'SUMMARY', exact: true })).toHaveAttribute('aria-selected', 'true');
    await expect(codeModal.getByRole('tab', { name: 'COMPONENT', exact: true })).toBeVisible();
    await expect(codeModal).toContainText('IMPORTED CAD EVIDENCE — READ ONLY');
    await expect(codeModal).toContainText('IMPORTED STEP — READ ONLY');
    await expect(codeModal).toContainText('/mock/freecad-library/Mechanical Parts/Bearings/608.step');
    await expect(codeModal.getByRole('button', { name: 'APPLY', exact: true })).toHaveCount(0);
    await expect(codeModal.getByRole('button', { name: 'COMMIT VERSION' })).toHaveCount(0);
    await expect(codeModal.getByRole('button', { name: 'TRANSLATE TO ECKY' })).toHaveCount(0);
    await expect(codeModal.getByRole('button', { name: 'OPEN FILE' })).toHaveCount(0);
    await codeModal.getByRole('button', { name: 'OPEN CAD' }).click();
    await expect(codeModal.getByText('/mock/projects/608-bearing/608.step')).toBeVisible();
    await expect(page.evaluate(() => (window as any).__OPENED_CAD__?.messageId)).resolves.toBe('msg-imported-608');
    await codeModal.getByRole('tab', { name: 'COMPONENT', exact: true }).click();
    await expect(codeModal).toContainText('FREECAD-COMPONENT — SOURCE IDENTITY AND BINDINGS');
    await expect(codeModal.locator('.code-container')).toContainText('(freecad-component');
    await expect(codeModal.locator('.code-container')).toContainText(':source-kind :step');
    await codeModal.getByRole('button', { name: 'APPLY', exact: true }).click();
    await expect.poll(() => page.evaluate(() => (window as any).__IMPORTED_APPLY_CALLS__.length)).toBe(1);
    await expect(page.evaluate(() => (window as any).__RENDER_CALLS__.length)).resolves.toBe(0);
    await expect(codeModal).not.toBeVisible();
  });

  test('Given more than one catalog page When library opens Then next page reaches later nested models', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'paged');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await expect(page.getByText('Nested Part 100')).toBeVisible();
    await page.getByRole('button', { name: 'NEXT' }).click();
    await expect(page.getByText('Nested Part 101')).toBeVisible();
    await expect(page.getByText('PAGE 2')).toBeVisible();
    await expect(page.getByRole('button', { name: 'NEXT' })).toBeDisabled();
    await page.getByRole('button', { name: 'PREVIOUS' }).click();
    await expect(page.getByText('Nested Part 1', { exact: true })).toBeVisible();
  });

  test('Given imported STEP source read is pending When CODE opens Then imported summary appears immediately', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'sourcePending');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await page.getByRole('button', { name: 'IMPORT 608 Bearing' }).click();
    await expect.poll(() => page.evaluate(() => (window as any).__ADDED_IMPORTED__?.title)).toBe('608 Bearing');

    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'CODE' }).click();
    const codeModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(codeModal).toBeVisible({ timeout: 250 });
    await expect(codeModal).toContainText('IMPORTED CAD EVIDENCE — READ ONLY');
  });

  test('Given no configured library When user picks folder Then config persists root and search works', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'unconfigured');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await page.getByRole('button', { name: 'SET FOLDER' }).click();

    await expect(page.evaluate(() => (window as any).__PERSISTED_LIBRARY_ROOTS__)).resolves.toEqual([
      '/mock/freecad-library',
    ]);
    await expect(page.getByText('/mock/freecad-library')).toBeVisible();
    await expect(page.getByText('608 Bearing')).toBeVisible();
  });

  test('Given folder picker fails When user sets local source Then raw error stays visible', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'pickerError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await page.getByRole('button', { name: 'SET FOLDER' }).click();

    await expect(page.getByText('LIBRARY ERROR')).toBeVisible();
    await expect(page.getByText('raw folder picker failure')).toBeVisible();
  });

  test('Given backend scan fails When search runs Then raw error body stays visible', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'searchError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await page.getByPlaceholder('Search library...').fill('bearing');
    await page.getByRole('button', { name: 'SEARCH', exact: true }).click();

    await expect(page.getByText('FreeCAD library scan failed')).toBeVisible();
    await expect(page.getByText('raw root missing: /mock/freecad-library')).toBeVisible();
  });

  test('Given import is running When user clicks import Then button shows pending state', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'pending');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await page.getByPlaceholder('Search library...').fill('608');
    await page.getByRole('button', { name: 'SEARCH', exact: true }).click();
    await page.getByRole('button', { name: 'IMPORT 608 Bearing' }).click();

    await expect(page.getByRole('button', { name: 'IMPORTING 608 Bearing' })).toBeDisabled();
  });

  test('Given mesh-only library item When user imports Then imported mesh version opens', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'mesh');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'FREECAD PARTS' }).click();
    await page.getByPlaceholder('Search library...').fill('fan guard');
    await page.getByRole('button', { name: 'SEARCH', exact: true }).click();

    await expect(page.getByText('Fan Guard')).toBeVisible();
    await page.getByRole('button', { name: 'IMPORT Fan Guard' }).click();

    await expect
      .poll(() => page.evaluate(() => (window as any).__ADDED_IMPORTED__?.modelManifest?.sourceKind))
      .toBe('importedMesh');
  });
});
