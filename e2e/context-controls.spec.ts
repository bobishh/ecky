import { expect, test, type Page } from '@playwright/test';

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

const artifactBundle = {
  modelId: 'context-model',
  sourceKind: 'generated',
  engineKind: 'freecad',
  sourceLanguage: 'legacyPython',
  geometryBackend: 'freecad',
  contentHash: 'context-hash',
  artifactVersion: 1,
  fcstdPath: '/mock/context/model.FCStd',
  manifestPath: '/mock/context/manifest.json',
  macroPath: '/mock/context/source.FCMacro',
  previewStlPath: '/mock/context/preview.stl',
  viewerAssets: [],
  edgeTargets: [],
  faceTargets: [
    {
      targetId: 'low:face:top',
      durableTargetId: 'low:node:low-node:face:top',
      canonicalTargetId: 'low:face:top:canonical',
      aliasIds: ['low:face:top:alias'],
      partId: 'low',
      viewerNodeId: 'low-node',
      label: 'Low Top Face',
      editable: true,
      center: { x: 0.33, y: 0.33, z: 0 },
      normal: [0, 0, 1],
      area: 100,
    },
  ],
};

const modelManifest = {
  schemaVersion: 2,
  modelId: 'context-model',
  sourceKind: 'generated',
  document: {
    documentName: 'Context Controls',
    documentLabel: 'Context Controls',
    sourcePath: null,
    objectCount: 2,
    warnings: [],
  },
  parts: [
    {
      partId: 'low',
      freecadObjectName: 'Low',
      label: 'Low',
      kind: 'Part::Feature',
      semanticRole: 'body',
      viewerAssetPath: '/mock/context/low.stl',
      viewerNodeIds: ['low-node'],
      parameterKeys: ['low_width'],
      editable: true,
      bounds: null,
      volume: null,
      area: null,
    },
    {
      partId: 'nose',
      freecadObjectName: 'Nose',
      label: 'Nose',
      kind: 'Part::Feature',
      semanticRole: 'connector',
      viewerAssetPath: '/mock/context/nose.stl',
      viewerNodeIds: ['nose-node'],
      parameterKeys: [],
      editable: true,
      bounds: null,
      volume: null,
      area: null,
    },
  ],
  parameterGroups: [],
  controlPrimitives: [],
  controlRelations: [],
  controlViews: [],
  advisories: [],
  selectionTargets: [
    {
      targetId: 'low:face:top',
      durableTargetId: 'low:node:low-node:face:top',
      canonicalTargetId: 'low:face:top:canonical',
      aliasIds: ['low:face:top:alias'],
      partId: 'low',
      viewerNodeId: 'low-node',
      label: 'Low Top Face',
      kind: 'face',
      editable: true,
      parameterKeys: ['low_width'],
      primitiveIds: [],
      viewIds: [],
    },
  ],
  measurementAnnotations: [],
  warnings: [],
  enrichmentState: { status: 'none', proposals: [] },
};

const design = {
  title: 'Context Controls',
  versionName: 'V1',
  response: '',
  interactionMode: 'design',
  macroCode: '# context controls',
  sourceLanguage: 'legacyPython',
  geometryBackend: 'freecad',
  uiSpec: {
    fields: [
      { type: 'number', key: 'hose_od', label: 'Hose OD' },
      { type: 'number', key: 'low_width', label: 'Low Width' },
    ],
  },
  initialParams: { hose_od: 19, low_width: 42 } as Record<string, number>,
  postProcessing: null,
};

async function installContextMocks(
  page: Page,
  overrides?: {
    artifactBundle?: typeof artifactBundle;
    modelManifest?: typeof modelManifest;
    design?: typeof design;
  },
) {
  const bundle = overrides?.artifactBundle ?? artifactBundle;
  const manifest = overrides?.modelManifest ?? modelManifest;
  const mockedDesign = overrides?.design ?? design;
  await page.route(/\/mock\/context\/.*\.stl(?:\?.*)?$/, async (route) => {
    await route.fulfill({ status: 200, contentType: 'model/stl', body: MOCK_STL });
  });

  await page.addInitScript(({ config, runtimeCapabilities, artifactBundle, modelManifest, design }) => {
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') return config;
      if (cmd === 'get_runtime_capabilities') return runtimeCapabilities;
      if (cmd === 'get_history') return [];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_default_macro') return '';
      if (cmd === 'check_freecad') return true;
      if (cmd === 'init_generation_attempt') return 'msg-context';
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
        return {
          design,
          threadId: 'thread-context',
          messageId: 'msg-context',
          usage: null,
        };
      }
      if (cmd === 'render_model') return artifactBundle;
      if (cmd === 'get_model_manifest') return modelManifest;
      if (cmd === 'get_thread') {
        return {
          id: args.id,
          title: 'Context Controls',
          summary: '',
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
      }
      if (cmd === 'get_thread_latest_version' || cmd === 'get_thread_message_version') {
        return {
          id: 'msg-context',
          role: 'assistant',
          content: 'Context Controls',
          status: 'success',
          output: design,
          artifactBundle,
          modelManifest,
          timestamp: 100,
        };
      }
      if (cmd === 'get_thread_messages_page') {
        return {
          messages: [],
          nextBefore: null,
          hasMore: false,
        };
      }
      if (cmd === 'verify_generated_model') {
        return {
          passed: true,
          summary: 'Structural checks passed.',
          issues: [],
          metrics: {
            partCount: 2,
            previewStlSizeBytes: 1024,
            totalVolume: 1000,
            totalArea: 500,
            bbox: { xMin: 0, yMin: 0, zMin: 0, xMax: 10, yMax: 10, zMax: 10 },
          },
          verifierStatus: 'ok',
          verifierSource: 'mock',
        };
      }
      if (cmd === 'finalize_generation_attempt') return null;
      if (cmd === 'save_last_design') return null;
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
      return null;
    };
  }, { config, runtimeCapabilities, artifactBundle: bundle, modelManifest: manifest, design: mockedDesign });
}

test('Given part selection When only model-global controls exist Then part panel does not duplicate them', async ({ page }) => {
  await installContextMocks(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'make contextual controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await expect(page.locator('.param-panel')).toBeVisible();

  await page.getByRole('button', { name: 'Low' }).click();
  await expect(page.locator('.param-panel').getByText('Low Width', { exact: true })).toBeVisible();

  await page.getByRole('button', { name: 'Nose' }).click();
  await expect(page.getByText('No semantic controls are mapped to this part yet. Open RAW for fallback.')).toBeVisible();
  await expect(page.getByText('Hose OD')).toHaveCount(0);
});

test('Given mapped film gate context When selected Then Params shows gap-related controls only', async ({ page }) => {
  await installContextMocks(page, {
    design: {
      ...design,
      title: 'Film Adapter Coupon',
      versionName: 'V-film-coupon',
      macroCode: '# film adapter coupon',
      uiSpec: {
        fields: [
          { type: 'number', key: 'film_gap', label: 'Film Gap' },
          { type: 'number', key: 'lens_bore_d', label: 'Lens Bore D' },
        ],
      },
      initialParams: { film_gap: 0.35, lens_bore_d: 59.6 },
    },
    artifactBundle: {
      ...artifactBundle,
      modelId: 'film-coupon-model',
      faceTargets: [
        {
          targetId: 'film_gate:face:slot',
          durableTargetId: 'film_gate:node:film-gate-node:face:slot',
          canonicalTargetId: 'film_gate:face:slot:canonical',
          aliasIds: ['film_gate:face:slot:alias'],
          partId: 'film_gate',
          viewerNodeId: 'film-gate-node',
          label: 'Film Gate Slot Face',
          editable: true,
          center: { x: 0.4, y: 0.4, z: 0.1 },
          normal: [0, 0, 1],
          area: 18,
        },
      ],
    },
    modelManifest: {
      ...modelManifest,
      modelId: 'film-coupon-model',
      parts: [
        {
          partId: 'film_gate',
          freecadObjectName: 'FilmGate',
          label: 'Film Gate',
          kind: 'Part::Feature',
          semanticRole: 'gate',
          viewerAssetPath: '/mock/context/film-gate.stl',
          viewerNodeIds: ['film-gate-node'],
          parameterKeys: ['film_gap'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
        {
          partId: 'lens_adapter',
          freecadObjectName: 'LensAdapter',
          label: 'Lens Adapter',
          kind: 'Part::Feature',
          semanticRole: 'lens',
          viewerAssetPath: '/mock/context/lens-adapter.stl',
          viewerNodeIds: ['lens-adapter-node'],
          parameterKeys: ['lens_bore_d'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
      ],
      selectionTargets: [
        {
          targetId: 'film_gate:face:slot',
          durableTargetId: 'film_gate:node:film-gate-node:face:slot',
          canonicalTargetId: 'film_gate:face:slot:canonical',
          aliasIds: ['film_gate:face:slot:alias'],
          partId: 'film_gate',
          viewerNodeId: 'film-gate-node',
          label: 'Film Gate Slot Face',
          kind: 'face',
          editable: true,
          parameterKeys: ['film_gap'],
          primitiveIds: [],
          viewIds: [],
        },
      ],
    },
  });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'load film coupon controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await expect(page.locator('.param-panel')).toBeVisible();
  await page.getByRole('button', { name: 'Film Gate' }).click();

  await expect(page.locator('.param-panel')).toContainText('Film Gap');
  await expect(page.locator('.param-panel')).not.toContainText('Lens Bore D');
  await expect(page.locator('.param-panel .param-list .param-field')).toHaveCount(1);
});

test('Given Params select mode on mapped film gate target When target selected Then panel shows frame and gap controls without helicoid controls', async ({
  page,
}) => {
  await installContextMocks(page, {
    design: {
      ...design,
      title: 'Film Gate Isolation',
      versionName: 'V-film-gate-isolation',
      macroCode: '# film gate isolation',
      uiSpec: {
        fields: [
          { type: 'number', key: 'film_gap', label: 'Film Gap' },
          { type: 'number', key: 'film_frame_width', label: 'Frame Width' },
          { type: 'number', key: 'helicoid_pitch', label: 'Helicoid Pitch' },
          { type: 'number', key: 'helicoid_clearance', label: 'Helicoid Clearance' },
        ],
      },
      initialParams: { film_gap: 0.4, film_frame_width: 13.8, helicoid_pitch: 1.2, helicoid_clearance: 0.25 },
    },
    artifactBundle: {
      ...artifactBundle,
      modelId: 'film-gate-isolation-model',
      faceTargets: [
        {
          targetId: 'film_gate:face:slot',
          durableTargetId: 'film_gate:node:film-gate-node:face:slot',
          canonicalTargetId: 'film_gate:face:slot:canonical',
          aliasIds: ['film_gate:face:slot:alias'],
          partId: 'film_gate',
          viewerNodeId: 'film-gate-node',
          label: 'Film Gate Slot Face',
          editable: true,
          center: { x: 0.42, y: 0.42, z: 0.12 },
          normal: [0, 0, 1],
          area: 21,
        },
      ],
    },
    modelManifest: {
      ...modelManifest,
      modelId: 'film-gate-isolation-model',
      parts: [
        {
          partId: 'film_gate',
          freecadObjectName: 'FilmGate',
          label: 'Film Gate',
          kind: 'Part::Feature',
          semanticRole: 'gate',
          viewerAssetPath: '/mock/context/film-gate.stl',
          viewerNodeIds: ['film-gate-node'],
          parameterKeys: ['film_gap', 'film_frame_width'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
        {
          partId: 'helicoid_adapter',
          freecadObjectName: 'HelicoidAdapter',
          label: 'Helicoid Adapter',
          kind: 'Part::Feature',
          semanticRole: 'thread',
          viewerAssetPath: '/mock/context/helicoid-adapter.stl',
          viewerNodeIds: ['helicoid-adapter-node'],
          parameterKeys: ['helicoid_pitch', 'helicoid_clearance'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
      ],
      selectionTargets: [
        {
          targetId: 'film_gate:face:slot',
          durableTargetId: 'film_gate:node:film-gate-node:face:slot',
          canonicalTargetId: 'film_gate:face:slot:canonical',
          aliasIds: ['film_gate:face:slot:alias'],
          partId: 'film_gate',
          viewerNodeId: 'film-gate-node',
          label: 'Film Gate Slot Face',
          kind: 'face',
          editable: true,
          parameterKeys: ['film_gap', 'film_frame_width'],
          primitiveIds: [],
          viewIds: [],
        },
      ],
    },
  });

  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'load film gate isolation controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await page.getByRole('button', { name: 'SELECT' }).click();
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  const viewer = page.locator('.viewer-host').first();
  const bounds = await viewer.boundingBox();
  expect(bounds).not.toBeNull();
  if (!bounds) throw new Error('viewer bounds missing');

  await page.waitForTimeout(250);
  await page.mouse.click(bounds.x + bounds.width * 0.5, bounds.y + bounds.height * 0.5);

  await expect(page.locator('.param-panel')).toContainText('Film Gap');
  await expect(page.locator('.param-panel')).toContainText('Frame Width');
  await expect(page.locator('.param-panel')).not.toContainText('Helicoid Pitch');
  await expect(page.locator('.param-panel')).not.toContainText('Helicoid Clearance');
  await expect(page.locator('.param-panel .param-list .param-field')).toHaveCount(2);
  await expect(page.locator('.part-chip.part-chip-active')).toContainText('Film Gate');
});

test('Given Params select mode When mapped lens-bore target selected Then panel shows exactly one relevant lens-bore control and excludes unrelated controls', async ({
  page,
}) => {
  await installContextMocks(page, {
    design: {
      ...design,
      title: 'Lens Bore Isolation',
      versionName: 'V-lens-bore-isolation',
      macroCode: '# lens bore isolation',
      uiSpec: {
        fields: [
          { type: 'number', key: 'film_gap', label: 'Film Gap' },
          { type: 'number', key: 'film_frame_width', label: 'Frame Width' },
          { type: 'number', key: 'lens_bore_d', label: 'Lens Bore D' },
          { type: 'number', key: 'helicoid_pitch', label: 'Helicoid Pitch' },
        ],
      },
      initialParams: { film_gap: 0.4, film_frame_width: 13.8, lens_bore_d: 59.6, helicoid_pitch: 1.2 },
    },
    artifactBundle: {
      ...artifactBundle,
      modelId: 'lens-bore-isolation-model',
      faceTargets: [
        {
          targetId: 'lens_adapter:face:bore',
          durableTargetId: 'lens_adapter:node:lens-adapter-node:face:bore',
          canonicalTargetId: 'lens_adapter:face:bore:canonical',
          aliasIds: ['lens_adapter:face:bore:alias'],
          partId: 'lens_adapter',
          viewerNodeId: 'lens-adapter-node',
          label: 'Lens Bore Face',
          editable: true,
          center: { x: 0.52, y: 0.52, z: 0.12 },
          normal: [0, 0, 1],
          area: 20,
        },
      ],
    },
    modelManifest: {
      ...modelManifest,
      modelId: 'lens-bore-isolation-model',
      parts: [
        {
          partId: 'film_gate',
          freecadObjectName: 'FilmGate',
          label: 'Film Gate',
          kind: 'Part::Feature',
          semanticRole: 'gate',
          viewerAssetPath: '/mock/context/film-gate.stl',
          viewerNodeIds: ['film-gate-node'],
          parameterKeys: ['film_gap', 'film_frame_width'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
        {
          partId: 'lens_adapter',
          freecadObjectName: 'LensAdapter',
          label: 'Lens Adapter',
          kind: 'Part::Feature',
          semanticRole: 'lens',
          viewerAssetPath: '/mock/context/lens-adapter.stl',
          viewerNodeIds: ['lens-adapter-node'],
          parameterKeys: ['lens_bore_d'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
        {
          partId: 'helicoid_adapter',
          freecadObjectName: 'HelicoidAdapter',
          label: 'Helicoid Adapter',
          kind: 'Part::Feature',
          semanticRole: 'thread',
          viewerAssetPath: '/mock/context/helicoid-adapter.stl',
          viewerNodeIds: ['helicoid-adapter-node'],
          parameterKeys: ['helicoid_pitch'],
          editable: true,
          bounds: null,
          volume: null,
          area: null,
        },
      ],
      selectionTargets: [
        {
          targetId: 'lens_adapter:face:bore',
          durableTargetId: 'lens_adapter:node:lens-adapter-node:face:bore',
          canonicalTargetId: 'lens_adapter:face:bore:canonical',
          aliasIds: ['lens_adapter:face:bore:alias'],
          partId: 'lens_adapter',
          viewerNodeId: 'lens-adapter-node',
          label: 'Lens Bore Face',
          kind: 'face',
          editable: true,
          parameterKeys: ['lens_bore_d'],
          primitiveIds: [],
          viewIds: [],
        },
      ],
    },
  });

  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'load lens bore isolation controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await page.getByRole('button', { name: 'SELECT' }).click();
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  const viewer = page.locator('.viewer-host').first();
  const bounds = await viewer.boundingBox();
  expect(bounds).not.toBeNull();
  if (!bounds) throw new Error('viewer bounds missing');

  await page.waitForTimeout(250);
  await page.mouse.click(bounds.x + bounds.width * 0.5, bounds.y + bounds.height * 0.5);

  await expect(page.locator('.param-panel')).toContainText('Lens Bore D');
  await expect(page.locator('.param-panel')).not.toContainText('Film Gap');
  await expect(page.locator('.param-panel')).not.toContainText('Frame Width');
  await expect(page.locator('.param-panel')).not.toContainText('Helicoid Pitch');
  await expect(page.locator('.param-panel .param-list .param-field')).toHaveCount(1);
  await expect(page.locator('.part-chip.part-chip-active')).toContainText('Lens Adapter');
});

test('Given workbench idle When Params opens Then prewarmed panel is reused', async ({ page }) => {
  await installContextMocks(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);

  await page.waitForFunction(() => Boolean(document.querySelector('[data-window-id="params"] .param-panel')));
  await expect(page.locator('[data-window-id="params"].window--hidden .param-panel')).toHaveCount(1);

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await expect(page.locator('[data-window-id="params"] .param-panel')).toBeVisible();

  await page.locator('[data-window-id="params"] .window-close').click();
  await expect(page.locator('[data-window-id="params"].window--hidden .param-panel')).toHaveCount(1);

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await expect(page.locator('[data-window-id="params"] .param-panel')).toBeVisible();
});

test('Given Params select mode When viewer face is clicked Then Params focuses exact face control', async ({ page }) => {
  await installContextMocks(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'make contextual controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await page.getByRole('button', { name: 'SELECT' }).click();
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  const viewer = page.locator('.viewer-host').first();
  const bounds = await viewer.boundingBox();
  expect(bounds).not.toBeNull();
  if (!bounds) throw new Error('viewer bounds missing');

  await page.waitForTimeout(250);
  for (const yRatio of [0.45, 0.53, 0.61]) {
    for (const xRatio of [0.36, 0.5, 0.64]) {
      await page.mouse.click(bounds.x + bounds.width * xRatio, bounds.y + bounds.height * yRatio);
      const panelText = (await page.locator('.param-panel').allTextContents())[0] ?? '';
      if (panelText.includes('Low Width')) break;
    }
    const panelText = (await page.locator('.param-panel').allTextContents())[0] ?? '';
    if (panelText.includes('Low Width')) break;
  }

  await expect(page.locator('.viewer-part-overlay')).toHaveCount(0);
  await expect(page.locator('.param-panel')).toContainText('Low Width');
  await expect(page.locator('.param-panel')).not.toContainText('Hose OD');
  await expect(page.locator('.param-panel .param-list .param-field')).toHaveCount(1);
  await expect(page.locator('.part-chip.part-chip-active')).toContainText('Low');
});

test('Given Params select mode When viewer click hits unmapped part Then Params shows empty semantic state', async ({ page }) => {
  await installContextMocks(page, {
    artifactBundle: {
      ...artifactBundle,
      faceTargets: [],
    },
    modelManifest: {
      ...modelManifest,
      selectionTargets: [],
    },
  });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'make contextual controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await page.getByRole('button', { name: 'SELECT' }).click();
  await page.getByRole('button', { name: 'Nose' }).click();
  await expect(page.locator('.param-panel')).toContainText(
    'No semantic controls are mapped to this part yet. Open RAW for fallback.',
  );
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  const viewer = page.locator('.viewer-host').first();
  const bounds = await viewer.boundingBox();
  expect(bounds).not.toBeNull();
  if (!bounds) throw new Error('viewer bounds missing');

  await page.mouse.click(bounds.x + bounds.width * 0.94, bounds.y + bounds.height * 0.1);
  await expect(page.locator('.param-panel')).toContainText(
    'No semantic controls are mapped to this part yet. Open RAW for fallback.',
  );
  await expect(page.locator('.part-chip.part-chip-active')).toContainText('Nose');
});

test('Given select mode with ambiguous face targets When no face selected Then Params shows pending target message and no fallback controls', async ({
  page,
}) => {
  await installContextMocks(page, {
    artifactBundle: {
      ...artifactBundle,
      faceTargets: [
        {
          targetId: 'low:face:threadA',
          durableTargetId: 'low:node:low-node:face:threadA',
          canonicalTargetId: 'low:face:threadA:canonical',
          aliasIds: ['low:face:threadA:alias'],
          partId: 'low',
          viewerNodeId: 'low-node',
          label: 'Low Thread Face A',
          editable: true,
          center: { x: 0.28, y: 0.35, z: 0.06 },
          normal: [0, 0, 1],
          area: 12,
        },
        {
          targetId: 'low:face:threadB',
          durableTargetId: 'low:node:low-node:face:threadB',
          canonicalTargetId: 'low:face:threadB:canonical',
          aliasIds: ['low:face:threadB:alias'],
          partId: 'low',
          viewerNodeId: 'low-node',
          label: 'Low Thread Face B',
          editable: true,
          center: { x: 0.62, y: 0.38, z: 0.08 },
          normal: [0, 0, 1],
          area: 10,
        },
      ],
    },
    modelManifest: {
      ...modelManifest,
      selectionTargets: [
        {
          targetId: 'low:face:threadA',
          durableTargetId: 'low:node:low-node:face:threadA',
          canonicalTargetId: 'low:face:threadA:canonical',
          aliasIds: ['low:face:threadA:alias'],
          partId: 'low',
          viewerNodeId: 'low-node',
          label: 'Low Thread Face A',
          kind: 'face',
          editable: true,
          parameterKeys: ['low_width'],
          primitiveIds: [],
          viewIds: [],
        },
        {
          targetId: 'low:face:threadB',
          durableTargetId: 'low:node:low-node:face:threadB',
          canonicalTargetId: 'low:face:threadB:canonical',
          aliasIds: ['low:face:threadB:alias'],
          partId: 'low',
          viewerNodeId: 'low-node',
          label: 'Low Thread Face B',
          kind: 'face',
          editable: true,
          parameterKeys: ['low_width'],
          primitiveIds: [],
          viewIds: [],
        },
      ],
    },
  });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'show thread face controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await page.getByRole('button', { name: 'SELECT' }).click();
  await expect(page.locator('.param-panel')).toContainText(
    'Multiple face targets found. Select one in viewport; fallback waits for explicit target.',
  );
  await expect(page.locator('.param-panel')).not.toContainText('Low Width');
  await expect(page.locator('.viewer-part-overlay')).toHaveCount(0);
});

test('Given Params measure mode When viewer is clicked and dragged Then Params focus and part selection stay unchanged', async ({ page }) => {
  await installContextMocks(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'make contextual controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await page.getByRole('button', { name: 'MEASURE' }).click();
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  const viewer = page.locator('.viewer-host').first();
  const bounds = await viewer.boundingBox();
  expect(bounds).not.toBeNull();
  if (!bounds) throw new Error('viewer bounds missing');

  const centerX = bounds.x + bounds.width * 0.5;
  const centerY = bounds.y + bounds.height * 0.5;
  await page.mouse.click(centerX, centerY);
  await page.mouse.move(centerX, centerY);
  await page.mouse.down();
  await page.mouse.move(centerX + 100, centerY + 35, { steps: 12 });
  await page.mouse.up();

  await expect(page.locator('.viewer-part-overlay')).toHaveCount(0);
  await expect(page.locator('.part-chip-active')).toHaveCount(0);
  await expect(page.locator('.param-panel .param-list .param-field')).toHaveCount(2);
  await expect(page.locator('.param-panel')).toContainText('Hose OD');
  await expect(page.locator('.param-panel')).toContainText('Low Width');
});

test('Given default orbit mode and no selection When user drags viewer Then it does not select a part or open viewport controls', async ({ page }) => {
  await installContextMocks(page, {
    artifactBundle: {
      ...artifactBundle,
      faceTargets: [],
    },
    modelManifest: {
      ...modelManifest,
      selectionTargets: [],
    },
  });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dismissError = page.getByRole('button', { name: 'Dismiss error' });
  if (await dismissError.count()) {
    await dismissError.click();
  }

  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'make contextual controls');
  await page
    .locator('textarea.prompt-input')
    .press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

  await page.getByRole('button', { name: 'PARAMS' }).click();
  await expect(page.getByRole('button', { name: 'ORBIT' })).toHaveClass(/panel-mode-tab-active/);
  await expect(page.locator('.part-chip-active')).toHaveCount(0);
  await expect(page.locator('.viewer-part-overlay')).toHaveCount(0);
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  const viewer = page.locator('.viewer-host').first();
  const bounds = await viewer.boundingBox();
  expect(bounds).not.toBeNull();
  if (!bounds) throw new Error('viewer bounds missing');

  const startX = bounds.x + bounds.width * 0.5;
  const startY = bounds.y + bounds.height * 0.5;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + 90, startY + 30, { steps: 12 });
  await page.mouse.up();

  await expect(page.locator('.viewer-part-overlay')).toHaveCount(0);
  await expect(page.locator('.part-chip-active')).toHaveCount(0);
});
