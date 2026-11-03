import { expect, test, type Locator, type Page } from '@playwright/test';

type ProfileMockOptions = {
  deriveMode?: 'success' | 'pending-error';
  failedProjectionView?: 'front' | 'top' | 'side';
  missingArtifact?: boolean;
  applyMode?: 'success' | 'render-error' | 'validation-error';
};

async function installProfileMocks(page: Page, options: ProfileMockOptions = {}) {
  await page.addInitScript((mockOptions) => {
    const design = {
      title: 'Flange fixture',
      versionName: 'V1',
      interactionMode: 'design',
      macroCode: '(model (part flange (difference (box 30 30 5) (translate 15 15 -1 (cylinder 3 7)))))',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    };
    const artifactBundle = {
      modelId: 'profile-model',
      sourceKind: 'generated',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      contentHash: 'profile-model-hash',
      artifactVersion: 1,
      fcstdPath: mockOptions.missingArtifact ? null : '/mock/profile-model.step',
      manifestPath: '/mock/profile-manifest.json',
      macroPath: '/mock/model.ecky',
      modelStlPath: mockOptions.missingArtifact ? null : '/mock/profile-model.stl',
      exportArtifacts: mockOptions.missingArtifact ? [] : [
        { label: 'STEP', format: 'step', path: '/mock/profile-model.step', role: 'primary' },
      ],
      viewerAssets: [],
    };
    const modelManifest = {
      modelId: 'profile-model',
      sourceKind: 'generated',
      document: {
      documentName: 'Flange fixture',
      documentLabel: 'Flange fixture',
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
    const version = {
      id: 'profile-version-1',
      role: 'assistant',
      content: 'Flange fixture',
      status: 'success',
      output: design,
      artifactBundle: mockOptions.missingArtifact ? null : artifactBundle,
      modelManifest,
      timestamp: 100,
    };
    const thread = {
      id: 'profile-thread',
      title: 'Flange fixture',
      summary: '',
      updatedAt: 100,
      versionCount: 1,
      pendingCount: 0,
      queuedCount: 0,
      errorCount: 0,
      status: 'active',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      messages: [],
    };
    const config = {
      engines: [],
      selectedEngineId: '',
      hasSeenOnboarding: true,
      freecadCmd: '',
      cadTextFontPath: '',
      freecadLibraryRoots: [],
      assets: [],
      connectionType: null,
      defaultEngineKind: 'ecky',
      defaultGeometryBackend: 'mesh',
      defaultSourceLanguage: 'ecky',
      maxGenerationAttempts: 3,
      maxVerifyAttempts: 0,
      mcp: { mode: 'passive', autoAgents: [] },
      voice: { sttLanguageCode: 'en-US' },
    };
    const capabilities = {
      freecad: { available: false, detail: 'Unavailable in mock', path: null },
      build123d: { available: false, detail: 'Unavailable in mock', path: null },
      mesh: { available: true, detail: 'bundled', path: null },
      recommendedAuthoringContext: {
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
      },
    };
    const messages = [version];
    const mock = {
      deriveCalls: [] as Array<Record<string, unknown>>,
      applyCalls: [] as Array<Record<string, unknown>>,
      projectionRequests: [] as Array<Record<string, unknown>>,
      historyRefreshes: [] as Array<Record<string, unknown>>,
      rejectPendingDerivation: null as null | (() => void),
    };
    const pendingDerivationRejectors: Array<(error: Error) => void> = [];
    mock.rejectPendingDerivation = () => {
      for (const reject of pendingDerivationRejectors.splice(0)) reject(new Error('Mock profile derivation failed.'));
    };
    (window as any).__PROFILE_AUTHORING__ = mock;
    (window as any).__TAURI_INTERNALS__ = (window as any).__TAURI_INTERNALS__ || {};
    (window as any).__TAURI_INTERNALS__.convertFileSrc = (path: string) => path;
    const eventHandlers = new Map<string, number[]>();
    let nextCallbackId = 1;
    (window as any).__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
      const callbackId = nextCallbackId++;
      (window as any)[`_${callbackId}`] = callback;
      return callbackId;
    };

    const workspaceProjection = (preferredMessageId?: string | null) => {
      const selectedVersion = preferredMessageId
        ? messages.find((message) => message.id === preferredMessageId) ?? messages[messages.length - 1]
        : messages[messages.length - 1];
      return {
        thread: { ...thread, versionCount: messages.length },
        messagesPage: {
          messages,
          nextBefore: null,
          hasMore: false,
          observedBytes: 0,
          truncatedFields: [],
        },
        selectedVersion,
        requestedMessageFound: Boolean(preferredMessageId && selectedVersion?.id === preferredMessageId),
      };
    };

    (window as any).__TAURI_INTERNALS__.invoke = async (command: string, args?: Record<string, any>) => {
      if (command === 'plugin:event|listen') {
        const event = String(args?.event ?? '');
        eventHandlers.set(event, [...(eventHandlers.get(event) ?? []), Number(args?.handler)]);
        return Number(args?.handler);
      }
      if (command === 'plugin:event|unlisten') return null;
      if (command === 'get_boot_projection') {
        return {
          config,
          history: [{ ...thread, versionCount: messages.length }],
          workspace: workspaceProjection('profile-version-1'),
          selectedPartId: null,
        };
      }
      if (command === 'get_boot_runtime_projection') return { config, capabilities };
      if (command === 'get_model_catalog_projection') return { config, models: [] };
      if (command === 'get_active_agent_sessions') return [];
      if (command === 'get_agent_terminal_snapshots') return [];
      if (command === 'get_agent_activity') {
        return { events: [], latestCursor: 0, oldestCursor: 0, hasMore: false, droppedCount: 0, retainedBytes: 0 };
      }
      if (command === 'get_thread_window_layout') {
        return {
          schemaVersion: 1,
          rememberLayout: true,
          windows: {
            sketch: { visible: false, minimized: false, x: 100, y: 80, width: 760, height: 520, z: 5 },
          },
        };
      }
      if (command === 'save_thread_window_layout') return null;
      if (command === 'get_thread_agent_state') {
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
      if (command === 'derive_profile_geometry') {
        const request = args?.request ?? {};
        mock.deriveCalls.push(request);
        if (mockOptions.deriveMode === 'pending-error') {
          return new Promise((_resolve, reject) => pendingDerivationRejectors.push(reject));
        }
        if (request.operation.kind === 'projection' && mockOptions.failedProjectionView === request.operation.view) {
          throw new Error(`Mock ${request.operation.view} projection failed.`);
        }
        return {
          sourceArtifactPath: request.artifactPath,
          sourceArtifactDigest: 'sha256:profile-source',
          operation: request.operation,
          toleranceMm: 0.5,
          units: 'mm',
          accuracy: request.operation.kind === 'section' && request.artifactPath.endsWith('.stl')
            ? 'approximateMesh'
            : 'exactBrep',
          profileAsset: request.artifactPath.endsWith('.stl')
            ? null
            : {
              artifactPath: `/mock/profile-assets/${request.operation.view ?? 'section'}.step`,
              artifactDigest: `sha256:profile-${request.operation.view ?? 'section'}`,
            },
          loops: request.operation.view === 'front'
            ? [
              { role: 'outer', parentIndex: null, points: [[-14, -6], [14, -6], [14, 6], [-14, 6]] },
              { role: 'hole', parentIndex: 0, points: [[-2, -2], [-2, 2], [2, 2], [2, -2]] },
            ]
            : request.operation.view === 'side'
              ? [{ role: 'outer', parentIndex: null, points: [[-4, -14], [4, -14], [4, 14], [-4, 14]] }]
              : [
                { role: 'outer', parentIndex: null, points: [[-10, -10], [10, -10], [10, 10], [-10, 10]] },
                { role: 'hole', parentIndex: 0, points: [[-2, -2], [-2, 2], [2, 2], [2, -2]] },
              ],
          displayAccuracy: 'approximateDisplayOnly',
          warnings: request.artifactPath.endsWith('.stl')
            ? ['Section is approximate because it was extracted from a mesh.']
            : [],
        };
      }
      if (command === 'apply_profile_feature') {
        const request = args?.request ?? {};
        mock.applyCalls.push(request);
        if (mockOptions.applyMode === 'validation-error') {
          throw new Error('Profile clearance or offset failed validation.');
        }
        const newVersion = {
          ...version,
          id: 'profile-version-2',
          content: 'Applied profile feature',
          status: mockOptions.applyMode === 'render-error' ? 'error' : 'success',
          error: mockOptions.applyMode === 'render-error'
            ? { code: 'render', message: 'Native render rejected profile.' }
            : null,
          timestamp: 200,
          output: { ...design, title: request.partName ?? 'Flange fixture', versionName: 'Profile feature' },
        };
        messages.push(newVersion);
        return {
          source: '(model (part profile_feature (extrude profile 10)))',
          draft: {
            threadId: thread.id,
            baseMessageId: 'profile-version-1',
            messageId: newVersion.id,
            status: mockOptions.applyMode === 'render-error' ? 'error' : 'success',
            designOutput: newVersion.output,
            artifactBundle,
            modelManifest,
            snapshotId: 'profile-snapshot-2',
            parserMatched: true,
            error: mockOptions.applyMode === 'render-error'
              ? { code: 'render', message: 'Native render rejected profile.' }
              : null,
          },
        };
      }
      if (command === 'get_workspace_projection') {
        mock.projectionRequests.push(args ?? {});
        return workspaceProjection(args?.preferredMessageId);
      }
      if (command === 'get_history') {
        mock.historyRefreshes.push({ kind: 'summary' });
        return [{ ...thread, versionCount: messages.length }];
      }
      if (command === 'get_thread_messages_page') {
        mock.historyRefreshes.push({ kind: 'messages', threadId: args?.threadId });
        return {
          messages,
          nextBefore: null,
          hasMore: false,
          observedBytes: 0,
          truncatedFields: [],
        };
      }
      if (command === 'get_thread_message_version') {
        return messages.find((message) => message.id === args?.messageId) ?? null;
      }
      if (command === 'get_app_window_layout') return null;
      if (command === 'save_app_window_layout') return null;
      if (command === 'get_last_design') return null;
      if (command === 'project_folder_render_activity') return [];
      if (command === 'get_project_folder_snapshot') return null;
      if (command === 'list_capture_runs') return [];
      return null;
    };
  }, options);
}

async function openSketch(page: Page) {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.route(/\.stl(?:\?|$)/i, (route) => route.fulfill({
    status: 200,
    contentType: 'model/stl',
    body: 'solid mock\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid mock\n',
  }));
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dock = page.getByRole('toolbar', { name: 'Workbench tools' });
  await dock.getByRole('button', { name: 'Sketch', exact: true }).click();
  const workspace = page.getByTestId('profile-workspace');
  await expect(workspace.getByText('Profile authoring requires an Ecky source model.')).toHaveCount(0);
  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  return workspace;
}

async function drawRectangle(workspace: Locator, view: 'front' | 'top' | 'side' = 'front') {
  await workspace.getByLabel('Drawing tool').selectOption('rectangle');
  await workspace.getByRole('button', { name: 'Draw profile' }).click();
  const canvas = workspace.getByTestId(`profile-view-${view}`);
  const bounds = await canvas.boundingBox();
  if (!bounds) throw new Error('Profile canvas is not visible.');
  const draftPolygon = canvas.locator('polygon.selected-loop');
  const points = [
    { x: bounds.width * 0.28, y: bounds.height * 0.3 },
    { x: bounds.width * 0.72, y: bounds.height * 0.7 },
  ];
  for (const [index, position] of points.entries()) {
    await canvas.click({ position });
    await expect.poll(async () => {
      const pathPoints = (await draftPolygon.getAttribute('points'))?.trim() ?? '';
      return pathPoints ? pathPoints.split(/\s+/).length : 0;
    })
      .toBe(index === 0 ? 1 : 4);
  }
}

test('manual rectangle applies as a normal model version without section reference', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await expect(workspace.getByRole('button', { name: 'Project all three views' })).toBeEnabled();
  await drawRectangle(workspace, 'top');
  await workspace.getByLabel('Profile name').fill('mounting-tab');
  await workspace.getByLabel('Depth (mm)').fill('8');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCalls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls);
  expect(applyCalls[0]).toMatchObject({
    threadId: 'profile-thread',
    baseMessageId: 'profile-version-1',
    partName: 'mounting-tab',
    depthMm: 8,
    plane: 'xy',
    reference: null,
  });
  expect(applyCalls[0].loops).toHaveLength(1);
  expect(applyCalls[0].loops[0].points).toHaveLength(4);
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionRequests.some((request: any) => request.preferredMessageId === 'profile-version-2'));
});

test('exact section applies profile asset while sampled display loops stay out of the request', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByLabel('Derivation mode').selectOption('exactSection');
  await workspace.getByLabel('Section plane').selectOption('xy');
  await workspace.getByLabel('Section offset (mm)').fill('3');
  await workspace.getByRole('button', { name: 'Extract exact section' }).click();
  await expect(workspace.getByText('EXACT CAD PROFILE')).toBeVisible();
  await expect(workspace.getByText(/XY section · offset 3.00 mm/i)).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeEnabled();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.loops).toBeUndefined();
  expect(applyCall.reference).toEqual({
    sourceArtifactPath: '/mock/profile-model.step',
    sourceArtifactDigest: 'sha256:profile-source',
    operation: { kind: 'section', plane: 'xy', offsetMm: 3 },
    toleranceMm: 0.5,
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/mock/profile-assets/section.step', artifactDigest: 'sha256:profile-section' },
  });
  const deriveCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.deriveCalls[0]);
  expect(deriveCall.artifactPath).toBe('/mock/profile-model.step');
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionRequests.some((request: any) => request.preferredMessageId === 'profile-version-2'));
});

test('approximate mesh section uses STL and submits editable loops with approximate provenance', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByLabel('Derivation mode').selectOption('meshSection');
  await workspace.getByRole('button', { name: 'Extract approximate section' }).click();
  await expect(workspace.getByText('APPROXIMATE PROFILE')).toBeVisible();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const state = await page.evaluate(() => ({
    derive: (window as any).__PROFILE_AUTHORING__.deriveCalls[0],
    apply: (window as any).__PROFILE_AUTHORING__.applyCalls[0],
  }));
  expect(state.derive.artifactPath).toBe('/mock/model.stl');
  expect(state.apply.loops).toHaveLength(2);
  expect(state.apply.reference).toMatchObject({
    sourceArtifactPath: '/mock/model.stl',
    accuracy: 'approximateMesh',
    profileAsset: null,
  });
});

test('exact projection requires explicit conversion before sampled loops can be edited or applied', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByLabel('Derivation mode').selectOption('exactProjection');
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await expect(workspace.getByTestId('profile-view-front').locator('polygon.outer')).toBeVisible();
  await expect(workspace.getByText('EXACT CAD PROFILE')).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Convert to editable approximate polygon' })).toBeVisible();
  await expect(workspace.locator('circle.vertex-handle')).toHaveCount(0);

  await workspace.getByRole('button', { name: 'Convert to editable approximate polygon' }).click();
  await expect(workspace.getByText('APPROXIMATE PROFILE')).toBeVisible();
  await expect(workspace.locator('circle.vertex-handle')).toHaveCount(8);
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.loops).toHaveLength(2);
  expect(applyCall.reference).toMatchObject({
    sourceArtifactPath: '/mock/profile-model.step',
    operation: { kind: 'projection', view: 'front' },
    accuracy: 'approximatePolygon',
    profileAsset: null,
  });
});

test('one exact projection action fills front, top, and side views; selected top view applies to same project', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await expect(workspace.getByTestId('profile-projection-views').getByTestId('profile-view-front')).toBeVisible();
  await expect(workspace.getByTestId('profile-projection-views').getByTestId('profile-view-top')).toBeVisible();
  await expect(workspace.getByTestId('profile-projection-views').getByTestId('profile-view-side')).toBeVisible();
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await expect(workspace.getByTestId('profile-view-front').locator('polygon.outer')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-top').locator('polygon.hole')).toHaveCount(1);
  await expect(workspace.getByTestId('profile-view-side').locator('polygon.outer')).toBeVisible();
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.deriveCalls.length === 3);
  const deriveCalls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.deriveCalls);
  expect(deriveCalls.map((call: any) => call.operation.view).sort()).toEqual(['front', 'side', 'top']);
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await expect(workspace.getByRole('button', { name: 'Select top projection' })).toHaveAttribute('aria-pressed', 'true');
  if (process.env.PROFILE_AUTHORING_SCREENSHOT_PATH) {
    const dismiss = page.getByRole('button', { name: 'DISMISS' });
    if (await dismiss.isVisible()) await dismiss.click();
    await page.screenshot({ path: process.env.PROFILE_AUTHORING_SCREENSHOT_PATH, fullPage: true });
  }
  await workspace.getByLabel('Clearance (mm)').fill('1.2');
  await workspace.getByLabel('Depth (mm)').fill('3.5');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.threadId).toBe('profile-thread');
  expect(applyCall.baseMessageId).toBe('profile-version-1');
  expect(applyCall.expectedSource).toContain('(part flange');
  expect(applyCall.clearanceMm).toBe(1.2);
  expect(applyCall.depthMm).toBe(3.5);
  expect(applyCall.loops).toBeUndefined();
  expect(applyCall.reference).toMatchObject({
    sourceArtifactPath: '/mock/profile-model.step',
    operation: { kind: 'projection', view: 'top' },
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/mock/profile-assets/top.step', artifactDigest: 'sha256:profile-top' },
  });
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionRequests.some((request: any) => request.preferredMessageId === 'profile-version-2'));
});

test('selecting SIDE applies side projection on YZ plane', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await expect(workspace.getByTestId('profile-view-side').locator('polygon.outer')).toBeVisible();
  await workspace.getByRole('button', { name: 'Select side projection' }).click();
  await expect(workspace.getByLabel('Profile plane', { exact: true })).toHaveValue('yz');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.plane).toBe('yz');
  expect(applyCall.reference.operation).toEqual({ kind: 'projection', view: 'side' });
  expect(applyCall.reference.profileAsset.artifactPath).toBe('/mock/profile-assets/side.step');
});

test('one failed view does not discard successful projections or block applying another view', async ({ page }) => {
  await installProfileMocks(page, { failedProjectionView: 'side' });
  const workspace = await openSketch(page);

  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await expect(workspace.getByTestId('profile-view-front').locator('polygon.outer')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-top').locator('polygon.outer')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-side')).toContainText('Mock side projection failed.');
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.reference.operation).toEqual({ kind: 'projection', view: 'top' });
  expect(applyCall.loops).toBeUndefined();
});

test('pending derivation failure keeps the manual profile and fixed controls visible', async ({ page }) => {
  await installProfileMocks(page, { deriveMode: 'pending-error' });
  const workspace = await openSketch(page);
  await drawRectangle(workspace);
  const status = workspace.getByTestId('profile-status');

  await workspace.getByLabel('Derivation mode').selectOption('exactSection');
  const statusBox = await status.boundingBox();
  await workspace.getByRole('button', { name: 'Extract exact section' }).click();
  const extracting = workspace.getByRole('button', { name: 'Deriving…' });
  await expect(extracting).toBeVisible();
  await expect(extracting).toBeDisabled();
  await expect(workspace.getByLabel('Profile name')).toBeDisabled();
  expect(await status.boundingBox()).toEqual(statusBox);

  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.rejectPendingDerivation());
  await expect(status).toContainText('Mock profile derivation failed.');
  await expect(workspace.locator('circle.vertex-handle')).toHaveCount(4);
  await expect(workspace.getByRole('button', { name: 'Draw profile' })).toBeEnabled();
  await expect(workspace.getByLabel('Profile name')).toBeEnabled();
  expect(await status.boundingBox()).toEqual(statusBox);
});

test('render failure refreshes history but preserves current preview and editable profile', async ({ page }) => {
  await installProfileMocks(page, { applyMode: 'render-error' });
  const workspace = await openSketch(page);
  await drawRectangle(workspace);
  await workspace.getByLabel('Profile name').fill('failed-tab');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await expect(workspace.getByTestId('profile-status')).toContainText('Native render rejected profile.');
  await expect(workspace.getByTestId('profile-status')).toContainText('choose it from history');
  await expect(workspace.locator('polygon.selected-loop')).toHaveCount(1);
  await expect(workspace.getByLabel('Profile name')).toHaveValue('failed-tab');
  const refreshed = await page.evaluate(() => ({
    history: (window as any).__PROFILE_AUTHORING__.historyRefreshes,
    projectedFailedVersion: (window as any).__PROFILE_AUTHORING__.projectionRequests
      .some((request: any) => request.preferredMessageId === 'profile-version-2'),
  }));
  expect(refreshed.history.some((request: any) => request.kind === 'messages')).toBe(true);
  expect(refreshed.projectedFailedVersion).toBe(false);
});

test('pre-submit validation failure keeps the current source and does not refresh version history', async ({ page }) => {
  await installProfileMocks(page, { applyMode: 'validation-error' });
  const workspace = await openSketch(page);
  await drawRectangle(workspace);
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await expect(workspace.getByTestId('profile-status')).toContainText('Profile clearance or offset failed validation.');
  await expect(workspace.locator('polygon.selected-loop')).toHaveCount(1);
  const state = await page.evaluate(() => ({
    historyRefreshes: (window as any).__PROFILE_AUTHORING__.historyRefreshes,
    messages: (window as any).__PROFILE_AUTHORING__.applyCalls.length,
    projectedFailedVersion: (window as any).__PROFILE_AUTHORING__.projectionRequests
      .some((request: any) => request.preferredMessageId === 'profile-version-2'),
  }));
  expect(state.historyRefreshes.some((request: any) => request.kind === 'messages')).toBe(false);
  expect(state.messages).toBe(1);
  expect(state.projectedFailedVersion).toBe(false);
});

test('without a model artifact Sketch remains available but derivation stays disabled', async ({ page }) => {
  await installProfileMocks(page, { missingArtifact: true });
  const workspace = await openSketch(page);

  await expect(workspace.getByLabel('Derivation mode').locator('option[value="exactProjection"]')).toHaveAttribute('disabled', '');
  await expect(workspace.getByLabel('Derivation mode').locator('option[value="exactSection"]')).toHaveAttribute('disabled', '');
  await expect(workspace.getByLabel('Derivation mode').locator('option[value="meshSection"]')).toHaveAttribute('disabled', '');
  await expect(workspace.getByRole('button', { name: 'Project all three views' })).toBeDisabled();
  await expect(workspace.getByText(/supported STEP model is needed/i)).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Draw profile' })).toBeEnabled();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
});
