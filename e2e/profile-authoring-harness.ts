import { expect, type Locator, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';

type ProfileMockOptions = {
  deriveMode?: 'success' | 'pending-error';
  failedProjectionView?: 'front' | 'top' | 'side';
  failedProfileView?: 'front' | 'top' | 'side';
  structuredProjectionError?: boolean;
  nativeProjectionDirectory?: string;
  analyticProjectionCurves?: boolean;
  missingArtifact?: boolean;
  missingProject?: boolean;
  applyMode?: 'success' | 'render-error' | 'validation-error';
  drawingMode?: 'success' | 'recognition-error' | 'review-error';
  livePaper?: boolean;
  restoreForeignLivePaper?: boolean;
  livePaperProfileView?: 'front' | 'top' | 'side';
  holdLivePaperProfile?: boolean;
};

export async function installProfileMocks(page: Page, options: ProfileMockOptions = {}) {
  await page.route('https://127.0.0.1:4444/**', (route) => route.fulfill({
    status: 200, contentType: 'image/svg+xml',
    body: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" fill="white"/><path d="M10 10h25v25H10zM65 10h25v25H65zM10 65h25v25H10z" fill="black"/></svg>',
  }));
  const [sourceImage, transformedPreview, cameraQr] = await Promise.all([
    readFile(new URL('./fixtures/plate-drawing-source.png', import.meta.url)),
    readFile(new URL('./fixtures/plate-drawing-preview.png', import.meta.url)),
    readFile(new URL('./fixtures/live-camera-qr.svg', import.meta.url)),
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
      return [view, JSON.parse(raw) as { displayEdges: number[][][]; displayCurves?: any[]; hiddenDisplayEdges?: number[][][]; hiddenDisplayCurves?: any[]; warnings?: string[] }] as const;
    }))
    : [];
  const images = {
    source: `data:image/png;base64,${sourceImage.toString('base64')}`,
    transformed: `data:image/png;base64,${transformedPreview.toString('base64')}`,
    cameraQr: `data:image/svg+xml;base64,${cameraQr.toString('base64')}`,
  };
  await page.addInitScript((payload) => {
    const mockOptions = payload.options;
    const imageDataUrls = payload.images;
    const nativeProjectionByView = Object.fromEntries(payload.nativeProjectionEntries ?? []) as Record<string, { displayEdges: number[][][]; displayCurves?: any[]; hiddenDisplayEdges?: number[][][]; hiddenDisplayCurves?: any[]; warnings?: string[] }>;
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
      livePaperStarts: [] as Array<Record<string, unknown>>,
      livePaperViewChanges: [] as Array<Record<string, unknown>>,
      resolveLivePaperProfile: null as (() => void) | null,
    };
    const makeLivePaper = (scopeKey: string, view: string) => ({ sessionId: 'live-test', scopeKey, view, trustUrl: null, phoneUrl: 'https://127.0.0.1:4444/live-paper?session=live-test&token=test', phoneQrDataUrl: imageDataUrls.cameraQr, templateDataUrl: 'data:application/pdf;base64,JVBERi0xLjQK', templateUrl: 'https://127.0.0.1:4444/api/live-paper/live-test/template.pdf?token=test', state: 'waiting', detail: 'Open the camera link on your phone.', sequence: 0, widthMm: 160, heightMm: 220, previewDataUrl: null, cameraPreviewDataUrl: null, previewKind: 'camera', accepted: [], pending: [], diagnostics: [] });
    let livePaper: any = mockOptions.restoreForeignLivePaper ? { ...makeLivePaper('other-project', 'top'), accepted: [{kind: 'line', id: 'foreign-line', startMm: [0, 0], endMm: [20, 0], residualMm: 0.1}] } : null;
    const livePaperViews = new Map<string, any>();
    const pendingDerivationRejectors: Array<(error: Error) => void> = [];
    mock.rejectPendingDerivation = () => {
      for (const reject of pendingDerivationRejectors.splice(0)) reject(new Error('Mock profile derivation failed.'));
    };
    (window as any).__PROFILE_AUTHORING__ = mock;
    (window as any).__TAURI_INTERNALS__ = (window as any).__TAURI_INTERNALS__ || {};
    (window as any).__TAURI_INTERNALS__.convertFileSrc = (path: string) => path;
    const eventHandlers = new Map<string, number[]>();
    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (event: string, id: number) => {
        eventHandlers.set(event, (eventHandlers.get(event) ?? []).filter((handler) => handler !== id));
      },
    };
    let nextCallbackId = 1;
    (window as any).__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
      const callbackId = nextCallbackId++;
      (window as any)[`_${callbackId}`] = callback;
      return callbackId;
    };

    (mock as any).emitLivePaper = (patch: Record<string, unknown>) => {
      livePaper = { ...livePaper, ...patch };
      for (const id of eventHandlers.get('live-paper-state') ?? []) {
        (window as any)[`_${id}`]?.({ event: 'live-paper-state', id, payload: livePaper });
      }
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
      if (command === 'get_live_paper') return livePaper;
      if (command === 'start_live_paper') {
        mock.livePaperStarts.push(args ?? {});
        livePaper = makeLivePaper(String(args?.scopeKey ?? ''), String(args?.view ?? 'front'));
        return livePaper;
      }
      if (command === 'set_live_paper_view') {
        mock.livePaperViewChanges.push(args ?? {});
        livePaperViews.set(livePaper.view, { accepted: livePaper.accepted, pending: livePaper.pending });
        const target = livePaperViews.get(args?.view) ?? { accepted: [], pending: [] };
        livePaper = { ...livePaper, ...target, view: args?.view, previewKind: 'camera', previewDataUrl: null,
          state: 'templateMismatch', detail: `Show the ${String(args?.view).toUpperCase()} template. Geometry updates paused.`,
          templateDataUrl: `data:application/pdf;base64,${btoa('%PDF-1.4\n' + String(args?.view).toUpperCase())}` };
        return livePaper;
      }
      if (command === 'stop_live_paper') return livePaper = { ...livePaper, state: 'stopped', detail: 'Capture stopped. Accepted geometry retained.' };
      if (command === 'confirm_live_paper') {
        const primitive = livePaper.pending.find((item: any) => item.id === args?.primitiveId);
        livePaper = { ...livePaper, accepted: [...livePaper.accepted, primitive], pending: livePaper.pending.filter((item: any) => item.id !== args?.primitiveId) };
        return livePaper;
      }
      if (command === 'reject_live_paper') {
        livePaper = { ...livePaper, accepted: livePaper.accepted.filter((item: any) => item.id !== args?.primitiveId), pending: livePaper.pending.filter((item: any) => item.id !== args?.primitiveId) };
        return livePaper;
      }
      if (command === 'edit_live_paper') {
        const edited = args?.primitive;
        livePaper = { ...livePaper, accepted: livePaper.accepted.map((item: any) => item.id === edited?.id ? edited : item), pending: livePaper.pending.map((item: any) => item.id === edited?.id ? edited : item) };
        return livePaper;
      }
      if (command === 'prepare_live_paper_profile') {
        const circle = livePaper.accepted.find((item: any) => item.kind === 'circle');
        const outline = [[10, 10], [90, 10], [90, 60], [10, 60]];
        const profile = {
          view: mockOptions.livePaperProfileView ?? livePaper.view,
          loops: [
            { role: 'outer', parentIndex: null, points: outline },
            { role: 'hole', parentIndex: 0, points: [] },
          ],
          sketchLoops: [
            { role: 'outer', parentIndex: null, geometry: { kind: 'polygon', pointsMm: outline } },
            { role: 'hole', parentIndex: 0, geometry: { kind: 'circle', centerMm: circle.centerMm, radiusMm: circle.radiusMm } },
          ], diagnostics: [],
        };
        if (mockOptions.holdLivePaperProfile) {
          return new Promise((resolve) => { mock.resolveLivePaperProfile = () => resolve(profile); });
        }
        return profile;
      }
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
          history: mockOptions.missingProject ? [] : [{ ...thread, versionCount: messages.length }],
          workspace: mockOptions.missingProject ? null : workspaceProjection('profile-version-1'),
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
          hiddenDisplayEdges: request.visibility === 'allEdges' ? (nativeProjection?.hiddenDisplayEdges ?? [[[-8, -4], [8, -4]]]) : [],
          hiddenDisplayCurves: request.visibility === 'allEdges' ? (nativeProjection?.hiddenDisplayCurves ?? [{ kind: 'circle', centerMm: [0, 0], radiusMm: 7 }]) : [],
          displayCurves: nativeProjection?.displayCurves ?? (mockOptions.analyticProjectionCurves ? [
              { kind: 'line', startMm: [-20, 0], endMm: [20, 0] },
              { kind: 'circle', centerMm: [0, 0], radiusMm: 5 },
              { kind: 'circularArc', centerMm: [8, 0], radiusMm: 3, startAngleRad: 0, sweepAngleRad: Math.PI * 1.5 },
              { kind: 'ellipse', centerMm: [0, 0], majorRadiusMm: 10, minorRadiusMm: 5, rotationRad: 0.25 },
              { kind: 'ellipticalArc', centerMm: [0, 0], majorRadiusMm: 8, minorRadiusMm: 4, rotationRad: 0.5, startAngleRad: 0.2, sweepAngleRad: -1.1 },
            ] : []),
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
  const draftPolygon = canvas.locator('.selected-loop');
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

