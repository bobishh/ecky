import { expect, test, type Page } from '@playwright/test';

type PackageLibraryMockMode = 'ok' | 'discovery' | 'error' | 'empty' | 'installError' | 'componentImportError' | 'componentImportPending';

async function installProjectLibraryMocks(page: Page, mode: PackageLibraryMockMode) {
  await page.route(/\/mock\/.*\.stl(\?.*)?$/, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: 'solid component\nendsolid component',
    });
  });
  await page.addInitScript((mockMode) => {
    const packageHeader = {
      schemaVersion: 1,
      packageId: 'bike-bottle-system',
      version: '0.1.0',
      displayName: 'Bike Bottle System',
      visibility: 'public',
      tags: ['bike', 'holder'],
      portTypes: [
        {
          typeId: 'dovetail',
          displayName: 'Dovetail',
          params: [{ key: 'railWidthMm', label: 'Rail width', kind: 'number', unit: 'mm' }],
          compatibleWith: ['dovetail_slot'],
          allowedMateTypes: ['insert_rail_into_slot'],
        },
        {
          typeId: 'bolt_pattern',
          displayName: 'Bolt pattern',
          params: [{ key: 'spacingMm', label: 'Spacing', kind: 'number', unit: 'mm' }],
          compatibleWith: ['bolt_pattern'],
          allowedMateTypes: ['bolt_pattern_match'],
        },
      ],
      components: [
        {
          componentId: 'bottle_cage',
          version: '0.1.0',
          displayName: 'Bottle Cage',
          params: [{ key: 'bottleDiameterMm', label: 'Bottle diameter', kind: 'number', unit: 'mm' }],
          ports: [
            { portId: 'dovetail_slot', typeId: 'dovetail', interfaces: ['slot'] },
            { portId: 'bolt_pattern', typeId: 'bolt_pattern', interfaces: ['mount'] },
          ],
        },
        {
          componentId: 'frame_rail',
          version: '0.1.0',
          displayName: 'Frame Rail',
          params: [],
          ports: [{ portId: 'dovetail_rail', typeId: 'dovetail', interfaces: ['rail'] }],
        },
      ],
      assemblies: [
        {
          assemblyId: 'rail_cage_mount',
          displayName: 'Rail Cage Mount',
          componentCount: 2,
          mateCount: 2,
          operationCount: 1,
          output: { mode: 'separateParts' },
        },
      ],
    };
    const source = '(model (part base (box 1 1 1)))';
    const design = {
      title: 'Component Assembly',
      versionName: 'V1',
      response: 'Ready.',
      interactionMode: 'design',
      macroCode: source,
      macroDialect: 'ecky',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    };
    const artifactBundle = {
      modelId: 'component-model-1',
      sourceKind: 'generated',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      contentHash: 'sha256:artifact-1',
      artifactVersion: 1,
      fcstdPath: '',
      manifestPath: '/mock/component-manifest.json',
      modelStlPath: '/mock/component-model.stl',
      viewerAssets: [],
      exportArtifacts: [],
    };
    const modelManifest = {
      schemaVersion: 1,
      modelId: 'component-model-1',
      sourceKind: 'generated',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      sourceDigest: 'sha256:source-1',
      document: {
        documentName: 'Component Assembly',
        documentLabel: 'Component Assembly',
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
    const message = {
      id: 'component-message-1',
      role: 'assistant',
      content: 'Ready.',
      status: 'success',
      timestamp: 1,
      output: design,
      artifactBundle,
      modelManifest,
      usage: null,
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
    };
    const thread = {
      id: 'component-thread-1',
      title: 'Component Assembly',
      summary: '',
      messages: [],
      updatedAt: 1,
      versionCount: 1,
      pendingCount: 0,
      queuedCount: 0,
      errorCount: 0,
      status: 'active',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    };
    const importedSource = '(define-component bottle_cage () (box 1 1 1)) (model (part bottle_cage (bottle_cage)))';
    const importedDesign = { ...design, macroCode: importedSource, versionName: 'Inline component import' };
    const importedBundle = {
      ...artifactBundle,
      modelId: 'component-model-2',
      contentHash: 'sha256:artifact-2',
    };
    const importedManifest = {
      ...modelManifest,
      modelId: 'component-model-2',
      sourceDigest: 'sha256:source-2',
    };
    const importedResult = {
      version: {
        threadId: 'component-thread-1',
        baseMessageId: 'component-message-1',
        messageId: 'component-message-2',
        snapshotId: 'component-snapshot-2',
        status: 'success',
        designOutput: importedDesign,
        artifactBundle: importedBundle,
        modelManifest: importedManifest,
        parserMatched: false,
        error: null,
      },
      sourceDigest: 'sha256:source-2',
      entrySymbol: 'bottle_cage',
      partKey: 'bottle_cage',
    };

    const mockWindow = window as any;
    mockWindow.__PACKAGE_HEADERS__ = ['ok', 'componentImportError', 'componentImportPending'].includes(mockMode)
      ? [packageHeader]
      : [];
    mockWindow.__LIBRARY_LOAD_COUNT__ = 0;
    mockWindow.__LAST_PACKAGE_ARCHIVE__ = null;
    mockWindow.__LAST_PACKAGE_FILTERS__ = null;
    mockWindow.__LAST_COMPONENT_IMPORT__ = null;
    mockWindow.__LEGACY_COMPONENT_IMPORT_CALLED__ = false;
    mockWindow.__RESOLVE_COMPONENT_IMPORT__ = null;

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') {
        return {
          engines: [],
          selectedEngineId: '',
          freecadCmd: '',
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
      }
      if (cmd === 'save_config') return null;
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
      if (cmd === 'get_history') return [thread];
      if (cmd === 'get_last_design') {
        return {
          snapshotId: 'component-snapshot-1',
          design,
          threadId: 'component-thread-1',
          messageId: 'component-message-1',
          artifactBundle,
          modelManifest,
          selectedPartId: null,
        };
      }
      if (cmd === 'get_thread_latest_version' || cmd === 'get_thread_message_version') return message;
      if (cmd === 'get_thread_messages_page') {
        return { messages: [message], nextBefore: null, hasMore: false };
      }
      if (cmd === 'get_thread_window_layout') return null;
      if (cmd === 'save_thread_window_layout') return null;
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
        if (intent?.kind === 'loadComponents') {
          mockWindow.__LIBRARY_LOAD_COUNT__ += 1;
          if (mockMode === 'error') {
            throw {
              code: 'persistence',
              message: 'component library failed',
              details: 'raw package index missing',
            };
          }
          return { kind: 'componentPackages', packageHeaders: mockWindow.__PACKAGE_HEADERS__, components: mockMode === 'discovery' ? [
            { name: 'hex-bolt', version: '1.0.0', oneLiner: 'Parametric threaded bolt', paramKeys: ['d', 'length'], tags: ['fastener'], ports: [] },
            { name: 'bottle-cage', oneLiner: 'Saved bicycle bottle cage', paramKeys: ['bottleDiameter'], tags: ['bike'], ports: [] },
          ] : [] };
        }
        if (intent?.kind === 'installPackage') {
          mockWindow.__LAST_PACKAGE_ARCHIVE__ = intent.archivePath ?? null;
          if (mockMode === 'installError') {
            throw {
              code: 'validation',
              message: 'package install failed',
              details: 'raw invalid package manifest',
            };
          }
          mockWindow.__PACKAGE_HEADERS__ = [packageHeader];
          return { kind: 'componentPackages', packageHeaders: mockWindow.__PACKAGE_HEADERS__, components: [] };
        }
        throw new Error(`unexpected library panel intent: ${intent?.kind}`);
      }
      if (cmd === 'component_import_copy_inline') {
        mockWindow.__LEGACY_COMPONENT_IMPORT_CALLED__ = true;
        throw new Error('legacy component import command called');
      }
      if (cmd === 'apply_inline_component_import') {
        mockWindow.__LAST_COMPONENT_IMPORT__ = args?.input ?? null;
        if (mockMode === 'componentImportError') {
          throw {
            code: 'validation',
            message: 'component import failed',
            details: 'raw package source has unresolved dependency',
          };
        }
        if (mockMode === 'componentImportPending') {
          return new Promise((resolve) => {
            mockWindow.__RESOLVE_COMPONENT_IMPORT__ = () => resolve(importedResult);
          });
        }
        return importedResult;
      }
      if (cmd === 'plugin:dialog|open') {
        mockWindow.__LAST_PACKAGE_FILTERS__ = args?.options?.filters ?? [];
        return '/mock/bike-bottle-system.zip';
      }
      return null;
    };
  }, mode);
}

test.describe('Component package library', () => {
  test('Given saved and builtin components without packages When Components opens Then both appear and search finds their metadata', async ({ page }) => {
    await installProjectLibraryMocks(page, 'discovery');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.getByRole('button', { name: 'Reusable component library' }).click();
    await expect(page.getByRole('heading', { name: 'hex-bolt', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'bottle-cage', exact: true })).toBeVisible();
    await expect(page.getByText('NO COMPONENT PACKAGES', { exact: true })).toHaveCount(0);
    await page.getByPlaceholder('Search library...').fill('bottleDiameter');
    await expect(page.getByRole('heading', { name: 'bottle-cage', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'hex-bolt', exact: true })).toHaveCount(0);
    await page.getByPlaceholder('Search library...').fill('missing component');
    await expect(page.getByText('NO MATCHING COMPONENTS', { exact: true })).toBeVisible();
    await page.getByPlaceholder('Search library...').fill('');
    await page.getByRole('button', { name: 'REFRESH' }).click();
    await expect.poll(() => page.evaluate(() => (window as any).__LIBRARY_LOAD_COUNT__)).toBeGreaterThan(1);
    await expect(page.getByRole('heading', { name: 'hex-bolt', exact: true })).toBeVisible();
  });
  test('Given installed packages When Library opens Then concise package facts are visible', async ({ page }) => {
    await installProjectLibraryMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();

    await expect(page.getByRole('heading', { name: 'Bike Bottle System', exact: true })).toBeVisible();
    await expect(page.getByText('bike-bottle-system / 0.1.0')).toBeVisible();
    await expect(page.getByText('2 components')).toBeVisible();
    await expect(page.getByText('2 port types')).toBeVisible();
    await expect(page.getByText('Bottle Cage')).toHaveCount(0);
    await expect(page.getByText('dovetail_slot')).toHaveCount(0);
  });

  test('Given package loading fails When Library opens Then raw backend body stays visible', async ({ page }) => {
    await installProjectLibraryMocks(page, 'error');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();

    await expect(page.getByText('component library failed')).toBeVisible();
    await expect(page.getByText('raw package index missing')).toBeVisible();
  });

  test('imports a package archive and refreshes the installed list', async ({ page }) => {
    await installProjectLibraryMocks(page, 'empty');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await expect(page.getByText('NO COMPONENTS OR PACKAGES')).toBeVisible();

    await page.getByRole('button', { name: 'IMPORT PACKAGE' }).click();

    await expect(page.getByRole('heading', { name: 'Bike Bottle System', exact: true })).toBeVisible();
    await expect(page.getByText('bike-bottle-system / 0.1.0')).toBeVisible();
    await expect(page.evaluate(() => (window as any).__LAST_PACKAGE_ARCHIVE__)).resolves.toBe('/mock/bike-bottle-system.zip');
    await expect(page.evaluate(() => (window as any).__LAST_PACKAGE_FILTERS__[0]?.extensions)).resolves.toEqual(['zip']);
  });

  test('shows raw backend error when package import fails', async ({ page }) => {
    await installProjectLibraryMocks(page, 'installError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'IMPORT PACKAGE' }).click();

    await expect(page.getByText('package install failed')).toBeVisible();
    await expect(page.getByText('raw invalid package manifest')).toBeVisible();
  });

  test('Given an installed component When user imports Then one identity intent is submitted', async ({ page }) => {
    await installProjectLibraryMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'SHOW Bike Bottle System COMPONENTS' }).click();
    await page.getByRole('button', { name: 'IMPORT Bottle Cage' }).click();

    await expect(page.evaluate(() => (window as any).__LAST_COMPONENT_IMPORT__)).resolves.toMatchObject({
      threadId: 'component-thread-1',
      baseMessageId: 'component-message-1',
      expectedSourceDigest: 'sha256:source-1',
      packageId: 'bike-bottle-system',
      version: '0.1.0',
      componentId: 'bottle_cage',
    });
    await expect(page.evaluate(() => (window as any).__LAST_COMPONENT_IMPORT__)).resolves.not.toHaveProperty('authoredSource');
    await expect(page.evaluate(() => (window as any).__LEGACY_COMPONENT_IMPORT_CALLED__)).resolves.toBe(false);
  });

  test('Given component import fails When user clicks import Then raw backend body stays visible', async ({ page }) => {
    await installProjectLibraryMocks(page, 'componentImportError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'SHOW Bike Bottle System COMPONENTS' }).click();
    await page.getByRole('button', { name: 'IMPORT Bottle Cage' }).click();

    await expect(page.getByText('component import failed')).toBeVisible();
    await expect(page.getByText('raw package source has unresolved dependency')).toBeVisible();
  });

  test('Given component import is pending When user clicks import Then pending state is visible', async ({ page }) => {
    await installProjectLibraryMocks(page, 'componentImportPending');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    await page.getByRole('button', { name: 'SHOW Bike Bottle System COMPONENTS' }).click();
    await page.getByRole('button', { name: 'IMPORT Bottle Cage' }).click();

    await expect(page.getByRole('button', { name: 'IMPORTING Bottle Cage' })).toBeVisible();
    await page.evaluate(() => (window as any).__RESOLVE_COMPONENT_IMPORT__?.());
  });
});
