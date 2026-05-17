import { expect, test, type Page } from '@playwright/test';

type FreecadLibraryMockMode = 'ok' | 'mesh' | 'searchError' | 'importError' | 'pending';

const importedBundle = {
  modelId: 'imported-step-freecad-library-608',
  sourceKind: 'importedStep',
  contentHash: 'freecad-library-608',
  artifactVersion: 1,
  fcstdPath: '/mock/runtime/model.FCStd',
  manifestPath: '/mock/runtime/manifest.json',
  previewStlPath: '/mock/runtime/preview.stl',
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
  previewStlPath: '/mock/freecad-library/Printable/Fan Guard.stl',
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
    mockWindow.__SAVED_CONFIG__ = null;
    mockWindow.__IMPORT_CALLS__ = [];
    mockWindow.__ADDED_IMPORTED__ = null;
    mockWindow.__PACKAGE_HEADERS__ = [];
    mockWindow.__CONFIG__ = {
      engines: [],
      selectedEngineId: '',
      freecadCmd: '',
      freecadLibraryRoots: mockMode === 'ok' || mockMode === 'mesh' || mockMode === 'pending'
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
      defaultGeometryBackend: 'mesh',
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
      if (cmd === 'save_config') {
        mockWindow.__SAVED_CONFIG__ = args?.config ?? null;
        mockWindow.__CONFIG__ = args?.config ?? mockWindow.__CONFIG__;
        return null;
      }
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
      if (cmd === 'list_installed_component_package_headers') return mockWindow.__PACKAGE_HEADERS__;
      if (cmd === 'plugin:dialog|open') return '/mock/freecad-library';
      if (cmd === 'search_freecad_library') {
        if (mockMode === 'searchError') {
          throw {
            code: 'persistence',
            message: 'FreeCAD library scan failed',
            details: 'raw root missing: /mock/freecad-library',
          };
        }
        return mockMode === 'mesh' ? [meshItem] : [item];
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

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();

    await expect(page.getByText('FREECAD LIBRARY')).toBeVisible();
    await page.getByPlaceholder('Search FreeCAD library...').fill('608 bearing');
    await page.getByRole('button', { name: 'SEARCH FREECAD' }).click();

    await expect(page.getByText('608 Bearing')).toBeVisible();
    await expect(page.getByText('Mechanical Parts / Bearings')).toBeVisible();
    await expect(page.getByText('step')).toBeVisible();

    await page.getByRole('button', { name: 'IMPORT 608 Bearing' }).click();

    await expect(page.evaluate(() => (window as any).__IMPORT_CALLS__)).resolves.toEqual([
      'Mechanical Parts/Bearings/608',
    ]);
    await expect
      .poll(() => page.evaluate(() => (window as any).__ADDED_IMPORTED__?.title))
      .toBe('608 Bearing');
  });

  test('Given no configured library When user picks folder Then config persists root and search works', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'searchError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();
    await page.getByRole('button', { name: 'SET LIBRARY FOLDER' }).click();

    await expect(page.evaluate(() => (window as any).__SAVED_CONFIG__?.freecadLibraryRoots)).resolves.toEqual([
      '/mock/freecad-library',
    ]);
  });

  test('Given backend scan fails When search runs Then raw error body stays visible', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'searchError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();
    await page.getByPlaceholder('Search FreeCAD library...').fill('bearing');
    await page.getByRole('button', { name: 'SEARCH FREECAD' }).click();

    await expect(page.getByText('FreeCAD library scan failed')).toBeVisible();
    await expect(page.getByText('raw root missing: /mock/freecad-library')).toBeVisible();
  });

  test('Given import is running When user clicks import Then button shows pending state', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'pending');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();
    await page.getByPlaceholder('Search FreeCAD library...').fill('608');
    await page.getByRole('button', { name: 'SEARCH FREECAD' }).click();
    await page.getByRole('button', { name: 'IMPORT 608 Bearing' }).click();

    await expect(page.getByRole('button', { name: 'IMPORTING 608 Bearing' })).toBeDisabled();
  });

  test('Given mesh-only library item When user imports Then imported mesh version opens', async ({ page }) => {
    await installFreecadLibraryMocks(page, 'mesh');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();
    await page.getByPlaceholder('Search FreeCAD library...').fill('fan guard');
    await page.getByRole('button', { name: 'SEARCH FREECAD' }).click();

    await expect(page.getByText('Fan Guard')).toBeVisible();
    await expect(page.getByText('meshOnly')).toBeVisible();
    await page.getByRole('button', { name: 'IMPORT Fan Guard' }).click();

    await expect
      .poll(() => page.evaluate(() => (window as any).__ADDED_IMPORTED__?.modelManifest?.sourceKind))
      .toBe('importedMesh');
  });
});
