import { expect, test, type Locator, type Page } from '@playwright/test';

type SketchMockMode = 'ok' | 'error' | 'delay' | 'accept-error';
type SketchSuggestionMockMode = 'ok' | 'error';
type SketchBrepCandidateMockMode = 'ok' | 'error';
type SketchBrepCandidateAcceptMockMode = 'ok' | 'error' | 'edge-targets';
type SketchBrepPackageMockMode = 'ok' | 'error';
type BrepHiddenLineMockMode =
  | 'unavailable'
  | 'ok'
  | 'step-ok'
  | 'warning-entry-top-no-edges'
  | 'multi-loop-ok'
  | 'multi-loop-edges-topology-mismatch-then-ok'
  | 'multi-loop-edges-topology-churn-then-ok'
  | 'error'
  | 'bounds-mismatch'
  | 'bounds-kind-then-ok'
  | 'bounds-hole-topology-then-ok'
  | 'bounds-mismatch-then-ok'
  | 'bounds-mismatch-no-primitive-then-ok'
  | 'bounds-mismatch-repeat'
  | 'bounds-mismatch-unsupported'
  | 'containment-expand-then-ok'
  | 'containment-kind-then-ok'
  | 'containment-expand-repeat'
  | 'containment-mismatch'
  | 'containment-view-edge-topology'
  | 'containment-view-edge-topology-then-ok'
  | 'containment-plus-topology-then-topology'
  | 'containment-duplicate-topology-exact-front'
  | 'containment-topology-no-view-neutral'
  | 'containment-topology-stale-view-neutral'
  | 'containment-topology-generic-sketch-neutral'
  | 'view-only-stale-top-no-locator'
  | 'topology-mismatch'
  | 'topology-kind-then-ok'
  | 'topology-mismatch-then-ok'
  | 'concavity-mismatch'
  | 'concavity-kind-then-ok'
  | 'concavity-mismatch-then-ok';
type SketchPointTuple = [number, number];

const sketchPreviewPath = '/mock/sketch/generated/draft/session/with/long/internal/cache/path/preview.stl';
const sketchSource = '(solid sketch-seed (extrude 12))';
const sketchSuggestionReason = 'Closed Front profile detected; deterministic extrusion depth from sketch profile.';

async function installSketchMocks(
  page: Page,
  mode: SketchMockMode,
  suggestionMode: SketchSuggestionMockMode = 'ok',
  source: string = sketchSource,
  candidateMode: SketchBrepCandidateMockMode = 'ok',
  hiddenLineMode: BrepHiddenLineMockMode = 'unavailable',
  candidateAcceptMode: SketchBrepCandidateAcceptMockMode = 'ok',
  brepPackageMode: SketchBrepPackageMockMode = 'ok',
) {
  await page.addInitScript(({ mockMode, mockSource, mockPreviewPath, mockSuggestionMode, mockSuggestionReason, mockCandidateMode, mockHiddenLineMode, mockCandidateAcceptMode, mockBrepPackageMode }) => {
    const mockWindow = window as any;
    mockWindow.__SKETCH_DRAFT_CALLS__ = [];
    mockWindow.__SKETCH_PREVIEW_HULL_CALLS__ = [];
    mockWindow.__SKETCH_SUGGESTION_CALLS__ = [];
    mockWindow.__SKETCH_BREP_CANDIDATE_CALLS__ = [];
    mockWindow.__SKETCH_BREP_ACCEPT_CALLS__ = [];
    mockWindow.__SKETCH_BREP_PACKAGE_CALLS__ = [];
    mockWindow.__BREP_HIDDEN_LINE_CALLS__ = [];

    const draftStorageKey = '__SKETCH_PREVIEW_DRAFTS__';
    const readDraftStore = () => {
      try {
        return JSON.parse(localStorage.getItem(draftStorageKey) ?? '{}') as Record<string, any>;
      } catch {
        return {};
      }
    };
    const writeDraftStore = (store: Record<string, any>) => {
      localStorage.setItem(draftStorageKey, JSON.stringify(store));
    };
    const draftScopeKey = (scopeId: string | null | undefined) => {
      const normalized = `${scopeId ?? 'global'}`.trim();
      return normalized ? normalized : 'global';
    };

    const previewHullSourceForRequest = (request: any) => {
      if (mockHiddenLineMode !== 'multi-loop-ok' && mockHiddenLineMode !== 'multi-loop-edges-topology-mismatch-then-ok') {
        return `; preview-hull-source\n${mockSource}`;
      }

      const frontSketch = request?.document?.sketches?.find((sketch: any) => sketch?.view === 'front');
      const closedPolylines = (frontSketch?.primitives ?? []).filter(
        (primitive: any) => primitive?.kind === 'polyline' && primitive?.closed && Array.isArray(primitive?.points) && primitive.points.length >= 4,
      );
      if (closedPolylines.length < 2) {
        return `; preview-hull-source\n${mockSource}`;
      }

      const polygonSource = (primitive: any) =>
        `(polygon (${primitive.points.map((point: any) => `(${point[0]} ${point[1]})`).join(' ')}))`;
      const sortedPolylines = [...closedPolylines].sort((left: any, right: any) => {
        const roleRank = (primitive: any) =>
          primitive?.topology?.loopRole === 'outer' ? 0 : primitive?.topology?.loopRole === 'hole' ? 1 : 2;
        return roleRank(left) - roleRank(right);
      });
      const [outer, ...holes] = sortedPolylines;
      return `; preview-hull-source\n(profile :outer ${polygonSource(outer)} :holes (${holes.map(polygonSource).join(' ')}))`;
    };

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
      if (cmd === 'get_history') return [];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_default_macro') return '';
      if (cmd === 'save_sketch_preview_draft') {
        const request = args?.request ?? {};
        const scopeId = draftScopeKey(request.scopeId);
        const draft = {
          scopeId: request.scopeId ?? null,
          draftSource: request.draftSource,
          artifactBundle: request.artifactBundle,
          updatedAt: Math.floor(Date.now() / 1000),
        };
        const store = readDraftStore();
        store[scopeId] = draft;
        store.global = draft;
        writeDraftStore(store);
        return draft;
      }
      if (cmd === 'load_sketch_preview_draft') {
        const request = args?.request ?? {};
        const scopeId = draftScopeKey(request.scopeId);
        const store = readDraftStore();
        return store[scopeId] ?? store.global ?? null;
      }
      if (cmd === 'clear_sketch_preview_draft') {
        const request = args?.request ?? {};
        const scopeId = draftScopeKey(request.scopeId);
        const store = readDraftStore();
        delete store[scopeId];
        delete store.global;
        writeDraftStore(store);
        return null;
      }
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
      if (cmd === 'generate_sketch_draft_preview') {
        mockWindow.__SKETCH_DRAFT_CALLS__.push(args?.request ?? null);
        if (mockMode === 'delay') {
          await new Promise((resolve) => setTimeout(resolve, 800));
        }
        if (mockMode === 'accept-error' && mockWindow.__SKETCH_DRAFT_CALLS__.length > 1) {
          throw {
            code: 'accepted_suggestion_preview_failed',
            message: 'accepted suggestion preview failed',
            details: 'raw sketch backend body: deterministic accepted extrude unavailable',
          };
        }
        if (mockMode === 'error') {
          throw {
            code: 'sketch_draft_failed',
            message: 'draft generation failed',
            details: 'raw sketch backend body: missing closed profile',
          };
        }
        return [
          {
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
            macroDialect: 'ecky',
            source: mockSource,
            warnings: ['draft from seed geometry'],
          },
          {
            modelId: 'sketch-preview',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
            contentHash: 'sketch-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/sketch/manifest.json',
            macroPath: '/mock/sketch/source.ecky',
            previewStlPath: mockPreviewPath,
            viewerAssets: [
              {
                partId: 'sketch-seed-part',
                nodeId: 'sketch-seed-part',
                objectName: 'sketch-seed-part',
                label: 'Sketch Seed Part',
                path: '/mock/sketch/part.stl',
                format: 'stl',
              },
            ],
          },
        ];
      }
      if (cmd === 'generate_sketch_preview_hull') {
        const request = args?.request ?? null;
        mockWindow.__SKETCH_PREVIEW_HULL_CALLS__.push(request);
        const viewLabel = request?.document?.sketches?.map((sketch: any) => sketch.view).join('/') || 'front/top/side';
        if (mockMode === 'delay') {
          await new Promise((resolve) => setTimeout(resolve, 800));
        }
        if (mockMode === 'error') {
          throw {
            code: 'sketch_preview_hull_failed',
            message: 'preview hull generation failed',
            details: 'raw sketch backend body: candidate cell search failed',
          };
        }
        return [
          {
            sourceLanguage: 'ecky',
            geometryBackend: mockHiddenLineMode === 'unavailable' || mockHiddenLineMode === 'step-ok' ? 'mesh' : 'freecad',
            macroDialect: 'ecky',
            source: previewHullSourceForRequest(request),
            warnings: [`preview hull from ${viewLabel} candidate cell search; not accepted BRep`],
          },
          {
            modelId: 'sketch-preview-hull',
            sourceKind: 'generated',
            engineKind: mockHiddenLineMode === 'unavailable' || mockHiddenLineMode === 'step-ok' ? 'ecky' : 'freecad',
            sourceLanguage: 'ecky',
            geometryBackend: mockHiddenLineMode === 'unavailable' || mockHiddenLineMode === 'step-ok' ? 'mesh' : 'freecad',
            contentHash: 'sketch-hull-hash',
            artifactVersion: 1,
            fcstdPath: mockHiddenLineMode === 'unavailable' || mockHiddenLineMode === 'step-ok' ? '' : '/mock/sketch/model.FCStd',
            manifestPath: '/mock/sketch/hull-manifest.json',
            macroPath: '/mock/sketch/hull-source.ecky',
            previewStlPath: mockPreviewPath,
            viewerAssets: [
              {
                partId: 'sketch-preview-hull',
                nodeId: 'sketch-preview-hull',
                objectName: 'sketch-preview-hull',
                label: 'Sketch Preview Hull',
                path: '/mock/sketch/hull-part.stl',
                format: 'stl',
              },
            ],
            exportArtifacts:
              mockHiddenLineMode === 'step-ok'
                ? [{ label: 'STEP', format: 'step', path: '/mock/sketch/model.step', role: 'primary' }]
                : [],
          },
        ];
      }
      if (cmd === 'extract_brep_hidden_line_projections') {
        mockWindow.__BREP_HIDDEN_LINE_CALLS__.push(args?.request ?? null);
        if (mockHiddenLineMode === 'error') {
          throw {
            code: 'render',
            message: 'FreeCAD runner failed.',
            details: 'raw hidden-line backend body: Drawing.projectEx failed on final BRep',
          };
        }
        if (mockHiddenLineMode === 'unavailable') {
          throw {
            code: 'validation',
            message: 'Exact BRep hidden-line requires a FreeCAD/OCCT FCStd or STEP artifact.',
            details: 'geometryBackend=mesh; fcstdPath=; stepPath=',
          };
        }
        const hiddenLineCallCount = mockWindow.__BREP_HIDDEN_LINE_CALLS__.length;
        const shouldReturnBoundsMismatch =
          mockHiddenLineMode === 'bounds-mismatch' ||
          (mockHiddenLineMode === 'bounds-kind-then-ok' && hiddenLineCallCount === 1) ||
          mockHiddenLineMode === 'bounds-mismatch-repeat' ||
          mockHiddenLineMode === 'bounds-mismatch-unsupported' ||
          (mockHiddenLineMode === 'bounds-mismatch-no-primitive-then-ok' && hiddenLineCallCount === 1) ||
          (mockHiddenLineMode === 'bounds-mismatch-then-ok' && hiddenLineCallCount === 1);
        if (shouldReturnBoundsMismatch) {
          const primitiveId =
            mockHiddenLineMode === 'bounds-mismatch-unsupported'
              ? 'primitive-front-not-in-document'
              : mockHiddenLineMode === 'bounds-mismatch-no-primitive-then-ok'
                ? null
              : 'primitive-front-hidden-line-mismatch';
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[80, 0], [80, 40]], sourceClass: 'V1' },
                ],
                hiddenEdges: [
                  { edgeId: 'front-h0', points: [[0, 40], [80, 40]], sourceClass: 'H' },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-v0', points: [[10, 5], [60, 5]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-front',
                  primitiveId,
                  kind: 'boundsMismatch',
                  view: 'front',
                  severity: 'error',
                  message:
                    mockHiddenLineMode === 'bounds-kind-then-ok'
                      ? 'projection envelope deviates from source profile'
                      : 'raw BREP/SKETCH bounds mismatch: front sketch bounds x=10..60 y=20..50; OCCT bounds x=0..80 y=0..40',
                },
              ],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'bounds-hole-topology-then-ok' && hiddenLineCallCount === 1) {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'outer-a', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'outer-b', points: [[80, 0], [80, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-c', points: [[80, 50], [0, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-d', points: [[0, 50], [0, 0]], sourceClass: 'V' },
                  { edgeId: 'inner-a', points: [[25, 18], [46, 18]], sourceClass: 'V' },
                  { edgeId: 'inner-b', points: [[46, 18], [46, 35]], sourceClass: 'V' },
                  { edgeId: 'inner-c', points: [[46, 35], [25, 35]], sourceClass: 'V' },
                  { edgeId: 'inner-d', points: [[25, 35], [25, 18]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
                loops: [
                  {
                    loopId: 'front-outer',
                    edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
                    points: [[0, 0], [80, 0], [80, 50], [0, 50], [0, 0]],
                    role: 'outer',
                    sourceClass: 'V',
                  },
                  {
                    loopId: 'front-hole',
                    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                    points: [[25, 18], [46, 18], [46, 35], [25, 35], [25, 18]],
                    role: 'hole',
                    sourceClass: 'V',
                  },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [{ edgeId: 'top-a', points: [[0, 0], [80, 20]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [{ edgeId: 'side-a', points: [[0, 0], [20, 50]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-alpha',
                  primitiveId: 'primitive-front-outer',
                  kind: 'boundsMismatch',
                  view: 'front',
                  severity: 'error',
                  message: 'projection envelope deviates from source profile',
                  topology: {
                    loopId: 'front-hole',
                    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                    loopRole: 'hole',
                    sourceClass: 'derived',
                  },
                },
              ],
              evidence: [],
            },
          };
        }
        if (
          mockHiddenLineMode === 'containment-expand-repeat' ||
          (mockHiddenLineMode === 'containment-kind-then-ok' && hiddenLineCallCount === 1) ||
          (mockHiddenLineMode === 'containment-expand-then-ok' && hiddenLineCallCount === 1)
        ) {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[10, 20], [64.2, 20]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[64.2, 20], [64.2, 50]], sourceClass: 'V1' },
                ],
                hiddenEdges: [
                  { edgeId: 'front-h0', points: [[10, 50], [64.2, 50]], sourceClass: 'H' },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-front',
                  primitiveId: 'primitive-front-hidden-line-containment',
                  kind: 'containmentMismatch',
                  view: 'front',
                  edgeId: 'front-v1',
                  severity: 'error',
                  message:
                    mockHiddenLineMode === 'containment-kind-then-ok'
                      ? 'projection exits source profile, maxOutside=4.2mm'
                      : 'raw BREP/SKETCH containment mismatch: front edge front-v1 has 8 samples outside source profile, maxOutside=4.2mm',
                },
              ],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'containment-mismatch') {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[10, 20], [60, 20]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[60, 20], [60, 50]], sourceClass: 'V1' },
                ],
                hiddenEdges: [
                  { edgeId: 'front-h0', points: [[10, 50], [60, 50]], sourceClass: 'H' },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-front',
                  view: 'front',
                  primitiveId: 'primitive-front-hidden-line-containment',
                  kind: 'containmentMismatch',
                  severity: 'error',
                  message:
                    'raw BREP/SKETCH containment mismatch: front edge front-v1 has 8 samples outside source profile, maxOutside=4.2mm',
                },
              ],
              evidence: [],
            },
          };
        }
        if (
          mockHiddenLineMode === 'containment-view-edge-topology' ||
          mockHiddenLineMode === 'containment-topology-no-view-neutral' ||
          mockHiddenLineMode === 'containment-topology-stale-view-neutral' ||
          mockHiddenLineMode === 'containment-topology-generic-sketch-neutral' ||
          mockHiddenLineMode === 'containment-duplicate-topology-exact-front' ||
          (mockHiddenLineMode === 'containment-plus-topology-then-topology' && hiddenLineCallCount === 1) ||
          (mockHiddenLineMode === 'containment-view-edge-topology-then-ok' && hiddenLineCallCount === 1)
        ) {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[46, 17], [46, 35]], sourceClass: 'V1' },
                ],
                hiddenEdges: [
                  { edgeId: 'front-h0', points: [[0, 50], [80, 50]], sourceClass: 'H' },
                ],
                ...(mockHiddenLineMode === 'containment-view-edge-topology-then-ok'
                  ? {
                      loops: [
                        {
                          loopId: 'front-outer',
                          edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
                          points: [[0, 0], [80, 0], [80, 50], [0, 50], [0, 0]],
                          role: 'outer',
                          sourceClass: 'derived',
                        },
                        {
                          loopId: 'front-hole',
                          edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                          points: [[25, 18], [46, 18], [46, 35], [25, 35], [25, 18]],
                          role: 'hole',
                          sourceClass: 'derived',
                        },
                      ],
                    }
                  : {}),
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId:
                    mockHiddenLineMode === 'containment-topology-generic-sketch-neutral'
                      ? 'model'
                      : mockHiddenLineMode === 'containment-duplicate-topology-exact-front'
                        ? 'sketch-front-exact'
                        : 'sketch-alpha',
                  kind: 'containmentMismatch',
                  view: mockHiddenLineMode === 'containment-topology-stale-view-neutral' ? 'top' : 'front',
                  primitiveId:
                    mockHiddenLineMode === 'containment-duplicate-topology-exact-front'
                      ? 'primitive-front-hole'
                      : mockHiddenLineMode === 'containment-topology-no-view-neutral' ||
                    mockHiddenLineMode === 'containment-topology-stale-view-neutral' ||
                    mockHiddenLineMode === 'containment-topology-generic-sketch-neutral'
                      ? 'primitive-outer'
                      : 'primitive-front-outer',
                  edgeId: 'front-v1',
                  severity: 'error',
                  message:
                    mockHiddenLineMode === 'containment-topology-no-view-neutral' ||
                    mockHiddenLineMode === 'containment-topology-stale-view-neutral' ||
                    mockHiddenLineMode === 'containment-topology-generic-sketch-neutral'
                      ? 'projection exits source profile'
                      : 'raw BREP/SKETCH containment mismatch: projection exits source profile, maxOutside=4.2mm',
                  topology: {
                    loopId:
                      mockHiddenLineMode === 'containment-duplicate-topology-exact-front'
                        ? 'shared-hole'
                        : 
                      mockHiddenLineMode === 'containment-topology-no-view-neutral' ||
                      mockHiddenLineMode === 'containment-topology-stale-view-neutral' ||
                      mockHiddenLineMode === 'containment-topology-generic-sketch-neutral'
                        ? 'hole-alpha'
                        : 'front-hole',
                    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                    loopRole: 'hole',
                    sourceClass: 'derived',
                  },
                },
                ...(mockHiddenLineMode === 'containment-plus-topology-then-topology'
                  ? [
                      {
                        sketchId: 'sketch-alpha',
                        primitiveId: 'primitive-front-hole',
                        kind: 'topologyMismatch' as const,
                        view: 'front' as const,
                        severity: 'error' as const,
                        message: 'closed loop cannot be matched',
                        topology: {
                          loopId: 'front-hole',
                          edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                          loopRole: 'hole' as const,
                          sourceClass: 'derived' as const,
                        },
                      },
                    ]
                  : []),
              ],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'containment-plus-topology-then-topology') {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[46, 18], [46, 35]], sourceClass: 'V1' },
                ],
                hiddenEdges: [{ edgeId: 'front-h0', points: [[0, 50], [80, 50]], sourceClass: 'H' }],
                loops: [
                  {
                    loopId: 'front-outer',
                    edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
                    points: [[0, 0], [80, 0], [80, 50], [0, 50], [0, 0]],
                    role: 'outer',
                    sourceClass: 'derived',
                  },
                  {
                    loopId: 'front-hole',
                    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                    points: [[25, 18], [46, 18], [46, 35], [25, 35], [25, 18]],
                    role: 'hole',
                    sourceClass: 'derived',
                  },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [{ edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [{ edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-alpha',
                  primitiveId: 'primitive-front-hole',
                  kind: 'topologyMismatch',
                  view: 'front',
                  severity: 'error',
                  message: 'closed loop cannot be matched',
                  topology: {
                    loopId: 'front-hole',
                    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                    loopRole: 'hole',
                    sourceClass: 'derived',
                  },
                },
              ],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'containment-view-edge-topology-then-ok') {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[46, 18], [46, 35]], sourceClass: 'V1' },
                ],
                hiddenEdges: [{ edgeId: 'front-h0', points: [[0, 50], [80, 50]], sourceClass: 'H' }],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [{ edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [{ edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
            ],
            validation: { passed: true, issues: [], evidence: [] },
          };
        }
        if (
          mockHiddenLineMode === 'topology-mismatch' ||
          (mockHiddenLineMode === 'topology-kind-then-ok' && hiddenLineCallCount === 1) ||
          (mockHiddenLineMode === 'topology-mismatch-then-ok' && hiddenLineCallCount === 1)
        ) {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[10, 20], [60, 20]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[60, 20], [60, 50]], sourceClass: 'V1' },
                  { edgeId: 'front-v2', points: [[60, 50], [10, 50]], sourceClass: 'V2' },
                  { edgeId: 'front-v3', points: [[10, 50], [10, 20]], sourceClass: 'V3' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-front',
                  primitiveId: 'primitive-front-hidden-line-topology',
                  kind: 'topologyMismatch',
                  view: 'front',
                  severity: 'error',
                  message:
                    mockHiddenLineMode === 'topology-kind-then-ok' || mockHiddenLineMode === 'topology-mismatch-then-ok'
                      ? 'closed loop cannot be matched'
                      : 'raw BREP/SKETCH topology mismatch: front face loop cannot be matched',
                },
              ],
              evidence: [],
            },
          };
        }
        if (
          mockHiddenLineMode === 'concavity-mismatch' ||
          (mockHiddenLineMode === 'concavity-kind-then-ok' && hiddenLineCallCount === 1) ||
          (mockHiddenLineMode === 'concavity-mismatch-then-ok' && hiddenLineCallCount === 1)
        ) {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-c0', points: [[10, 20], [60, 20]], sourceClass: 'V' },
                  { edgeId: 'front-c1', points: [[60, 20], [60, 50]], sourceClass: 'V1' },
                  { edgeId: 'front-c2', points: [[60, 50], [35, 35]], sourceClass: 'V2' },
                  { edgeId: 'front-c3', points: [[35, 35], [10, 50]], sourceClass: 'V3' },
                  { edgeId: 'front-c4', points: [[10, 50], [10, 20]], sourceClass: 'V4' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'sketch-front',
                  primitiveId: 'primitive-front-hidden-line-concavity',
                  kind: 'concavityMismatch',
                  view: 'front',
                  severity: 'error',
                  message:
                    mockHiddenLineMode === 'concavity-kind-then-ok'
                      ? 'visible silhouette has concave notch missing from source profile'
                      : 'raw BREP/SKETCH concavity mismatch: front silhouette has concave notch missing from source profile',
                },
              ],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'multi-loop-ok') {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'outer-a', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'outer-b', points: [[80, 0], [80, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-c', points: [[80, 50], [0, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-d', points: [[0, 50], [0, 0]], sourceClass: 'V' },
                  { edgeId: 'inner-a', points: [[25, 18], [45, 18]], sourceClass: 'V' },
                  { edgeId: 'inner-b', points: [[45, 18], [45, 34]], sourceClass: 'V' },
                  { edgeId: 'inner-c', points: [[45, 34], [25, 34]], sourceClass: 'V' },
                  { edgeId: 'inner-d', points: [[25, 34], [25, 18]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
                loops: [
                  {
                    loopId: 'front-outer',
                    edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
                    points: [
                      [0, 0],
                      [80, 0],
                      [80, 50],
                      [0, 50],
                      [0, 0],
                    ],
                    role: 'outer',
                    sourceClass: 'V',
                  },
                  {
                    loopId: 'front-hole',
                    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                    points: [
                      [25, 18],
                      [45, 18],
                      [45, 34],
                      [25, 34],
                      [25, 18],
                    ],
                    role: 'hole',
                    sourceClass: 'V',
                  },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-a', points: [[0, 0], [80, 20]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-a', points: [[0, 0], [20, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: true,
              issues: [],
              evidence: ['backend BRep/sketch validation: front 8 visible / 0 hidden; top 1 visible / 0 hidden; side 1 visible / 0 hidden'],
            },
          };
        }
        if (mockHiddenLineMode === 'warning-entry-top-no-edges') {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[10, 20], [60, 20]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[60, 20], [60, 50]], sourceClass: 'V1' },
                ],
                hiddenEdges: [
                  { edgeId: 'front-h0', points: [[10, 50], [60, 50]], sourceClass: 'H' },
                ],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            warningEntries: [
              {
                kind: 'projectionNoEdges',
                view: 'top',
                message: 'projection produced no edges.',
              },
            ],
            validation: {
              passed: true,
              issues: [],
              evidence: [
                'backend BRep/sketch validation: front 2 visible / 1 hidden; top 0 visible / 0 hidden; side 1 visible / 0 hidden',
              ],
            },
          };
        }
        if (mockHiddenLineMode === 'view-only-stale-top-no-locator') {
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'front-v0', points: [[10, 20], [60, 20]], sourceClass: 'V' },
                  { edgeId: 'front-v1', points: [[60, 20], [60, 50]], sourceClass: 'V1' },
                ],
                hiddenEdges: [{ edgeId: 'front-h0', points: [[10, 50], [60, 50]], sourceClass: 'H' }],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [{ edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [{ edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: false,
              issues: [
                {
                  sketchId: 'ghost-sketch',
                  view: 'top',
                  primitiveId: 'ghost-primitive',
                  kind: 'containmentMismatch',
                  severity: 'error',
                  message: 'projection lies outside source profile',
                },
              ],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'multi-loop-edges-topology-mismatch-then-ok') {
          const mismatch = hiddenLineCallCount === 1;
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'outer-a', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'outer-b', points: [[80, 0], [80, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-c', points: [[80, 50], [0, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-d', points: [[0, 50], [0, 0]], sourceClass: 'V' },
                  { edgeId: 'inner-a', points: [[24, 17], [46, 17]], sourceClass: 'V' },
                  { edgeId: 'inner-b', points: [[46, 17], [46, 35]], sourceClass: 'V' },
                  { edgeId: 'inner-c', points: [[46, 35], [24, 35]], sourceClass: 'V' },
                  { edgeId: 'inner-d', points: [[24, 35], [24, 17]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [
                  { edgeId: 'top-a', points: [[0, 0], [80, 20]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [
                  { edgeId: 'side-a', points: [[0, 0], [20, 50]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: !mismatch,
              issues: mismatch
                ? [
                    {
                      sketchId: 'sketch-front',
                      view: 'front',
                      kind: 'topologyMismatch',
                      severity: 'error',
                      message: 'raw BREP/SKETCH topology mismatch: front face loop cannot be matched',
                      topology: {
                        loopId: 'front-hole',
                        edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                        loopRole: 'hole',
                        sourceClass: 'derived',
                      },
                    },
                  ]
                : [],
              evidence: [],
            },
          };
        }
        if (mockHiddenLineMode === 'multi-loop-edges-topology-churn-then-ok') {
          const mismatch = hiddenLineCallCount === 1;
          return {
            modelId: 'sketch-preview-hull',
            sourceArtifactPath: '/mock/sketch/model.FCStd',
            views: [
              {
                view: 'front',
                direction: [0, -1, 0],
                visibleEdges: [
                  { edgeId: 'outer-z0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
                  { edgeId: 'outer-z1', points: [[80, 0], [80, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-z2', points: [[80, 50], [0, 50]], sourceClass: 'V' },
                  { edgeId: 'outer-z3', points: [[0, 50], [0, 0]], sourceClass: 'V' },
                  { edgeId: 'hole-a-z0', points: [[12, 12], [22, 12]], sourceClass: 'V' },
                  { edgeId: 'hole-a-z1', points: [[22, 12], [22, 22]], sourceClass: 'V' },
                  { edgeId: 'hole-a-z2', points: [[22, 22], [12, 22]], sourceClass: 'V' },
                  { edgeId: 'hole-a-z3', points: [[12, 22], [12, 12]], sourceClass: 'V' },
                  { edgeId: 'hole-b-z0', points: [[52, 24], [66, 24]], sourceClass: 'V' },
                  { edgeId: 'hole-b-z1', points: [[66, 24], [66, 38]], sourceClass: 'V' },
                  { edgeId: 'hole-b-z2', points: [[66, 38], [52, 38]], sourceClass: 'V' },
                  { edgeId: 'hole-b-z3', points: [[52, 38], [52, 24]], sourceClass: 'V' },
                ],
                hiddenEdges: [],
              },
              {
                view: 'top',
                direction: [0, 0, -1],
                visibleEdges: [{ edgeId: 'top-a', points: [[0, 0], [80, 20]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
              {
                view: 'side',
                direction: [-1, 0, 0],
                visibleEdges: [{ edgeId: 'side-a', points: [[0, 0], [20, 50]], sourceClass: 'V' }],
                hiddenEdges: [],
              },
            ],
            validation: {
              passed: !mismatch,
              issues: mismatch
                ? [
                    {
                      sketchId: 'sketch-front',
                      view: 'front',
                      kind: 'topologyMismatch',
                      severity: 'error',
                      message: 'closed loop cannot be matched',
                      topology: {
                        loopId: 'front-hole-b',
                        edgeIds: ['hole-b-a', 'hole-b-b', 'hole-b-c', 'hole-b-d'],
                        loopRole: 'hole',
                        sourceClass: 'derived',
                      },
                    },
                  ]
                : [],
              evidence: [],
            },
          };
        }
        return {
          modelId: 'sketch-preview-hull',
          sourceArtifactPath: '/mock/sketch/model.FCStd',
          views: [
            {
              view: 'front',
              direction: [0, -1, 0],
              visibleEdges: [
                { edgeId: 'front-v0', points: [[10, 20], [60, 20]], sourceClass: 'V' },
                { edgeId: 'front-v1', points: [[60, 20], [60, 50]], sourceClass: 'V1' },
              ],
              hiddenEdges: [
                { edgeId: 'front-h0', points: [[10, 50], [60, 50]], sourceClass: 'H' },
              ],
            },
            {
              view: 'top',
              direction: [0, 0, -1],
              visibleEdges: [
                { edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' },
              ],
              hiddenEdges: [],
            },
            {
              view: 'side',
              direction: [-1, 0, 0],
              visibleEdges: [
                { edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' },
              ],
              hiddenEdges: [],
            },
          ],
          validation: {
            passed: true,
            issues: [],
            evidence: [
              'backend BRep/sketch validation: front 2 visible / 1 hidden; top 1 visible / 0 hidden; side 1 visible / 0 hidden',
            ],
          },
        };
      }
      if (cmd === 'suggest_sketch_features') {
        mockWindow.__SKETCH_SUGGESTION_CALLS__.push(args ?? null);
        if (mockSuggestionMode === 'error') {
          throw {
            code: 'sketch_suggestion_failed',
            message: 'suggestion failed',
            details: 'raw suggestion backend body: deterministic feature service unavailable',
          };
        }
        return {
          suggestions: [
            {
              suggestionId: 'suggest-extrude-12mm',
              sketchId: 'sketch-front',
              primitiveId: 'primitive-front-1',
              partId: 'sketch-draft-part',
              operation: 'extrude',
              amount: 12,
              symmetric: false,
              confidence: 0.93,
              reason: mockSuggestionReason,
              warnings: [],
            },
          ],
          warnings: [],
        };
      }
      if (cmd === 'analyze_sketch_brep_candidates') {
        mockWindow.__SKETCH_BREP_CANDIDATE_CALLS__.push(args?.request ?? null);
        if (mockCandidateMode === 'error') {
          throw {
            code: 'sketch_brep_candidate_failed',
            message: 'candidate graph failed',
            details: 'raw candidate backend body: projection edge mismatch',
          };
        }
        return {
          graph: {
            vertices: Array.from({ length: 8 }, (_, index) => ({
              vertexId: `v${index}`,
              point: [index % 2 ? 60 : 10, index & 2 ? 50 : 20, index & 4 ? 27 : 5],
              evidenceViews: ['front', 'top', 'side'],
            })),
            edges: Array.from({ length: 12 }, (_, index) => ({
              edgeId: `e${index}`,
              a: `v${index % 8}`,
              b: `v${(index + 1) % 8}`,
              supportViews: ['front', 'top'],
            })),
          },
          search: {
            cells: [
              {
                cellId: 'cell0',
                min: [10, 20, 5],
                max: [60, 50, 27],
                supportViews: ['front', 'top', 'side'],
              },
            ],
            rejectedCellCount: 0,
            solutions: [
              {
                solutionId: 'solution0',
                cellIds: ['cell0'],
                score: 1,
                evidence: ['searched 1/1 candidate cells', 'selected 1 silhouette-consistent cell'],
              },
            ],
            evidence: ['searched 1/1 candidate cells', 'selected 1 silhouette-consistent cell'],
          },
          validation: {
            passed: true,
            issues: [],
            evidence: ['front 4/4 edges covered', 'top 4/4 edges covered', 'side 4/4 edges covered'],
          },
        };
      }
      if (cmd === 'accept_sketch_brep_candidate_solution') {
        const request = args?.request ?? null;
        mockWindow.__SKETCH_BREP_ACCEPT_CALLS__.push(request);
        if (mockCandidateAcceptMode === 'error') {
          throw {
            code: 'validation',
            message: 'Accepted BRep candidate requires a STEP export artifact; mesh preview fallback is not CAD acceptance.',
            details: 'geometryBackend=mesh; fcstdPath=; stepPath=',
          };
        }
        const candidateResponse = {
          graph: {
            vertices: Array.from({ length: 8 }, (_: unknown, index: number) => ({
              vertexId: `v${index}`,
              point: [index % 2 ? 60 : 10, index & 2 ? 50 : 20, index & 4 ? 27 : 5],
              evidenceViews: ['front', 'top', 'side'],
            })),
            edges: Array.from({ length: 12 }, (_: unknown, index: number) => ({
              edgeId: `e${index}`,
              a: `v${index % 8}`,
              b: `v${(index + 1) % 8}`,
              supportViews: ['front', 'top'],
            })),
          },
          search: {
            cells: [{ cellId: 'cell0', min: [10, 20, 5], max: [60, 50, 27], supportViews: ['front', 'top', 'side'] }],
            rejectedCellCount: 0,
            solutions: [{ solutionId: 'solution0', cellIds: ['cell0'], score: 1, evidence: ['selected 1 silhouette-consistent cell'] }],
            evidence: ['searched 1/1 candidate cells', 'selected 1 silhouette-consistent cell'],
          },
          validation: {
            passed: true,
            issues: [],
            evidence: ['front 4/4 edges covered', 'top 4/4 edges covered', 'side 4/4 edges covered'],
          },
        };
        return {
          draftSource: {
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
            macroDialect: 'ecky',
            source: `; accepted candidate\n${mockSource}`,
            warnings: [],
          },
          artifactBundle: {
            modelId: 'accepted-brep-candidate',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
            contentHash: 'accepted-brep-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/sketch/accepted-manifest.json',
            macroPath: '/mock/sketch/accepted-source.ecky',
            previewStlPath: mockPreviewPath,
            viewerAssets: [],
            edgeTargets:
              mockCandidateAcceptMode === 'edge-targets'
                ? Array.from({ length: 12 }, (_: unknown, index: number) => ({
                    targetId: `accepted-edge-${index}`,
                    durableTargetId: `accepted-body:node:42:edge:${index}`,
                    partId: 'sketch-preview-hull',
                    viewerNodeId: 'sketch-preview-hull',
                    label: `Accepted BRep edge ${index + 1}`,
                    editable: false,
                    start: { x: index, y: 0, z: 0 },
                    end: { x: index, y: 1, z: 0 },
                  }))
                : [],
            exportArtifacts: [{ label: 'STEP', format: 'step', path: '/mock/sketch/accepted.step', role: 'primary' }],
          },
          hiddenLineResponse: {
            modelId: 'accepted-brep-candidate',
            sourceArtifactPath: '/mock/sketch/accepted.step',
            views: [
              { view: 'front', direction: [0, -1, 0], visibleEdges: [{ edgeId: 'front-v0', points: [[10, 20], [60, 20]], sourceClass: 'V' }], hiddenEdges: [] },
              { view: 'top', direction: [0, 0, -1], visibleEdges: [{ edgeId: 'top-v0', points: [[10, 5], [60, 27]], sourceClass: 'V' }], hiddenEdges: [] },
              { view: 'side', direction: [-1, 0, 0], visibleEdges: [{ edgeId: 'side-v0', points: [[5, 20], [27, 50]], sourceClass: 'V' }], hiddenEdges: [] },
            ],
            validation: {
              passed: true,
              issues: [],
              evidence: ['backend BRep/sketch validation: accepted candidate STEP artifact passed 3 views'],
            },
          },
          candidateResponse,
          acceptedSolution: { solutionId: request?.solutionId ?? 'solution0', cellIds: ['cell0'], score: 1, evidence: ['selected 1 silhouette-consistent cell'] },
          evidence: ['accepted BRep candidate solution solution0 with 1 cell', 'STEP export artifact: /mock/sketch/accepted.step'],
        };
      }
      if (cmd === 'accepted_brep_candidate_to_component_package') {
        const request = args?.request ?? null;
        mockWindow.__SKETCH_BREP_PACKAGE_CALLS__.push(request);
        if (mockBrepPackageMode === 'error') {
          throw {
            code: 'validation',
            message: 'Accepted BRep component port failed validation.',
            details: 'raw package backend body: port front_mount references unknown type',
          };
        }
        return {
          schemaVersion: 1,
          packageId: request?.packageId ?? 'sketch-preview-hull.accepted-brep',
          version: request?.version ?? '0.1.0',
          displayName: request?.displayName ?? 'Accepted BRep Candidate',
          visibility: 'source',
          tags: request?.tags ?? ['accepted-brep'],
          portTypes: request?.portTypes ?? [],
          mateTypes: [],
          components: [
            {
              componentId: request?.componentId ?? 'sketch-preview-hull',
              version: request?.componentVersion ?? '0.1.0',
              displayName: request?.componentDisplayName ?? 'Sketch Preview Hull',
              sourceRef: request?.sourceRef ?? '/mock/sketch/accepted.step',
              sketches: request?.document?.sketches ?? [],
              keepouts: [],
              fusionZones: [],
              params: [],
              ports: request?.ports ?? [],
            },
          ],
          assemblies: [],
        };
      }
      return null;
    };
  }, {
    mockMode: mode,
    mockSource: source,
    mockPreviewPath: sketchPreviewPath,
    mockSuggestionMode: suggestionMode,
    mockSuggestionReason: sketchSuggestionReason,
    mockCandidateMode: candidateMode,
    mockHiddenLineMode: hiddenLineMode,
    mockCandidateAcceptMode: candidateAcceptMode,
    mockBrepPackageMode: brepPackageMode,
  });
}

function sketchSourceWithEnvelope(document: unknown): string {
  const encoded = Buffer.from(JSON.stringify(document), 'utf8').toString('base64');
  return `; ecky-sketch-document-base64: ${encoded}\n(model (part body (extrude (polygon ((0 0) (10 0) (10 10) (0 10))) 12)))`;
}

function threeViewSketchDocument(prefix: string) {
  return {
    documentId: 'workspace-sketch-document',
    activeSketchId: 'sketch-front',
    units: 'mm',
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: `primitive-front-${prefix}`,
            kind: 'polyline',
            points: [
              [10, 20],
              [60, 20],
              [60, 50],
              [10, 50],
              [10, 20],
            ],
            closed: true,
          },
        ],
      },
      {
        sketchId: 'sketch-top',
        view: 'top',
        primitives: [
          {
            primitiveId: `primitive-top-${prefix}`,
            kind: 'polyline',
            points: [
              [10, 5],
              [60, 5],
              [60, 27],
              [10, 27],
              [10, 5],
            ],
            closed: true,
          },
        ],
      },
      {
        sketchId: 'sketch-side',
        view: 'side',
        primitives: [
          {
            primitiveId: `primitive-side-${prefix}`,
            kind: 'polyline',
            points: [
              [5, 20],
              [27, 20],
              [27, 50],
              [5, 50],
              [5, 20],
            ],
            closed: true,
          },
        ],
      },
    ],
  };
}

async function generateSketchPreview(page: Page, options: { reopenSketch?: boolean } = {}) {
  const { reopenSketch = true } = options;
  await drawClosedRectangle(page);
  await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();
  await expect(mainModelViewport(page)).toBeVisible();
  await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  if (!reopenSketch) {
    return;
  }
  if (reopenSketch) {
    await page.getByRole('button', { name: 'SKETCH', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'SKETCH WORKSPACE' })).toBeVisible();
  }
  await expect(page.getByText('SOURCE STATUS')).toBeVisible();
  await expectWorkspacePreviewEvidence(page);
}

async function openSketchWorkspace(page: Page) {
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'SKETCH', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'SKETCH WORKSPACE' })).toBeVisible();
}

async function selectSketchTool(page: Page, name: RegExp | string) {
  await page.getByRole('button', { name }).click();
}

async function clickFrontPaneRelativePoint(page: Page, point: SketchPointTuple) {
  const clientPoint = await frontSvgClientPoint(page, point);
  await page.mouse.click(clientPoint.x, clientPoint.y);
}

async function drawClosedRectangle(page: Page) {
  const points: SketchPointTuple[] = [
    [25, 35],
    [72, 35],
    [72, 72],
    [25, 72],
  ];

  for (const point of points) {
    await clickFrontPaneRelativePoint(page, point);
  }
  await page.keyboard.press('Enter');
}

async function drawClosedFrontPaneRelativePolyline(page: Page, points: SketchPointTuple[]) {
  expect(points.length).toBeGreaterThan(1);
  for (const point of points.slice(0, -1)) {
    await clickFrontPaneRelativePoint(page, point);
  }
  await page.keyboard.press('Enter');
}

async function drawClosedFrontPolyline(page: Page, points: SketchPointTuple[]) {
  expect(points.length).toBeGreaterThan(1);
  for (const point of points.slice(0, -1)) {
    await clickFrontPaneRelativePoint(page, point);
  }
  await page.keyboard.press('Enter');
}

async function drawOpenStroke(page: Page) {
  await clickFrontPaneRelativePoint(page, [20, 40]);
  await clickFrontPaneRelativePoint(page, [52, 24]);
  await clickFrontPaneRelativePoint(page, [70, 60]);
}

function mainModelViewport(page: Page) {
  return page.locator('.viewport-area');
}

function frontSketchPointHandles(page: Page) {
  return page.locator('[aria-label="Front sketch pane"]').locator('svg circle, [data-sketch-point-handle], [data-point-handle]');
}

async function firstSketchSuggestionDocument(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__SKETCH_SUGGESTION_CALLS__ ?? [];
    const args = calls[0];
    return (
      args?.sketch ??
      args?.sketchDocument ??
      args?.document ??
      args?.request?.sketch ??
      args?.request?.sketchDocument ??
      args?.request?.document ??
      null
    );
  });
}

async function sketchDraftCallCount(page: Page) {
  return page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0);
}

async function sketchPreviewRequestCount(page: Page) {
  return page.evaluate(
    () => ((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0),
  );
}

async function lastSketchDraftRequest(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__SKETCH_DRAFT_CALLS__ ?? [];
    return calls[calls.length - 1] ?? null;
  });
}

async function lastSketchPreviewHullRequest(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__SKETCH_PREVIEW_HULL_CALLS__ ?? [];
    return calls[calls.length - 1] ?? null;
  });
}

async function lastSketchBrepCandidateRequest(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__SKETCH_BREP_CANDIDATE_CALLS__ ?? [];
    return calls[calls.length - 1] ?? null;
  });
}

async function lastSketchBrepAcceptRequest(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__SKETCH_BREP_ACCEPT_CALLS__ ?? [];
    return calls[calls.length - 1] ?? null;
  });
}

async function lastSketchBrepPackageRequest(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__SKETCH_BREP_PACKAGE_CALLS__ ?? [];
    return calls[calls.length - 1] ?? null;
  });
}

async function lastBrepHiddenLineRequest(page: Page) {
  return page.evaluate(() => {
    const calls = (window as any).__BREP_HIDDEN_LINE_CALLS__ ?? [];
    return calls[calls.length - 1] ?? null;
  });
}

async function brepHiddenLineCallCount(page: Page) {
  return page.evaluate(() => ((window as any).__BREP_HIDDEN_LINE_CALLS__ ?? []).length);
}

async function extrude12SuggestionPanel(page: Page) {
  const panel = page.getByLabel('Suggested features').filter({ hasText: 'EXTRUDE 12MM' }).first();

  await expect(panel).toBeVisible();
  return panel;
}

function validationLedgerPanel(page: Page) {
  return page
    .locator('section, aside, article, details, div')
    .filter({ has: page.getByText('VALIDATION LEDGER', { exact: true }) })
    .first();
}

function validationLedgerRows(ledger: Locator) {
  return ledger.locator('tr, li, [role="row"], [data-validation-ledger-row], .validation-ledger-row, .validation-row');
}

function validationLedgerRow(ledger: Locator, label: string) {
  return validationLedgerRows(ledger)
    .filter({ hasText: new RegExp(`\\b${label}\\b`) })
    .first();
}

function occtHiddenLinePanel(page: Page) {
  return page.getByLabel('OCCT hidden-line projection');
}

function brepTopologyRepairPanel(page: Page) {
  return page.getByLabel('BRep topology repair proposals');
}

function draftModePanel(page: Page) {
  return page.getByLabel('Draft mode');
}

function brepCandidatePanel(page: Page) {
  return page.getByLabel('BRep candidate graph');
}

async function expectValidationLedgerPassRow(ledger: Locator, label: string) {
  const row = validationLedgerRow(ledger, label);

  await expect(row, `${label} needs visible validation ledger row`).toBeVisible();
  await expect(row).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
}

async function expectValidationLedgerNoPassRow(ledger: Locator, label: string) {
  const passRow = validationLedgerRows(ledger)
    .filter({ hasText: new RegExp(`\\b${label}\\b`) })
    .filter({ hasText: /\b(PASS|PASSED|OK)\b|✓/i });

  await expect(passRow, `${label} must not be marked pass`).toHaveCount(0);
}

async function expectDraftFailureValidationLedgerIfPresent(page: Page) {
  const ledger = validationLedgerPanel(page);
  if ((await ledger.count()) === 0) return;

  await expect(ledger).toBeVisible();
  await expect(ledger).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
  await expect(ledger).toContainText('raw sketch backend body: missing closed profile');
}

async function acceptExtrude12Suggestion(page: Page) {
  const panel = page.locator('.sketch-suggestion').filter({ hasText: 'EXTRUDE 12MM' }).first();
  await expect(panel, 'EXTRUDE 12MM suggestion card required').toBeVisible();
  const control = panel.getByRole('button', { name: /\b(USE|APPLY|ACCEPT)\b/i }).first();

  await expect(control, 'EXTRUDE 12MM suggestion needs visible USE/APPLY/ACCEPT control').toBeVisible();
  await control.click();
}

function deepFieldValues(value: unknown, fieldName: string): unknown[] {
  const values: unknown[] = [];
  const visit = (node: unknown) => {
    if (!node || typeof node !== 'object') return;
    if (Array.isArray(node)) {
      for (const item of node) visit(item);
      return;
    }

    for (const [key, child] of Object.entries(node)) {
      if (key === fieldName) values.push(child);
      visit(child);
    }
  };

  visit(value);
  return values;
}

function expectDeepFieldValue(value: unknown, fieldName: string, expected: unknown) {
  expect(deepFieldValues(value, fieldName), `${fieldName} in accepted suggestion preview request`).toContain(expected);
}

const sketchDocumentEvidenceTitle = /SKETCH DOCUMENT|SKETCH IR/i;

function sketchDocumentEvidencePanels(page: Page) {
  return page
    .locator('section, aside, article, details, [role="region"], [aria-label]:not(button)')
    .filter({ hasText: sketchDocumentEvidenceTitle });
}

function sketchDocumentEvidencePanel(page: Page) {
  return sketchDocumentEvidencePanels(page).first();
}

function sketchDocumentReplayControl(page: Page) {
  return page.getByRole('button', { name: /\bREPLAY\b/i });
}

async function expectReplayControlPending(page: Page) {
  const replayControl = sketchDocumentReplayControl(page);
  const count = await replayControl.count();
  if (count === 0) return;

  await expect(replayControl, 'Replay control should stay disabled until a valid SketchDocument snapshot exists').toBeDisabled();
}

async function openSketchDocumentEvidenceIfCollapsed(page: Page) {
  await page.locator('details').evaluateAll((nodes) => {
    for (const node of nodes) {
      const details = node as HTMLDetailsElement;
      const text = `${details.querySelector('summary')?.textContent ?? ''} ${details.textContent ?? ''}`;
      if (/SKETCH DOCUMENT|SKETCH IR/i.test(text)) {
        details.open = true;
      }
    }
  });

  const toggles = page.getByRole('button', { name: sketchDocumentEvidenceTitle });
  const count = await toggles.count();
  for (let index = 0; index < count; index += 1) {
    const toggle = toggles.nth(index);
    if (!(await toggle.isVisible().catch(() => false))) continue;

    if ((await toggle.getAttribute('aria-expanded')) === 'false') {
      await toggle.click();
    }
  }
}

function sketchDocumentImportPanel(page: Page) {
  return page
    .locator('dialog, section, aside, article, div')
    .filter({ hasText: /SKETCH DOCUMENT.*(IMPORT|APPLY)|IMPORT.*SKETCH DOCUMENT|SKETCH IR.*(IMPORT|APPLY)|IMPORT.*SKETCH IR/i })
    .first();
}

async function importSketchDocumentJson(page: Page, source: string) {
  await page.locator('details[aria-label="Sketch import tools"]').evaluateAll((nodes) => {
    for (const node of nodes) (node as HTMLDetailsElement).open = true;
  });

  const panel = sketchDocumentImportPanel(page);
  await expect(panel, 'sketch document import panel required').toBeVisible();

  const editor = panel.locator('textarea, [contenteditable="true"], [role="textbox"]').first();
  await expect(editor, 'sketch document import editor required').toBeVisible();

  const tagName = await editor.evaluate((node) => node.tagName.toLowerCase());
  if (tagName === 'textarea' || tagName === 'input') {
    await editor.fill(source);
  } else {
    await editor.click();
    await page.keyboard.insertText(source);
  }

  const action = panel.getByRole('button', { name: /^(APPLY|IMPORT)$/i }).first();
  await expect(action, 'sketch document import action required').toBeVisible();
  await action.click();
}

function closedFrontSketchDocument(points: SketchPointTuple[], primitiveId: string = 'primitive-front-1') {
  return {
    documentId: 'workspace-sketch-document',
    activeSketchId: 'sketch-front',
    units: 'mm',
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId,
            kind: 'polyline',
            points,
            closed: true,
          },
        ],
      },
    ],
  };
}

async function importClosedFrontProfile(page: Page, points: SketchPointTuple[], primitiveId: string = 'primitive-front-1') {
  await importSketchDocumentJson(page, JSON.stringify(closedFrontSketchDocument(points, primitiveId), null, 2));
  await expect(page.getByText(new RegExp(`${escapeRegExp(primitiveId)} / front / closed`))).toBeVisible();
  await ensureSketchPreviewRequested(page, 0);
}

async function ensureSketchPreviewRequested(page: Page, previousCount: number) {
  const previewReady = await page
    .waitForFunction(
      (count) =>
        ((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0) > count,
      previousCount,
      { timeout: 1200 },
    )
    .then(() => true)
    .catch(() => false);

  if (!previewReady) {
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();
    await page.waitForFunction(
      (count) =>
        ((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0) > count,
      previousCount,
      { timeout: 5000 },
    );
  }

  await expectWorkspacePreviewEvidence(page);
}

async function expectWorkspacePreviewEvidence(page: Page) {
  let workspace = page.locator('.sketch-workspace').first();
  let summary = workspace.getByLabel('Preview artifact summary');
  const summaryVisible = await summary.isVisible().catch(() => false);
  if (!summaryVisible) {
    const sketchButton = page.getByRole('button', { name: 'SKETCH', exact: true });
    await sketchButton.waitFor({ state: 'visible', timeout: 5000 }).catch(() => null);
    if (await sketchButton.isVisible().catch(() => false)) {
      await sketchButton.click();
      await expect(page.getByRole('heading', { name: 'SKETCH WORKSPACE' })).toBeVisible();
      workspace = page.locator('.sketch-workspace').first();
      summary = workspace.getByLabel('Preview artifact summary');
    }
  }
  await expect(summary.getByText('preview.stl', { exact: true })).toBeVisible();
  await expect(summary.getByText('1 assets')).toBeVisible();
}

async function expectSketchSourceAvailable(page: Page, expectedSource: string = sketchSource) {
  const workspace = page.locator('.sketch-workspace').first();
  await workspace.locator('details[aria-label="Sketch artifact evidence"]').evaluateAll((nodes) => {
    for (const node of nodes) (node as HTMLDetailsElement).open = true;
  });
  await workspace.locator('.sketch-workspace__inspect').evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  const sourceStatus = workspace.locator('.sketch-workspace__section').filter({ hasText: 'SOURCE STATUS' }).first();
  await sourceStatus.scrollIntoViewIfNeeded();
  await expect(sourceStatus).toBeVisible();
  await sourceStatus.locator('summary').filter({ hasText: 'VIEW SOURCE' }).click();
  await expect(sourceStatus.locator('pre.sketch-source')).toContainText(expectedSource);
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function sketchSnapControl(page: Page) {
  const name = /^SNAP$/i;
  return page
    .getByRole('button', { name })
    .or(page.getByRole('checkbox', { name }))
    .or(page.getByRole('switch', { name }))
    .first();
}

function sketchGridInput(page: Page) {
  const name = /GRID/i;
  return page.getByRole('spinbutton', { name }).or(page.getByRole('textbox', { name })).first();
}

async function fillSketchGridInput(page: Page, value: string) {
  const input = sketchGridInput(page);
  await expect(input, 'GRID input required for configurable sketch snap size').toBeVisible();
  await input.fill(value);
  await input.press('Tab');
  await expect(input).toHaveValue(value);
}

async function enableSketchSnap(page: Page) {
  const control = sketchSnapControl(page);
  await expect(control, 'SNAP control required before snapped point editing').toBeVisible();

  const checked = await control.isChecked().catch(() => false);
  const pressed = await control.getAttribute('aria-pressed');
  const ariaChecked = await control.getAttribute('aria-checked');
  if (checked || pressed === 'true' || ariaChecked === 'true') return;

  await control.click();
}

async function expectSketchSnapEnabled(page: Page) {
  const control = sketchSnapControl(page);
  await expect(control, 'SNAP control required').toBeVisible();
  await expect
    .poll(
      async () => {
        const checked = await control.isChecked().catch(() => false);
        const pressed = await control.getAttribute('aria-pressed');
        const ariaChecked = await control.getAttribute('aria-checked');
        return checked || pressed === 'true' || ariaChecked === 'true';
      },
      { message: 'SNAP control should remain enabled' },
    )
    .toBe(true);
}

function deletePointControl(page: Page) {
  return page.getByRole('button', { name: /^DELETE POINT$/i });
}

function pointCoordinateInputs(page: Page, label: 'POINT X' | 'POINT Y') {
  const name = new RegExp(`^${escapeRegExp(label)}$`, 'i');
  return page.getByRole('spinbutton', { name }).or(page.getByRole('textbox', { name }));
}

function pointCoordinateInput(page: Page, label: 'POINT X' | 'POINT Y') {
  return pointCoordinateInputs(page, label).first();
}

function applyPointControl(page: Page) {
  return page.getByRole('button', { name: /^APPLY POINT$/i });
}

function profileDimensionInputs(page: Page, label: 'PROFILE WIDTH' | 'PROFILE HEIGHT') {
  const name = new RegExp(`^${escapeRegExp(label)}$`, 'i');
  return page.getByRole('spinbutton', { name }).or(page.getByRole('textbox', { name }));
}

function profileDimensionInput(page: Page, label: 'PROFILE WIDTH' | 'PROFILE HEIGHT') {
  return profileDimensionInputs(page, label).first();
}

function profilePositionInputs(page: Page, label: 'PROFILE X' | 'PROFILE Y') {
  const name = new RegExp(`^${escapeRegExp(label)}$`, 'i');
  return page.getByRole('spinbutton', { name }).or(page.getByRole('textbox', { name }));
}

function profilePositionInput(page: Page, label: 'PROFILE X' | 'PROFILE Y') {
  return profilePositionInputs(page, label).first();
}

function applyProfileSizeControl(page: Page) {
  return page.getByRole('button', { name: /^APPLY SIZE$/i });
}

function applyProfilePositionControl(page: Page) {
  return page.getByRole('button', { name: /^APPLY POSITION$/i });
}

function cleanUpSketchControl(page: Page) {
  return page.getByRole('button', { name: /^CLEAN UP$/i });
}

function sourcePatchLedgerPanel(page: Page) {
  return page.getByLabel('Source patch ledger');
}

async function expectPointCoordinateEditorDisabledEmptyOrAbsent(page: Page) {
  const xInputs = pointCoordinateInputs(page, 'POINT X');
  const yInputs = pointCoordinateInputs(page, 'POINT Y');
  const applyControls = applyPointControl(page);
  const xCount = await xInputs.count();
  const yCount = await yInputs.count();
  const applyCount = await applyControls.count();

  if (xCount === 0 && yCount === 0 && applyCount === 0) return;

  const xInput = xInputs.first();
  const yInput = yInputs.first();
  await expect(xInput, 'POINT X should be disabled and empty when no point is selected').toBeVisible();
  await expect(yInput, 'POINT Y should be disabled and empty when no point is selected').toBeVisible();
  await expect(xInput).toBeDisabled();
  await expect(yInput).toBeDisabled();
  await expect(xInput).toHaveValue('');
  await expect(yInput).toHaveValue('');

  if (applyCount > 0) {
    await expect(applyControls.first(), 'APPLY POINT should stay disabled when no point is selected').toBeDisabled();
  }
}

async function expectProfileDimensionEditorDisabledEmptyOrAbsent(page: Page) {
  const widthInputs = profileDimensionInputs(page, 'PROFILE WIDTH');
  const heightInputs = profileDimensionInputs(page, 'PROFILE HEIGHT');
  const xInputs = profilePositionInputs(page, 'PROFILE X');
  const yInputs = profilePositionInputs(page, 'PROFILE Y');
  const applyControls = applyProfileSizeControl(page);
  const applyPositionControls = applyProfilePositionControl(page);
  const widthCount = await widthInputs.count();
  const heightCount = await heightInputs.count();
  const xCount = await xInputs.count();
  const yCount = await yInputs.count();
  const applyCount = await applyControls.count();
  const applyPositionCount = await applyPositionControls.count();

  if (widthCount === 0 && heightCount === 0 && xCount === 0 && yCount === 0 && applyCount === 0 && applyPositionCount === 0) return;

  const widthInput = widthInputs.first();
  const heightInput = heightInputs.first();
  const xInput = xInputs.first();
  const yInput = yInputs.first();
  await expect(widthInput, 'PROFILE WIDTH should be disabled and empty when no closed profile exists').toBeVisible();
  await expect(heightInput, 'PROFILE HEIGHT should be disabled and empty when no closed profile exists').toBeVisible();
  await expect(xInput, 'PROFILE X should be disabled and empty when no closed profile exists').toBeVisible();
  await expect(yInput, 'PROFILE Y should be disabled and empty when no closed profile exists').toBeVisible();
  await expect(widthInput).toBeDisabled();
  await expect(heightInput).toBeDisabled();
  await expect(xInput).toBeDisabled();
  await expect(yInput).toBeDisabled();
  await expect(widthInput).toHaveValue('');
  await expect(heightInput).toHaveValue('');
  await expect(xInput).toHaveValue('');
  await expect(yInput).toHaveValue('');

  if (applyCount > 0) {
    await expect(applyControls.first(), 'APPLY SIZE should stay disabled when no closed profile exists').toBeDisabled();
  }
  if (applyPositionCount > 0) {
    await expect(applyPositionControls.first(), 'APPLY POSITION should stay disabled when no closed profile exists').toBeDisabled();
  }
}

function frontSketchPointHandle(page: Page, _primitiveId: string, pointIndex: number) {
  return frontSketchPointHandles(page).nth(pointIndex);
}

async function frontSvgClientPoint(page: Page, point: SketchPointTuple) {
  const svg = page.locator('[aria-label="Front sketch pane"]').locator('svg').first();
  await expect(svg, 'Front sketch SVG required for point edit coordinates').toBeVisible();

  return svg.evaluate((node, targetPoint) => {
    const frontSvg = node as SVGSVGElement;
    const screenCtm = frontSvg.getScreenCTM();
    if (!screenCtm) throw new Error('Front sketch SVG screen CTM missing.');

    const svgPoint = frontSvg.createSVGPoint();
    svgPoint.x = targetPoint[0];
    svgPoint.y = targetPoint[1];
    const screenPoint = svgPoint.matrixTransform(screenCtm);
    return { x: screenPoint.x, y: screenPoint.y };
  }, point);
}

async function dragFrontSketchPointHandleTo(page: Page, primitiveId: string, pointIndex: number, point: SketchPointTuple) {
  const handle = frontSketchPointHandle(page, primitiveId, pointIndex);
  await expect(handle, `editable point ${pointIndex} handle required`).toBeVisible();

  const handleBox = await handle.boundingBox();
  expect(handleBox).not.toBeNull();
  if (!handleBox) return;

  const target = await frontSvgClientPoint(page, point);
  await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(target.x, target.y, { steps: 6 });
  await page.mouse.up();
}

async function selectFrontSketchPointHandle(page: Page, primitiveId: string, pointIndex: number) {
  const handle = frontSketchPointHandle(page, primitiveId, pointIndex);
  await expect(handle, `editable point ${pointIndex} handle required before DELETE POINT`).toBeVisible();

  const handleBox = await handle.boundingBox();
  expect(handleBox).not.toBeNull();
  if (!handleBox) return;

  await page.mouse.click(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2);
}

async function sketchDocumentSourceDocument(page: Page) {
  const sourcePanel = page.getByLabel('Sketch document source');
  await expect(sourcePanel, 'Sketch document source panel required').toBeVisible();

  const sourceText = await sourcePanel.locator('pre').first().textContent();
  expect(sourceText, 'Sketch document source JSON required').toBeTruthy();
  return JSON.parse(sourceText ?? '');
}

async function frontProjectionProfilePath(page: Page) {
  const projection = page.getByLabel('FRONT projection');
  await expect(projection, 'FRONT projection required').toBeVisible();

  const path = await projection.locator('path').first().getAttribute('d');
  expect(path, 'FRONT projection profile path required').toBeTruthy();
  return path ?? '';
}

function sketchDraftPrimitivePoints(request: any): SketchPointTuple[] {
  const points = request?.sketch?.primitives?.[0]?.points;
  expect(Array.isArray(points), 'preview request needs primitive points').toBe(true);
  return points;
}

function expectPointOnGrid(point: SketchPointTuple, gridSize: number) {
  for (const coordinate of point) {
    expect(Number.isFinite(coordinate), 'sketch coordinate must be finite').toBe(true);
    expect(Math.abs(coordinate / gridSize - Math.round(coordinate / gridSize))).toBeLessThan(1e-9);
  }
}

function isPointOnGrid(point: SketchPointTuple, gridSize: number) {
  return point.every((coordinate) => Number.isFinite(coordinate) && Math.abs(coordinate / gridSize - Math.round(coordinate / gridSize)) < 1e-9);
}

test.describe('Sketch workspace', () => {
  test('Given empty workbench When app opens Then no sketch prompt panel is injected into the viewport and SKETCH opens the workspace', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await expect(page.getByLabel('Sketch-first modeling')).toHaveCount(0);
    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);

    await openSketchWorkspace(page);

    await expect(page.getByRole('heading', { name: 'SKETCH WORKSPACE' })).toBeVisible();
  });

  test('Given small workbench window When Sketch Workspace opens Then Preview action stays visible', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 620, height: 480 });
    await installSketchMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await openSketchWorkspace(page);

    const workspace = page.locator('.sketch-workspace');
    await expect(workspace).toBeVisible();
    await expect(workspace.getByRole('button', { name: /^PREVIEW NOW$/i })).toBeVisible();
  });

  test('Given polyline tool is active When user adds points before closure Then open handles are visible and selected point coordinates appear', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await selectSketchTool(page, /^POLYLINE$/i);
    await clickFrontPaneRelativePoint(page, [20, 30]);
    await clickFrontPaneRelativePoint(page, [68, 30]);
    await clickFrontPaneRelativePoint(page, [68, 65]);

    await expect(frontSketchPointHandles(page)).toHaveCount(3);
    await frontSketchPointHandles(page).nth(1).click();
    await expect(page.getByLabel('POINT X')).toHaveValue(/67|68/);
    await expect(page.getByLabel('POINT Y')).toHaveValue(/29|30/);
    await expect(page.getByText(/primitive-front-\d+ \/ front \/ open/i)).toBeVisible();
  });

  test('Given front pane reset control is clicked When sketch is idle Then pan label is explicit and no point is created', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const frontPane = page.getByLabel('Front sketch pane');
    const resetPan = frontPane.getByRole('button', { name: /^RESET PAN$/i });
    await expect(resetPan).toBeVisible();
    await resetPan.click();

    await expect(page.getByText(/primitive-front-\d+ \/ front \/ open/i)).toHaveCount(0);
    await expect(frontSketchPointHandles(page)).toHaveCount(0);
  });

  test('Given sketch pane controls render When open polyline exists Then point handles stay compact and reset chip matches pane chrome', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawOpenStroke(page);

    const handle = frontSketchPointHandle(page, 'primitive-front-1', 0);
    await expect(handle).toBeVisible();
    const handleBox = await handle.boundingBox();
    expect(handleBox).not.toBeNull();
    expect(handleBox?.width ?? 0).toBeLessThanOrEqual(16);
    expect(handleBox?.height ?? 0).toBeLessThanOrEqual(16);

    const resetPan = page.getByLabel('Front sketch pane').getByRole('button', { name: /^RESET PAN$/i });
    await expect(resetPan).toBeVisible();
    const resetPanBox = await resetPan.boundingBox();
    expect(resetPanBox).not.toBeNull();
    expect(resetPanBox?.height ?? 0).toBeLessThanOrEqual(22);
    expect(resetPanBox?.width ?? 0).toBeLessThanOrEqual(86);
  });

  test('Given open Front polyline exists When first point handle is clicked with slight pointer jitter Then loop closes instead of moving point', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await selectSketchTool(page, /^POLYLINE$/i);
    await clickFrontPaneRelativePoint(page, [20, 30]);
    await clickFrontPaneRelativePoint(page, [68, 30]);
    await clickFrontPaneRelativePoint(page, [68, 65]);

    const firstHandle = frontSketchPointHandle(page, 'primitive-front-1', 0);
    await expect(firstHandle).toBeVisible();
    const handleBox = await firstHandle.boundingBox();
    expect(handleBox).not.toBeNull();
    if (!handleBox) return;

    const centerX = handleBox.x + handleBox.width / 2;
    const centerY = handleBox.y + handleBox.height / 2;
    await page.mouse.move(centerX, centerY);
    await page.mouse.down();
    await page.mouse.move(centerX + 2, centerY + 1, { steps: 2 });
    await page.mouse.up();

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/i)).toBeVisible();
    await ensureSketchPreviewRequested(page, 0);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.closed)).resolves.toBe(true);
  });

  test('Given rectangle tool is active When bounds are dragged Then preview request carries a closed front polyline', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await selectSketchTool(page, /^RECTANGLE$/i);
    const from = await frontSvgClientPoint(page, [22, 28]);
    const to = await frontSvgClientPoint(page, [74, 70]);
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps: 6 });
    await page.mouse.up();
    await ensureSketchPreviewRequested(page, 0);

    const primitive = (await lastSketchDraftRequest(page))?.sketch?.primitives?.[0];
    expect(primitive?.kind).toBe('polyline');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points?.length).toBe(5);
  });

  test('Given circle tool is active When center and radius are defined Then preview request carries a circle primitive', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await selectSketchTool(page, /^CIRCLE$/i);
    const center = await frontSvgClientPoint(page, [38, 44]);
    const radiusEdge = await frontSvgClientPoint(page, [61, 44]);
    await page.mouse.move(center.x, center.y);
    await page.mouse.down();
    await page.mouse.move(radiusEdge.x, radiusEdge.y, { steps: 6 });
    await page.mouse.up();
    await ensureSketchPreviewRequested(page, 0);

    const primitive = (await lastSketchDraftRequest(page))?.sketch?.primitives?.[0];
    expect(primitive?.kind).toBe('circle');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.radius).toBeGreaterThan(20);
    expect(primitive?.points?.[0]?.[0]).toBeCloseTo(38, 0);
    expect(primitive?.points?.[0]?.[1]).toBeCloseTo(44, 0);
  });

  test('Given synced sketch editor text changes When Apply is clicked Then canvas and next preview use the edited document', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);

    const sourceEditor = page.getByLabel(/Sketch document or ecky source/i);
    await expect(sourceEditor).toBeVisible();
    await sourceEditor.fill(
      JSON.stringify(
        closedFrontSketchDocument(
          [
            [10, 12],
            [48, 12],
            [48, 42],
            [10, 42],
            [10, 12],
          ],
          'primitive-front-sidecar',
        ),
        null,
        2,
      ),
    );
    await page.getByRole('button', { name: /^(APPLY|IMPORT)$/i }).click();
    await ensureSketchPreviewRequested(page, 1);

    await expect(page.getByText(/primitive-front-sidecar \/ front \/ closed/i)).toBeVisible();
    const points = sketchDraftPrimitivePoints(await lastSketchDraftRequest(page));
    expect(points[0][0]).toBeCloseTo(10, 0);
    expect(points[0][1]).toBeCloseTo(12, 0);
    expect(points[2][0]).toBeCloseTo(48, 0);
    expect(points[2][1]).toBeCloseTo(42, 0);
  });

  test('Given Sketch Workspace opens When header actions render Then replay stays visible and close sketch action is not rendered inside the workspace', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const workspace = page.locator('.sketch-workspace');
    const actions = workspace.locator('.sketch-workspace__actions');
    const replay = actions.getByRole('button', { name: /^REPLAY IR$/i });

    await expect(page.locator('.app-overlay-actions .settings-overlay-btn[title="Close"]')).toHaveCount(0);
    await expect(actions.getByRole('button', { name: /^CLOSE SKETCH$/i })).toHaveCount(0);
    await expect(replay).toBeVisible();

    await page.getByRole('button', { name: 'SKETCH', exact: true }).click();
    await expect(page.locator('[data-window-id="sketch"]')).toBeHidden();
    await expect(mainModelViewport(page)).toBeVisible();
  });

  test('Given wide workbench window When Sketch Workspace opens Then mode fills the viewport instead of leaving a dead right gutter', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await installSketchMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await openSketchWorkspace(page);

    const workspace = page.locator('.sketch-workspace');
    const inspector = page.locator('.sketch-workspace__inspect');
    await expect(workspace).toBeVisible();
    await expect(inspector).toBeVisible();

    const workspaceBox = await workspace.boundingBox();
    const inspectorBox = await inspector.boundingBox();
    expect(workspaceBox).not.toBeNull();
    expect(inspectorBox).not.toBeNull();
    if (!workspaceBox || !inspectorBox) return;

    expect(workspaceBox.x).toBeLessThanOrEqual(2);
    expect(workspaceBox.width).toBeGreaterThan(1820);
    expect(inspectorBox.x + inspectorBox.width).toBeGreaterThan(1830);
  });

  test('Given sketch preview exists When inspector renders Then artifact and import noise sit behind collapsible sections without fake mode tabs', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const inspector = page.locator('.sketch-workspace__inspect');
    await expect(inspector).toBeVisible();
    await expect(inspector.getByLabel('Sketch inspector mode panels')).toHaveCount(0);
    await expect(inspector.getByText(/DRAW MODE|VERIFY MODE|ARTIFACT MODE/i)).toHaveCount(0);
    await expect(inspector.getByLabel('Preview artifact summary')).toContainText('preview.stl');
    await expect(inspector.getByLabel('Preview artifact summary')).toContainText('1 assets');

    const importTools = inspector.locator('details[aria-label="Sketch import tools"]');
    await expect(importTools.locator('summary')).toContainText(/SKETCH SOURCE/i);
    await expect(importTools.locator('textarea')).toBeVisible();

    const collapsedEvidence = inspector.locator('details[aria-label="Sketch artifact evidence"]');
    await expect(collapsedEvidence.locator('summary').first()).toContainText(/ARTIFACT EVIDENCE/i);
    await expect(collapsedEvidence.locator('pre.sketch-source')).toBeHidden();
  });

  test('Given sketch inspector has generated evidence When blocks render Then panels are native collapsible sections without internal vertical scroll', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(
      page,
      [
        [12, 8],
        [57, 11],
        [55, 42],
        [10, 39],
        [12, 8],
      ],
      'primitive-front-messy',
    );

    await cleanUpSketchControl(page).click();
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 2);

    const inspector = page.locator('.sketch-workspace__inspect');
    const suggestedFeatures = inspector.getByLabel('Suggested features');
    const validationLedger = inspector.getByLabel('Validation ledger');
    const sourcePatchLedger = inspector.getByLabel('Source patch ledger');
    const sketchDocumentSource = inspector.getByLabel('Sketch document source');
    const projectionPanel = inspector.getByLabel('Projection panel');

    for (const panel of [suggestedFeatures, validationLedger, sourcePatchLedger, sketchDocumentSource, projectionPanel]) {
      await expect(panel).toBeVisible();
      await expect(panel).toHaveJSProperty('tagName', 'DETAILS');
      await expect(panel).toHaveJSProperty('open', true);
      await expect(
        panel.evaluate((node) => node.scrollHeight > node.clientHeight + 1),
        'expanded sketch inspector panels must not own vertical scroll',
      ).resolves.toBe(false);
      await panel.locator('summary').first().click();
      await expect(panel).toHaveJSProperty('open', false);
      await panel.locator('summary').first().click();
      await expect(panel).toHaveJSProperty('open', true);
    }
  });

  test('Given empty Sketch Workspace inspector When controls render Then top blocks and ledger rows are readable collapsible nodes', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const inspector = page.locator('.sketch-workspace__inspect');
    await expect(inspector).toBeVisible();

    const panelLabels = ['Sketch primitives', 'Draft mode', 'Step ledger', 'Point editor', 'Profile size', 'Validation ledger'];
    for (const label of panelLabels) {
      const panel = inspector.getByLabel(label);
      await expect(panel, `${label} should be a native collapsible section`).toBeVisible();
      await expect(panel).toHaveJSProperty('tagName', 'DETAILS');
      await panel.locator('summary').first().click();
      await expect(panel).toHaveJSProperty('open', false);
      await panel.locator('summary').first().click();
      await expect(panel).toHaveJSProperty('open', true);
    }

    const stepRows = inspector.locator('[data-step-ledger-row]');
    await expect(stepRows).toHaveCount(3);
    const firstStepRow = stepRows.first();
    await expect(firstStepRow).toHaveJSProperty('tagName', 'DETAILS');
    await expect(firstStepRow.locator('.sketch-ledger__detail')).toBeHidden();
    await firstStepRow.locator('summary').click();
    await expect(firstStepRow.locator('.sketch-ledger__detail')).toBeVisible();

    const validationRows = validationLedgerRows(inspector.getByLabel('Validation ledger'));
    const sourceGeneratedRow = validationRows.filter({ hasText: 'SOURCE GENERATED' }).first();
    await expect(sourceGeneratedRow).toHaveJSProperty('tagName', 'DETAILS');
    await expect(sourceGeneratedRow.locator('summary')).not.toContainText(/Waiting for/i);
    await expect(sourceGeneratedRow.locator('.sketch-validation-ledger__detail')).toBeHidden();
    await sourceGeneratedRow.locator('summary').click();
    await expect(sourceGeneratedRow.locator('.sketch-validation-ledger__detail')).toBeVisible();
    await expect(sourceGeneratedRow.locator('.sketch-validation-ledger__detail')).toContainText(/closed profile|source draft/i);
  });

  test('Given Sketch mode is open When Preview succeeds Then app returns to model viewport with sketch preview model', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const workspace = page.locator('.sketch-workspace');
    await expect(workspace).toBeVisible();
    await expect(workspace.locator('xpath=ancestor::*[contains(concat(" ", normalize-space(@class), " "), " window ")]')).toHaveCount(0);

    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /^PREVIEW NOW$/i }).click();

    await expect(page.getByRole('heading', { name: 'SKETCH WORKSPACE' })).toHaveCount(0);
    await expect(mainModelViewport(page)).toBeVisible();
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(1);
  });

  test('Given sketch preview handoff renders in the model viewport When CAD acceptance is not proven Then viewport marks it preview-only', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page, { reopenSketch: false });

    const viewport = mainModelViewport(page);
    await expect(viewport, 'sketch preview handoff should render in main model viewport').toBeVisible();

    const status = viewport.getByLabel('Sketch preview status');
    await expect(status, 'viewport must not silently present a diagnostic hull as accepted CAD').toBeVisible();
    await expect(status).toContainText(/SKETCH PREVIEW/i);
    await expect(status).toContainText(/NOT ACCEPTED CAD/i);
    await expect(status).toContainText(/EXPORT LOCKED/i);
    await expect(status).toContainText(/preview\.stl/i);
  });

  test('Given closed Front profile is drawn When no manual preview click happens Then auto preview queues one backend sketch step and renders evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'delay');
    await openSketchWorkspace(page);

    await expect(page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ })).toBeVisible();
    await drawClosedRectangle(page);

    await expect(page.getByText(/AUTO PREVIEW|QUEUED|GENERATING/i).first()).toBeVisible();
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);

    await expect(page.getByText('SOURCE STATUS')).toBeVisible();
    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeVisible();

    await page.waitForTimeout(250);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(1);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.view)).resolves.toBe('front');
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.length)).resolves.toBe(1);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.primitiveId)).resolves.not.toBe(
      'seed-rectangle',
    );
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.closed)).resolves.toBe(true);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.points?.length)).resolves.toBeGreaterThan(
      3,
    );
  });

  test('Given Front pane is rectangular When drawing pane-relative closed rectangle Then preview request coordinates match cursor percentages', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawClosedFrontPaneRelativePolyline(page, [
      [30, 30],
      [70, 30],
      [70, 70],
      [30, 70],
      [30.5, 30.5],
    ]);
    await ensureSketchPreviewRequested(page, 0);

    const points = sketchDraftPrimitivePoints(await lastSketchDraftRequest(page));
    const xs = points.map(([x]) => x);
    const ys = points.map(([, y]) => y);
    expect(points[0][0]).toBeCloseTo(30, 0);
    expect(points[0][1]).toBeCloseTo(30, 0);
    expect(points.at(-1)?.[0]).toBeCloseTo(30, 0);
    expect(points.at(-1)?.[1]).toBeCloseTo(30, 0);
    expect(Math.min(...xs)).toBeCloseTo(30, 0);
    expect(Math.max(...xs)).toBeCloseTo(70, 0);
    expect(Math.min(...ys)).toBeCloseTo(30, 0);
    expect(Math.max(...ys)).toBeCloseTo(70, 0);
  });

  test('Given closed Front profile is drawn When sketch suggestions resolve Then deterministic feature card uses drawn document and preview stays available', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);

    await page.waitForFunction(() => (window as any).__SKETCH_SUGGESTION_CALLS__.length >= 1, undefined, { timeout: 5000 });
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);

    const suggestionPanel = page.getByLabel('Suggested features').filter({ hasText: 'EXTRUDE 12MM' }).first();

    await expect(suggestionPanel).toBeVisible();
    await expect(suggestionPanel).toContainText(/confidence/i);
    await expect(suggestionPanel).toContainText(/93%|0\.93/);
    await expect(suggestionPanel).toContainText(sketchSuggestionReason);

    await expect(page.getByText('SOURCE STATUS')).toBeVisible();
    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeVisible();

    const suggestionDocument = await firstSketchSuggestionDocument(page);
    const suggestionSketch = suggestionDocument?.sketches?.[0];
    expect(suggestionDocument?.documentId).toBe('workspace-sketch-document');
    expect(suggestionDocument?.activeSketchId).toBe('sketch-front');
    expect(suggestionSketch?.view).toBe('front');
    expect(suggestionSketch?.primitives?.length).toBe(1);
    expect(suggestionSketch?.primitives?.[0]?.primitiveId).not.toBe('seed-rectangle');
    expect(suggestionSketch?.primitives?.[0]?.closed).toBe(true);
    expect(suggestionSketch?.primitives?.[0]?.points?.length).toBeGreaterThan(3);
  });

  test('Given closed Front profile is drawn When point handle is dragged Then primitive stays primitive-front-1 / front / closed and preview request uses edited first point', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1, undefined, { timeout: 5000 });

    const initialRequest = await lastSketchDraftRequest(page);
    const initialFirstPoint = initialRequest?.sketch?.primitives?.[0]?.points?.[0];

    const handle = frontSketchPointHandles(page).first();
    await expect(handle, 'closed Front profile needs editable point handles in Front pane').toBeVisible();

    const handleBox = await handle.boundingBox();
    expect(handleBox).not.toBeNull();
    if (!handleBox) return;

    const callsBeforeDrag = await sketchDraftCallCount(page);
    const startX = handleBox.x + handleBox.width / 2;
    const startY = handleBox.y + handleBox.height / 2;

    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 18, startY + 14, { steps: 6 });
    await page.mouse.up();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeDrag,
      { timeout: 5000 },
    );

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();

    const updatedRequest = await lastSketchDraftRequest(page);
    expect(updatedRequest?.sketch?.view).toBe('front');
    expect(updatedRequest?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-1');
    expect(updatedRequest?.sketch?.primitives?.[0]?.primitiveId).not.toBe('seed-rectangle');
    expect(updatedRequest?.sketch?.primitives?.[0]?.closed).toBe(true);
    expect(updatedRequest?.sketch?.primitives?.[0]?.points?.[0]).not.toEqual(initialFirstPoint);
  });

  test('Given a closed Front profile exists When SNAP is enabled and a point handle is dragged Then preview request uses grid-snapped coordinates and primitive remains primitive-front-1 / front / closed', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await enableSketchSnap(page);

    const callsBeforeDrag = await sketchDraftCallCount(page);
    await dragFrontSketchPointHandleTo(page, 'primitive-front-1', 0, [37, 47]);

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeDrag,
      { timeout: 5000 },
    );

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();

    const updatedRequest = await lastSketchDraftRequest(page);
    const primitive = updatedRequest?.sketch?.primitives?.[0];
    expect(updatedRequest?.sketch?.view).toBe('front');
    expect(primitive?.primitiveId).toBe('primitive-front-1');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points?.[0]).toEqual([40, 50]);
    expect(primitive?.points?.at(-1)).toEqual([40, 50]);
  });

  test('Given SKETCH workspace open When grid value changes to 2 and SNAP enabled Then dragging closed Front point handle to [17,27] previews snapped point [18,28]', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await fillSketchGridInput(page, '2');
    await enableSketchSnap(page);

    const callsBeforeDrag = await sketchDraftCallCount(page);
    await dragFrontSketchPointHandleTo(page, 'primitive-front-1', 0, [17, 27]);

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeDrag,
      { timeout: 5000 },
    );

    const updatedRequest = await lastSketchDraftRequest(page);
    const primitive = updatedRequest?.sketch?.primitives?.[0];
    expect(updatedRequest?.sketch?.view).toBe('front');
    expect(primitive?.primitiveId).toBe('primitive-front-1');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points?.[0]).toEqual([18, 28]);
    expect(primitive?.points?.at(-1)).toEqual([18, 28]);
  });

  test('Given SNAP enabled and grid value changes to 5 When drawing a closed Front rectangle Then generated preview request points lie on 5mm increments, not raw pointer floats', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await fillSketchGridInput(page, '5');
    await enableSketchSnap(page);

    const rawPointerPoints: SketchPointTuple[] = [
      [13.2, 17.7],
      [56.1, 18.3],
      [56.7, 41.2],
      [13.6, 41.8],
      [13.1, 17.9],
    ];
    expect(rawPointerPoints.some((point) => !isPointOnGrid(point, 5))).toBe(true);

    await drawClosedFrontPolyline(page, rawPointerPoints);
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1, undefined, { timeout: 5000 });

    const request = await lastSketchDraftRequest(page);
    const points = sketchDraftPrimitivePoints(request);
    expect(points.length).toBeGreaterThan(3);
    for (const point of points) {
      expectPointOnGrid(point, 5);
    }
    for (const rawPoint of rawPointerPoints) {
      expect(points).not.toContainEqual(rawPoint);
    }
  });

  test('Given invalid grid value 0 is entered When SNAP remains enabled and point drag happens Then visible exact local validation says Invalid sketch grid size and backend call count does not increase', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await fillSketchGridInput(page, '2');
    await enableSketchSnap(page);
    await fillSketchGridInput(page, '0');
    await expectSketchSnapEnabled(page);

    const callsBeforeDrag = await sketchDraftCallCount(page);
    await dragFrontSketchPointHandleTo(page, 'primitive-front-1', 0, [17, 27]);

    await expect(page.getByRole('alert')).toHaveText('Invalid sketch grid size.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeDrag);
  });

  test('Given messy closed Front profile is imported When CLEAN UP is clicked Then source-bounds rectangle replaces it and preview/source evidence update', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(
      page,
      [
        [12, 8],
        [57, 11],
        [55, 42],
        [10, 39],
        [12, 8],
      ],
      'primitive-front-messy',
    );

    const initialProjectionPath = await frontProjectionProfilePath(page);
    const callsBeforeCleanUp = await sketchDraftCallCount(page);
    const cleanUpControl = cleanUpSketchControl(page);
    await expect(cleanUpControl, 'CLEAN UP control required for messy closed profile cleanup').toBeVisible();
    await expect(cleanUpControl).toBeEnabled();
    await cleanUpControl.click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeCleanUp,
      { timeout: 5000 },
    );

    const expectedPoints = [
      [10, 8],
      [57, 8],
      [57, 42],
      [10, 42],
      [10, 8],
    ];
    await expect(page.getByText(/primitive-front-messy \/ front \/ closed/)).toBeVisible();
    await expect(page.getByText(/CLEAN UP[\s\S]*(SOURCE BOUNDS|RECTANGLE|CLOSED)/i).first()).toBeVisible();
    await expectWorkspacePreviewEvidence(page);

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.view).toBe('front');
    expect(previewRequest?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-messy');
    expect(previewRequest?.sketch?.primitives?.[0]?.closed).toBe(true);
    expect(previewRequest?.sketch?.primitives?.[0]?.points).toEqual(expectedPoints);

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.view).toBe('front');
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.primitiveId).toBe('primitive-front-messy');
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.closed).toBe(true);
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual(expectedPoints);

    const updatedProjectionPath = await frontProjectionProfilePath(page);
    expect(updatedProjectionPath).not.toBe(initialProjectionPath);
  });

  test('Given open Front profile exists When CLEAN UP is clicked Then exact local validation appears and no backend preview call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await drawOpenStroke(page);

    await expect(page.getByText(/primitive-front-1 \/ front \/ open/)).toBeVisible();
    const callsBeforeCleanUp = await sketchDraftCallCount(page);
    const cleanUpControl = cleanUpSketchControl(page);
    await expect(cleanUpControl, 'CLEAN UP control required for open profile validation').toBeVisible();
    await expect(cleanUpControl).toBeEnabled();
    await cleanUpControl.click();

    await expect(page.getByRole('alert')).toHaveText('Close profile before cleanup.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeCleanUp);
  });

  test('Given messy closed Front profile cleanup succeeds When source patch ledger renders Then cleanup patch evidence stays visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(
      page,
      [
        [12, 8],
        [57, 11],
        [55, 42],
        [10, 39],
        [12, 8],
      ],
      'primitive-front-clean-ledger',
    );

    await cleanUpSketchControl(page).click();
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length > 1, { timeout: 5000 });

    const ledger = sourcePatchLedgerPanel(page);
    await expect(ledger, 'SOURCE PATCH LEDGER required after cleanup mutation').toBeVisible();
    await expect(ledger).toContainText(/SOURCE PATCH LEDGER/i);
    await expect(ledger).toContainText(/CLEAN UP/i);
    await expect(ledger).toContainText(/primitive-front-clean-ledger/i);
    await expect(ledger).toContainText(/width 47mm/i);
    await expect(ledger).toContainText(/height 34mm/i);
  });

  test('Given a closed Front profile exists and a point handle is selected When DELETE POINT is clicked Then primitive remains closed, preview omits that point, and source/projection panels update', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeVisible();
    const initialProjectionPath = await frontProjectionProfilePath(page);

    await selectFrontSketchPointHandle(page, 'primitive-front-1', 1);
    await page.waitForTimeout(750);

    const callsBeforeDelete = await sketchDraftCallCount(page);
    const deleteControl = deletePointControl(page);
    await expect(deleteControl, 'DELETE POINT control required after point selection').toBeEnabled();
    await deleteControl.click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeDelete,
      { timeout: 5000 },
    );

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();

    const expectedPoints = [
      [10, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ];
    const updatedRequest = await lastSketchDraftRequest(page);
    const primitive = updatedRequest?.sketch?.primitives?.[0];
    expect(updatedRequest?.sketch?.view).toBe('front');
    expect(primitive?.primitiveId).toBe('primitive-front-1');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points).toEqual(expectedPoints);
    expect(primitive?.points).not.toContainEqual([60, 10]);

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual(expectedPoints);

    const updatedProjectionPath = await frontProjectionProfilePath(page);
    expect(updatedProjectionPath).not.toBe(initialProjectionPath);
  });

  test('Given deleting would leave fewer than 3 logical points When DELETE POINT is clicked Then exact local validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 10],
    ]);

    await selectFrontSketchPointHandle(page, 'primitive-front-1', 1);
    await page.waitForTimeout(750);

    const callsBeforeDelete = await sketchDraftCallCount(page);
    const deleteControl = deletePointControl(page);
    await expect(deleteControl, 'DELETE POINT control required after point selection').toBeEnabled();
    await deleteControl.click();

    await expect(page.getByRole('alert')).toHaveText('Closed profile needs at least 3 points.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeDelete);
  });

  test('Given a closed Front profile exists and point handle 0 selected When POINT X and POINT Y are entered Then generated preview request uses edited first and closing points and SketchDocument source JSON updates', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await selectFrontSketchPointHandle(page, 'primitive-front-1', 0);

    const pointX = pointCoordinateInput(page, 'POINT X');
    const pointY = pointCoordinateInput(page, 'POINT Y');
    await expect(pointX, 'POINT X editor required after point selection').toBeVisible();
    await expect(pointY, 'POINT Y editor required after point selection').toBeVisible();
    await expect(pointX).toBeEnabled();
    await expect(pointY).toBeEnabled();

    const callsBeforeApply = await sketchDraftCallCount(page);
    await pointX.fill('22.5');
    await pointY.fill('33.25');
    await applyPointControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeApply,
      { timeout: 5000 },
    );

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();

    const expectedPoints = [
      [22.5, 33.25],
      [60, 10],
      [60, 60],
      [10, 60],
      [22.5, 33.25],
    ];
    const updatedRequest = await lastSketchDraftRequest(page);
    const primitive = updatedRequest?.sketch?.primitives?.[0];
    expect(updatedRequest?.sketch?.view).toBe('front');
    expect(primitive?.primitiveId).toBe('primitive-front-1');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points?.[0]).toEqual([22.5, 33.25]);
    expect(primitive?.points?.at(-1)).toEqual([22.5, 33.25]);
    expect(primitive?.points).toEqual(expectedPoints);

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual(expectedPoints);
  });

  test('Given selected point coordinate editor has invalid X nope When APPLY POINT happens Then exact local validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await selectFrontSketchPointHandle(page, 'primitive-front-1', 0);

    const pointX = pointCoordinateInput(page, 'POINT X');
    const pointY = pointCoordinateInput(page, 'POINT Y');
    await expect(pointX, 'POINT X editor required after point selection').toBeVisible();
    await expect(pointY, 'POINT Y editor required after point selection').toBeVisible();

    const callsBeforeApply = await sketchDraftCallCount(page);
    await pointX.fill('nope');
    await pointY.fill('33.25');
    await applyPointControl(page).click();

    await expect(page.getByRole('alert')).toHaveText('Invalid sketch coordinate.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeApply);
  });

  test('Given imported closed Front rectangle When PROFILE WIDTH 80 and PROFILE HEIGHT 20 are applied Then preview request points scale from min corner and SketchDocument source updates', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const profileWidth = profileDimensionInput(page, 'PROFILE WIDTH');
    const profileHeight = profileDimensionInput(page, 'PROFILE HEIGHT');
    await expect(profileWidth, 'PROFILE WIDTH editor required for closed profile dimension scaling').toBeVisible();
    await expect(profileHeight, 'PROFILE HEIGHT editor required for closed profile dimension scaling').toBeVisible();
    await expect(profileWidth).toBeEnabled();
    await expect(profileHeight).toBeEnabled();

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profileWidth.fill('80');
    await profileHeight.fill('20');
    await applyProfileSizeControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeApply,
      { timeout: 5000 },
    );

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();

    const expectedPoints = [
      [10, 10],
      [90, 10],
      [90, 30],
      [10, 30],
      [10, 10],
    ];
    const updatedRequest = await lastSketchDraftRequest(page);
    const primitive = updatedRequest?.sketch?.primitives?.[0];
    expect(updatedRequest?.sketch?.view).toBe('front');
    expect(primitive?.primitiveId).toBe('primitive-front-1');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points).toEqual(expectedPoints);
    expect(primitive?.points?.at(-1)).toEqual(primitive?.points?.[0]);

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual(expectedPoints);
  });

  test('Given SNAP enabled and grid 10 When PROFILE WIDTH 83 and PROFILE HEIGHT 26 are applied Then profile size snaps to 80 by 30', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    await enableSketchSnap(page);

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profileDimensionInput(page, 'PROFILE WIDTH').fill('83');
    await profileDimensionInput(page, 'PROFILE HEIGHT').fill('26');
    await applyProfileSizeControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeApply,
      { timeout: 5000 },
    );

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.primitives?.[0]?.points).toEqual([
      [10, 10],
      [90, 10],
      [90, 40],
      [10, 40],
      [10, 10],
    ]);
  });

  test('Given SNAP enabled with invalid grid value 0 When PROFILE WIDTH and PROFILE HEIGHT are applied Then exact grid validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    await enableSketchSnap(page);
    await fillSketchGridInput(page, '0');

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profileDimensionInput(page, 'PROFILE WIDTH').fill('83');
    await profileDimensionInput(page, 'PROFILE HEIGHT').fill('26');
    await applyProfileSizeControl(page).click();

    await expect(page.getByRole('alert')).toHaveText('Invalid sketch grid size.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeApply);
  });

  test('Given imported closed Front rectangle When PROFILE X 25 and PROFILE Y 35 are applied Then preview request translates all points and SketchDocument source updates', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const profileX = profilePositionInput(page, 'PROFILE X');
    const profileY = profilePositionInput(page, 'PROFILE Y');
    await expect(profileX, 'PROFILE X editor required for closed profile placement').toBeVisible();
    await expect(profileY, 'PROFILE Y editor required for closed profile placement').toBeVisible();
    await expect(profileX).toHaveValue('10');
    await expect(profileY).toHaveValue('10');

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profileX.fill('25');
    await profileY.fill('35');
    await applyProfilePositionControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeApply,
      { timeout: 5000 },
    );

    const expectedPoints = [
      [25, 35],
      [75, 35],
      [75, 65],
      [25, 65],
      [25, 35],
    ];
    const updatedRequest = await lastSketchDraftRequest(page);
    const primitive = updatedRequest?.sketch?.primitives?.[0];
    expect(primitive?.primitiveId).toBe('primitive-front-1');
    expect(primitive?.closed).toBe(true);
    expect(primitive?.points).toEqual(expectedPoints);

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual(expectedPoints);
  });

  test('Given SNAP enabled and grid 10 When PROFILE X 23 and PROFILE Y 36 are applied Then profile origin snaps to [20,40]', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    await enableSketchSnap(page);

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profilePositionInput(page, 'PROFILE X').fill('23');
    await profilePositionInput(page, 'PROFILE Y').fill('36');
    await applyProfilePositionControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeApply,
      { timeout: 5000 },
    );

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.primitives?.[0]?.points).toEqual([
      [20, 40],
      [70, 40],
      [70, 70],
      [20, 70],
      [20, 40],
    ]);
  });

  test('Given SNAP enabled with invalid grid value 0 When PROFILE X and PROFILE Y are applied Then exact grid validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    await enableSketchSnap(page);
    await fillSketchGridInput(page, '0');

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profilePositionInput(page, 'PROFILE X').fill('23');
    await profilePositionInput(page, 'PROFILE Y').fill('36');
    await applyProfilePositionControl(page).click();

    await expect(page.getByRole('alert')).toHaveText('Invalid sketch grid size.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeApply);
  });

  test('Given locked profile dimensions When PROFILE X and PROFILE Y are applied Then profile translates and dimension constraints stay attached', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const callsBeforeLock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profilePositionInput(page, 'PROFILE X').fill('20');
    await profilePositionInput(page, 'PROFILE Y').fill('25');
    await applyProfilePositionControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeApply,
      { timeout: 5000 },
    );

    await expect(page.getByLabel('Dimension constraints')).toContainText(/WIDTH[\s\S]*50MM[\s\S]*LOCKED/i);
    await expect(page.getByLabel('Dimension constraints')).toContainText(/HEIGHT[\s\S]*30MM[\s\S]*LOCKED/i);

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.primitives?.[0]?.points).toEqual([
      [20, 25],
      [70, 25],
      [70, 55],
      [20, 55],
      [20, 25],
    ]);
    expect(previewRequest?.sketch?.constraints).toEqual([
      { constraintId: 'primitive-front-1-closed', kind: 'closed', targetIds: ['primitive-front-1'] },
      { constraintId: 'primitive-front-1-width-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 50 },
      { constraintId: 'primitive-front-1-height-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 30 },
    ]);
  });

  test('Given imported closed Front rectangle When LOCK DIMENSIONS is clicked Then source and preview request carry width height dimension constraints', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const lockButton = page.getByRole('button', { name: /LOCK DIMENSIONS/i });
    await expect(lockButton, 'LOCK DIMENSIONS control required for source-backed dimension constraints').toBeVisible();
    await expect(lockButton).toBeEnabled();

    const callsBeforeLock = await sketchDraftCallCount(page);
    await lockButton.click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );

    const constraints = page.getByLabel('Dimension constraints');
    await expect(constraints, 'DIMENSION CONSTRAINTS panel required after lock').toBeVisible();
    await expect(constraints).toContainText(/WIDTH[\s\S]*50MM[\s\S]*LOCKED/i);
    await expect(constraints).toContainText(/HEIGHT[\s\S]*30MM[\s\S]*LOCKED/i);

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.constraints).toEqual([
      { constraintId: 'primitive-front-1-closed', kind: 'closed', targetIds: ['primitive-front-1'] },
      { constraintId: 'primitive-front-1-width-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 50 },
      { constraintId: 'primitive-front-1-height-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 30 },
    ]);

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.constraints).toEqual(previewRequest?.sketch?.constraints);
  });

  test('Given locked profile dimensions When UNLOCK DIMENSIONS is clicked Then source and preview request remove dimension constraints', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await expect(page.getByLabel('Dimension constraints')).toContainText(/LOCKED/i);

    const callsBeforeUnlock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /UNLOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeUnlock,
      { timeout: 5000 },
    );

    const constraints = page.getByLabel('Dimension constraints');
    await expect(constraints).toContainText(/WIDTH[\s\S]*UNLOCKED/i);
    await expect(constraints).toContainText(/HEIGHT[\s\S]*UNLOCKED/i);

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.constraints).toEqual([
      { constraintId: 'primitive-front-1-closed', kind: 'closed', targetIds: ['primitive-front-1'] },
    ]);
  });

  test('Given locked profile dimensions When PROFILE WIDTH changes Then exact local validation blocks the edit and backend call', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const callsBeforeLock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );
    await expect(page.getByLabel('Dimension constraints')).toContainText(/WIDTH[\s\S]*LOCKED/i);

    const callsBeforeEdit = await sketchDraftCallCount(page);
    await profileDimensionInput(page, 'PROFILE WIDTH').fill('80');
    await applyProfileSizeControl(page).click();

    await expect(page.getByRole('alert')).toHaveText('Locked sketch dimension would change.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeEdit);
    await expect(page.getByLabel('Dimension constraints')).toContainText(/WIDTH[\s\S]*50MM[\s\S]*LOCKED/i);
  });

  test('Given locked profile dimensions When selected point coordinate changes bounds Then solver translates the profile and keeps dimension constraints', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const callsBeforeLock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );

    await selectFrontSketchPointHandle(page, 'primitive-front-1', 0);
    const callsBeforeEdit = await sketchDraftCallCount(page);
    await pointCoordinateInput(page, 'POINT X').fill('20');
    await pointCoordinateInput(page, 'POINT Y').fill('25');
    await applyPointControl(page).click();

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeEdit,
      { timeout: 5000 },
    );
    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByLabel('Dimension constraints')).toContainText(/WIDTH[\s\S]*50MM[\s\S]*LOCKED/i);
    await expect(page.getByLabel('Dimension constraints')).toContainText(/HEIGHT[\s\S]*30MM[\s\S]*LOCKED/i);

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.primitives?.[0]?.points).toEqual([
      [20, 25],
      [70, 25],
      [70, 55],
      [20, 55],
      [20, 25],
    ]);
    expect(previewRequest?.sketch?.constraints).toEqual([
      { constraintId: 'primitive-front-1-closed', kind: 'closed', targetIds: ['primitive-front-1'] },
      { constraintId: 'primitive-front-1-width-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 50 },
      { constraintId: 'primitive-front-1-height-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 30 },
    ]);
  });

  test('Given locked profile dimensions When a point handle is dragged Then solver moves the profile instead of changing width or height', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const callsBeforeLock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );

    const callsBeforeDrag = await sketchDraftCallCount(page);
    await dragFrontSketchPointHandleTo(page, 'primitive-front-1', 0, [25, 20]);

    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeDrag,
      { timeout: 5000 },
    );

    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByLabel('Dimension constraints')).toContainText(/WIDTH[\s\S]*50MM[\s\S]*LOCKED/i);
    await expect(page.getByLabel('Dimension constraints')).toContainText(/HEIGHT[\s\S]*30MM[\s\S]*LOCKED/i);

    const previewRequest = await lastSketchDraftRequest(page);
    expect(previewRequest?.sketch?.primitives?.[0]?.points).toEqual([
      [25, 20],
      [75, 20],
      [75, 50],
      [25, 50],
      [25, 20],
    ]);
  });

  test('Given locked profile dimensions When solver translates point drag Then validation ledger shows constraint solver evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const callsBeforeLock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );

    const callsBeforeDrag = await sketchDraftCallCount(page);
    await dragFrontSketchPointHandleTo(page, 'primitive-front-1', 0, [25, 20]);
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeDrag,
      { timeout: 5000 },
    );

    const ledger = validationLedgerPanel(page);
    const solverRow = validationLedgerRow(ledger, 'CONSTRAINT SOLVER');
    await expect(solverRow, 'CONSTRAINT SOLVER row required after locked-dimension solve').toBeVisible();
    await expect(solverRow).toContainText(/PASS/i);
    await expect(solverRow).toContainText(/locked-axis translation/i);
    await expect(solverRow).toContainText(/width 50mm/i);
    await expect(solverRow).toContainText(/height 30mm/i);
  });

  test('Given locked profile dimensions preview succeeds When validation ledger renders Then CONSTRAINT VALUES row passes with width and height evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const callsBeforeLock = await sketchDraftCallCount(page);
    await page.getByRole('button', { name: /LOCK DIMENSIONS/i }).click();
    await page.waitForFunction(
      (previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount,
      callsBeforeLock,
      { timeout: 5000 },
    );

    const ledger = validationLedgerPanel(page);
    const valuesRow = validationLedgerRow(ledger, 'CONSTRAINT VALUES');
    await expect(valuesRow, 'CONSTRAINT VALUES row required after locked-dimension preview').toBeVisible();
    await expect(valuesRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    await expect(valuesRow).toContainText(/width/i);
    await expect(valuesRow).toContainText(/height/i);
  });

  test('Given PROFILE WIDTH invalid nope When APPLY SIZE happens Then exact local validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const profileWidth = profileDimensionInput(page, 'PROFILE WIDTH');
    await expect(profileWidth, 'PROFILE WIDTH editor required for dimension validation').toBeVisible();

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profileWidth.fill('nope');
    await applyProfileSizeControl(page).click();

    await expect(page.getByRole('alert')).toHaveText('Invalid sketch dimension.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeApply);
  });

  test('Given PROFILE X invalid nope When APPLY POSITION happens Then exact local validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 40],
      [10, 40],
      [10, 10],
    ]);

    const profileX = profilePositionInput(page, 'PROFILE X');
    await expect(profileX, 'PROFILE X editor required for position validation').toBeVisible();

    const callsBeforeApply = await sketchDraftCallCount(page);
    await profileX.fill('nope');
    await applyProfilePositionControl(page).click();

    await expect(page.getByRole('alert')).toHaveText('Invalid sketch coordinate.');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(callsBeforeApply);
  });

  test('Given no closed profile Then profile dimension editor is disabled empty or absent', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await expectProfileDimensionEditorDisabledEmptyOrAbsent(page);
  });

  test('Given no point selected Then coordinate editor is disabled empty or absent and DELETE POINT remains disabled', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await importClosedFrontProfile(page, [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ]);

    await expect(deletePointControl(page)).toBeDisabled();
    await expectPointCoordinateEditorDisabledEmptyOrAbsent(page);
  });

  test('Given closed Front profile is drawn When SketchDocument source evidence opens Then workspace shows replayable camelCase document JSON', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const replayControl = sketchDocumentReplayControl(page);
    await expectReplayControlPending(page);

    await drawClosedRectangle(page);

    await page.waitForFunction(() => (window as any).__SKETCH_SUGGESTION_CALLS__.length >= 1, undefined, { timeout: 5000 });
    await openSketchDocumentEvidenceIfCollapsed(page);

    const panel = sketchDocumentEvidencePanel(page);
    await expect(panel, 'visible SKETCH DOCUMENT or SKETCH IR evidence panel required').toBeVisible();
    await expect(panel).toContainText(/"documentId"\s*:\s*"workspace-sketch-document"/);
    await expect(panel).toContainText(/"activeSketchId"\s*:\s*"sketch-front"/);
    await expect(panel).toContainText(/"primitiveId"\s*:\s*"primitive-front-1"/);
    await expect(panel).toContainText(/"closed"\s*:\s*true/);
    await expect(panel).toContainText(/"points"\s*:/);
    await expect(panel).not.toContainText(/seed-rectangle|sketch-seed|seed geometry/i);
    await expect(panel).not.toContainText(/document_id|active_sketch_id|primitive_id/);

    await expect(replayControl, 'Replay control required for saved SketchDocument/IR').toBeVisible();
    await expect(replayControl).toBeEnabled();

    await page.getByRole('button', { name: 'CLEAR' }).click();
    await expect(page.getByLabel('Sketch primitives').getByText('NO PROFILE')).toBeVisible();
    await expect(page.locator('.sketch-primitive-list').getByText(/primitive-front-1/)).toHaveCount(0);

    const callsBeforeReplay = await sketchPreviewRequestCount(page);
    await replayControl.click();

    await ensureSketchPreviewRequested(page, callsBeforeReplay);

    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();

    const replayRequest = await lastSketchDraftRequest(page);
    const replaySketch = replayRequest?.sketch;
    expect(replayRequest?.partId).toBe('sketch-draft-part');
    expect(replaySketch?.view).toBe('front');
    expect(replaySketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-1');
    expect(replaySketch?.primitives?.[0]?.closed).toBe(true);
    expect(replaySketch?.primitives?.[0]?.points?.length).toBeGreaterThan(3);
    expect(replaySketch?.primitives?.[0]?.points?.[0]).not.toEqual([0, 0]);
    expect(replaySketch?.primitives?.[0]?.primitiveId).not.toBe('seed-rectangle');
  });

  test('Given open Front profile is drawn When point handles are inspected Then editable handles stay visible before closure and backend preview stays local', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawOpenStroke(page);
    await expect(page.getByText(/primitive-front-1 \/ front \/ open/)).toBeVisible();
    await expect(frontSketchPointHandles(page), 'open Front profile should expose editable handles before closure').toHaveCount(3);

    await page.waitForTimeout(1000);

    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
    await expect(page.getByText('SOURCE STATUS')).toHaveCount(0);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeHidden();
  });

  test('Given camelCase SketchDocument JSON is pasted into import editor When import applies Then workspace shows imported primitive and backend preview uses imported points', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-77',
              kind: 'polyline',
              points: [
                [12, 18],
                [58, 18],
                [58, 49],
                [12, 49],
                [12, 18],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByText(/primitive-front-77 \/ front \/ closed/)).toBeVisible();
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText('SOURCE STATUS')).toBeVisible();
    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeVisible();

    const request = await lastSketchDraftRequest(page);
    expect(request?.sketch?.view).toBe('front');
    expect(request?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-77');
    expect(request?.sketch?.primitives?.[0]?.closed).toBe(true);
    expect(request?.sketch?.primitives?.[0]?.points).toEqual([
      [12, 18],
      [58, 18],
      [58, 49],
      [12, 49],
      [12, 18],
    ]);
    expect(request?.sketch?.primitives?.[0]?.primitiveId).not.toBe('seed-rectangle');
  });

  test('Given Front and Top closed profiles are imported When preview runs Then Top constrains preview hull depth instead of fake 12mm depth', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-box',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10, 20],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-top',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-depth',
              kind: 'polyline',
              points: [
                [10, 10],
                [60, 10],
                [60, 32],
                [10, 32],
                [10, 10],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText(/MULTI-VIEW CONSTRAINED/i).first()).toBeVisible();
    await expect(page.getByText(/DEPTH 22MM/i).first()).toBeVisible();
    await expect(page.getByText(/preview hull from front\/top candidate cell search; not accepted BRep/i).first()).toBeVisible();

    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
    const request = await lastSketchPreviewHullRequest(page);
    expect(request?.document?.sketches?.map((sketch: any) => sketch.view)).toEqual(['front', 'top']);
    expect(request?.document?.sketches?.[0]?.primitives?.[0]?.primitiveId).toBe('primitive-front-box');
    expect(request?.fallbackDepth).toBe(22);
  });

  test('Given Front Top and Side closed profiles are imported When preview runs Then hull request carries all three orthographic views', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-hull',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10, 20],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-top',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-hull',
              kind: 'polyline',
              points: [
                [10, 5],
                [60, 5],
                [60, 27],
                [10, 27],
                [10, 5],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-side',
          view: 'side',
          primitives: [
            {
              primitiveId: 'primitive-side-hull',
              kind: 'polyline',
              points: [
                [5, 20],
                [27, 20],
                [27, 50],
                [5, 50],
                [5, 20],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText(/MULTI-VIEW CONSTRAINED/i).first()).toBeVisible();
    await expect(page.getByText(/DEPTH 22MM/i).first()).toBeVisible();
    await expect(page.getByText(/preview hull from front\/top\/side candidate cell search; not accepted BRep/i).first()).toBeVisible();

    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
    await expect(page.evaluate(() => (window as any).__SKETCH_PREVIEW_HULL_CALLS__.length)).resolves.toBe(1);
    const request = await lastSketchPreviewHullRequest(page);
    expect(request?.partId).toBe('sketch-preview-hull');
    expect(request?.fallbackDepth).toBe(22);
    expect(request?.document?.sketches?.map((sketch: any) => sketch.view)).toEqual(['front', 'top', 'side']);
  });

  test('Given preview hull is displayed When main viewport renders Then no sketch status panel is injected outside the workspace', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('viewport-hull'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
    await expect(page.getByText(/preview hull from front\/top\/side candidate cell search; not accepted BRep/i).first()).toBeVisible();
  });

  test('Given sketch preview owns the viewport When preview resolves Then sketch-only viewport panels stay absent', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
    await expect(page.getByLabel('Sketch-first modeling')).toHaveCount(0);
    await expect(page.getByText(/MESH PREVIEW/i).first()).toBeVisible();
  });

  test('Given Front Top and Side closed profiles are imported When preview runs Then BRep candidate graph shows replay evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-candidate',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10, 20],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-top',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-candidate',
              kind: 'polyline',
              points: [
                [10, 5],
                [60, 5],
                [60, 27],
                [10, 27],
                [10, 5],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-side',
          view: 'side',
          primitives: [
            {
              primitiveId: 'primitive-side-candidate',
              kind: 'polyline',
              points: [
                [5, 20],
                [27, 20],
                [27, 50],
                [5, 50],
                [5, 20],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText('BREP CANDIDATE GRAPH', { exact: true })).toBeVisible();
    await expect(page.getByText(/VERTICES 8/i).first()).toBeVisible();
    await expect(page.getByText(/EDGES 12/i).first()).toBeVisible();
    await expect(page.getByText(/CELLS 1/i).first()).toBeVisible();
    await expect(page.getByText(/SOLUTIONS 1/i).first()).toBeVisible();
    await expect(page.getByText(/CANDIDATE SEARCH/i).first()).toBeVisible();
    await expect(page.getByText(/searched 1\/1 candidate cells/i).first()).toBeVisible();
    await expect(page.getByText(/PROJECTION REPLAY PASS/i).first()).toBeVisible();
    await expect(page.getByText(/front 4\/4 edges covered/i).first()).toBeVisible();

    const request = await lastSketchBrepCandidateRequest(page);
    expect(request?.document?.sketches?.map((sketch: any) => sketch.view)).toEqual(['front', 'top', 'side']);
    expect(request?.document?.sketches?.[0]?.primitives?.[0]?.primitiveId).toBe('primitive-front-candidate');
  });

  test('Given candidate graph is ready When candidate is accepted Then STEP-backed accepted CAD evidence appears', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('accept-candidate'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await expect(candidatePanel).toContainText(/CANDIDATE SEARCH READY/i);
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();

    await expect(candidatePanel).toContainText(/ACCEPTED BREP solution0/i);
    await expect(candidatePanel).toContainText(/STEP export artifact: .*accepted\.step/i);
    const acceptedCadRow = validationLedgerRow(validationLedgerPanel(page), 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    await expect(acceptedCadRow).toContainText(/accepted BRep/i);
    await expect(acceptedCadRow).toContainText(/3 views/i);

    const request = await lastSketchBrepAcceptRequest(page);
    expect(request?.partId).toBe('sketch-preview-hull');
    expect(request?.solutionId).toBe('solution0');
    expect(request?.document?.sketches?.map((sketch: any) => sketch.view)).toEqual(['front', 'top', 'side']);
  });

  test('Given empty workspace When sketch opens Then draft mode shows pending mesh state without workspace scene panel', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await expect(page.getByLabel('Workspace scene')).toHaveCount(0);
    const panel = draftModePanel(page);
    await expect(panel).toBeVisible();
    await expect(panel).toContainText(/PENDING/i);
    await expect(panel.getByRole('button')).toHaveCount(0);
  });

  test('Given exact candidate accepted When sketch changes Then scene keeps draft fresh and exact turns stale', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('scene-stale'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();
    await expect(candidatePanel).toContainText(/ACCEPTED BREP solution0/i);
    await expect(candidatePanel).toContainText(/EXACT MODEL COMMITTED/i);

    const callsBeforeResize = await sketchPreviewRequestCount(page);
    await frontSketchPointHandles(page).first().click();
    await page.getByRole('textbox', { name: 'PROFILE WIDTH' }).fill('64');
    await page.getByRole('button', { name: 'APPLY SIZE' }).click();
    await page.waitForFunction(
      (previousCount) =>
        ((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0) > previousCount,
      callsBeforeResize,
    );

    const draftPanel = draftModePanel(page);
    await expect(draftPanel).toContainText(/MESH DRAFT FRESH/i);
    await expect(candidatePanel).toContainText(/EXACT MODEL STALE/i);
  });

  test('Given exact candidate ready When candidate graph accepts exact Then exact state commits in existing exact panel', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('scene-panel-accept'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const exactAction = brepCandidatePanel(page).getByRole('button', { name: /ACCEPT EXACT/i });
    await expect(exactAction).toHaveText(/ACCEPT EXACT/i);
    await exactAction.click();

    await expect(brepCandidatePanel(page)).toContainText(/EXACT MODEL COMMITTED/i);
  });

  test('Given fresh draft scene When draft mode refreshes draft Then mesh draft reruns preview from existing draft panel', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await generateSketchPreview(page);

    const draftPanel = draftModePanel(page);
    await expect(draftPanel).toContainText(/MESH DRAFT FRESH/i);

    const callsBeforeRefresh = await sketchPreviewRequestCount(page);
    const draftAction = draftPanel.getByRole('button', { name: /REFRESH DRAFT/i });
    await expect(draftAction).toHaveText(/REFRESH DRAFT/i);
    await draftAction.click();
    await page.waitForFunction(
      (previousCount) =>
        ((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0) > previousCount,
      callsBeforeRefresh,
    );

    await expect(draftPanel).toContainText(/MESH DRAFT FRESH/i);
  });

  test('Given sketch preview is saved When app reloads Then the same draft restores and discard clears it', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    const sketchStatus = page.getByLabel('Sketch preview status');
    await expect(sketchStatus).toContainText(/DRAFT ACTIVE/i);

    const draftPanel = draftModePanel(page);
    await expect(draftPanel.getByRole('button', { name: /SAVE DRAFT/i })).toBeVisible();
    await draftPanel.getByRole('button', { name: /SAVE DRAFT/i }).click();
    await expect(sketchStatus).toContainText(/DRAFT SAVED/i);

    await page.reload();
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await openSketchWorkspace(page);

    const restoredStatus = page.getByLabel('Sketch preview status');
    await expect(restoredStatus).toContainText(/DRAFT SAVED/i);
    const restoredDraftPanel = draftModePanel(page);
    await expect(restoredDraftPanel.getByRole('button', { name: /DISCARD DRAFT/i })).toBeVisible();
    await restoredDraftPanel.getByRole('button', { name: /DISCARD DRAFT/i }).click();

    await page.reload();
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await openSketchWorkspace(page);
    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
  });

  test('Given delayed draft refresh When draft mode rebuilds mesh draft Then draft panel locks and shows pending label until fresh', async ({ page }) => {
    await installSketchMocks(page, 'delay');
    await openSketchWorkspace(page);
    await generateSketchPreview(page);

    const draftPanel = draftModePanel(page);
    const draftAction = draftPanel.getByRole('button', { name: /REFRESH DRAFT/i });
    await expect(draftAction).toHaveText(/REFRESH DRAFT/i);

    await draftAction.click();

    await expect(draftPanel).toContainText(/MESH DRAFT PENDING/i);
    await expect(draftPanel.getByRole('button')).toHaveText(/BUILDING\.\.\./i);
    await expect(draftPanel.getByRole('button')).toBeDisabled();

    await page.waitForFunction(
      () => (((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0)) >= 2,
    );
    await expect(draftPanel).toContainText(/MESH DRAFT FRESH/i);
  });

  test('Given draft preview is pending When draft mode previews draft and backend fails Then mesh draft shows failure from existing draft panel', async ({ page }) => {
    await installSketchMocks(page, 'error');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);

    const draftPanel = draftModePanel(page);
    await expect(draftPanel).toContainText(/MESH DRAFT PENDING/i);

    const draftAction = draftPanel.getByRole('button', { name: /BUILD DRAFT/i });
    await expect(draftAction).toHaveText(/BUILD DRAFT/i);
    await draftAction.click();

    await expect(page.getByRole('alert')).toContainText('draft generation failed');
    await expect(page.getByRole('alert')).toContainText('raw sketch backend body: missing closed profile');
    await expect(draftPanel).toContainText(/MESH DRAFT FAILED/i);
  });

  test('Given stale exact scene When candidate graph rebuilds exact Then exact state recommits in existing exact panel', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('scene-panel-rebuild'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = brepCandidatePanel(page);
    await candidatePanel.getByRole('button', { name: /ACCEPT EXACT/i }).click();
    await expect(candidatePanel).toContainText(/EXACT MODEL COMMITTED/i);

    const callsBeforeResize = await sketchPreviewRequestCount(page);
    await frontSketchPointHandles(page).first().click();
    await page.getByRole('textbox', { name: 'PROFILE WIDTH' }).fill('66');
    await page.getByRole('button', { name: 'APPLY SIZE' }).click();
    await page.waitForFunction(
      (previousCount) =>
        ((window as any).__SKETCH_DRAFT_CALLS__?.length ?? 0) + ((window as any).__SKETCH_PREVIEW_HULL_CALLS__?.length ?? 0) > previousCount,
      callsBeforeResize,
    );

    await expect(candidatePanel).toContainText(/EXACT MODEL STALE/i);
    const rebuildAction = candidatePanel.getByRole('button', { name: /REBUILD EXACT/i });
    await expect(rebuildAction).toHaveText(/REBUILD EXACT/i);
    await rebuildAction.click();

    await expect(candidatePanel).toContainText(/EXACT MODEL COMMITTED/i);
  });

  test('Given accepted STEP BRep When reusable package is created Then explicit port package evidence appears', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('package-candidate'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();
    await expect(candidatePanel).toContainText(/ACCEPTED BREP solution0/i);
    await candidatePanel.getByRole('button', { name: /CREATE REUSABLE PACKAGE/i }).click();

    const packagePanel = page.getByLabel('Accepted BRep component package');
    await expect(packagePanel).toContainText(/PACKAGE sketch-preview-hull\.accepted-brep/i);
    await expect(packagePanel).toContainText(/COMPONENT sketch-preview-hull/i);
    await expect(packagePanel).toContainText(/SKETCHES 3/i);
    await expect(packagePanel).toContainText(/PORT front_mount/i);
    await expect(packagePanel).toContainText(/TYPE mechanical\.plane\.mount\.v1/i);
    await expect(packagePanel).toContainText(/SOURCE .*accepted\.step/i);

    const request = await lastSketchBrepPackageRequest(page);
    expect(request?.sourceRef).toBe('/mock/sketch/accepted.step');
    expect(request?.solutionId).toBe('solution0');
    expect(request?.document?.sketches?.map((sketch: any) => sketch.view)).toEqual(['front', 'top', 'side']);
    expect(request?.portTypes).toContainEqual(
      expect.objectContaining({
        typeId: 'mechanical.plane.mount.v1',
        displayName: 'Mechanical plane mount',
      }),
    );
    expect(request?.ports).toContainEqual(
      expect.objectContaining({
        portId: 'front_mount',
        typeId: 'mechanical.plane.mount.v1',
      }),
    );
  });

  test('Given accepted STEP BRep exposes exact edge targets When candidate is accepted Then exact topology evidence appears', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'unavailable', 'edge-targets');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('edge-target-candidate'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();

    await expect(candidatePanel).toContainText(/EXACT BREP TOPOLOGY 12 EDGES/i);
    await expect(candidatePanel).not.toContainText(/PREVIEW-ONLY TOPOLOGY/i);
  });

  test('Given accepted STEP BRep exposes exact topology When reusable package is created Then request carries exact targetIds', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'unavailable', 'edge-targets');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('package-topology-targets'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();
    await expect(candidatePanel).toContainText(/EXACT BREP TOPOLOGY 12 EDGES/i);
    await candidatePanel.getByRole('button', { name: /CREATE REUSABLE PACKAGE/i }).click();

    const request = await lastSketchBrepPackageRequest(page);
    expect(request?.ports).toContainEqual(
      expect.objectContaining({
        portId: 'front_mount',
        typeId: 'mechanical.plane.mount.v1',
        targetIds: ['accepted-body:node:42:edge:0'],
      }),
    );
  });

  test('Given accepted STEP BRep has no exact edge targets When candidate is accepted Then topology status is pending', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('edge-target-pending'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();

    await expect(candidatePanel).toContainText(/EXACT BREP TOPOLOGY PENDING/i);
    await expect(candidatePanel).toContainText(/ACCEPTED BREP solution0/i);
  });

  test('Given accepted STEP BRep When package creation fails Then raw package error stays visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'unavailable', 'ok', 'error');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('package-candidate-fail'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();
    await expect(candidatePanel).toContainText(/ACCEPTED BREP solution0/i);
    await candidatePanel.getByRole('button', { name: /CREATE REUSABLE PACKAGE/i }).click();

    await expect(candidatePanel).toContainText(/raw package backend body: port front_mount references unknown type/i);
  });

  test('Given candidate accept lacks STEP When candidate is accepted Then raw gate error stays visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'unavailable', 'error');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('accept-candidate-fail'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const candidatePanel = page.getByLabel('BRep candidate graph');
    await expect(candidatePanel).toContainText(/CANDIDATE SEARCH READY/i);
    await candidatePanel.getByRole('button', { name: /ACCEPT CANDIDATE/i }).click();

    await expect(candidatePanel).toContainText(/mesh preview fallback is not CAD acceptance/i);
    const acceptedCadRow = validationLedgerRow(validationLedgerPanel(page), 'ACCEPTED CAD');
    await expect(acceptedCadRow).not.toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
  });

  test('Given preview hull has final FreeCAD BRep When preview runs Then OCCT hidden-line projection evidence appears', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText('OCCT HIDDEN-LINE PROJECTION', { exact: true })).toBeVisible();
    await expect(page.getByText(/FRONT 2 visible \/ 1 hidden/i).first()).toBeVisible();
    await expect(page.getByText(/TOP 1 visible \/ 0 hidden/i).first()).toBeVisible();
    await expect(page.getByText(/SIDE 1 visible \/ 0 hidden/i).first()).toBeVisible();

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.artifactBundle?.fcstdPath).toBe('/mock/sketch/model.FCStd');
    expect(request?.artifactBundle?.geometryBackend).toBe('freecad');
  });

  test('Given preview hull has direct OCCT STEP artifact When preview runs Then OCCT hidden-line projection uses STEP evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'step-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-step'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText('OCCT HIDDEN-LINE PROJECTION', { exact: true })).toBeVisible();
    await expect(page.getByText(/FRONT 2 visible \/ 1 hidden/i).first()).toBeVisible();

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.artifactBundle?.fcstdPath).toBe('');
    expect(request?.artifactBundle?.geometryBackend).toBe('mesh');
    expect(request?.artifactBundle?.exportArtifacts).toContainEqual({
      label: 'STEP',
      format: 'step',
      path: '/mock/sketch/model.step',
      role: 'primary',
    });
  });

  test('Given OCCT hidden-line projection returns edges When preview runs Then sketch panes show BRep overlay edges', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-overlay'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const frontOverlay = page.locator('[data-brep-hidden-line-overlay="front"]');
    const topOverlay = page.locator('[data-brep-hidden-line-overlay="top"]');
    const sideOverlay = page.locator('[data-brep-hidden-line-overlay="side"]');

    await expect(frontOverlay).toHaveAttribute('data-brep-projection-status', 'pass');
    await expect(frontOverlay.locator('[data-brep-edge="visible"]')).toHaveCount(2);
    await expect(frontOverlay.locator('[data-brep-edge="hidden"]')).toHaveCount(1);
    await expect(topOverlay.locator('[data-brep-edge="visible"]')).toHaveCount(1);
    await expect(sideOverlay.locator('[data-brep-edge="visible"]')).toHaveCount(1);
  });

  test('Given matching SketchDocument and successful OCCT hidden-line response When preview runs Then BREP/SKETCH validation passes with concrete view evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-validated'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const hiddenLinePanel = occtHiddenLinePanel(page);
    await expect(hiddenLinePanel).toBeVisible();
    await expect(hiddenLinePanel).toContainText(/FRONT 2 visible \/ 1 hidden/i);
    await expect(hiddenLinePanel).toContainText(/TOP 1 visible \/ 0 hidden/i);
    await expect(hiddenLinePanel).toContainText(/SIDE 1 visible \/ 0 hidden/i);

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();
    const brepSketchRow = validationLedgerRow(ledger, 'BREP/SKETCH VALIDATION');
    await expect(brepSketchRow, 'BREP/SKETCH VALIDATION needs visible validation ledger row').toBeVisible();
    await expect(brepSketchRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    await expect(brepSketchRow).toContainText(/backend BRep\/sketch validation/i);
    await expect(brepSketchRow).toContainText(/front.*2 visible.*1 hidden/i);
    await expect(brepSketchRow).toContainText(/top.*1 visible.*0 hidden/i);
    await expect(brepSketchRow).toContainText(/side.*1 visible.*0 hidden/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.artifactBundle?.fcstdPath).toBe('/mock/sketch/model.FCStd');
    expect(request?.views).toEqual(['front', 'top', 'side']);
    expect(request?.sketchDocument?.documentId).toBe('workspace-sketch-document');
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.primitiveId).toBe(
      'primitive-front-hidden-line-validated',
    );
  });

  test('Given matching SketchDocument and accepted BRep validation When preview runs Then ACCEPTED CAD passes only after exact BRep evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('accepted-cad'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow, 'ACCEPTED CAD row gates manufacturable model acceptance').toBeVisible();
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    await expect(acceptedCadRow).toContainText(/accepted BRep/i);
    await expect(acceptedCadRow).toContainText(/3 views/i);
  });

  test('Given arbitrary accepted BRep projections When user converts derived sketches Then editable source is marked as derived not history', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('derived-brep'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const derivedPanel = page.getByLabel('Derived BRep sketches');
    await expect(derivedPanel, 'BRep projection must expose derived editable sketches').toBeVisible();
    await expect(derivedPanel).toContainText(/DERIVED FROM BREP/i);
    await expect(derivedPanel).toContainText(/NOT AUTHORING HISTORY/i);
    await expect(derivedPanel).toContainText(/FRONT/i);
    await expect(derivedPanel).toContainText(/TOP/i);
    await expect(derivedPanel).toContainText(/SIDE/i);

    await derivedPanel.getByRole('button', { name: /CONVERT DERIVED SKETCHES/i }).click();

    await expect(page.getByLabel('Source patch ledger')).toContainText(/DERIVE BREP/i);
    await expect(page.getByLabel('Cleanup evidence')).toContainText(/NOT AUTHORING HISTORY/i);
    const document = JSON.parse(await page.getByLabel('Sketch document or ecky source').inputValue());
    expect(document.metadata?.provenance).toBe('derivedFromBRep');
    expect(document.metadata?.sourceArtifactPath).toBe('/mock/sketch/model.FCStd');
    expect(document.sketches?.map((sketch: any) => sketch.view)).toEqual(['front', 'top', 'side']);
    expect(document.sketches?.[0]?.primitives?.[0]?.primitiveId).toBe('derived-brep-front');
  });

  test('Given accepted BRep projection has multiple closed loops When user converts derived sketches Then front source keeps separate editable primitives', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'multi-loop-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('derived-brep-multi-loop'), null, 2));
    await ensureSketchPreviewRequested(page, 0);
    const callsBeforeReplay = await sketchPreviewRequestCount(page);

    const derivedPanel = page.getByLabel('Derived BRep sketches');
    await expect(derivedPanel).toContainText(/DERIVED FROM BREP/i);
    await derivedPanel.getByRole('button', { name: /CONVERT DERIVED SKETCHES/i }).click();

    await expect(page.getByLabel('Source patch ledger')).toContainText(/DERIVE BREP/i);
    await ensureSketchPreviewRequested(page, callsBeforeReplay);
    const document = JSON.parse(await page.getByLabel('Sketch document or ecky source').inputValue());
    const frontSketch = document.sketches?.find((sketch: any) => sketch.view === 'front');
    expect(frontSketch?.primitives?.map((primitive: any) => primitive.primitiveId)).toEqual([
      'derived-brep-front-front-outer',
      'derived-brep-front-front-hole',
    ]);
    expect(frontSketch?.primitives?.map((primitive: any) => primitive.topology?.loopId)).toEqual([
      'front-outer',
      'front-hole',
    ]);
    expect(frontSketch?.primitives?.map((primitive: any) => primitive.topology?.loopRole)).toEqual([
      'outer',
      'hole',
    ]);
    expect(frontSketch?.primitives?.[1]?.points).toEqual([
      [25, 18],
      [45, 18],
      [45, 34],
      [25, 34],
      [25, 18],
    ]);
    const replayRequest = await lastSketchPreviewHullRequest(page);
    const replayFrontSketch = replayRequest?.document?.sketches?.find((sketch: any) => sketch.view === 'front');
    expect(replayFrontSketch?.primitives?.map((primitive: any) => primitive.primitiveId)).toEqual([
      'derived-brep-front-front-outer',
      'derived-brep-front-front-hole',
    ]);
    expect(replayFrontSketch?.primitives?.map((primitive: any) => primitive.topology?.loopId)).toEqual([
      'front-outer',
      'front-hole',
    ]);
    expect(replayFrontSketch?.primitives?.map((primitive: any) => primitive.topology?.loopRole)).toEqual([
      'outer',
      'hole',
    ]);
    const sourcePanel = page
      .locator('.sketch-workspace__section')
      .filter({ hasText: 'SOURCE STATUS' })
      .first();
    await expect(sourcePanel.locator('pre.sketch-source')).toContainText(/\(profile :outer[\s\S]*:holes/i);
  });

  test('Given repairable BRep bounds mismatch When preview runs Then BRep auto snap reruns and accepted CAD passes', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'bounds-mismatch-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-mismatch'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO SNAP FRONT/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-mismatch/i);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);

    const learningLens = page.getByLabel('Learning lens');
    await expect(learningLens).toContainText(/BREP AUTO SNAP/i);
    await expect(learningLens).toContainText(/bounds/i);
    await expect(learningLens).toContainText(/x'/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [0, 0],
      [80, 0],
      [80, 40],
      [0, 40],
      [0, 0],
    ]);
  });

  test('Given bounds issue kind is structured and message is neutral When preview runs Then BRep auto snap reruns', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'bounds-kind-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-mismatch'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO SNAP FRONT primitive-front-hidden-line-mismatch/i);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [0, 0],
      [80, 0],
      [80, 40],
      [0, 40],
      [0, 0],
    ]);
  });

  test('Given BRep bounds mismatch lacks primitive id When preview runs Then BRep auto snap does not guess source primitive', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'bounds-mismatch-no-primitive-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-mismatch'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(1);
    await expect(page.getByLabel('Source patch ledger')).toHaveCount(0);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
  });

  test('Given OCCT hidden-line bounds mismatch When preview runs Then BREP/SKETCH validation fails with raw evidence and OCCT panel stays visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'bounds-mismatch-repeat');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-mismatch'), null, 2));
    await ensureSketchPreviewRequested(page, 0);
    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);

    const hiddenLinePanel = occtHiddenLinePanel(page);
    await expect(hiddenLinePanel, 'OCCT panel must remain visible after bounds mismatch response').toBeVisible();
    await expect(hiddenLinePanel).toContainText(/FRONT 2 visible \/ 1 hidden/i);
    await expect(hiddenLinePanel).toContainText(
      /raw BREP\/SKETCH bounds mismatch: front sketch bounds x=10\.\.60 y=20\.\.50; OCCT bounds x=0\.\.80 y=0\.\.40/i,
    );
    await expect(hiddenLinePanel).toContainText(/REPAIR TARGETS/i);
    const repairTarget = hiddenLinePanel.locator('[data-brep-repair-target]').first();
    await expect(repairTarget, 'bounds mismatch must expose a concrete repair target').toBeVisible();
    await expect(repairTarget).toContainText(/FRONT/i);
    await expect(repairTarget).toContainText(/primitive-front-hidden-line-mismatch/i);
    await expect(repairTarget).toContainText(/raw BREP\/SKETCH bounds mismatch/i);

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();
    const brepSketchRow = validationLedgerRow(ledger, 'BREP/SKETCH VALIDATION');
    await expect(brepSketchRow, 'BREP/SKETCH VALIDATION needs visible failure row').toBeVisible();
    await expect(brepSketchRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(brepSketchRow).toContainText(
      /raw BREP\/SKETCH bounds mismatch: front sketch bounds x=10\.\.60 y=20\.\.50; OCCT bounds x=0\.\.80 y=0\.\.40/i,
    );
    await expectValidationLedgerNoPassRow(ledger, 'BREP/SKETCH VALIDATION');
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow, 'ACCEPTED CAD must fail when BRep/sketch validation fails').toBeVisible();
    await expect(acceptedCadRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(acceptedCadRow).toContainText(
      /raw BREP\/SKETCH bounds mismatch: front sketch bounds x=10\.\.60 y=20\.\.50; OCCT bounds x=0\.\.80 y=0\.\.40/i,
    );
    await expectValidationLedgerNoPassRow(ledger, 'ACCEPTED CAD');

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
  });

  test('Given BRep bounds mismatch targets no source primitive When preview runs Then no BRep auto snap mutation runs', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'bounds-mismatch-unsupported');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-mismatch'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(1);
    await expect(page.getByText(/BREP AUTO (SNAP|CONTAIN)/i)).toHaveCount(0);
    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(occtHiddenLinePanel(page)).toContainText(/raw BREP\/SKETCH bounds mismatch/i);
  });

  test('Given OCCT hidden-line bounds match but silhouette exits source profile When preview runs Then BREP/SKETCH validation fails with containment evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-mismatch');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-containment'), null, 2));
    await ensureSketchPreviewRequested(page, 0);
    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(1);
    await expect(page.getByText(/BREP AUTO SNAP/i)).toHaveCount(0);

    const hiddenLinePanel = occtHiddenLinePanel(page);
    await expect(hiddenLinePanel).toBeVisible();
    await expect(hiddenLinePanel).toContainText(
      /raw BREP\/SKETCH containment mismatch: front edge front-v1 has 8 samples outside source profile, maxOutside=4\.2mm/i,
    );
    await expect(hiddenLinePanel).toContainText(/REPAIR TARGETS/i);
    const repairTarget = hiddenLinePanel.locator('[data-brep-repair-target]').first();
    await expect(repairTarget, 'containment mismatch must expose a concrete repair target').toBeVisible();
    await expect(repairTarget).toContainText(/FRONT/i);
    await expect(repairTarget).toContainText(/primitive-front-hidden-line-containment/i);

    const ledger = validationLedgerPanel(page);
    const brepSketchRow = validationLedgerRow(ledger, 'BREP/SKETCH VALIDATION');
    await expect(brepSketchRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(brepSketchRow).toContainText(/raw BREP\/SKETCH containment mismatch/i);
    await expectValidationLedgerNoPassRow(ledger, 'BREP/SKETCH VALIDATION');

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
  });

  test('Given containment issue carries explicit view and edge locator When preview runs Then front repair target resolves exact hole without message parsing', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-view-edge-topology');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-containment-explicit');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const hiddenLinePanel = occtHiddenLinePanel(page);
    await expect(hiddenLinePanel).toContainText(/raw BREP\/SKETCH containment mismatch: projection exits source profile/i);
    await expect(hiddenLinePanel.locator('[data-brep-repair-target]')).toHaveCount(1);
    const repairTarget = hiddenLinePanel.locator('[data-brep-repair-target]').first();
    await expect(repairTarget).toContainText(/FRONT/i);
    await expect(repairTarget).toContainText(/primitive-front-hole/i);
    await expect(repairTarget).not.toContainText(/primitive-front-outer/i);
    await expect(repairTarget).toContainText(/front-v1/i);
    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
  });

  test('Given containment issue targets hole topology When preview runs Then BRep auto contain reruns on hole only', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-view-edge-topology-then-ok');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-containment-explicit-auto');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect.poll(() => brepHiddenLineCallCount(page), { timeout: 5000 }).toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO CONTAIN FRONT/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hole/i);
    await expect(page.getByLabel('Source patch ledger')).not.toContainText(/primitive-front-outer/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [0, 0],
      [80, 0],
      [80, 50],
      [0, 50],
      [0, 0],
    ]);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[1]?.points).toEqual([
      [25, 18],
      [46, 18],
      [46, 35],
      [25, 35],
      [25, 18],
    ]);
  });

  test('Given bounds issue targets hole topology When preview runs Then BRep auto snap reruns on hole only', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'bounds-hole-topology-then-ok');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-bounds-explicit-auto');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect.poll(() => brepHiddenLineCallCount(page), { timeout: 5000 }).toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO SNAP FRONT/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hole/i);
    await expect(page.getByLabel('Source patch ledger')).not.toContainText(/primitive-front-outer/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [0, 0],
      [80, 0],
      [80, 50],
      [0, 50],
      [0, 0],
    ]);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[1]?.points).toEqual([
      [25, 18],
      [46, 18],
      [46, 35],
      [25, 35],
      [25, 18],
    ]);
  });

  test('Given mixed containment and topology issues When preview runs Then containment reruns and topology proposal remains', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-plus-topology-then-topology');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-containment-plus-topology');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect.poll(() => brepHiddenLineCallCount(page), { timeout: 5000 }).toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO CONTAIN FRONT/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hole/i);
    await expect(brepTopologyRepairPanel(page)).toContainText(/TOPOLOGY REPAIR PROPOSALS/i);
    await expect(brepTopologyRepairPanel(page)).toContainText(/primitive-front-hole/i);
  });

  test('Given containment issue has structured front view and neutral message When preview runs Then front overlay fails from structured topology match', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-topology-no-view-neutral');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-containment-no-view');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="side"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );

    const hiddenLinePanel = occtHiddenLinePanel(page);
    await expect(hiddenLinePanel).toContainText(/containment mismatch/i);
    await expect(hiddenLinePanel).toContainText(/FRONT/i);
    await expect(hiddenLinePanel).toContainText(/HOLE/i);
    await expect(hiddenLinePanel).toContainText(/front-v1/i);

    const ledger = validationLedgerPanel(page);
    const brepSketchRow = validationLedgerRow(ledger, 'BREP/SKETCH VALIDATION');
    await expect(brepSketchRow).toContainText(/hole/i);
    await expect(brepSketchRow).toContainText(/front-v1/i);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/hole/i);
    await expect(acceptedCadRow).toContainText(/front-v1/i);
  });

  test('Given containment issue has generic sketch id with structured front view When preview runs Then front overlay still fails from topology-only match', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-topology-generic-sketch-neutral');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-containment-topology-only');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="side"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
  });

  test('Given containment issue has stale top view but front topology When preview runs Then topology match beats stale view', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-topology-stale-view-neutral');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-containment-stale-view');
    document.activeSketchId = 'sketch-alpha';
    document.sketches[0] = {
      sketchId: 'sketch-alpha',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [80, 0],
            [80, 50],
            [0, 50],
            [0, 0],
          ],
          closed: true,
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'primitive-front-hole',
          kind: 'polyline',
          points: [
            [25, 18],
            [45, 18],
            [45, 34],
            [25, 34],
            [25, 18],
          ],
          closed: true,
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="side"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
  });

  test('Given exact front issue with duplicate topology in top sketch When preview runs Then exact front identity beats duplicate topology', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-duplicate-topology-exact-front');
    await openSketchWorkspace(page);

    const document: any = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front-exact',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-top-duplicate',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-hole',
              kind: 'polyline',
              points: [
                [25, 18],
                [45, 18],
                [45, 34],
                [25, 34],
                [25, 18],
              ],
              closed: true,
              topology: {
                loopId: 'shared-hole',
                edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                loopRole: 'hole',
                sourceClass: 'derived',
              },
            },
          ],
        },
        {
          sketchId: 'sketch-front-exact',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-outer',
              kind: 'polyline',
              points: [
                [0, 0],
                [80, 0],
                [80, 50],
                [0, 50],
                [0, 0],
              ],
              closed: true,
              topology: {
                loopId: 'front-outer',
                edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
                loopRole: 'outer',
                sourceClass: 'derived',
              },
            },
            {
              primitiveId: 'primitive-front-hole',
              kind: 'polyline',
              points: [
                [25, 18],
                [45, 18],
                [45, 34],
                [25, 34],
                [25, 18],
              ],
              closed: true,
              topology: {
                loopId: 'shared-hole',
                edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
                loopRole: 'hole',
                sourceClass: 'derived',
              },
            },
          ],
        },
        {
          sketchId: 'sketch-side',
          view: 'side',
          primitives: [
            {
              primitiveId: 'primitive-side',
              kind: 'polyline',
              points: [
                [5, 20],
                [27, 20],
                [27, 50],
                [5, 50],
                [5, 20],
              ],
              closed: true,
            },
          ],
        },
      ],
    };
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const repairTarget = occtHiddenLinePanel(page).locator('[data-brep-repair-target]').first();
    await expect(repairTarget).toContainText(/primitive-front-hole/i);
    await expect(repairTarget).not.toContainText(/primitive-top-hole/i);
    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'fail',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
  });

  test('Given validation issue has stale top view and no locator When preview runs Then pane overlays do not guess failing view', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'view-only-stale-top-no-locator');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-view-only'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="side"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
  });

  test('Given BRep topology mismatch is structured When preview runs Then redraw proposal appears and accepted CAD fails', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'topology-mismatch');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-topology'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/TOPOLOGY REPAIR PROPOSALS/i);
    await expect(topologyRepairPanel).toContainText(/primitive-front-hidden-line-topology/i);
    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute('data-brep-projection-status', 'fail');
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute('data-brep-projection-status', 'pass');
    await expect(page.locator('[data-brep-hidden-line-overlay="side"]')).toHaveAttribute('data-brep-projection-status', 'pass');

    const ledger = validationLedgerPanel(page);
    const brepSketchRow = validationLedgerRow(ledger, 'BREP/SKETCH VALIDATION');
    await expect(brepSketchRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(brepSketchRow).toContainText(/topology mismatch/i);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(acceptedCadRow).toContainText(/topology mismatch/i);
    await expect(occtHiddenLinePanel(page)).toContainText(/topology mismatch/i);
  });

  test('Given structured top projection warning When preview runs Then top pane warns but CAD rows still pass', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'warning-entry-top-no-edges');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-topology'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.locator('[data-brep-hidden-line-overlay="front"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="top"]')).toHaveAttribute(
      'data-brep-projection-status',
      'warn',
    );
    await expect(page.locator('[data-brep-hidden-line-overlay="side"]')).toHaveAttribute(
      'data-brep-projection-status',
      'pass',
    );

    const hiddenLinePanel = occtHiddenLinePanel(page);
    await expect(hiddenLinePanel).toContainText(/TOP projection produced no edges/i);

    const ledger = validationLedgerPanel(page);
    const brepSketchRow = validationLedgerRow(ledger, 'BREP/SKETCH VALIDATION');
    await expect(brepSketchRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
  });

  test('Given BRep topology repair proposal When user applies redraw seed Then source updates and accepted CAD reruns', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'topology-mismatch-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-topology'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/TOPOLOGY REPAIR PROPOSALS/i);
    await topologyRepairPanel.getByRole('button', { name: /APPLY REDRAW SEED/i }).click();

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/TOPOLOGY REDRAW/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-topology/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [10, 20],
      [60, 20],
      [60, 50],
      [10, 50],
      [10, 20],
    ]);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
  });

  test('Given topology issue kind is structured and message is neutral When user applies redraw seed Then source updates and accepted CAD reruns', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'topology-kind-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-topology'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/TOPOLOGY REPAIR PROPOSALS/i);
    await topologyRepairPanel.getByRole('button', { name: /APPLY REDRAW SEED/i }).click();

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/TOPOLOGY REDRAW/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-topology/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [10, 20],
      [60, 20],
      [60, 50],
      [10, 50],
      [10, 20],
    ]);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
  });

  test('Given edge-only multi-loop front sketch has hole before outer When topology redraw applies Then repair updates hole loop and leaves outer loop untouched', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'multi-loop-edges-topology-mismatch-then-ok');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-topology-edge-only');
    document.sketches[0].primitives = [
      {
        primitiveId: 'primitive-front-hole',
        kind: 'polyline',
        points: [
          [25, 18],
          [45, 18],
          [45, 34],
          [25, 34],
          [25, 18],
        ],
        closed: true,
        topology: {
          loopId: 'front-hole',
          edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
          loopRole: 'hole',
          sourceClass: 'derived',
        },
      },
      {
        primitiveId: 'primitive-front-outer',
        kind: 'polyline',
        points: [
          [0, 0],
          [80, 0],
          [80, 50],
          [0, 50],
          [0, 0],
        ],
        closed: true,
        topology: {
          loopId: 'front-outer',
          edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
          loopRole: 'outer',
          sourceClass: 'derived',
        },
      },
    ];
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/TOPOLOGY REPAIR PROPOSALS/i);
    await expect(topologyRepairPanel).toContainText(/primitive-front-hole/i);
    await topologyRepairPanel.getByRole('button', { name: /APPLY REDRAW SEED/i }).click();

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hole/i);

    const request = await lastBrepHiddenLineRequest(page);
    const frontSketch = request?.sketchDocument?.sketches?.find((sketch: any) => sketch?.view === 'front');
    expect(frontSketch?.primitives?.map((primitive: any) => primitive.primitiveId)).toEqual([
      'primitive-front-hole',
      'primitive-front-outer',
    ]);
    expect(frontSketch?.primitives?.[0]?.points).toEqual([
      [24, 17],
      [46, 17],
      [46, 35],
      [24, 35],
      [24, 17],
    ]);
    expect(frontSketch?.primitives?.[1]?.points).toEqual([
      [0, 0],
      [80, 0],
      [80, 50],
      [0, 50],
      [0, 0],
    ]);
    expect(frontSketch?.primitives?.map((primitive: any) => primitive.topology?.loopRole)).toEqual([
      'hole',
      'outer',
    ]);
  });

  test('Given edge-only multi-hole front sketch with churned topology ids When topology redraw applies Then exact target hole updates and sibling hole stays untouched', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'multi-loop-edges-topology-churn-then-ok');
    await openSketchWorkspace(page);

    const document: any = threeViewSketchDocument('hidden-line-topology-edge-churn');
    document.sketches[0].primitives = [
      {
        primitiveId: 'primitive-front-hole-b',
        kind: 'polyline',
        points: [
          [50, 22],
          [64, 22],
          [64, 36],
          [50, 36],
          [50, 22],
        ],
        closed: true,
        topology: {
          loopId: 'front-hole-b',
          edgeIds: ['hole-b-a', 'hole-b-b', 'hole-b-c', 'hole-b-d'],
          loopRole: 'hole',
          sourceClass: 'derived',
        },
      },
      {
        primitiveId: 'primitive-front-hole-a',
        kind: 'polyline',
        points: [
          [12, 12],
          [22, 12],
          [22, 22],
          [12, 22],
          [12, 12],
        ],
        closed: true,
        topology: {
          loopId: 'front-hole-a',
          edgeIds: ['hole-a-a', 'hole-a-b', 'hole-a-c', 'hole-a-d'],
          loopRole: 'hole',
          sourceClass: 'derived',
        },
      },
      {
        primitiveId: 'primitive-front-outer',
        kind: 'polyline',
        points: [
          [0, 0],
          [80, 0],
          [80, 50],
          [0, 50],
          [0, 0],
        ],
        closed: true,
        topology: {
          loopId: 'front-outer',
          edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
          loopRole: 'outer',
          sourceClass: 'derived',
        },
      },
    ];
    await importSketchDocumentJson(page, JSON.stringify(document, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/primitive-front-hole-b/i);
    await topologyRepairPanel.getByRole('button', { name: /APPLY REDRAW SEED/i }).click();

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);

    const request = await lastBrepHiddenLineRequest(page);
    const frontSketch = request?.sketchDocument?.sketches?.find((sketch: any) => sketch?.view === 'front');
    expect(frontSketch?.primitives?.map((primitive: any) => primitive.primitiveId)).toEqual([
      'primitive-front-hole-b',
      'primitive-front-hole-a',
      'primitive-front-outer',
    ]);
    expect(frontSketch?.primitives?.[0]?.points).toEqual([
      [52, 24],
      [66, 24],
      [66, 38],
      [52, 38],
      [52, 24],
    ]);
    expect(frontSketch?.primitives?.[1]?.points).toEqual([
      [12, 12],
      [22, 12],
      [22, 22],
      [12, 22],
      [12, 12],
    ]);
    expect(frontSketch?.primitives?.[2]?.points).toEqual([
      [0, 0],
      [80, 0],
      [80, 50],
      [0, 50],
      [0, 0],
    ]);
  });

  test('Given BRep concavity repair proposal When user applies redraw seed Then source uses concave projection loop and accepted CAD reruns', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'concavity-mismatch-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-concavity'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/CONCAVITY FRONT/i);
    await expect(topologyRepairPanel).toContainText(/explicit profile redraw/i);
    await topologyRepairPanel.getByRole('button', { name: /APPLY REDRAW SEED/i }).click();

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/TOPOLOGY REDRAW/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-concavity/i);
    const learningLens = page.getByLabel('Learning lens');
    await expect(learningLens).toContainText(/BREP TOPOLOGY REDRAW/i);
    await expect(learningLens).toContainText(/projection-derived loop/i);
    await expect(learningLens).toContainText(/not original authoring history/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [10, 20],
      [60, 20],
      [60, 50],
      [35, 35],
      [10, 50],
      [10, 20],
    ]);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
  });

  test('Given concavity issue kind is structured and message is neutral When user applies redraw seed Then source uses concave projection loop', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'concavity-kind-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-concavity'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    const topologyRepairPanel = brepTopologyRepairPanel(page);
    await expect(topologyRepairPanel).toContainText(/CONCAVITY FRONT/i);
    await topologyRepairPanel.getByRole('button', { name: /APPLY REDRAW SEED/i }).click();

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-concavity/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [10, 20],
      [60, 20],
      [60, 50],
      [35, 35],
      [10, 50],
      [10, 20],
    ]);
  });

  test('Given BRep containment mismatch has outside projection bounds When preview runs Then BRep auto contain reruns and accepted CAD passes', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-expand-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-containment'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO CONTAIN FRONT/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-containment/i);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [10, 20],
      [64.2, 20],
      [64.2, 50],
      [10, 50],
      [10, 20],
    ]);
  });

  test('Given containment issue kind is structured and message is neutral When preview runs Then BRep auto contain reruns', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-kind-then-ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-containment'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO CONTAIN FRONT/i);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/primitive-front-hidden-line-containment/i);

    const request = await lastBrepHiddenLineRequest(page);
    expect(request?.sketchDocument?.sketches?.[0]?.primitives?.[0]?.points).toEqual([
      [10, 20],
      [64.2, 20],
      [64.2, 50],
      [10, 50],
      [10, 20],
    ]);
  });

  test('Given BRep containment auto contain still fails When preview reruns Then repair does not loop', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'containment-expand-repeat');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-containment'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect
      .poll(() => brepHiddenLineCallCount(page), { timeout: 5000 })
      .toBe(2);
    await expect(page.getByLabel('Source patch ledger')).toContainText(/BREP AUTO CONTAIN FRONT/i);
    await expect(page.getByLabel('Source patch ledger').getByText(/BREP AUTO CONTAIN FRONT/i)).toHaveCount(1);

    const ledger = validationLedgerPanel(page);
    const acceptedCadRow = validationLedgerRow(ledger, 'ACCEPTED CAD');
    await expect(acceptedCadRow).toContainText(/\b(FAIL|FAILED|ERROR)\b/i);
    await expect(occtHiddenLinePanel(page)).toContainText(/raw BREP\/SKETCH containment mismatch/i);
  });

  test('Given OCCT hidden-line extraction fails When preview hull runs Then raw FreeCAD projection error remains visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'ok', 'error');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, JSON.stringify(threeViewSketchDocument('hidden-line-fail'), null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expect(page.getByText('OCCT HIDDEN-LINE PROJECTION', { exact: true })).toBeVisible();
    await expect(page.getByText(/FreeCAD runner failed/i).first()).toBeVisible();
    await expect(page.getByText(/raw hidden-line backend body: Drawing\.projectEx failed on final BRep/i).first()).toBeVisible();
  });

  test('Given BRep candidate graph backend fails When preview hull runs Then raw candidate error remains visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'ok', sketchSource, 'error');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-candidate-fail',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10, 20],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-top',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-candidate-fail',
              kind: 'polyline',
              points: [
                [10, 5],
                [60, 5],
                [60, 27],
                [10, 27],
                [10, 5],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));
    await ensureSketchPreviewRequested(page, 0);

    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText('BREP CANDIDATE GRAPH', { exact: true })).toBeVisible();
    await expect(page.getByText(/candidate graph failed/i).first()).toBeVisible();
    await expect(page.getByText(/raw candidate backend body: projection edge mismatch/i).first()).toBeVisible();
  });

  test('Given Front and Top closed profiles disagree on width When import applies Then auto snap repairs width before backend preview', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-wide',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10, 20],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-top',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-narrow',
              kind: 'polyline',
              points: [
                [10, 10],
                [50, 10],
                [50, 32],
                [10, 32],
                [10, 10],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByLabel('Source patch ledger')).toContainText('AUTO SNAP');
    await expect(page.getByLabel('Source patch ledger')).toContainText('TOP X 40MM -> 50MM');
    const ledger = validationLedgerPanel(page);
    await expect(validationLedgerRow(ledger, 'SOURCE PATCH EVIDENCE')).toContainText('AUTO SNAP');
    await expect(validationLedgerRow(ledger, 'SOURCE PATCH EVIDENCE')).toContainText('TOP X 40MM -> 50MM');
    await ensureSketchPreviewRequested(page, 0);
  });

  test('Given Front and Top closed profiles disagree on x range When import applies Then auto snap translates Top before backend preview', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-wide',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10, 20],
              ],
              closed: true,
            },
          ],
        },
        {
          sketchId: 'sketch-top',
          view: 'top',
          primitives: [
            {
              primitiveId: 'primitive-top-shifted',
              kind: 'polyline',
              points: [
                [30, 10],
                [80, 10],
                [80, 32],
                [30, 32],
                [30, 10],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByLabel('Source patch ledger')).toContainText('AUTO SNAP');
    await expect(page.getByLabel('Source patch ledger')).toContainText('TOP X RANGE 30..80MM -> 10..60MM');
    const ledger = validationLedgerPanel(page);
    await expect(validationLedgerRow(ledger, 'SOURCE PATCH EVIDENCE')).toContainText('TOP X RANGE 30..80MM -> 10..60MM');
    await ensureSketchPreviewRequested(page, 0);

    const request = await lastSketchPreviewHullRequest(page);
    expect(request?.document?.sketches?.[1]?.primitives?.[0]?.points).toEqual([
      [10, 10],
      [60, 10],
      [60, 32],
      [10, 32],
      [10, 10],
    ]);
    expect(request?.fallbackDepth).toBe(22);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given SketchDocument import has tiny endpoint gap When import applies Then auto snap closes profile before backend preview', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-gap',
              kind: 'polyline',
              points: [
                [10, 20],
                [60, 20],
                [60, 50],
                [10, 50],
                [10.35, 20.25],
              ],
              closed: false,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByLabel('Source patch ledger')).toContainText('AUTO SNAP');
    await expect(page.getByLabel('Source patch ledger')).toContainText('primitive-front-gap');
    await expect(page.getByLabel('Source patch ledger')).toContainText('closed endpoint gap');
    await ensureSketchPreviewRequested(page, 0);
  });

  test('Given SketchDocument JSON has a dimension constraint value that does not match the primitive width When import applies Then exact local validation appears and no backend call is made', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-46',
              kind: 'polyline',
              points: [
                [12, 18],
                [58, 18],
                [58, 49],
                [12, 49],
                [12, 18],
              ],
              closed: true,
            },
          ],
          constraints: [
            { constraintId: 'primitive-front-46-closed', kind: 'closed', targetIds: ['primitive-front-46'] },
            {
              constraintId: 'primitive-front-46-width-dimension',
              kind: 'dimension',
              targetIds: ['primitive-front-46'],
              value: 99,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toContainText('width dimension expected 99mm but measured 46mm');
    await page.waitForTimeout(1000);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given SketchDocument JSON has a tiny dimension constraint mismatch When import applies Then auto snap fits primitive geometry before preview', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-tiny-dimension',
              kind: 'polyline',
              points: [
                [12, 18],
                [58, 18],
                [58, 49],
                [12, 49],
                [12, 18],
              ],
              closed: true,
            },
          ],
          constraints: [
            {
              constraintId: 'primitive-front-tiny-dimension-width-dimension',
              kind: 'dimension',
              targetIds: ['primitive-front-tiny-dimension'],
              value: 46.4,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByLabel('Source patch ledger')).toContainText('AUTO SNAP');
    await expect(page.getByLabel('Source patch ledger')).toContainText('width dimension 46mm -> 46.4mm');
    const ledger = validationLedgerPanel(page);
    await expect(validationLedgerRow(ledger, 'SOURCE PATCH EVIDENCE')).toContainText('width dimension 46mm -> 46.4mm');
    await ensureSketchPreviewRequested(page, 0);

    const request = await lastSketchDraftRequest(page);
    expect(request?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-tiny-dimension');
    expect(request?.sketch?.primitives?.[0]?.points).toEqual([
      [11.8, 18],
      [58.2, 18],
      [58.2, 49],
      [11.8, 49],
      [11.8, 18],
    ]);
    expect(request?.sketch?.constraints).toContainEqual({
      constraintId: 'primitive-front-tiny-dimension-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-tiny-dimension'],
      value: 46.4,
    });
  });

  test('Given SketchDocument JSON has a stale dimension constraint When REPAIR IMPORT is clicked Then source constraint value is corrected and preview runs', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-stale',
              kind: 'polyline',
              points: [
                [12, 18],
                [58, 18],
                [58, 49],
                [12, 49],
                [12, 18],
              ],
              closed: true,
            },
          ],
          constraints: [
            { constraintId: 'primitive-front-stale-closed', kind: 'closed', targetIds: ['primitive-front-stale'] },
            {
              constraintId: 'primitive-front-stale-width-dimension',
              kind: 'dimension',
              targetIds: ['primitive-front-stale'],
              value: 99,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toContainText('width dimension expected 99mm but measured 46mm');
    const repairControl = page.getByRole('button', { name: /^REPAIR IMPORT$/i });
    await expect(repairControl, 'REPAIR IMPORT control required for stale dimension constraint').toBeVisible();
    await expect(page.getByText(/REPAIR AVAILABLE[\s\S]*width[\s\S]*99mm[\s\S]*46mm/i).first()).toBeVisible();

    await repairControl.click();
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length > 0, { timeout: 5000 });

    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByText(/primitive-front-stale \/ front \/ closed/)).toBeVisible();
    await expectWorkspacePreviewEvidence(page);

    const request = await lastSketchDraftRequest(page);
    expect(request?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-stale');
    expect(request?.sketch?.primitives?.[0]?.points).toEqual([
      [12, 18],
      [58, 18],
      [58, 49],
      [12, 49],
      [12, 18],
    ]);
    expect(request?.sketch?.constraints).toContainEqual({
      constraintId: 'primitive-front-stale-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-stale'],
      value: 46,
    });

    const sourceDocument = await sketchDocumentSourceDocument(page);
    expect(sourceDocument?.sketches?.[0]?.constraints).toContainEqual({
      constraintId: 'primitive-front-stale-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-stale'],
      value: 46,
    });
  });

  test('Given stale dimension import is repaired When source patch ledger renders Then repair patch evidence stays visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-repair-ledger',
              kind: 'polyline',
              points: [
                [12, 18],
                [58, 18],
                [58, 49],
                [12, 49],
                [12, 18],
              ],
              closed: true,
            },
          ],
          constraints: [
            { constraintId: 'primitive-front-repair-ledger-closed', kind: 'closed', targetIds: ['primitive-front-repair-ledger'] },
            {
              constraintId: 'primitive-front-repair-ledger-width-dimension',
              kind: 'dimension',
              targetIds: ['primitive-front-repair-ledger'],
              value: 99,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));
    await page.getByRole('button', { name: /^REPAIR IMPORT$/i }).click();
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length > 0, { timeout: 5000 });

    const ledger = sourcePatchLedgerPanel(page);
    await expect(ledger, 'SOURCE PATCH LEDGER required after import repair').toBeVisible();
    await expect(ledger).toContainText(/SOURCE PATCH LEDGER/i);
    await expect(ledger).toContainText(/REPAIR IMPORT/i);
    await expect(ledger).toContainText(/primitive-front-repair-ledger/i);
    await expect(ledger).toContainText(/99mm -> 46mm/i);
  });

  test('Given ecky source with SketchDocument envelope is pasted into import editor When import applies Then source-map sketch becomes editable', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const envelopeDocument = closedFrontSketchDocument(
      [
        [21, 24],
        [64, 24],
        [64, 52],
        [21, 52],
        [21, 24],
      ],
      'primitive-front-91',
    );

    await importSketchDocumentJson(page, sketchSourceWithEnvelope(envelopeDocument));

    await expect(page.getByText(/primitive-front-91 \/ front \/ closed/)).toBeVisible();
    await ensureSketchPreviewRequested(page, 0);

    const request = await lastSketchDraftRequest(page);
    expect(request?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-91');
    expect(request?.sketch?.primitives?.[0]?.points).toEqual([
      [21, 24],
      [64, 24],
      [64, 52],
      [21, 52],
      [21, 24],
    ]);
  });

  test('Given ecky source without SketchDocument envelope is pasted into import editor When import applies Then raw marker error appears and preview stays local', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, '(model (part body (box 1 1 1)))');

    await expect(page.getByRole('alert')).toContainText('Sketch document marker missing.');
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given invalid SketchDocument JSON is pasted into import editor When import applies Then raw validation text appears and preview stays local', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await importSketchDocumentJson(page, '{"documentId":"broken"');

    await expect(page.getByRole('alert')).toContainText('Sketch document JSON is invalid: Unexpected end of JSON input');
    await expect(page.getByText(/primitive-front-77 \/ front \/ closed/)).toHaveCount(0);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given circle primitive without radius is pasted into import editor When import applies Then raw validation text appears and preview stays local', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    const importedDocument = {
      documentId: 'workspace-sketch-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-77',
              kind: 'circle',
              points: [[12, 18]],
              closed: true,
            },
          ],
        },
      ],
    };

    await importSketchDocumentJson(page, JSON.stringify(importedDocument, null, 2));

    await expect(page.getByRole('alert')).toContainText("sketch 'sketch-front' primitive 'primitive-front-77' has invalid radius.");
    await expect(page.getByText(/primitive-front-77 \/ front \/ closed/)).toHaveCount(0);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given backend draft source carries SketchDocument envelope When replay runs Then source-map sketch becomes the editable profile', async ({
    page,
  }) => {
    const envelopeDocument = {
      documentId: 'source-map-document',
      activeSketchId: 'sketch-front',
      units: 'mm',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-88',
              kind: 'polyline',
              points: [
                [18, 22],
                [66, 22],
                [66, 54],
                [18, 54],
                [18, 22],
              ],
              closed: true,
            },
          ],
        },
      ],
    };

    await installSketchMocks(page, 'ok', 'ok', sketchSourceWithEnvelope(envelopeDocument));
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1, undefined, { timeout: 5000 });
    await expect(page.getByText('SOURCE STATUS')).toBeVisible();

    const importEditor = sketchDocumentImportPanel(page).locator('textarea, [contenteditable="true"], [role="textbox"]').first();
    await expect(importEditor).toHaveValue(/"documentId": "source-map-document"/);
    await expect(importEditor).toHaveValue(/"primitiveId": "primitive-front-88"/);

    await page.getByRole('button', { name: 'CLEAR' }).click();
    await expect(page.getByLabel('Sketch primitives').getByText('NO PROFILE')).toBeVisible();

    const callsBeforeReplay = await sketchPreviewRequestCount(page);
    await sketchDocumentReplayControl(page).click();

    await ensureSketchPreviewRequested(page, callsBeforeReplay);
    await expect(page.getByText(/primitive-front-88 \/ front \/ closed/)).toBeVisible();

    const replayRequest = await lastSketchDraftRequest(page);
    expect(replayRequest?.sketch?.primitives?.[0]?.primitiveId).toBe('primitive-front-88');
    expect(replayRequest?.sketch?.primitives?.[0]?.points).toEqual([
      [18, 22],
      [66, 22],
      [66, 54],
      [18, 54],
      [18, 22],
    ]);
  });

  test('Given deterministic extrusion suggestion is visible When accepted Then preview request uses suggestion and source stays in Sketch Workspace', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);

    await page.waitForFunction(() => (window as any).__SKETCH_SUGGESTION_CALLS__.length >= 1, undefined, { timeout: 5000 });
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);
    await extrude12SuggestionPanel(page);

    const callsBeforeAccept = await sketchDraftCallCount(page);
    await acceptExtrude12Suggestion(page);

    await page.waitForFunction((previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount, callsBeforeAccept);
    const acceptedRequest = await lastSketchDraftRequest(page);

    expectDeepFieldValue(acceptedRequest, 'operation', 'extrude');
    expectDeepFieldValue(acceptedRequest, 'amount', 12);
    expectDeepFieldValue(acceptedRequest, 'partId', 'sketch-draft-part');
    expectDeepFieldValue(acceptedRequest, 'sketchId', 'sketch-front');
    expectDeepFieldValue(acceptedRequest, 'primitiveId', 'primitive-front-1');
    expectDeepFieldValue(acceptedRequest, 'view', 'front');

    await expect(page.getByText('SOURCE STATUS')).toBeVisible();
    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeVisible();

    await expectSketchSourceAvailable(page);
  });

  test('Given deterministic extrusion suggestion is visible When accepted preview fails Then raw backend error appears', async ({ page }) => {
    await installSketchMocks(page, 'accept-error');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);

    await page.waitForFunction(() => (window as any).__SKETCH_SUGGESTION_CALLS__.length >= 1, undefined, { timeout: 5000 });
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);
    await extrude12SuggestionPanel(page);

    const callsBeforeAccept = await sketchDraftCallCount(page);
    await acceptExtrude12Suggestion(page);

    await page.waitForFunction((previousCount) => (window as any).__SKETCH_DRAFT_CALLS__.length > previousCount, callsBeforeAccept);

    await expect(page.getByRole('alert')).toContainText('accepted suggestion preview failed');
    await expect(page.getByRole('alert')).toContainText('raw sketch backend body: deterministic accepted extrude unavailable');
  });

  test('Given sketch suggestion backend fails When closed Front profile previews Then raw suggestion error appears and preview remains available', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok', 'error');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);

    await page.waitForFunction(() => (window as any).__SKETCH_SUGGESTION_CALLS__.length >= 1, undefined, { timeout: 5000 });

    await expect(page.getByLabel('Suggested features').getByRole('alert')).toContainText('suggestion failed');
    await expect(page.getByLabel('Suggested features').getByRole('alert')).toContainText(
      'raw suggestion backend body: deterministic feature service unavailable',
    );
    await expect(page.getByText('SOURCE STATUS')).toBeVisible();
    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeVisible();
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBeGreaterThan(0);
  });

  test('Given open Front profile is drawn When no manual preview click happens Then auto preview stays local and coach validation remains visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawOpenStroke(page);
    await expect(page.getByText(/primitive-front-1 \/ front \/ open/)).toBeVisible();

    await page.waitForTimeout(1000);

    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
    await expect(page.getByText('SOURCE STATUS')).toHaveCount(0);
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeHidden();
  });

  test('Given open Front profile is drawn When SketchDocument evidence is inspected Then closed document is not shown as accepted', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await drawOpenStroke(page);
    await expect(page.getByText(/primitive-front-1 \/ front \/ open/)).toBeVisible();

    await page.waitForTimeout(1000);
    await openSketchDocumentEvidenceIfCollapsed(page);

    const closedDocumentEvidence = sketchDocumentEvidencePanels(page)
      .filter({ hasText: /workspace-sketch-document/ })
      .filter({ hasText: /sketch-front/ })
      .filter({ hasText: /"closed"\s*:\s*true/ });
    const acceptedDocumentEvidence = sketchDocumentEvidencePanels(page).filter({ hasText: /\bACCEPTED\b|"accepted"\s*:\s*true/i });

    await expect(closedDocumentEvidence, 'open profile must not expose closed SketchDocument evidence').toHaveCount(0);
    await expect(acceptedDocumentEvidence, 'open profile must not expose accepted SketchDocument evidence').toHaveCount(0);
  });

  test('Given seed geometry When draft generated Then compact preview evidence appears from backend', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await expect(page.locator('[aria-label="Front sketch pane"]').locator('.sketch-pane__label')).toContainText('FRONT');
    await expect(page.locator('[aria-label="Top sketch pane"]').locator('.sketch-pane__label')).toContainText('TOP');
    await expect(page.locator('[aria-label="Side sketch pane"]').locator('.sketch-pane__label')).toContainText('SIDE');

    await generateSketchPreview(page);

    await expectWorkspacePreviewEvidence(page);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.view)).resolves.toBe('front');
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.length)).resolves.toBe(1);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.primitiveId)).resolves.not.toBe(
      'seed-rectangle',
    );
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.closed)).resolves.toBe(true);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.points?.length)).resolves.toBeGreaterThan(
      3,
    );
  });

  test('Given closed Front profile previews When validation ledger renders Then pass rows show preview evidence', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();

    for (const row of ['CLOSED PROFILE', 'SOURCE GENERATED', 'MESH PREVIEW', 'PROJECTIONS']) {
      await expectValidationLedgerPassRow(ledger, row);
    }

    await expect(ledger).toContainText(/preview\.stl|1 assets|sketch-seed-part|part\.stl/i);
  });

  test('Given successful closed sketch preview When validation ledger renders Then sketch contract and preview artifact rows pass with concrete evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();

    const sketchContract = validationLedgerRow(ledger, 'SKETCH CONTRACT');
    await expect(sketchContract, 'SKETCH CONTRACT needs visible validation ledger row').toBeVisible();
    await expect(sketchContract).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    await expect(sketchContract).toContainText(/front/i);
    await expect(sketchContract).toContainText(/points?/i);
    await expect(sketchContract).toContainText(/12\s*mm|depth\s*12/i);

    const previewArtifact = validationLedgerRow(ledger, 'PREVIEW ARTIFACT');
    await expect(previewArtifact, 'PREVIEW ARTIFACT needs visible validation ledger row').toBeVisible();
    await expect(previewArtifact).toContainText(/\b(PASS|PASSED|OK)\b|✓/i);
    await expect(previewArtifact).toContainText(/preview\.stl/i);
    await expect(previewArtifact).toContainText(/assets?|part\.stl|sketch-seed-part/i);
  });

  test('Given closed Front profile preview succeeds When validation ledger renders Then SOURCE FIT CHECK shows containment tolerance pass without BRep claim', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();

    const sourceFitCheck = validationLedgerRow(ledger, 'SOURCE FIT CHECK');
    await expect(sourceFitCheck, 'SOURCE FIT CHECK needs visible validation row after closed preview').toBeVisible();
    await expect(sourceFitCheck).toContainText(/\bPASS\b/i);
    await expect(sourceFitCheck).toContainText(/CONTAINMENT/i);
    await expect(sourceFitCheck).toContainText(/TOLERANCE/i);
    await expect(sourceFitCheck, 'source fit check must not claim full BRep validation').not.toContainText(/BRep|boundary representation/i);
  });

  test('Given closed Front profile preview succeeds When source fit report renders Then containment tolerance and preview artifact evidence are visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const report = page.getByLabel('Source fit report');
    await expect(report, 'SOURCE FIT REPORT needs visible source-backed fit evidence').toBeVisible();
    await expect(report).toContainText(/SOURCE FIT REPORT/i);
    await expect(report).toContainText(/SOURCE-BACKED/i);
    await expect(report).toContainText(/CONTAINMENT[\s\S]*PASS/i);
    await expect(report).toContainText(/TOLERANCE[\s\S]*PASS/i);
    await expect(report).toContainText(/PREVIEW ARTIFACT[\s\S]*PASS/i);
    await expect(report, 'source fit report must not claim full BRep validation').not.toContainText(/BRep|boundary representation/i);
  });

  test('Given closed profile preview succeeds When Sketch workspace shows dimensions Then constraint readout includes width height depth and closed profile evidence', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const dimensions = page.getByLabel('Dimensions and constraints');

    await expect(dimensions, 'DIMENSIONS/CONSTRAINTS readout required after closed profile preview').toBeVisible();
    await expect(dimensions).toContainText(/DIMENSIONS\/CONSTRAINTS/i);
    await expect(dimensions).toContainText(/WIDTH/i);
    await expect(dimensions).toContainText(/HEIGHT/i);
    await expect(dimensions).toContainText(/DEPTH\s*12\s*MM/i);
    await expect(dimensions).toContainText(/CLOSED PROFILE/i);
    await expect(dimensions).toContainText(/CONSTRAINT/i);
  });

  test('Given backend rejects closed sketch preview When validation ledger renders Then contract and preview artifact rows are not pass and raw error remains', async ({
    page,
  }) => {
    await installSketchMocks(page, 'error');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expect(page.getByRole('alert')).toContainText('draft generation failed');
    await expect(page.getByRole('alert')).toContainText('raw sketch backend body: missing closed profile');

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();
    await expectValidationLedgerNoPassRow(ledger, 'SKETCH CONTRACT');
    await expectValidationLedgerNoPassRow(ledger, 'PREVIEW ARTIFACT');
    await expect(ledger).toContainText('raw sketch backend body: missing closed profile');
  });

  test('Given backend preview fails When source fit validation renders Then SOURCE FIT CHECK is not pass and raw backend details stay visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'error');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expect(page.getByRole('alert')).toContainText('draft generation failed');
    await expect(page.getByRole('alert')).toContainText('raw sketch backend body: missing closed profile');

    const ledger = validationLedgerPanel(page);
    await expect(ledger).toBeVisible();
    const sourceFitCheck = validationLedgerRow(ledger, 'SOURCE FIT CHECK');
    await expect(sourceFitCheck, 'SOURCE FIT CHECK needs visible failure row when backend preview fails').toBeVisible();
    await expect(sourceFitCheck).toContainText(/raw sketch backend body: missing closed profile/);
    await expectValidationLedgerNoPassRow(ledger, 'SOURCE FIT CHECK');
  });

  test('Given backend preview fails When source fit report renders Then preview artifact failure keeps raw backend detail visible', async ({
    page,
  }) => {
    await installSketchMocks(page, 'error');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expect(page.getByRole('alert')).toContainText('raw sketch backend body: missing closed profile');

    const report = page.getByLabel('Source fit report');
    await expect(report, 'SOURCE FIT REPORT needs visible raw failure evidence').toBeVisible();
    await expect(report).toContainText(/SOURCE FIT REPORT/i);
    await expect(report).toContainText(/PREVIEW ARTIFACT[\s\S]*(FAIL|FAILED)/i);
    await expect(report).toContainText(/raw sketch backend body: missing closed profile/);
  });

  test('Given closed sketch preview succeeds When main model viewport receives the handoff Then generated artifact evidence and ecky source are available', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page, { reopenSketch: false });

    const viewport = page.locator('.viewport-area');
    await expect(viewport, 'main model viewport required for sketch preview handoff').toBeVisible();
    await expect(viewport.getByLabel('Sketch preview status')).toContainText(/NOT ACCEPTED CAD/i);
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    await page.getByRole('button', { name: 'SKETCH', exact: true }).click();
    await expectWorkspacePreviewEvidence(page);
    await expectSketchSourceAvailable(page);
  });

  test('Given closed sketch preview succeeds When main model viewport renders Then no source silhouette appears and preview status is explicit', async ({
    page,
  }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page, { reopenSketch: false });

    const viewport = mainModelViewport(page);
    await expect(viewport, 'main model viewport required for source silhouette overlay evidence').toBeVisible();

    await expect(viewport.getByLabel('Sketch preview status')).toContainText(/SKETCH PREVIEW/i);
    await expect(viewport.getByLabel('Sketch preview status')).toContainText(/NOT ACCEPTED CAD/i);
    await expect(viewport.getByLabel('Source silhouette overlay')).toHaveCount(0);
    await expect(viewport).not.toContainText(/SOURCE SILHOUETTE|SKETCH DRAFT ONLY/i);
  });

  test('Given user is drawing an open Front profile When backend preview does not exist Then main model viewport shows no sketch overlay', async ({
    page,
  }) => {
    await installSketchMocks(page, 'delay');
    await openSketchWorkspace(page);

    const pane = page.locator('[aria-label="Front sketch pane"]');
    await expect(pane).toBeVisible();
    const box = await pane.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;

    const viewport = mainModelViewport(page);
    await expect(viewport, 'sketch mode replaces the model viewport while drawing').toHaveCount(0);

    await page.mouse.move(box.x + box.width * 0.2, box.y + box.height * 0.4);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.65, box.y + box.height * 0.55, { steps: 5 });

    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
    await expect(page.getByLabel('Local sketch ghost')).toHaveCount(0);

    await page.mouse.up();

    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given closed Front profile preview is delayed When backend work is queued Then main model viewport keeps sketch overlays absent', async ({
    page,
  }) => {
    await installSketchMocks(page, 'delay');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();
    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);

    await expect(mainModelViewport(page), 'sketch mode replaces the model viewport while preview is queued').toHaveCount(0);
    await expect(page.getByLabel('Sketch preview status')).toHaveCount(0);
    await expect(page.getByLabel('Local sketch ghost')).toHaveCount(0);

    const viewport = mainModelViewport(page);
    await expect(viewport, 'manual preview success returns to model viewport').toBeVisible();
    await expect(viewport.getByLabel('Sketch preview status')).toContainText(/NOT ACCEPTED CAD/i);
    await expect(viewport.getByLabel('Local sketch ghost')).toHaveCount(0);
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  });

  test('Given closed profile preview exists When learning lens appears Then extrusion math and source remain available', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const learningLens = page.getByLabel('Learning lens');
    await expect(learningLens).toBeVisible();
    await expect(learningLens).toContainText(/LEARNING LENS|MATH LENS/i);
    await expect(learningLens).toContainText(/extrud/i);
    await expect(learningLens).toContainText(/\(x,\s*y\)\s*->\s*\(x,\s*y,\s*z\)/);
    await expect(learningLens).toContainText(/0\s*(<=|≤)\s*z\s*(<=|≤)\s*12/);
    await expect(learningLens).toContainText(/EXTRUDE\s*12\s*MM/i);

    await expectSketchSourceAvailable(page);
  });

  test('Given closed Front profile When preview generated Then projection evidence appears and source stays in Sketch Workspace', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    const projectionsPanel = page
      .locator('section, aside, div')
      .filter({ has: page.getByText('PROJECTIONS', { exact: true }) })
      .filter({ has: page.getByText('FRONT', { exact: true }) })
      .filter({ has: page.getByText('TOP', { exact: true }) })
      .filter({ has: page.getByText('SIDE', { exact: true }) })
      .first();

    await expect(projectionsPanel).toBeVisible();
    await expect(projectionsPanel).toContainText(/FRONT[\s\S]*(SOURCE SKETCH|AUTHORING)/);
    await expect(projectionsPanel).toContainText(/TOP[\s\S]*(DERIVED|EXTRUDE DEPTH)/);
    await expect(projectionsPanel).toContainText(/SIDE[\s\S]*(DERIVED|EXTRUDE DEPTH)/);

    await expectSketchSourceAvailable(page);
  });

  test('Given sketch preview exists Then generated ecky source stays available inside Sketch Workspace', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);
    await expectSketchSourceAvailable(page);
  });

  test('Given sketch preview path is long When draft generated Then inspector shows compact preview evidence only', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);

    await generateSketchPreview(page);

    await expectWorkspacePreviewEvidence(page);
    await expect(page.getByText(sketchPreviewPath)).toHaveCount(0);
  });

  test('Given backend rejects sketch When draft generated Then raw backend error appears', async ({ page }) => {
    await installSketchMocks(page, 'error');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expect(page.getByRole('alert')).toContainText('draft generation failed');
    await expect(page.getByRole('alert')).toContainText('raw sketch backend body: missing closed profile');
    await expectDraftFailureValidationLedgerIfPresent(page);
  });

  test('Given preview is running When sketch preview requested Then pending state is visible', async ({ page }) => {
    await installSketchMocks(page, 'delay');
    await openSketchWorkspace(page);
    await drawClosedRectangle(page);

    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expect(page.getByRole('button', { name: 'GENERATING...' })).toBeDisabled();
    await expect(page.getByText('GENERATING...')).toBeVisible();
    await expectWorkspacePreviewEvidence(page);
  });

  test('Given closed Front profile is drawn When preview queues Then projections appear before preview resolves', async ({ page }) => {
    await installSketchMocks(page, 'delay');
    await openSketchWorkspace(page);

    await drawClosedRectangle(page);
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await page.waitForFunction(() => (window as any).__SKETCH_DRAFT_CALLS__.length >= 1);

    const projectionsPanel = page
      .locator('section, aside, article, div')
      .filter({ has: page.getByText('PROJECTIONS', { exact: true }) })
      .first();

    await expect(page.getByText('GENERATING...')).toBeVisible();
    await expect(projectionsPanel).toBeVisible();
    await expect(projectionsPanel).toContainText('FRONT');
    await expect(projectionsPanel).toContainText('TOP');
    await expect(projectionsPanel).toContainText('SIDE');
  });

  test('Given profile is open When preview requested Then local validation blocks backend call', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await drawOpenStroke(page);

    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expect(page.getByRole('alert')).toContainText('Close profile before preview.');
    await expect(page.getByText('PROJECTIONS', { exact: true })).toBeHidden();
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__.length)).resolves.toBe(0);
  });

  test('Given open sketch loops When close open is clicked Then preview uses closed user profile', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await drawOpenStroke(page);

    await page.getByRole('button', { name: 'CLOSE OPEN' }).click();
    await expect(page.getByText(/primitive-front-1 \/ front \/ closed/)).toBeVisible();
    await page.getByRole('button', { name: /PREVIEW NOW|GENERATE DRAFT/ }).click();

    await expectWorkspacePreviewEvidence(page);
    await expect(page.evaluate(() => (window as any).__SKETCH_DRAFT_CALLS__[0]?.sketch?.primitives?.[0]?.closed)).resolves.toBe(true);
  });

  test('Given messy sketch When clear clicked Then primitives reset', async ({ page }) => {
    await installSketchMocks(page, 'ok');
    await openSketchWorkspace(page);
    await drawOpenStroke(page);

    await expect(page.getByText(/primitive-front-1/)).toBeVisible();
    await page.getByRole('button', { name: 'CLEAR' }).click();

    await expect(page.getByLabel('Sketch primitives').getByText('NO PROFILE')).toBeVisible();
    await expect(page.getByText(/primitive-front-1/)).toHaveCount(0);
  });
});
