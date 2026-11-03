import { expect, type Locator, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';

type ProfileMockOptions = {
  deriveMode?: 'success' | 'pending-error';
  failedProjectionView?: 'front' | 'top' | 'side';
  failedProfileView?: 'front' | 'top' | 'side';
  structuredProjectionError?: boolean;
  nativeProjectionDirectory?: string;
  missingArtifact?: boolean;
  applyMode?: 'success' | 'render-error' | 'validation-error';
  drawingMode?: 'success' | 'recognition-error' | 'review-error';
};

export async function installProfileMocks(page: Page, options: ProfileMockOptions = {}) {
  const [sourceImage, transformedPreview] = await Promise.all([
    readFile(new URL('./fixtures/plate-drawing-source.png', import.meta.url)),
    readFile(new URL('./fixtures/plate-drawing-preview.png', import.meta.url)),
  ]);
  const nativeProjectionEntries = options.nativeProjectionDirectory
    ? await Promise.all((['front', 'top', 'side'] as const).map(async (view) => {
      const candidates = [
        `${options.nativeProjectionDirectory}/${view}/final2/display-edges.json`,
        `${options.nativeProjectionDirectory}/${view}/out/display-edges.json`,
      ];
      let raw: string | null = null;
      for (const path of candidates) {
        try {
          raw = await readFile(path, 'utf8');
          break;
        } catch {
          // Try the next known native-output layout.
        }
      }
      if (raw === null) throw new Error(`No native projection edges found for ${view} in ${options.nativeProjectionDirectory}.`);
      return [view, JSON.parse(raw) as { displayEdges: number[][][]; warnings?: string[] }] as const;
    }))
    : [];
  const images = {
    source: `data:image/png;base64,${sourceImage.toString('base64')}`,
    transformed: `data:image/png;base64,${transformedPreview.toString('base64')}`,
  };
  await page.addInitScript((payload) => {
    const mockOptions = payload.options;
    const imageDataUrls = payload.images;
    const nativeProjectionByView = Object.fromEntries(payload.nativeProjectionEntries ?? []) as Record<string, { displayEdges: number[][][]; warnings?: string[] }>;
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
      projectionDrawingCalls: [] as Array<Record<string, unknown>>,
      applyCalls: [] as Array<Record<string, unknown>>,
      projectionRequests: [] as Array<Record<string, unknown>>,
      historyRefreshes: [] as Array<Record<string, unknown>>,
      rejectPendingDerivation: null as null | (() => void),
      preparedDrawingPath: null as string | null,
      recognitionCalls: [] as Array<Record<string, any>>,
      reviewCalls: [] as Array<Record<string, any>>,
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
      if (command === 'plugin:dialog|open') return '/mock/plate-photo.png';
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
      if (command === 'prepare_drawing_image') {
        const sourcePath = args?.sourcePath ?? args?.request?.sourcePath;
        mock.preparedDrawingPath = sourcePath;
        return {
          image: { artifactPath: '/managed/plate-photo.png', digest: 'sha256:plate-photo', widthPx: 640, heightPx: 480, format: 'png' },
          previewDataUrl: imageDataUrls.source,
          previewWidthPx: 640,
          previewHeightPx: 480,
        };
      }
      if (command === 'recognize_drawing_profile') {
        mock.recognitionCalls.push(args?.request ?? args ?? {});
        if (mockOptions.drawingMode === 'recognition-error') throw new Error('Mock drawing recognition failed.');
        const request = args?.request ?? args ?? {};
        const evidence = {
          image: request.image,
          view: request.view,
          region: request.region,
          rectification: request.rectification ?? null,
          calibration: request.calibration,
          toleranceMm: request.toleranceMm,
          extractorVersion: 'mock-centerline-1',
        };
        const sourceToPreviewTransform = [1.5625, 0, -100, 0, 1.4880952381, -107.1428571, 0, 0, 1];
        const transformPoint = ([x, y]: number[]) => {
          const w = sourceToPreviewTransform[6]! * x + sourceToPreviewTransform[7]! * y + sourceToPreviewTransform[8]!;
          return [
            (sourceToPreviewTransform[0]! * x + sourceToPreviewTransform[1]! * y + sourceToPreviewTransform[2]!) / w,
            (sourceToPreviewTransform[3]! * x + sourceToPreviewTransform[4]! * y + sourceToPreviewTransform[5]!) / w,
          ];
        };
        const calibrationStart = transformPoint(request.calibration.startPx);
        const calibrationEnd = transformPoint(request.calibration.endPx);
        const transformedCalibrationLength = Math.hypot(calibrationEnd[0]! - calibrationStart[0]!, calibrationEnd[1]! - calibrationStart[1]!);
        return {
          evidenceId: 'drawing-evidence-1',
          evidence,
          primitives: [
            { kind: 'polyline', id: 'plate-outline', pointsMm: [[0, 50], [80, 50], [80, 0], [0, 0]], closed: true, residualMm: 0.08 },
            { kind: 'circle', id: 'hole-1', centerMm: [40, 25], radiusMm: 5, residualMm: 0.12 },
          ],
          profiles: [
            { id: 'plate-outline', primitiveIds: ['plate-outline'], role: 'outer', parentId: null, closed: true, residualMm: 0.08, issues: [] },
            { id: 'plate-hole', primitiveIds: ['hole-1'], role: 'hole', parentId: 'plate-outline', closed: true, residualMm: 0.12, issues: [] },
          ],
          diagnostics: [],
          transformedPreviewDataUrl: imageDataUrls.transformed,
          previewWidthPx: 800,
          previewHeightPx: 500,
          sourceToPreviewTransform,
          millimetersPerPixel: request.calibration.distanceMm / transformedCalibrationLength,
        };
      }
      if (command === 'review_drawing_profile') {
        const request = args?.request ?? args ?? {};
        mock.reviewCalls.push(request);
        if (mockOptions.drawingMode === 'review-error') throw new Error('Mock contour review failed.');
        const outerPrimitives = request.primitives.filter((primitive: any) => primitive.kind !== 'circle');
        const circles = request.primitives.filter((primitive: any) => primitive.kind === 'circle');
        return {
          evidenceId: request.evidenceId,
          profiles: [
            { id: 'plate-outline', primitiveIds: outerPrimitives.map((primitive: any) => primitive.id), role: 'outer', parentId: null, closed: true, residualMm: 0.08, issues: [] },
            ...circles.map((primitive: any) => ({ id: 'plate-hole', primitiveIds: [primitive.id], role: 'hole', parentId: 'plate-outline', closed: true, residualMm: primitive.residualMm, issues: [] })),
          ],
          diagnostics: [],
        };
      }
      if (command === 'project_profile_drawing') {
        const request = args?.request ?? {};
        mock.projectionDrawingCalls.push(request);
        if (mockOptions.failedProjectionView === request.view) {
          if (mockOptions.structuredProjectionError) {
            throw {
              code: 'render',
              message: 'Could not create exact projection.',
              details: 'No visible projection edges were returned. view=side op=projection',
            };
          }
          throw new Error(`Mock ${request.view} drawing projection failed.`);
        }
        const fixtureDisplayEdges = request.view === 'front'
          ? [[[-14, -6], [14, -6]], [[14, -6], [14, 6]], [[14, 6], [-14, 6]], [[-14, 6], [-14, -6]]]
          : request.view === 'side'
            ? [[[-4, -14], [4, -14]], [[4, -14], [4, 14]], [[4, 14], [-4, 14]], [[-4, 14], [-4, -14]]]
            : [[[-10, -10], [10, -10]], [[10, -10], [10, 10]], [[10, 10], [-10, 10]], [[-10, 10], [-10, -10]]];
        const nativeProjection = nativeProjectionByView[request.view];
        return {
          sourceArtifactPath: request.artifactPath,
          sourceArtifactDigest: 'sha256:profile-source',
          view: request.view,
          visibility: request.visibility ?? 'allEdges',
          toleranceMm: request.toleranceMm,
          units: 'mm',
          projectionAsset: { artifactPath: `/mock/profile-projections/${request.view}.step`, artifactDigest: `sha256:projection-${request.view}` },
          displayEdges: nativeProjection?.displayEdges ?? fixtureDisplayEdges,
          warnings: nativeProjection?.warnings ?? [],
        };
      }
      if (command === 'derive_profile_geometry') {
        const request = args?.request ?? {};
        mock.deriveCalls.push(request);
        if (mockOptions.deriveMode === 'pending-error') {
          return new Promise((_resolve, reject) => pendingDerivationRejectors.push(reject));
        }
        if (request.operation.kind === 'projection' && mockOptions.failedProfileView === request.operation.view) {
          throw new Error(`Mock ${request.operation.view} closed profile failed.`);
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
  }, { options, images, nativeProjectionEntries });
}

export async function openSketch(page: Page, allThree = true) {
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
  if (allThree) await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  return workspace;
}

export async function drawRectangle(workspace: Locator, view: 'front' | 'top' | 'side' = 'front') {
  await workspace.getByRole('button', { name: `Select ${view} projection` }).click();
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

