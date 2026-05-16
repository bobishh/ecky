<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import SketchInspectorSection from './SketchInspectorSection.svelte';
  import {
    acceptSketchBrepCandidateSolution,
    acceptedBrepCandidateToComponentPackage,
    analyzeSketchBrepCandidates,
    extractBrepHiddenLineProjections,
    formatBackendError,
    generateSketchDraftPreview,
    generateSketchPreviewHull,
    suggestSketchFeatures,
  } from './tauri/client';
  import type {
    ComponentPackage,
    ComponentPort,
    PortTypeDefinition,
    SketchDraftRequest,
    SketchDraftSource,
    BrepHiddenLineProjectionResponse,
    BrepHiddenLineProjectionView,
    BrepProjectedEdge2d,
    SketchBrepCandidateResponse,
    SketchDocument,
    SketchFeatureSuggestion,
    SketchSuggestionRequest,
    SketchSuggestionResponse,
    SketchView,
  } from './tauri/contracts';
  import {
    basename,
    buildSketchDraftRequest,
    clientPointToSvgPoint,
    closeStroke,
    finishStroke,
    pointsToSvg,
    sourceLineCount,
    strokeKind,
    summarizeSketchDraftMode,
    type SketchPoint,
    type SketchStroke,
  } from './sketchWorkspaceState';
  import { buildSketchLearningLens } from './sketchLearningLens';
  import { buildSketchProjections, type SketchProjection, type SketchProjectionBounds } from './sketchProjection';
  import { summarizeSketchPreviewStep, type SketchPreviewStepState } from './sketchAutoPreview';
  import {
    formatSketchDocumentSource,
    sketchDocumentSummary as buildSketchDocumentSourceSummary,
  } from './sketchDocumentSource';
  import { parseSketchDocumentEnvelope } from './sketchDocumentEnvelope';
  import { buildSketchSuggestionRequest } from './sketchSuggestionDocument';
  import { buildDraftRequestFromSuggestion } from './sketchSuggestionAccept';
  import { nextPrimitiveSequenceFromStrokes, parseSketchDocumentImportSource, sketchDocumentToStrokes } from './sketchDocumentReplay';
  import {
    assertLockedDimensionsPreserved,
    closedStrokeBounds,
    deleteClosedStrokePoint,
    editablePointIndices,
    moveClosedStrokePointWithDimensionLocks,
    normalizeSketchCoordinate,
    normalizeSketchGridSize,
    resizeClosedStrokeBounds,
    resizeClosedStrokeBoundsSnapped,
    setClosedStrokeBoundsOrigin,
    setClosedStrokeBoundsOriginSnapped,
    snapPointToGrid,
  } from './sketchEditState';
  import { summarizeSketchGhostPreview, type SketchGhostPreviewState } from './sketchGhostPreview';
  import { buildSketchDimensionSummary } from './sketchDimensionSummary';
  import { buildSketchValidationRows, type SketchValidationRow } from './sketchValidationLedger';
  import { buildSketchFitValidationSeed } from './sketchFitValidation';
  import {
    buildSketchBrepProjectionRepairTargets,
    buildSketchBrepProjectionValidationSummary,
  } from './sketchBrepProjectionValidation';
  import { findSketchIssueMatch } from './sketchIssueLocator';
  import { autoRepairSketchDocumentFromBrepProjection } from './sketchBrepAutoRepair';
  import { buildSketchDocumentFromBrepProjection } from './sketchBrepDerivedSketch';
  import {
    applySketchTopologyRepairProposal,
    buildSketchTopologyRepairProposals,
    type SketchTopologyRepairProposal,
  } from './sketchTopologyRepairProposal';
  import { buildSketchAcceptedCadRow } from './sketchAcceptedCad';
  import { summarizeSketchValidationIssue, summarizeSketchValidationIssues } from './sketchValidationIssueSummary';
  import {
    brepHiddenLineViewHasWarning,
    brepHiddenLineWarningMessages,
  } from './sketchHiddenLineWarnings';
  import { cleanupSketchStrokes } from './sketchCleanup';
  import { repairSketchDocumentEndpointGaps } from './sketchEndpointRepair';
  import { autoRepairOrthographicSketchStrokes } from './sketchOrthographicRepair';
  import {
    autoRepairSketchDocumentDimensionConstraintGeometry,
    repairSketchDocumentDimensionConstraints,
  } from './sketchConstraintValidation';
  import { buildSketchPreviewHullRequest, shouldUseSketchPreviewHull } from './sketchPreviewHull';
  import {
    appendSketchSourcePatch,
    compactRepairDetail,
    type SketchSourcePatchEntry,
  } from './sketchSourcePatchLedger';
  import {
    buildSketchWorkspaceSceneState,
    sceneSignatureFromStrokes,
    workspaceSceneActionLabel,
    type SketchWorkspaceLens,
  } from './sketchWorkspaceScene';
  import type { ArtifactBundle } from './types/domain';

  type PreviewResult = { draft: SketchDraftSource; artifactBundle: ArtifactBundle } | null;
  type ProjectionRect = { x: number; y: number; width: number; height: number };
  type PreviewMode = 'manual' | 'auto';
  type GenerateDraftOptions = { preserveBrepAutoRepairAttempts?: boolean };
  type PointDragState = {
    primitiveId: string;
    pointIndex: number;
    pointerId: number;
    view: SketchView;
    originalPoint: SketchPoint;
    startClientX: number;
    startClientY: number;
    moved: boolean;
  };
  type SelectedPointState = {
    primitiveId: string;
    pointIndex: number;
    view: SketchView;
  };
  type SketchTool = 'select' | 'polyline' | 'rectangle' | 'circle';
  type ShapeDraftState = {
    kind: 'rectangle' | 'circle';
    primitiveId: string;
    sketchId: string;
    view: SketchView;
    pointerId: number;
    start: SketchPoint;
    current: SketchPoint;
  };
  type PaneCamera = {
    zoom: number;
    panX: number;
    panY: number;
  };
  type PanePanState = {
    view: SketchView;
    pointerId: number;
    origin: SketchPoint;
    startCamera: PaneCamera;
  };

  const EXTRUDE_AMOUNT = 12;
  const AUTO_PREVIEW_DEBOUNCE_MS = 650;
  const DEFAULT_SNAP_GRID_SIZE = '10';
  const DEFAULT_PANE_ZOOM = 1;
  const POINT_DRAG_THRESHOLD_PX = 6;
  const POINT_HANDLE_RADIUS = 1.1;
  const ACCEPTED_BREP_COMPONENT_ID = 'sketch-preview-hull';
  const ACCEPTED_BREP_PACKAGE_ID = 'sketch-preview-hull.accepted-brep';
  const ACCEPTED_BREP_PORT_ID = 'front_mount';
  const ACCEPTED_BREP_PORT_TYPE_ID = 'mechanical.plane.mount.v1';

  let {
    onPreviewResult = null,
    onManualPreviewResult = null,
    onGhostPreviewChange = null,
    onClose = null,
  }: {
    onPreviewResult?: ((result: PreviewResult) => void) | null;
    onManualPreviewResult?: ((result: PreviewResult) => void) | null;
    onGhostPreviewChange?: ((result: SketchGhostPreviewState | null) => void) | null;
    onClose?: (() => void) | null;
  } = $props();
  const dispatch = createEventDispatcher<{ previewResult: PreviewResult }>();

  let draft = $state<SketchDraftSource | null>(null);
  let artifactBundle = $state<ArtifactBundle | null>(null);
  let errorText = $state('');
  let generating = $state(false);
  let strokes = $state<SketchStroke[]>([]);
  let activeStroke = $state<SketchStroke | null>(null);
  let previewProfile = $state<SketchStroke | null>(null);
  let frontSvg = $state<SVGSVGElement | null>(null);
  let topSvg = $state<SVGSVGElement | null>(null);
  let sideSvg = $state<SVGSVGElement | null>(null);
  let autoQueued = $state(false);
  let autoPreviewPrimitiveId = $state<string | null>(null);
  let suggestionResponse = $state<SketchSuggestionResponse | null>(null);
  let suggestionErrorText = $state('');
  let suggestingFeatures = $state(false);
  let acceptingSuggestionId = $state<string | null>(null);
  let acceptedSuggestionId = $state<string | null>(null);
  let acceptedSuggestionLabel = $state('');
  let sketchDocumentSnapshot = $state<SketchDocument | null>(null);
  let sketchDocumentImportText = $state('');
  let sketchDocumentEditorDirty = $state(false);
  let pointDrag = $state<PointDragState | null>(null);
  let selectedPoint = $state<SelectedPointState | null>(null);
  let snapToGrid = $state(false);
  let sketchGridSize = $state(DEFAULT_SNAP_GRID_SIZE);
  let activeTool = $state<SketchTool>('polyline');
  let hoverView = $state<SketchView | null>(null);
  let hoverPoint = $state<SketchPoint | null>(null);
  let shapeDraft = $state<ShapeDraftState | null>(null);
  let panePan = $state<PanePanState | null>(null);
  let suppressNextPaneClick = $state(false);
  let activeLens = $state<SketchWorkspaceLens>('sketch');
  let draftSceneSignature = $state<string | null>(null);
  let acceptedExactState = $state<{ solutionId: string; sceneSignature: string | null } | null>(null);
  let paneCameras = $state<Record<SketchView, PaneCamera>>({
    front: { zoom: DEFAULT_PANE_ZOOM, panX: 0, panY: 0 },
    top: { zoom: DEFAULT_PANE_ZOOM, panX: 0, panY: 0 },
    side: { zoom: DEFAULT_PANE_ZOOM, panX: 0, panY: 0 },
    custom: { zoom: DEFAULT_PANE_ZOOM, panX: 0, panY: 0 },
  });
  let selectedPointX = $state('');
  let selectedPointY = $state('');
  let profileX = $state('');
  let profileY = $state('');
  let profileWidth = $state('');
  let profileHeight = $state('');
  let cleanupEvidenceText = $state('');
  let importRepairDocument = $state<SketchDocument | null>(null);
  let importRepairEvidenceText = $state('');
  let sourcePatchEntries = $state<SketchSourcePatchEntry[]>([]);
  let brepCandidateResponse = $state<SketchBrepCandidateResponse | null>(null);
  let brepCandidateErrorText = $state('');
  let brepCandidateLoading = $state(false);
  let brepCandidateDocument = $state<SketchDocument | null>(null);
  let brepCandidateAcceptingSolutionId = $state<string | null>(null);
  let brepCandidateAcceptedSolutionId = $state<string | null>(null);
  let brepCandidateAcceptErrorText = $state('');
  let brepCandidateAcceptEvidence = $state<string[]>([]);
  let brepComponentPackage = $state<ComponentPackage | null>(null);
  let brepComponentPackageLoading = $state(false);
  let brepComponentPackageErrorText = $state('');
  let hiddenLineResponse = $state<BrepHiddenLineProjectionResponse | null>(null);
  let hiddenLineErrorText = $state('');
  let hiddenLineLoading = $state(false);
  let brepAutoRepairAttempts = $state(0);
  const frontHiddenLineOverlay = $derived(hiddenLineResponse?.views?.find((view) => view.view === 'front') ?? null);
  const topHiddenLineOverlay = $derived(hiddenLineResponse?.views?.find((view) => view.view === 'top') ?? null);
  const sideHiddenLineOverlay = $derived(hiddenLineResponse?.views?.find((view) => view.view === 'side') ?? null);
  const brepDerivedSketch = $derived.by(() =>
    hiddenLineResponse ? buildSketchDocumentFromBrepProjection(hiddenLineResponse) : null,
  );
  let autoPreviewTimer: ReturnType<typeof setTimeout> | null = null;
  let autoPreviewRunId = 0;
  let suggestionRunId = 0;
  let primitiveSequence = 0;
  const latestClosedProfile = $derived.by(() => latestClosedStroke(strokes));
  const profileSizeTarget = $derived.by(() => selectedClosedStroke() ?? latestClosedProfile);
  const projectionProfile = $derived.by(() => previewProfile ?? latestClosedProfile);
  const draftModeSummary = $derived.by(() => summarizeSketchDraftMode(strokes));
  const draftDepth = $derived.by(() => {
    const request = buildSketchDraftRequest(strokes);
    return 'error' in request ? EXTRUDE_AMOUNT : request.amount;
  });
  const learningLens = $derived.by(() => buildSketchLearningLens(draftDepth, sourcePatchEntries));
  const projections = $derived.by(() => (projectionProfile ? buildSketchProjections(projectionProfile, draftDepth) : []));
  const dimensionSummary = $derived.by(() => (projectionProfile ? buildSketchDimensionSummary(projectionProfile, draftDepth) : null));
  const dimensionConstraintSummary = $derived.by(() => summarizeDimensionConstraints(profileSizeTarget));
  const sourceFitSeed = $derived.by(() =>
    projectionProfile
      ? buildSketchFitValidationSeed({
          profilePoints: displayStrokePoints(projectionProfile).map(([x, y]) => ({ x, y })),
          view: { width: 100, height: 100 },
          extrudeDepth: draftDepth,
          artifactEvidence: {
            ...(artifactBundle?.previewStlPath ? { previewArtifactPath: artifactBundle.previewStlPath } : {}),
            ...(draft?.source ? { source: draft.source } : {}),
          },
          ...(errorText ? { backendError: errorText } : {}),
        })
      : null,
  );
  const sketchGhostPreview = $derived.by(() =>
    summarizeSketchGhostPreview({
      activeStroke,
      strokes,
      generating,
      autoQueued,
      extrudeDepth: draftDepth,
    }),
  );
  const openProfileCount = $derived.by(() => strokes.filter((stroke) => !stroke.closed).length + (activeStroke && !activeStroke.closed ? 1 : 0));
  const closedProfileCount = $derived(strokes.filter((stroke) => stroke.closed).length);
  const validationRows = $derived.by(() =>
    buildSketchValidationRows({
      strokes,
      draft,
      artifactBundle,
      extrudeDepth: draftDepth,
      projectionsCount: projections.length,
      errorText,
      sourcePatchEntries,
    }),
  );
  const brepSketchValidationSummary = $derived.by(() =>
    sketchDocumentSource && hiddenLineResponse
      ? buildSketchBrepProjectionValidationSummary(sketchDocumentSource, hiddenLineResponse)
      : null,
  );
  const brepSketchRepairTargets = $derived.by(() =>
    sketchDocumentSource && hiddenLineResponse
      ? buildSketchBrepProjectionRepairTargets(sketchDocumentSource, hiddenLineResponse)
      : [],
  );
  const brepTopologyRepairProposals = $derived.by(() =>
    sketchDocumentSource && hiddenLineResponse
      ? buildSketchTopologyRepairProposals(sketchDocumentSource, hiddenLineResponse)
      : [],
  );
  const acceptedCadValidationRow = $derived.by(() =>
    buildSketchAcceptedCadRow({
      artifactBundle,
      hiddenLineResponse,
      hiddenLineErrorText,
      hiddenLineLoading,
    }),
  );
  const visibleValidationRows = $derived.by(() => [
    ...validationRows,
    ...(brepSketchValidationSummary ? [brepSketchValidationLedgerRow(brepSketchValidationSummary)] : []),
    ...(acceptedCadValidationRow ? [acceptedCadValidationRow] : []),
  ]);
  const sketchDocumentSource = $derived.by(() => {
    const request = buildSketchSuggestionRequest(strokes);
    if ('error' in request) return null;
    return preserveSketchDocumentEnvelope(request.document, strokes, sketchDocumentSnapshot);
  });
  const sketchDocumentSourceSummary = $derived.by(() => buildSketchDocumentSourceSummary(sketchDocumentSource));
  const sketchDocumentJson = $derived.by(() => formatSketchDocumentSource(sketchDocumentSource));
  const currentSceneSignature = $derived.by(() => sceneSignatureFromStrokes(strokes));
  const exactSceneActionSolutionId = $derived.by(() => {
    const solutions = brepCandidateResponse?.validation?.passed ? (brepCandidateResponse.search?.solutions ?? []) : [];
    return solutions.length === 1 ? solutions[0]?.solutionId ?? null : null;
  });
  const sketchDocumentStatus = $derived.by(() => {
    if (!sketchDocumentSourceSummary.error) return 'READY';
    if (openProfileCount > 0) return 'PROFILE OPEN';
    return 'WAITING';
  });
  const workspaceScene = $derived.by(() =>
    buildSketchWorkspaceSceneState({
      currentSceneSignature,
      draftSceneSignature,
      exactSceneSignature: acceptedExactState?.sceneSignature ?? null,
      hasSketch: Boolean(sketchDocumentSource),
      hasDraft: Boolean(draft),
      hasAcceptedExact: Boolean(acceptedExactState),
      hasRebuildableExact: Boolean(brepCandidateResponse?.validation?.passed && (brepCandidateResponse.search?.solutions?.length ?? 0) > 0),
      exactCandidateSolutionId: exactSceneActionSolutionId,
      draftErrorText: errorText && !draft ? errorText : '',
      exactErrorText: hiddenLineErrorText || brepCandidateAcceptErrorText || '',
      activeLens,
    }),
  );
  const draftSceneRow = $derived.by(() => workspaceScene.rows.find((row) => row.key === 'draft') ?? null);
  const exactSceneRow = $derived.by(() => workspaceScene.rows.find((row) => row.key === 'exact') ?? null);
  const featureSuggestions = $derived.by(() => suggestionResponse?.suggestions ?? []);
  const suggestionWarnings = $derived.by(() => suggestionResponse?.warnings ?? []);
  const showSuggestionPanel = $derived.by(
    () => suggestingFeatures || Boolean(suggestionErrorText) || featureSuggestions.length > 0 || suggestionWarnings.length > 0,
  );
  const previewStep = $derived.by(() =>
    summarizeSketchPreviewStep({
      hasClosedProfile: closedProfileCount > 0,
      hasDraft: Boolean(draft),
      generating,
      errorText,
      autoQueued,
    }),
  );
  const profileLedgerState = $derived<SketchPreviewStepState>(openProfileCount > 0 || closedProfileCount === 0 ? 'blocked' : 'accepted');
  const profileLedgerDetail = $derived.by(() => {
    if (openProfileCount > 0) return `${openProfileCount} open`;
    if (closedProfileCount > 0) return `${closedProfileCount} closed`;
    return 'none';
  });
  const suggestionLedgerState = $derived<SketchPreviewStepState>(
    acceptingSuggestionId
      ? 'generating'
      : acceptedSuggestionId
        ? 'accepted'
        : suggestionErrorText
          ? 'failed'
          : suggestingFeatures
            ? 'queued'
            : featureSuggestions.length > 0
              ? 'idle'
              : 'blocked',
  );
  const suggestionLedgerLabel = $derived.by(() => {
    if (acceptingSuggestionId) return 'APPLYING FEATURE';
    if (acceptedSuggestionId) return 'FEATURE APPLIED';
    if (suggestionErrorText) return 'SUGGESTION FAILED';
    if (suggestingFeatures) return 'SUGGESTING FEATURE';
    if (featureSuggestions.length > 0) return 'FEATURE READY';
    return 'NO SUGGESTION';
  });
  const suggestionLedgerDetail = $derived.by(() => {
    if (acceptingSuggestionId) return acceptingSuggestionId;
    if (acceptedSuggestionId) return acceptedSuggestionLabel || acceptedSuggestionId;
    if (suggestionErrorText) return suggestionErrorText;
    if (suggestingFeatures) return 'Suggestion request running.';
    if (featureSuggestions.length > 0) return `${featureSuggestions.length} available`;
    return 'none';
  });

  $effect(() => {
    return () => {
      clearAutoPreviewQueue();
      onGhostPreviewChange?.(null);
    };
  });

  $effect(() => {
    onGhostPreviewChange?.(sketchGhostPreview);
  });

  $effect(() => {
    if (sketchDocumentSource && !sketchDocumentSnapshot) {
      sketchDocumentSnapshot = sketchDocumentSource;
    }
  });

  $effect(() => {
    if (sketchDocumentEditorDirty) return;
    sketchDocumentImportText = sketchDocumentJson;
  });

  $effect(() => {
    if (!profileSizeTarget) {
      profileX = '';
      profileY = '';
      profileWidth = '';
      profileHeight = '';
      return;
    }

    try {
      const bounds = closedStrokeBounds(profileSizeTarget);
      profileX = formatNumber(bounds.minX);
      profileY = formatNumber(bounds.minY);
      profileWidth = formatNumber(bounds.width);
      profileHeight = formatNumber(bounds.height);
    } catch {
      profileX = '';
      profileY = '';
      profileWidth = '';
      profileHeight = '';
    }
  });

  function handlePanePointerDown(event: PointerEvent, view: SketchView) {
    if (generating) return;
    if (event.button === 1 || activeTool === 'select' || event.altKey) {
      beginPanePan(event, view);
      return;
    }
    if (activeTool === 'rectangle' || activeTool === 'circle') {
      beginShapeDraft(event, view, activeTool);
    }
  }

  function handlePanePointerMove(event: PointerEvent, view: SketchView) {
    if (pointDrag) {
      dragPoint(event);
      return;
    }
    if (panePan) {
      dragPanePan(event);
      return;
    }
    if (shapeDraft) {
      dragShapeDraft(event);
      return;
    }
    updateHoverPoint(event, view);
  }

  function handlePanePointerUp(event: PointerEvent, view: SketchView) {
    if (pointDrag) {
      endPointDrag(event);
      return;
    }
    if (panePan) {
      endPanePan(event);
      return;
    }
    if (shapeDraft) {
      endShapeDraft(event);
      return;
    }
    updateHoverPoint(event, view);
  }

  function handlePaneClick(event: MouseEvent, view: SketchView) {
    if (generating || suppressNextPaneClick || activeTool !== 'polyline') {
      suppressNextPaneClick = false;
      return;
    }
    const pointResult = pointForEditEvent(event as unknown as PointerEvent, view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      return;
    }
    addPolylinePoint(view, pointResult.point);
  }

  function addPolylinePoint(view: SketchView, point: SketchPoint) {
    prepareSketchMutation();
    if (activeStroke && !activeStroke.closed && activeStroke.view === view && strokeKind(activeStroke) === 'polyline') {
      activeStroke = {
        ...activeStroke,
        points: [...activeStroke.points, point],
      };
      return;
    }

    activeStroke = {
      primitiveId: `primitive-${view}-${++primitiveSequence}`,
      sketchId: resolvedWorkspaceSketchId(view),
      view,
      kind: 'polyline',
      points: [point],
      closed: false,
    };
  }

  function closeActivePolyline() {
    if (!activeStroke || activeStroke.closed || strokeKind(activeStroke) !== 'polyline') return;
    const finished = finishStroke({
      ...activeStroke,
      points: [...activeStroke.points, activeStroke.points[0]],
    });
    commitFinishedPrimitive(finished);
  }

  function commitFinishedPrimitive(finished: SketchStroke) {
    if (finished.points.length === 0) return;
    strokes = [...strokes, finished];
    activeStroke = null;
    clearSelectedPoint();
    errorText = '';
    cleanupEvidenceText = '';
    clearPreviewResult();
    requestFeatureSuggestions(strokes);
    if (finished.closed) {
      queueAutoPreview(finished);
    }
  }

  function beginShapeDraft(event: PointerEvent, view: SketchView, kind: 'rectangle' | 'circle') {
    const pointResult = pointForEditEvent(event, view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      return;
    }
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    prepareSketchMutation();
    suppressNextPaneClick = true;
    shapeDraft = {
      kind,
      primitiveId: `primitive-${view}-${++primitiveSequence}`,
      sketchId: resolvedWorkspaceSketchId(view),
      view,
      pointerId: event.pointerId,
      start: pointResult.point,
      current: pointResult.point,
    };
  }

  function dragShapeDraft(event: PointerEvent) {
    if (!shapeDraft) return;
    const pointResult = pointForEditEvent(event, shapeDraft.view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      return;
    }
    shapeDraft = {
      ...shapeDraft,
      current: pointResult.point,
    };
  }

  function endShapeDraft(event: PointerEvent) {
    if (!shapeDraft) return;
    const target = event.currentTarget as HTMLElement;
    if (target.hasPointerCapture(shapeDraft.pointerId)) {
      target.releasePointerCapture(shapeDraft.pointerId);
    }
    const pointResult = pointForEditEvent(event, shapeDraft.view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      shapeDraft = null;
      return;
    }
    const draft = {
      ...shapeDraft,
      current: pointResult.point,
    };
    shapeDraft = null;
    const finished = draft.kind === 'rectangle' ? rectangleStrokeFromDraft(draft) : circleStrokeFromDraft(draft);
    if (!finished) return;
    commitFinishedPrimitive(finished);
  }

  function rectangleStrokeFromDraft(draft: ShapeDraftState): SketchStroke | null {
    const [x0, y0] = draft.start;
    const [x1, y1] = draft.current;
    if (Math.abs(x1 - x0) < 0.01 || Math.abs(y1 - y0) < 0.01) return null;
    const minX = Math.min(x0, x1);
    const maxX = Math.max(x0, x1);
    const minY = Math.min(y0, y1);
    const maxY = Math.max(y0, y1);
    return {
      primitiveId: draft.primitiveId,
      sketchId: draft.sketchId,
      view: draft.view,
      kind: 'polyline',
      points: [
        [minX, minY],
        [maxX, minY],
        [maxX, maxY],
        [minX, maxY],
        [minX, minY],
      ],
      closed: true,
    };
  }

  function circleStrokeFromDraft(draft: ShapeDraftState): SketchStroke | null {
    const radius = Math.hypot(draft.current[0] - draft.start[0], draft.current[1] - draft.start[1]);
    if (radius < 0.01) return null;
    return {
      primitiveId: draft.primitiveId,
      sketchId: draft.sketchId,
      view: draft.view,
      kind: 'circle',
      points: [draft.start],
      closed: true,
      radius,
    };
  }

  function beginPanePan(event: PointerEvent, view: SketchView) {
    const point = eventPoint(event, view);
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    suppressNextPaneClick = true;
    panePan = {
      view,
      pointerId: event.pointerId,
      origin: point,
      startCamera: { ...paneCameras[view] },
    };
  }

  function dragPanePan(event: PointerEvent) {
    if (!panePan) return;
    const point = eventPoint(event, panePan.view);
    const dx = panePan.origin[0] - point[0];
    const dy = panePan.origin[1] - point[1];
    paneCameras = {
      ...paneCameras,
      [panePan.view]: {
        ...panePan.startCamera,
        panX: Number((panePan.startCamera.panX + dx).toFixed(2)),
        panY: Number((panePan.startCamera.panY + dy).toFixed(2)),
      },
    };
  }

  function endPanePan(event: PointerEvent) {
    if (!panePan) return;
    const target = event.currentTarget as HTMLElement;
    if (target.hasPointerCapture(panePan.pointerId)) {
      target.releasePointerCapture(panePan.pointerId);
    }
    panePan = null;
  }

  function updateHoverPoint(event: PointerEvent, view: SketchView) {
    const pointResult = pointForEditEvent(event, view);
    if ('error' in pointResult) return;
    hoverView = view;
    hoverPoint = pointResult.point;
  }

  function beginPointDrag(event: PointerEvent, stroke: SketchStroke, pointIndex: number) {
    if (generating) return;

    event.preventDefault();
    event.stopPropagation();
    suppressNextPaneClick = true;
    const target = ((event.currentTarget as Element | null)?.closest('.sketch-pane') as HTMLElement | null) ?? (event.currentTarget as HTMLElement);
    target.setPointerCapture(event.pointerId);
    pointDrag = {
      primitiveId: stroke.primitiveId,
      pointIndex,
      pointerId: event.pointerId,
      view: stroke.view,
      originalPoint: stroke.points[pointIndex],
      startClientX: event.clientX,
      startClientY: event.clientY,
      moved: false,
    };
    selectPoint(stroke, pointIndex);
  }

  function dragPoint(event: PointerEvent) {
    if (!pointDrag) return;

    event.preventDefault();
    event.stopPropagation();
    const pointResult = pointForEditEvent(event, pointDrag.view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      pointDrag = null;
      clearPreviewResult();
      return;
    }

    const pointerDistance = Math.hypot(
      event.clientX - pointDrag.startClientX,
      event.clientY - pointDrag.startClientY,
    );
    if (pointerDistance < POINT_DRAG_THRESHOLD_PX) {
      return;
    }

    if (!pointDrag.moved) {
      pointDrag = {
        ...pointDrag,
        moved: true,
      };
    }

    if (isSamePoint(pointResult.point, pointDrag.originalPoint)) {
      return;
    }

    prepareSketchMutation();
    const result = moveDraggedPoint(pointResult.point);
    if ('error' in result) {
      errorText = result.error;
      pointDrag = null;
    }
  }

  function endPointDrag(event: PointerEvent) {
    if (!pointDrag) return;

    event.preventDefault();
    event.stopPropagation();
    const target = ((event.currentTarget as Element | null)?.closest('.sketch-pane') as HTMLElement | null) ?? (event.currentTarget as HTMLElement);
    if (target.hasPointerCapture(pointDrag.pointerId)) {
      target.releasePointerCapture(pointDrag.pointerId);
    }

    const pointResult = pointForEditEvent(event, pointDrag.view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      pointDrag = null;
      clearPreviewResult();
      return;
    }

    const originalPoint = pointDrag.originalPoint;
    if (!pointDrag.moved || isSamePoint(pointResult.point, originalPoint)) {
      if (activeStroke?.primitiveId === pointDrag.primitiveId && !activeStroke.closed && pointDrag.pointIndex === 0) {
        pointDrag = null;
        closeActivePolyline();
        return;
      }
      pointDrag = null;
      return;
    }

    prepareSketchMutation();
    const result = moveDraggedPoint(pointResult.point);
    pointDrag = null;
    if ('error' in result) {
      errorText = result.error;
      clearPreviewResult();
      return;
    }

    errorText = '';
    cleanupEvidenceText = '';
    requestFeatureSuggestions(result.strokes);
    queueAutoPreview(result.stroke);
  }

  function moveDraggedPoint(point: SketchPoint): { stroke: SketchStroke; strokes: SketchStroke[] } | { error: string } {
    const dragState = pointDrag;
    if (!dragState) return { error: 'Sketch point target missing.' };

    let movedStroke: SketchStroke | null = null;
    const nextStrokes: SketchStroke[] = strokes.map((stroke) => {
      if (stroke.primitiveId !== dragState.primitiveId || stroke.view !== dragState.view) return stroke;

      if (!stroke.closed && strokeKind(stroke) === 'polyline') {
        const points: SketchPoint[] = stroke.points.map((candidate, index) =>
          index === dragState.pointIndex ? [point[0], point[1]] : [candidate[0], candidate[1]],
        );
        movedStroke = {
          ...stroke,
          points,
        };
        return movedStroke;
      }

      movedStroke = moveClosedStrokePointWithDimensionLocks(stroke, dragState.pointIndex, point);
      assertLockedDimensionsPreserved(stroke, movedStroke);
      return movedStroke;
    });

    if (!movedStroke) return { error: 'Sketch point target missing.' };

    strokes = nextStrokes;
    syncSelectedPointInputs(movedStroke, dragState.pointIndex);
    return { stroke: movedStroke, strokes: nextStrokes };
  }

  function isSamePoint(left: SketchPoint, right: SketchPoint): boolean {
    return Math.hypot(left[0] - right[0], left[1] - right[1]) < 0.01;
  }

  function prepareSketchMutation() {
    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    clearPreviewResult();
    cleanupEvidenceText = '';
    errorText = '';
    activeLens = 'sketch';
  }

  function selectPoint(stroke: SketchStroke, pointIndex: number) {
    selectedPoint = {
      primitiveId: stroke.primitiveId,
      pointIndex,
      view: stroke.view,
    };
    syncSelectedPointInputs(stroke, pointIndex);
  }

  function syncSelectedPointInputs(stroke: SketchStroke, pointIndex: number) {
    const point = stroke.points[pointIndex];
    if (!point) {
      selectedPointX = '';
      selectedPointY = '';
      return;
    }

    selectedPointX = String(point[0]);
    selectedPointY = String(point[1]);
  }

  function clearSelectedPoint() {
    selectedPoint = null;
    selectedPointX = '';
    selectedPointY = '';
  }

  function deleteSelectedPoint() {
    if (generating) return;
    const targetPoint = selectedPoint;
    if (!targetPoint) {
      errorText = 'Sketch point target missing.';
      clearPreviewResult();
      return;
    }

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    cleanupEvidenceText = '';

    let deletedStroke: SketchStroke | null = null;
    try {
      const nextStrokes = strokes.map((stroke) => {
        if (stroke.primitiveId !== targetPoint.primitiveId || stroke.view !== targetPoint.view) return stroke;

        deletedStroke = deleteClosedStrokePoint(stroke, targetPoint.pointIndex);
        return deletedStroke;
      });

      if (!deletedStroke) {
        errorText = 'Sketch point target missing.';
        clearPreviewResult();
        return;
      }

      strokes = nextStrokes;
      selectedPoint = normalizeSelectedPoint(deletedStroke, targetPoint);
      if (selectedPoint) {
        syncSelectedPointInputs(deletedStroke, selectedPoint.pointIndex);
      } else {
        clearSelectedPoint();
      }
      activeLens = 'sketch';
      errorText = '';
      clearPreviewResult();
      requestFeatureSuggestions(nextStrokes);
      queueAutoPreview(deletedStroke);
    } catch (error) {
      errorText = error instanceof Error ? error.message : String(error);
      clearPreviewResult();
    }
  }

  function normalizeSelectedPoint(stroke: SketchStroke, point: SelectedPointState): SelectedPointState | null {
    const logicalCount = Math.max(stroke.points.length - 1, 0);
    if (logicalCount <= 0) return null;

    return {
      primitiveId: stroke.primitiveId,
      view: stroke.view,
      pointIndex: Math.min(point.pointIndex, logicalCount - 1),
    };
  }

  function selectedPointMatchesStroke(point: SelectedPointState | null, stroke: SketchStroke): point is SelectedPointState {
    return Boolean(point && point.primitiveId === stroke.primitiveId && point.view === stroke.view);
  }

  function selectedClosedStroke(): SketchStroke | null {
    if (!selectedPoint) return null;
    return (
      strokes.find(
        (stroke) => stroke.primitiveId === selectedPoint?.primitiveId && stroke.view === selectedPoint.view && stroke.closed,
      ) ?? null
    );
  }

  function pointForEditEvent(event: PointerEvent, view: SketchView): { point: SketchPoint } | { error: string } {
    const point = eventPoint(event, view);
    if (!snapToGrid) return { point };

    try {
      return { point: snapPointToGrid(point, normalizeSketchGridSize(sketchGridSize)) };
    } catch (error) {
      return { error: error instanceof Error ? error.message : String(error) };
    }
  }

  function applySelectedPointCoordinates() {
    if (generating) return;
    const targetPoint = selectedPoint;
    if (!targetPoint) return;

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    cleanupEvidenceText = '';

    let movedStroke: SketchStroke | null = null;
    try {
      const nextStrokes = strokes.map((stroke) => {
        if (stroke.primitiveId !== targetPoint.primitiveId || stroke.view !== targetPoint.view) return stroke;

        movedStroke = moveClosedStrokePointWithDimensionLocks(stroke, targetPoint.pointIndex, [
          normalizeSketchCoordinate(selectedPointX),
          normalizeSketchCoordinate(selectedPointY),
        ]);
        assertLockedDimensionsPreserved(stroke, movedStroke);
        return movedStroke;
      });

      if (!movedStroke) {
        errorText = 'Sketch point target missing.';
        clearPreviewResult();
        return;
      }

      strokes = nextStrokes;
      selectPoint(movedStroke, targetPoint.pointIndex);
      activeLens = 'sketch';
      errorText = '';
      clearPreviewResult();
      requestFeatureSuggestions(nextStrokes);
      queueAutoPreview(movedStroke);
    } catch (error) {
      errorText = error instanceof Error ? error.message : String(error);
      clearPreviewResult();
    }
  }

  function applyProfileSize() {
    if (generating) return;
    const targetProfile = profileSizeTarget;
    if (!targetProfile) return;

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    cleanupEvidenceText = '';

    let resizedStroke: SketchStroke | null = null;
    try {
      const nextStrokes = strokes.map((stroke) => {
        if (stroke.primitiveId !== targetProfile.primitiveId || stroke.view !== targetProfile.view) return stroke;

        resizedStroke = snapToGrid
          ? resizeClosedStrokeBoundsSnapped(stroke, profileWidth, profileHeight, sketchGridSize)
          : resizeClosedStrokeBounds(stroke, profileWidth, profileHeight);
        assertLockedDimensionsPreserved(stroke, resizedStroke);
        return resizedStroke;
      });

      if (!resizedStroke) {
        errorText = 'Sketch profile target missing.';
        clearPreviewResult();
        return;
      }

      const updatedStroke = resizedStroke;
      const currentSelectedPoint = selectedPoint as SelectedPointState | null;
      strokes = nextStrokes;
      if (selectedPointMatchesStroke(currentSelectedPoint, updatedStroke)) {
        selectedPoint = normalizeSelectedPoint(updatedStroke, currentSelectedPoint);
        if (selectedPoint) {
          syncSelectedPointInputs(updatedStroke, selectedPoint.pointIndex);
        }
      }
      activeLens = 'sketch';
      errorText = '';
      clearPreviewResult();
      requestFeatureSuggestions(nextStrokes);
      queueAutoPreview(resizedStroke);
    } catch (error) {
      errorText = error instanceof Error ? error.message : String(error);
      clearPreviewResult();
    }
  }

  function applyProfilePosition() {
    if (generating) return;
    const targetProfile = profileSizeTarget;
    if (!targetProfile) return;

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    cleanupEvidenceText = '';

    let movedStroke: SketchStroke | null = null;
    try {
      const nextStrokes = strokes.map((stroke) => {
        if (stroke.primitiveId !== targetProfile.primitiveId || stroke.view !== targetProfile.view) return stroke;

        movedStroke = snapToGrid
          ? setClosedStrokeBoundsOriginSnapped(stroke, profileX, profileY, sketchGridSize)
          : setClosedStrokeBoundsOrigin(stroke, profileX, profileY);
        assertLockedDimensionsPreserved(stroke, movedStroke);
        return movedStroke;
      });

      if (!movedStroke) {
        errorText = 'Sketch profile target missing.';
        clearPreviewResult();
        return;
      }

      const updatedStroke = movedStroke;
      const currentSelectedPoint = selectedPoint as SelectedPointState | null;
      strokes = nextStrokes;
      if (selectedPointMatchesStroke(currentSelectedPoint, updatedStroke)) {
        selectedPoint = normalizeSelectedPoint(updatedStroke, currentSelectedPoint);
        if (selectedPoint) {
          syncSelectedPointInputs(updatedStroke, selectedPoint.pointIndex);
        }
      }
      activeLens = 'sketch';
      errorText = '';
      clearPreviewResult();
      requestFeatureSuggestions(nextStrokes);
      queueAutoPreview(movedStroke);
    } catch (error) {
      errorText = error instanceof Error ? error.message : String(error);
      clearPreviewResult();
    }
  }

  function toggleProfileDimensionLocks() {
    if (generating) return;
    const targetProfile = profileSizeTarget;
    if (!targetProfile) return;

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();

    const nextLocked = !(targetProfile.dimensionLocks?.width && targetProfile.dimensionLocks?.height);
    let updatedStroke: SketchStroke | null = null;
    const nextStrokes = strokes.map((stroke) => {
      if (stroke.primitiveId !== targetProfile.primitiveId || stroke.view !== targetProfile.view) return stroke;

      updatedStroke = {
        ...stroke,
        ...(nextLocked ? { dimensionLocks: { width: true, height: true } } : { dimensionLocks: undefined }),
      };
      return updatedStroke;
    });

    if (!updatedStroke) {
      errorText = 'Sketch profile target missing.';
      clearPreviewResult();
      return;
    }

    strokes = nextStrokes;
    cleanupEvidenceText = '';
    errorText = '';
    clearPreviewResult();
    activeLens = 'sketch';
    requestFeatureSuggestions(nextStrokes);
    queueAutoPreview(updatedStroke);
  }

  function cleanupSketch() {
    if (generating) return;

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();

    const result = cleanupSketchStrokes(strokes);
    if ('error' in result) {
      errorText = result.error;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    const cleanedStroke = latestClosedStroke(result.strokes);
    if (!cleanedStroke) {
      errorText = 'Close profile before cleanup.';
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    strokes = result.strokes;
    activeStroke = null;
    clearSelectedPoint();
    activeLens = 'sketch';
    errorText = '';
    cleanupEvidenceText = `CLEAN UP SOURCE BOUNDS RECTANGLE CLOSED / ${result.evidence.join(' ')}`;
    sourcePatchEntries = appendSketchSourcePatch(sourcePatchEntries, {
      action: 'CLEAN UP',
      primitiveId: cleanedStroke.primitiveId,
      detail: result.evidence.join(' '),
    });
    clearPreviewResult();
    requestFeatureSuggestions(result.strokes);
    queueAutoPreview(cleanedStroke);
  }

  function eventPoint(event: PointerEvent, view: SketchView): SketchPoint {
    const svg = svgForView(view);
    if (!svg) return [0, 0];
    return clientPointToSvgPoint(event.clientX, event.clientY, svg);
  }

  function visibleStrokes(view: SketchView): SketchStroke[] {
    return [
      ...strokes.filter((stroke) => stroke.view === view),
      ...(activeStroke?.view === view ? [activeStroke] : []),
      ...(shapeDraft?.view === view ? [shapeDraft.kind === 'rectangle' ? rectangleStrokeFromDraft(shapeDraft) : circleStrokeFromDraft(shapeDraft)].filter(Boolean) as SketchStroke[] : []),
    ];
  }

  function resolvedWorkspaceSketchId(view: SketchView): string {
    if (activeStroke?.view === view && activeStroke.sketchId) return activeStroke.sketchId;
    for (let index = strokes.length - 1; index >= 0; index -= 1) {
      const stroke = strokes[index];
      if (stroke.view === view && stroke.sketchId) return stroke.sketchId;
    }
    return `sketch-${view}`;
  }

  function closeOpenProfiles() {
    const closedActiveStroke = activeStroke && !activeStroke.closed ? closeStroke(activeStroke) : activeStroke;
    const hadOpenProfile = Boolean(activeStroke && !activeStroke.closed) || strokes.some((stroke) => !stroke.closed);
    strokes = [...strokes.map((stroke) => closeStroke(stroke)), ...(closedActiveStroke ? [closedActiveStroke] : [])];
    activeStroke = null;
    errorText = '';
    cleanupEvidenceText = '';
    const closedProfiles = strokes.filter((stroke) => stroke.closed);
    const closedProfile = closedProfiles[closedProfiles.length - 1] ?? null;
    if (hadOpenProfile && closedProfile) {
      queueAutoPreview(closedProfile);
      requestFeatureSuggestions(strokes);
    }
  }

  function undoLastStroke() {
    clearAutoPreviewQueue();
    if (activeStroke) {
      activeStroke = null;
    } else {
      strokes = strokes.slice(0, -1);
    }
    selectedPoint = null;
    clearPreviewResult();
    errorText = '';
    cleanupEvidenceText = '';
    autoPreviewPrimitiveId = null;
    clearFeatureSuggestions();
    clearImportRepair();
    clearSourcePatchLedger();
  }

  function clearSketch() {
    clearAutoPreviewQueue();
    strokes = [];
    activeStroke = null;
    clearSelectedPoint();
    clearPreviewResult();
    activeLens = 'sketch';
    acceptedExactState = null;
    errorText = '';
    cleanupEvidenceText = '';
    autoPreviewPrimitiveId = null;
    clearFeatureSuggestions();
    clearImportRepair();
    clearSourcePatchLedger();
  }

  function replaySketchDocumentSnapshot() {
    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    clearSourcePatchLedger();

    if (!sketchDocumentSnapshot) {
      errorText = 'Sketch document unavailable.';
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    const replay = sketchDocumentToStrokes(sketchDocumentSnapshot);
    if ('error' in replay) {
      errorText = replay.error;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentEditorDirty = false;
    activeLens = 'sketch';
    errorText = '';
    cleanupEvidenceText = '';
    clearPreviewResult();
    requestFeatureSuggestions(replay.strokes);

    void generateDraft('auto', replay.strokes);
  }

  function importSketchDocumentSource() {
    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    clearSourcePatchLedger();

    const parsed = parseSketchDocumentImportSource(sketchDocumentImportText);
    if ('error' in parsed) {
      errorText = parsed.error;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    const endpointRepair = repairSketchDocumentEndpointGaps(parsed.document);
    const dimensionRepair = autoRepairSketchDocumentDimensionConstraintGeometry(endpointRepair.document);
    const autoRepairEvidence = [...endpointRepair.evidence, ...dimensionRepair.evidence];
    const replay = sketchDocumentToStrokes(dimensionRepair.document);
    if ('error' in replay) {
      errorText = replay.error;
      cleanupEvidenceText = '';
      primeImportRepair(dimensionRepair.document);
      clearPreviewResult();
      return;
    }

    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentSnapshot = dimensionRepair.document;
    sketchDocumentImportText = formatSketchDocumentSource(dimensionRepair.document);
    sketchDocumentEditorDirty = false;
    activeLens = 'sketch';
    acceptedExactState = null;
    errorText = '';
    cleanupEvidenceText = autoRepairEvidence.length
      ? `AUTO SNAP IMPORT / ${autoRepairEvidence.map((entry) => entry.detail).join(' / ')}`
      : '';
    if (autoRepairEvidence.length) {
      sourcePatchEntries = autoRepairEvidence.reduce(
        (entries, entry) =>
          appendSketchSourcePatch(entries, {
            action: 'AUTO SNAP',
            primitiveId: entry.primitiveId,
            detail: entry.detail,
          }),
        sourcePatchEntries,
      );
    }
    clearPreviewResult();
    requestFeatureSuggestions(replay.strokes);

    void generateDraft('auto', replay.strokes);
  }

  function repairSketchDocumentImport() {
    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();

    if (!importRepairDocument) {
      errorText = 'Sketch import repair unavailable.';
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    const replay = sketchDocumentToStrokes(importRepairDocument);
    if ('error' in replay) {
      errorText = replay.error;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    const repairedStroke = latestClosedStroke(replay.strokes);
    const repairDetail = compactRepairDetail(importRepairEvidenceText);

    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentSnapshot = importRepairDocument;
    sketchDocumentImportText = formatSketchDocumentSource(importRepairDocument);
    sketchDocumentEditorDirty = false;
    activeLens = 'sketch';
    acceptedExactState = null;
    errorText = '';
    cleanupEvidenceText = '';
    if (repairedStroke) {
      sourcePatchEntries = appendSketchSourcePatch(sourcePatchEntries, {
        action: 'REPAIR IMPORT',
        primitiveId: repairedStroke.primitiveId,
        detail: repairDetail,
      });
    }
    clearImportRepair();
    clearPreviewResult();
    requestFeatureSuggestions(replay.strokes);

    void generateDraft('auto', replay.strokes);
  }

  async function generateDraft(
    mode: PreviewMode = 'manual',
    currentStrokes: SketchStroke[] = strokes,
    options: GenerateDraftOptions = {},
  ) {
    clearAutoPreviewQueue();
    clearAcceptedSuggestion();
    if (!options.preserveBrepAutoRepairAttempts) {
      brepAutoRepairAttempts = 0;
    }
    const openError = currentStrokes.some((stroke) => !stroke.closed) || (activeStroke && !activeStroke.closed) ? 'Close profile before preview.' : '';
    if (openError) {
      errorText = openError;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    let draftStrokes = currentStrokes;
    const repairResult = autoRepairOrthographicSketchStrokes(draftStrokes);
    if (repairResult.repairs.length) {
      draftStrokes = repairResult.strokes;
      strokes = repairResult.strokes;
      activeStroke = null;
      clearSelectedPoint();
      cleanupEvidenceText = `AUTO SNAP ORTHOGRAPHIC / ${repairResult.repairs.map((repair) => repair.detail).join(' / ')}`;
      sourcePatchEntries = repairResult.repairs.reduce(
        (entries, repair) =>
          appendSketchSourcePatch(entries, {
            action: 'AUTO SNAP',
            primitiveId: repair.primitiveId,
            detail: repair.detail,
          }),
        sourcePatchEntries,
      );
    }

    const request = buildSketchDraftRequest(draftStrokes);
    if ('error' in request) {
      errorText = request.error;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    if (!suggestingFeatures && !suggestionResponse) {
      requestFeatureSuggestions(draftStrokes);
    }
    generating = true;
    autoQueued = mode === 'auto';
    autoPreviewPrimitiveId = request.sketch.primitives?.[0]?.primitiveId ?? null;
    errorText = '';
    clearPreviewResult();
    const runId = ++autoPreviewRunId;
    try {
      const usePreviewHull = shouldUseSketchPreviewHull(draftStrokes);
      const previewHullRequest = usePreviewHull ? assertPreviewHullRequest(draftStrokes) : null;
      const result = previewHullRequest
        ? await generateSketchPreviewHull(previewHullRequest)
        : await generateSketchDraftPreview(request);
      if (runId !== autoPreviewRunId) return;
      draft = result.draft;
      artifactBundle = result.artifactBundle;
      previewProfile = previewProfileFor(request.sketch.view, draftStrokes);
      syncSketchDocumentEnvelope(result.draft.source);
      draftSceneSignature = sceneSignatureFromStrokes(draftStrokes);
      autoQueued = false;
      publishPreviewResult(result);
      if (mode === 'manual') {
        onManualPreviewResult?.(result);
      }
      if (previewHullRequest) {
        void loadBrepCandidateGraph(previewHullRequest.document, runId);
        void loadHiddenLineProjection(result.artifactBundle, previewHullRequest.document, runId);
      } else {
        clearBrepCandidateGraph();
        clearHiddenLineProjection();
      }
    } catch (error) {
      if (runId !== autoPreviewRunId) return;
      errorText = formatBackendError(error);
      autoQueued = false;
    } finally {
      if (runId === autoPreviewRunId) {
        generating = false;
      }
    }
  }

  function assertPreviewHullRequest(currentStrokes: SketchStroke[]) {
    const request = buildSketchPreviewHullRequest(currentStrokes);
    if ('error' in request) {
      throw new Error(request.error);
    }
    return request;
  }

  async function loadBrepCandidateGraph(document: SketchDocument, runId: number) {
    brepCandidateLoading = true;
    brepCandidateErrorText = '';
    brepCandidateResponse = null;
    brepCandidateDocument = document;
    brepCandidateAcceptedSolutionId = null;
    brepCandidateAcceptErrorText = '';
    brepCandidateAcceptEvidence = [];
    brepComponentPackage = null;
    brepComponentPackageErrorText = '';
    brepComponentPackageLoading = false;
    try {
      const response = await analyzeSketchBrepCandidates({ document });
      if (runId !== autoPreviewRunId) return;
      brepCandidateResponse = response;
    } catch (error) {
      if (runId !== autoPreviewRunId) return;
      brepCandidateErrorText = formatBackendError(error);
    } finally {
      if (runId === autoPreviewRunId) {
        brepCandidateLoading = false;
      }
    }
  }

  function clearBrepCandidateGraph() {
    brepCandidateResponse = null;
    brepCandidateErrorText = '';
    brepCandidateLoading = false;
    brepCandidateDocument = null;
    brepCandidateAcceptingSolutionId = null;
    brepCandidateAcceptedSolutionId = null;
    brepCandidateAcceptErrorText = '';
    brepCandidateAcceptEvidence = [];
    brepComponentPackage = null;
    brepComponentPackageErrorText = '';
    brepComponentPackageLoading = false;
  }

  async function acceptBrepCandidateSolution(solutionId: string) {
    if (!brepCandidateDocument || brepCandidateAcceptingSolutionId) return;
    const runId = autoPreviewRunId;
    brepCandidateAcceptingSolutionId = solutionId;
    brepCandidateAcceptErrorText = '';
    brepCandidateAcceptEvidence = [];
    brepComponentPackage = null;
    brepComponentPackageErrorText = '';
    brepComponentPackageLoading = false;
    try {
      const response = await acceptSketchBrepCandidateSolution({
        partId: 'sketch-preview-hull',
        document: brepCandidateDocument,
        solutionId,
        tolerance: 0.1,
      });
      if (runId !== autoPreviewRunId) return;
      draft = response.draftSource;
      artifactBundle = response.artifactBundle;
      brepCandidateResponse = response.candidateResponse;
      hiddenLineResponse = response.hiddenLineResponse;
      hiddenLineErrorText = '';
      hiddenLineLoading = false;
      brepCandidateAcceptedSolutionId = response.acceptedSolution.solutionId;
      acceptedExactState = {
        solutionId: response.acceptedSolution.solutionId,
        sceneSignature: currentSceneSignature,
      };
      draftSceneSignature = currentSceneSignature;
      activeLens = 'exact';
      brepCandidateAcceptEvidence = response.evidence ?? [];
      publishPreviewResult({ draft: response.draftSource, artifactBundle: response.artifactBundle });
    } catch (error) {
      if (runId !== autoPreviewRunId) return;
      brepCandidateAcceptErrorText = formatBackendError(error);
    } finally {
      if (runId === autoPreviewRunId) {
        brepCandidateAcceptingSolutionId = null;
      }
    }
  }

  function runWorkspaceSceneAction(rowKey: 'sketch' | 'draft' | 'exact') {
    const row = workspaceScene.rows.find((candidate) => candidate.key === rowKey);
    const action = row?.action;
    if (!action) return;
    if (action.kind === 'previewDraft') {
      activeLens = 'draft';
      void generateDraft('manual');
      return;
    }
    if (action.kind === 'acceptExact' || action.kind === 'rebuildExact') {
      void acceptBrepCandidateSolution(action.solutionId);
    }
  }

  function acceptedBrepStepSourceRef(): string | null {
    return artifactBundle?.exportArtifacts?.find((artifact) => artifact.format.toLowerCase() === 'step')?.path ?? null;
  }

  function acceptedBrepTopologySummary(): string {
    const edgeCount = artifactBundle?.edgeTargets?.length ?? 0;
    if (edgeCount > 0) {
      return `EXACT BREP TOPOLOGY ${edgeCount} ${edgeCount === 1 ? 'EDGE' : 'EDGES'}`;
    }
    return 'EXACT BREP TOPOLOGY PENDING';
  }

  function acceptedBrepPortTypes(): PortTypeDefinition[] {
    return [
      {
        typeId: ACCEPTED_BREP_PORT_TYPE_ID,
        displayName: 'Mechanical plane mount',
        base: 'mechanical.plane',
        interfaces: ['mechanical.mount'],
        compatibleWith: [ACCEPTED_BREP_PORT_TYPE_ID],
        allowedOps: ['place', 'mate'],
        params: [],
      },
    ];
  }

  function acceptedBrepPorts(): ComponentPort[] {
    const preferredTargetId =
      artifactBundle?.faceTargets?.[0]?.durableTargetId ??
      artifactBundle?.faceTargets?.[0]?.targetId ??
      artifactBundle?.edgeTargets?.[0]?.durableTargetId ??
      artifactBundle?.edgeTargets?.[0]?.targetId ??
      null;
    return [
      {
        portId: ACCEPTED_BREP_PORT_ID,
        typeId: ACCEPTED_BREP_PORT_TYPE_ID,
        targetIds: preferredTargetId ? [preferredTargetId] : [],
        frame: {
          origin: [0, 0, 0],
          xAxis: [1, 0, 0],
          yAxis: [0, 1, 0],
          zAxis: [0, 0, 1],
        },
        params: {},
        interfaces: ['mechanical.mount'],
        compatibleWith: [ACCEPTED_BREP_PORT_TYPE_ID],
        allowedOps: ['place', 'mate'],
      },
    ];
  }

  async function createAcceptedBrepComponentPackage() {
    if (!brepCandidateDocument || !brepCandidateAcceptedSolutionId || brepComponentPackageLoading) return;
    const sourceRef = acceptedBrepStepSourceRef();
    if (!sourceRef) {
      brepComponentPackageErrorText = 'Accepted BRep component package requires an accepted STEP sourceRef.';
      return;
    }
    const runId = autoPreviewRunId;
    brepComponentPackageLoading = true;
    brepComponentPackageErrorText = '';
    brepComponentPackage = null;
    try {
      const componentPackage = await acceptedBrepCandidateToComponentPackage({
        packageId: ACCEPTED_BREP_PACKAGE_ID,
        version: '0.1.0',
        displayName: 'Accepted BRep Candidate',
        tags: ['accepted-brep', 'sketch-candidate'],
        componentId: ACCEPTED_BREP_COMPONENT_ID,
        componentVersion: '0.1.0',
        componentDisplayName: 'Sketch Preview Hull',
        sourceRef,
        document: brepCandidateDocument,
        solutionId: brepCandidateAcceptedSolutionId,
        portTypes: acceptedBrepPortTypes(),
        ports: acceptedBrepPorts(),
      });
      if (runId !== autoPreviewRunId) return;
      brepComponentPackage = componentPackage;
    } catch (error) {
      if (runId !== autoPreviewRunId) return;
      brepComponentPackageErrorText = formatBackendError(error);
    } finally {
      if (runId === autoPreviewRunId) {
        brepComponentPackageLoading = false;
      }
    }
  }

  async function loadHiddenLineProjection(bundle: ArtifactBundle, document: SketchDocument, runId: number) {
    hiddenLineResponse = null;
    hiddenLineErrorText = '';
    if (!hasBrepProjectionArtifact(bundle)) {
      hiddenLineLoading = false;
      return;
    }

    hiddenLineLoading = true;
    try {
      const response = await extractBrepHiddenLineProjections({
        artifactBundle: bundle,
        sketchDocument: document,
        views: ['front', 'top', 'side'],
        tolerance: 0.1,
      });
      if (runId !== autoPreviewRunId) return;
      if (!response.validation?.passed && applyBrepAutoRepairProjection(document, response)) {
        return;
      }
      hiddenLineResponse = response;
    } catch (error) {
      if (runId !== autoPreviewRunId) return;
      hiddenLineErrorText = formatBackendError(error);
    } finally {
      if (runId === autoPreviewRunId) {
        hiddenLineLoading = false;
      }
    }
  }

  function applyBrepAutoRepairProjection(
    document: SketchDocument,
    response: BrepHiddenLineProjectionResponse,
  ): boolean {
    if (brepAutoRepairAttempts >= 1) return false;

    const repair = autoRepairSketchDocumentFromBrepProjection(document, response);
    if (!repair.repaired || repair.evidence.length === 0) return false;

    const replay = sketchDocumentToStrokes(repair.document);
    if ('error' in replay) return false;

    brepAutoRepairAttempts += 1;
    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentSnapshot = repair.document;
    sketchDocumentImportText = formatSketchDocumentSource(repair.document);
    errorText = '';
    cleanupEvidenceText = repair.evidence.map((entry) => entry.detail).join(' / ');
    sourcePatchEntries = repair.evidence.reduce(
      (entries, entry) =>
        appendSketchSourcePatch(entries, {
          action: 'AUTO SNAP',
          primitiveId: entry.primitiveId,
          detail: entry.detail,
        }),
      sourcePatchEntries,
    );
    clearFeatureSuggestions();
    hiddenLineResponse = null;
    hiddenLineErrorText = '';
    hiddenLineLoading = false;

    void generateDraft('auto', replay.strokes, { preserveBrepAutoRepairAttempts: true });
    return true;
  }

  function convertDerivedBrepSketches() {
    if (!brepDerivedSketch) {
      errorText = 'BRep derived sketch unavailable.';
      return;
    }
    if ('error' in brepDerivedSketch) {
      errorText = brepDerivedSketch.error;
      return;
    }

    const replay = sketchDocumentToStrokes(brepDerivedSketch.document);
    if ('error' in replay) {
      errorText = replay.error;
      return;
    }

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentSnapshot = brepDerivedSketch.document;
    sketchDocumentImportText = formatSketchDocumentSource(brepDerivedSketch.document);
    activeLens = 'sketch';
    acceptedExactState = null;
    errorText = '';
    cleanupEvidenceText = brepDerivedSketch.evidence;
    sourcePatchEntries = appendSketchSourcePatch(sourcePatchEntries, {
      action: 'DERIVE BREP',
      primitiveId: brepDerivedSketch.views.map((view) => view.toUpperCase()).join(','),
      detail: brepDerivedSketch.evidence,
    });
    clearPreviewResult();
    requestFeatureSuggestions(replay.strokes);

    void generateDraft('auto', replay.strokes);
  }

  function applyBrepTopologyRepair(proposal: SketchTopologyRepairProposal) {
    if (!sketchDocumentSource || !hiddenLineResponse) {
      errorText = 'Topology repair source unavailable.';
      return;
    }

    const repair = applySketchTopologyRepairProposal(sketchDocumentSource, hiddenLineResponse, proposal.proposalId);
    if ('error' in repair) {
      errorText = repair.error;
      return;
    }

    const replay = sketchDocumentToStrokes(repair.document);
    if ('error' in replay) {
      errorText = replay.error;
      return;
    }

    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentSnapshot = repair.document;
    sketchDocumentImportText = formatSketchDocumentSource(repair.document);
    activeLens = 'sketch';
    errorText = '';
    cleanupEvidenceText = repair.evidence.detail;
    sourcePatchEntries = appendSketchSourcePatch(sourcePatchEntries, {
      action: 'TOPOLOGY REDRAW',
      primitiveId: repair.evidence.primitiveId,
      detail: repair.evidence.detail,
    });
    clearPreviewResult();
    requestFeatureSuggestions(replay.strokes);

    void generateDraft('auto', replay.strokes);
  }

  function hasBrepProjectionArtifact(bundle: ArtifactBundle): boolean {
    if (bundle.fcstdPath) return true;
    return Boolean(bundle.exportArtifacts?.some((artifact) => artifact.format === 'step' && artifact.path));
  }

  function clearHiddenLineProjection() {
    hiddenLineResponse = null;
    hiddenLineErrorText = '';
    hiddenLineLoading = false;
  }

  function queueAutoPreview(stroke: SketchStroke) {
    clearAutoPreviewQueue();
    if (!stroke.closed) {
      return;
    }

    autoQueued = true;
    autoPreviewPrimitiveId = stroke.primitiveId;
    autoPreviewTimer = setTimeout(() => {
      autoPreviewTimer = null;
      void generateDraft('auto');
    }, AUTO_PREVIEW_DEBOUNCE_MS);
  }

  function clearAutoPreviewQueue() {
    if (!autoPreviewTimer) return;
    clearTimeout(autoPreviewTimer);
    autoPreviewTimer = null;
    autoQueued = false;
  }

  function requestFeatureSuggestions(currentStrokes: SketchStroke[] = strokes) {
    const request = buildSketchSuggestionRequest(currentStrokes);
    const runId = ++suggestionRunId;
    suggestionErrorText = '';
    suggestionResponse = null;
    clearAcceptedSuggestion();

    if ('error' in request) {
      suggestingFeatures = false;
      return;
    }

    suggestingFeatures = true;
    void loadFeatureSuggestions(request, runId);
  }

  async function loadFeatureSuggestions(request: SketchSuggestionRequest, runId: number) {
    try {
      const response = await suggestSketchFeatures(request);
      if (runId !== suggestionRunId) return;
      suggestionResponse = response ?? null;
    } catch (error) {
      if (runId !== suggestionRunId) return;
      suggestionErrorText = formatBackendError(error);
    } finally {
      if (runId === suggestionRunId) {
        suggestingFeatures = false;
      }
    }
  }

  function clearFeatureSuggestions() {
    suggestionRunId += 1;
    suggestionResponse = null;
    suggestionErrorText = '';
    suggestingFeatures = false;
    acceptingSuggestionId = null;
    clearAcceptedSuggestion();
  }

  async function acceptSuggestion(suggestion: SketchFeatureSuggestion) {
    clearAutoPreviewQueue();
    const suggestionRequest = buildSketchSuggestionRequest(strokes);
    acceptedSuggestionId = null;
    acceptedSuggestionLabel = '';
    suggestionErrorText = '';

    if ('error' in suggestionRequest) {
      errorText = suggestionRequest.error;
      clearPreviewResult();
      return;
    }

    const request = buildDraftRequestFromSuggestion(suggestionRequest.document, suggestion);
    if ('error' in request) {
      errorText = request.error;
      clearPreviewResult();
      return;
    }

    generating = true;
    acceptingSuggestionId = suggestion.suggestionId;
    autoQueued = false;
    autoPreviewPrimitiveId = request.sketch.primitives?.[0]?.primitiveId ?? suggestion.primitiveId ?? null;
    errorText = '';
    clearPreviewResult();
    const runId = ++autoPreviewRunId;
    try {
      const result = await generateSketchDraftPreview(request);
      if (runId !== autoPreviewRunId) return;
      draft = result.draft;
      artifactBundle = result.artifactBundle;
      previewProfile = previewProfileForSuggestion(request, suggestion);
      syncSketchDocumentEnvelope(result.draft.source);
      draftSceneSignature = sceneSignatureFromStrokes(strokes);
      acceptedSuggestionId = suggestion.suggestionId;
      acceptedSuggestionLabel = formatSuggestionLabel(suggestion);
      autoQueued = false;
      publishPreviewResult(result);
    } catch (error) {
      if (runId !== autoPreviewRunId) return;
      errorText = formatBackendError(error);
      autoQueued = false;
    } finally {
      if (runId === autoPreviewRunId) {
        generating = false;
        acceptingSuggestionId = null;
      }
    }
  }

  function clearAcceptedSuggestion() {
    acceptedSuggestionId = null;
    acceptedSuggestionLabel = '';
  }

  function clearImportRepair() {
    importRepairDocument = null;
    importRepairEvidenceText = '';
  }

  function clearSourcePatchLedger() {
    sourcePatchEntries = [];
  }

  function primeImportRepair(document: SketchDocument) {
    const repair = repairSketchDocumentDimensionConstraints(document);
    if ('error' in repair) {
      clearImportRepair();
      return;
    }

    importRepairDocument = repair.document;
    importRepairEvidenceText = `REPAIR AVAILABLE / ${repair.evidence.join(' ')}`;
  }

  function latestClosedStroke(items: SketchStroke[]): SketchStroke | null {
    for (let index = items.length - 1; index >= 0; index -= 1) {
      const stroke = items[index];
      if (stroke.closed) return stroke;
    }
    return null;
  }

  function preserveSketchDocumentEnvelope(
    document: SketchDocument,
    currentStrokes: SketchStroke[],
    snapshot: SketchDocument | null,
  ): SketchDocument {
    if (!snapshot || !strokesMatchDocumentLineage(currentStrokes, snapshot)) {
      return document;
    }

    return {
      ...document,
      documentId: snapshot.documentId,
      activeSketchId: snapshot.activeSketchId,
      units: snapshot.units,
      metadata: snapshot.metadata,
    };
  }

  function strokesMatchDocumentLineage(currentStrokes: SketchStroke[], document: SketchDocument): boolean {
    const documentSignatures = new Set(
      (document.sketches ?? []).flatMap((sketch) =>
        (sketch.primitives ?? []).map((primitive) => `${primitive.primitiveId}:${sketch.view}`),
      ),
    );

    return currentStrokes.length > 0 && currentStrokes.every((stroke) => documentSignatures.has(`${stroke.primitiveId}:${stroke.view}`));
  }

  function syncSketchDocumentEnvelope(source: string) {
    const envelope = parseSketchDocumentEnvelope(source);
    if ('error' in envelope) {
      return;
    }

    sketchDocumentSnapshot = envelope.document;
    sketchDocumentImportText = formatSketchDocumentSource(envelope.document);
  }

  function publishPreviewResult(result: PreviewResult) {
    onPreviewResult?.(result);
    dispatch('previewResult', result);
  }

  function clearPreviewResult() {
    draft = null;
    draftSceneSignature = null;
    artifactBundle = null;
    previewProfile = null;
    clearBrepCandidateGraph();
    clearHiddenLineProjection();
    publishPreviewResult(null);
  }

  function previewProfileFor(view: SketchView, currentStrokes: SketchStroke[] = strokes): SketchStroke | null {
    return currentStrokes.find((stroke) => stroke.view === view && stroke.closed) ?? null;
  }

  function previewProfileForSuggestion(request: SketchDraftRequest, suggestion: SketchFeatureSuggestion): SketchStroke | null {
    const primitiveId = suggestion.primitiveId ?? request.sketch.primitives?.[0]?.primitiveId ?? null;
    if (primitiveId) {
      return strokes.find((stroke) => stroke.view === request.sketch.view && stroke.primitiveId === primitiveId && stroke.closed) ?? null;
    }
    return previewProfileFor(request.sketch.view);
  }

  function svgForView(view: SketchView): SVGSVGElement | null {
    if (view === 'front') return frontSvg;
    if (view === 'top') return topSvg;
    if (view === 'side') return sideSvg;
    return null;
  }

  function projectionRoleLabel(projection: SketchProjection): string {
    return projection.role === 'source' ? 'SOURCE SKETCH/AUTHORING' : 'DERIVED/EXTRUDE DEPTH';
  }

  function hiddenLineViewSummary(view: BrepHiddenLineProjectionView): string {
    return `${view.view.toUpperCase()} ${view.visibleEdges?.length ?? 0} visible / ${view.hiddenEdges?.length ?? 0} hidden`;
  }

  function hiddenLineProjectionStatus(view: SketchView): 'pass' | 'warn' | 'fail' {
    if (hiddenLineViewHasIssue(view)) return 'fail';
    if (brepHiddenLineViewHasWarning(hiddenLineResponse, view)) return 'warn';
    return 'pass';
  }

  function hiddenLineViewHasIssue(view: SketchView): boolean {
    if (brepSketchRepairTargets.some((target) => target.view === view)) {
      return true;
    }
    if (brepTopologyRepairProposals.some((proposal) => proposal.view === view)) {
      return true;
    }
    const issues = hiddenLineResponse?.validation?.issues ?? [];
    if (
      issues.some((issue) => {
        if (sketchDocumentSource) {
          const match = findSketchIssueMatch(sketchDocumentSource, issue);
          if (match) return match.sketch.view === view;
        }
        return false;
      })
    ) {
      return true;
    }
    return false;
  }

  function hiddenLineEdgePoints(edge: BrepProjectedEdge2d): string {
    return pointsToSvg(edge.points ?? []);
  }

  function brepSketchValidationLedgerRow(summary: ReturnType<typeof buildSketchBrepProjectionValidationSummary>): SketchValidationRow {
    const backendValidation = hiddenLineResponse?.validation;
    const backendEvidence = backendValidation?.evidence?.filter(Boolean).join('; ') ?? '';
    const backendIssue = summarizeSketchValidationIssues(hiddenLineResponse?.validation?.issues);
    const failingRow = summary.rows.find((row) => row.status === 'fail');
    const viewEvidence = summary.viewSummaries
      .map((view) => `${view.view} ${view.visibleEdgeCount} visible / ${view.hiddenEdgeCount} hidden`)
      .join('; ');
    if (backendValidation) {
      if (!backendValidation.passed || (backendValidation.issues?.length ?? 0) > 0) {
        return {
          id: 'brepSketchValidation',
          label: 'BRep/sketch validation',
          status: 'fail',
          detail: backendIssue || backendEvidence || 'BRep/sketch validation failed.',
        };
      }
      return {
        id: 'brepSketchValidation',
        label: 'BRep/sketch validation',
        status: 'pass',
        detail: backendEvidence || viewEvidence || 'BRep/sketch validation passed.',
      };
    }
    if (failingRow) {
      return {
        id: 'brepSketchValidation',
        label: 'BRep/sketch validation',
        status: 'fail',
        detail: failingRow?.evidence ?? 'BRep/sketch validation failed.',
      };
    }
    const rowsPassed = summary.rows.length > 0 && summary.rows.every((row) => row.status === 'pass');
    return {
      id: 'brepSketchValidation',
      label: 'BRep/sketch validation',
      status: rowsPassed ? 'pass' : 'pending',
      detail: viewEvidence || summary.rows.map((row) => row.evidence).join('; ') || 'Waiting for BRep projection evidence.',
    };
  }

  function projectionBoundsPath(bounds: SketchProjectionBounds): string {
    const rect = projectionRect(bounds);
    return `M${rect.x} ${rect.y}H${rect.x + rect.width}V${rect.y + rect.height}H${rect.x}Z`;
  }

  function projectionDepthPath(bounds: SketchProjectionBounds): string {
    const rect = projectionRect(bounds);
    if (Math.abs(bounds.width - bounds.depth) <= Math.abs(bounds.height - bounds.depth)) {
      const y = rect.y + rect.height + 8;
      return `M${rect.x} ${y}H${rect.x + rect.width}`;
    }

    const x = rect.x + rect.width + 8;
    return `M${x} ${rect.y}V${rect.y + rect.height}`;
  }

  function projectionRect(bounds: SketchProjectionBounds): ProjectionRect {
    const width = Math.max(bounds.width, 1);
    const height = Math.max(bounds.height, 1);
    const scale = Math.min(64 / width, 64 / height);
    const rectWidth = Math.max(width * scale, 8);
    const rectHeight = Math.max(height * scale, 8);

    return {
      x: Number(((100 - rectWidth) / 2).toFixed(2)),
      y: Number(((100 - rectHeight) / 2).toFixed(2)),
      width: Number(rectWidth.toFixed(2)),
      height: Number(rectHeight.toFixed(2)),
    };
  }

  function formatSuggestionAmount(suggestion: SketchFeatureSuggestion): string {
    const unit = suggestion.operation === 'revolve' ? 'deg' : 'mm';
    return `${formatNumber(suggestion.amount)}${unit.toUpperCase()}`;
  }

  function formatSuggestionLabel(suggestion: SketchFeatureSuggestion): string {
    return `${suggestion.operation.toUpperCase()} ${formatSuggestionAmount(suggestion)}`;
  }

  function formatConfidence(confidence: number): string {
    return `${Math.round(confidence * 100)}%`;
  }

  function formatNumber(value: number): string {
    if (Number.isInteger(value)) return String(value);
    return value.toFixed(2).replace(/\.?0+$/, '');
  }

  function displayStrokePoints(stroke: SketchStroke): SketchPoint[] {
    if (strokeKind(stroke) === 'circle') {
      const center = stroke.points[0];
      const radius = stroke.radius ?? 0;
      const segments = 24;
      const points: SketchPoint[] = [];
      for (let index = 0; index <= segments; index += 1) {
        const angle = (Math.PI * 2 * index) / segments;
        points.push([
          Number((center[0] + Math.cos(angle) * radius).toFixed(2)),
          Number((center[1] + Math.sin(angle) * radius).toFixed(2)),
        ]);
      }
      return points;
    }
    return stroke.points.map(([x, y]) => [x, y]);
  }

  function validationStatusLabel(row: SketchValidationRow): string {
    return row.status.toUpperCase();
  }

  function compactInspectorDetail(detail: string): string {
    const trimmed = detail.trim();
    if (!trimmed) return '';
    return trimmed.replace(/^Waiting for\s+/i, '').replace(/\.$/, '');
  }

  function paneViewBox(view: SketchView): string {
    const camera = paneCameras[view];
    const span = 100 / Math.max(camera.zoom, 0.25);
    const min = 50 - span / 2;
    return `${formatNumber(min + camera.panX)} ${formatNumber(min + camera.panY)} ${formatNumber(span)} ${formatNumber(span)}`;
  }

  function zoomPane(view: SketchView, deltaY: number) {
    const camera = paneCameras[view];
    const zoomFactor = deltaY < 0 ? 1.12 : 1 / 1.12;
    paneCameras = {
      ...paneCameras,
      [view]: {
        ...camera,
        zoom: Number(Math.min(8, Math.max(0.5, camera.zoom * zoomFactor)).toFixed(3)),
      },
    };
  }

  function resetPaneCamera(view: SketchView) {
    paneCameras = {
      ...paneCameras,
      [view]: { zoom: DEFAULT_PANE_ZOOM, panX: 0, panY: 0 },
    };
  }

  function handlePaneWheel(event: WheelEvent, view: SketchView) {
    event.preventDefault();
    zoomPane(view, event.deltaY);
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      activeStroke = null;
      shapeDraft = null;
      panePan = null;
      return;
    }

    if (event.key === 'Enter' && activeTool === 'polyline' && activeStroke && !activeStroke.closed) {
      event.preventDefault();
      closeActivePolyline();
      return;
    }

    if ((event.key === 'Backspace' || event.key === 'Delete') && activeStroke && !activeStroke.closed && strokeKind(activeStroke) === 'polyline') {
      event.preventDefault();
      const nextPoints = activeStroke.points.slice(0, -1);
      activeStroke = nextPoints.length
        ? {
            ...activeStroke,
            points: nextPoints,
          }
        : null;
      if (nextPoints.length === 0) {
        activeStroke = null;
      }
    }
  }

  function summarizeDimensionConstraints(stroke: SketchStroke | null): {
    width: string;
    height: string;
    widthLocked: boolean;
    heightLocked: boolean;
    allLocked: boolean;
  } | null {
    if (!stroke) return null;

    try {
      const bounds = closedStrokeBounds(stroke);
      const widthLocked = Boolean(stroke.dimensionLocks?.width);
      const heightLocked = Boolean(stroke.dimensionLocks?.height);
      return {
        width: formatNumber(bounds.width),
        height: formatNumber(bounds.height),
        widthLocked,
        heightLocked,
        allLocked: widthLocked && heightLocked,
      };
    } catch {
      return null;
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div class="sketch-workspace">
  <header class="sketch-workspace__header">
    <div>
      <h2>SKETCH WORKSPACE</h2>
      <div class="sketch-workspace__meta">ORTHOGRAPHIC SKETCH / EXTRUDE 12MM</div>
    </div>
    <div class="sketch-workspace__actions">
      <button class="btn btn-xs" class:btn-primary={activeTool === 'select'} onclick={() => (activeTool = 'select')} disabled={generating}>SELECT</button>
      <button class="btn btn-xs" class:btn-primary={activeTool === 'polyline'} onclick={() => (activeTool = 'polyline')} disabled={generating}>POLYLINE</button>
      <button class="btn btn-xs" class:btn-primary={activeTool === 'rectangle'} onclick={() => (activeTool = 'rectangle')} disabled={generating}>RECTANGLE</button>
      <button class="btn btn-xs" class:btn-primary={activeTool === 'circle'} onclick={() => (activeTool = 'circle')} disabled={generating}>CIRCLE</button>
      <button class="btn btn-xs" onclick={closeOpenProfiles} disabled={generating || (!strokes.length && !activeStroke)}>CLOSE OPEN</button>
      <button
        class="btn btn-xs"
        class:btn-primary={snapToGrid}
        aria-pressed={snapToGrid}
        title={`Snap edits to ${sketchGridSize}mm grid`}
        onclick={() => (snapToGrid = !snapToGrid)}
        disabled={generating}
      >
        SNAP
      </button>
      <label class="sketch-grid-control">
        <span>GRID</span>
        <input
          class="sketch-grid-control__input"
          type="number"
          step="0.0001"
          aria-label="GRID"
          value={sketchGridSize}
          oninput={(event) => {
            sketchGridSize = event.currentTarget.value;
          }}
          disabled={generating}
        />
      </label>
      <button class="btn btn-xs" onclick={deleteSelectedPoint} disabled={generating || !selectedPoint}>DELETE POINT</button>
      <button class="btn btn-xs" onclick={cleanupSketch} disabled={generating || (!strokes.length && !activeStroke)}>CLEAN UP</button>
      <button class="btn btn-xs" onclick={undoLastStroke} disabled={generating || (!strokes.length && !activeStroke)}>UNDO</button>
      <button class="btn btn-xs" onclick={clearSketch} disabled={generating || (!strokes.length && !activeStroke && !draft && !errorText)}>CLEAR</button>
      <button class="btn btn-xs" onclick={replaySketchDocumentSnapshot} disabled={generating || !sketchDocumentSnapshot}>REPLAY IR</button>
      {#if onClose}
        <button class="btn btn-xs" onclick={() => onClose?.()}>CLOSE SKETCH</button>
      {/if}
      <button class="btn btn-xs btn-primary sketch-workspace__primary-action" onclick={() => generateDraft('manual')} disabled={generating}>
        {generating ? 'GENERATING...' : 'PREVIEW NOW'}
      </button>
    </div>
  </header>

  <div class="sketch-workspace__body">
    <div class="sketch-workspace__panes">
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="sketch-pane sketch-pane--front"
        role="application"
        aria-label="Front sketch pane"
        onpointerdown={(event) => handlePanePointerDown(event, 'front')}
        onpointermove={(event) => handlePanePointerMove(event, 'front')}
        onpointerup={(event) => handlePanePointerUp(event, 'front')}
        onpointercancel={(event) => handlePanePointerUp(event, 'front')}
        onclick={(event) => handlePaneClick(event, 'front')}
        onwheel={(event) => handlePaneWheel(event, 'front')}
      >
        <div class="sketch-pane__label">FRONT <button class="sketch-pane__label-action" type="button" onclick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          resetPaneCamera('front');
        }}>RESET PAN</button></div>
        <svg bind:this={frontSvg} class="sketch-pane__drawing" viewBox={paneViewBox('front')} preserveAspectRatio="none" aria-hidden="true">
          {#each visibleStrokes('front') as stroke (stroke.primitiveId)}
            <polyline class:sketch-pane__stroke--closed={stroke.closed} points={pointsToSvg(displayStrokePoints(stroke))} fill="none" />
            {#each editablePointIndices(stroke) as pointIndex (`${stroke.primitiveId}-${pointIndex}`)}
              {@const point = stroke.points[pointIndex]}
              <circle
                class="sketch-point-handle"
                class:sketch-point-handle--active={(pointDrag?.primitiveId === stroke.primitiveId && pointDrag.pointIndex === pointIndex) ||
                  (selectedPoint?.primitiveId === stroke.primitiveId && selectedPoint.pointIndex === pointIndex)}
                cx={point[0]}
                cy={point[1]}
                r={POINT_HANDLE_RADIUS}
                role="button"
                tabindex="0"
                aria-label={`Edit ${stroke.primitiveId} point ${pointIndex}`}
                data-sketch-point-handle
                data-point-handle
                onclick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  suppressNextPaneClick = true;
                  if (activeStroke?.primitiveId === stroke.primitiveId && !stroke.closed && pointIndex === 0) {
                    closeActivePolyline();
                  } else {
                    selectPoint(stroke, pointIndex);
                  }
                }}
                onkeydown={(event) => {
                  if (event.key !== 'Enter' && event.key !== ' ') {
                    return;
                  }
                  event.preventDefault();
                  event.stopPropagation();
                  suppressNextPaneClick = true;
                  if (activeStroke?.primitiveId === stroke.primitiveId && !stroke.closed && pointIndex === 0) {
                    closeActivePolyline();
                  } else {
                    selectPoint(stroke, pointIndex);
                  }
                }}
                onpointerdown={(event) => beginPointDrag(event, stroke, pointIndex)}
              />
            {/each}
          {/each}
          {#if frontHiddenLineOverlay}
            <g
              class="sketch-pane__brep-overlay"
              class:sketch-pane__brep-overlay--fail={hiddenLineViewHasIssue('front')}
              class:sketch-pane__brep-overlay--warn={hiddenLineProjectionStatus('front') === 'warn'}
              data-brep-hidden-line-overlay="front"
              data-brep-projection-status={hiddenLineProjectionStatus('front')}
            >
              {#each frontHiddenLineOverlay.visibleEdges ?? [] as edge (edge.edgeId)}
                {#if edge.points?.length}
                  <polyline class="sketch-pane__brep-edge sketch-pane__brep-edge--visible" points={hiddenLineEdgePoints(edge)} fill="none" data-brep-edge="visible" />
                {/if}
              {/each}
              {#each frontHiddenLineOverlay.hiddenEdges ?? [] as edge (edge.edgeId)}
                {#if edge.points?.length}
                  <polyline class="sketch-pane__brep-edge sketch-pane__brep-edge--hidden" points={hiddenLineEdgePoints(edge)} fill="none" data-brep-edge="hidden" />
                {/if}
              {/each}
            </g>
          {/if}
        </svg>
      </div>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="sketch-pane"
        role="application"
        aria-label="Top sketch pane"
        onpointerdown={(event) => handlePanePointerDown(event, 'top')}
        onpointermove={(event) => handlePanePointerMove(event, 'top')}
        onpointerup={(event) => handlePanePointerUp(event, 'top')}
        onpointercancel={(event) => handlePanePointerUp(event, 'top')}
        onclick={(event) => handlePaneClick(event, 'top')}
        onwheel={(event) => handlePaneWheel(event, 'top')}
      >
        <div class="sketch-pane__label">TOP <button class="sketch-pane__label-action" type="button" onclick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          resetPaneCamera('top');
        }}>RESET PAN</button></div>
        <svg bind:this={topSvg} class="sketch-pane__drawing" viewBox={paneViewBox('top')} preserveAspectRatio="none" aria-hidden="true">
          {#each visibleStrokes('top') as stroke (stroke.primitiveId)}
            <polyline class:sketch-pane__stroke--closed={stroke.closed} points={pointsToSvg(displayStrokePoints(stroke))} fill="none" />
            {#each editablePointIndices(stroke) as pointIndex (`${stroke.primitiveId}-${pointIndex}`)}
              {@const point = stroke.points[pointIndex]}
              <circle
                class="sketch-point-handle"
                class:sketch-point-handle--active={(pointDrag?.primitiveId === stroke.primitiveId && pointDrag.pointIndex === pointIndex) ||
                  (selectedPoint?.primitiveId === stroke.primitiveId && selectedPoint.pointIndex === pointIndex)}
                cx={point[0]}
                cy={point[1]}
                r={POINT_HANDLE_RADIUS}
                role="button"
                tabindex="0"
                aria-label={`Edit ${stroke.primitiveId} point ${pointIndex}`}
                data-sketch-point-handle
                data-point-handle
                onclick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  suppressNextPaneClick = true;
                  if (activeStroke?.primitiveId === stroke.primitiveId && !stroke.closed && pointIndex === 0) {
                    closeActivePolyline();
                  } else {
                    selectPoint(stroke, pointIndex);
                  }
                }}
                onkeydown={(event) => {
                  if (event.key !== 'Enter' && event.key !== ' ') {
                    return;
                  }
                  event.preventDefault();
                  event.stopPropagation();
                  suppressNextPaneClick = true;
                  if (activeStroke?.primitiveId === stroke.primitiveId && !stroke.closed && pointIndex === 0) {
                    closeActivePolyline();
                  } else {
                    selectPoint(stroke, pointIndex);
                  }
                }}
                onpointerdown={(event) => beginPointDrag(event, stroke, pointIndex)}
              />
            {/each}
          {/each}
          {#if topHiddenLineOverlay}
            <g
              class="sketch-pane__brep-overlay"
              class:sketch-pane__brep-overlay--fail={hiddenLineViewHasIssue('top')}
              class:sketch-pane__brep-overlay--warn={hiddenLineProjectionStatus('top') === 'warn'}
              data-brep-hidden-line-overlay="top"
              data-brep-projection-status={hiddenLineProjectionStatus('top')}
            >
              {#each topHiddenLineOverlay.visibleEdges ?? [] as edge (edge.edgeId)}
                {#if edge.points?.length}
                  <polyline class="sketch-pane__brep-edge sketch-pane__brep-edge--visible" points={hiddenLineEdgePoints(edge)} fill="none" data-brep-edge="visible" />
                {/if}
              {/each}
              {#each topHiddenLineOverlay.hiddenEdges ?? [] as edge (edge.edgeId)}
                {#if edge.points?.length}
                  <polyline class="sketch-pane__brep-edge sketch-pane__brep-edge--hidden" points={hiddenLineEdgePoints(edge)} fill="none" data-brep-edge="hidden" />
                {/if}
              {/each}
            </g>
          {/if}
        </svg>
      </div>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="sketch-pane"
        role="application"
        aria-label="Side sketch pane"
        onpointerdown={(event) => handlePanePointerDown(event, 'side')}
        onpointermove={(event) => handlePanePointerMove(event, 'side')}
        onpointerup={(event) => handlePanePointerUp(event, 'side')}
        onpointercancel={(event) => handlePanePointerUp(event, 'side')}
        onclick={(event) => handlePaneClick(event, 'side')}
        onwheel={(event) => handlePaneWheel(event, 'side')}
      >
        <div class="sketch-pane__label">SIDE <button class="sketch-pane__label-action" type="button" onclick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          resetPaneCamera('side');
        }}>RESET PAN</button></div>
        <svg bind:this={sideSvg} class="sketch-pane__drawing" viewBox={paneViewBox('side')} preserveAspectRatio="none" aria-hidden="true">
          {#each visibleStrokes('side') as stroke (stroke.primitiveId)}
            <polyline class:sketch-pane__stroke--closed={stroke.closed} points={pointsToSvg(displayStrokePoints(stroke))} fill="none" />
            {#each editablePointIndices(stroke) as pointIndex (`${stroke.primitiveId}-${pointIndex}`)}
              {@const point = stroke.points[pointIndex]}
              <circle
                class="sketch-point-handle"
                class:sketch-point-handle--active={(pointDrag?.primitiveId === stroke.primitiveId && pointDrag.pointIndex === pointIndex) ||
                  (selectedPoint?.primitiveId === stroke.primitiveId && selectedPoint.pointIndex === pointIndex)}
                cx={point[0]}
                cy={point[1]}
                r={POINT_HANDLE_RADIUS}
                role="button"
                tabindex="0"
                aria-label={`Edit ${stroke.primitiveId} point ${pointIndex}`}
                data-sketch-point-handle
                data-point-handle
                onclick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  suppressNextPaneClick = true;
                  if (activeStroke?.primitiveId === stroke.primitiveId && !stroke.closed && pointIndex === 0) {
                    closeActivePolyline();
                  } else {
                    selectPoint(stroke, pointIndex);
                  }
                }}
                onkeydown={(event) => {
                  if (event.key !== 'Enter' && event.key !== ' ') {
                    return;
                  }
                  event.preventDefault();
                  event.stopPropagation();
                  suppressNextPaneClick = true;
                  if (activeStroke?.primitiveId === stroke.primitiveId && !stroke.closed && pointIndex === 0) {
                    closeActivePolyline();
                  } else {
                    selectPoint(stroke, pointIndex);
                  }
                }}
                onpointerdown={(event) => beginPointDrag(event, stroke, pointIndex)}
              />
            {/each}
          {/each}
          {#if sideHiddenLineOverlay}
            <g
              class="sketch-pane__brep-overlay"
              class:sketch-pane__brep-overlay--fail={hiddenLineViewHasIssue('side')}
              class:sketch-pane__brep-overlay--warn={hiddenLineProjectionStatus('side') === 'warn'}
              data-brep-hidden-line-overlay="side"
              data-brep-projection-status={hiddenLineProjectionStatus('side')}
            >
              {#each sideHiddenLineOverlay.visibleEdges ?? [] as edge (edge.edgeId)}
                {#if edge.points?.length}
                  <polyline class="sketch-pane__brep-edge sketch-pane__brep-edge--visible" points={hiddenLineEdgePoints(edge)} fill="none" data-brep-edge="visible" />
                {/if}
              {/each}
              {#each sideHiddenLineOverlay.hiddenEdges ?? [] as edge (edge.edgeId)}
                {#if edge.points?.length}
                  <polyline class="sketch-pane__brep-edge sketch-pane__brep-edge--hidden" points={hiddenLineEdgePoints(edge)} fill="none" data-brep-edge="hidden" />
                {/if}
              {/each}
            </g>
          {/if}
        </svg>
      </div>
    </div>

    <div class="sketch-workspace__inspect">
      <SketchInspectorSection title="PRIMITIVES" ariaLabel="Sketch primitives">
        {#snippet summaryExtra()}
          <span>{strokes.length ? `${strokes.length} profile${strokes.length === 1 ? '' : 's'}` : 'EMPTY'}</span>
        {/snippet}
        <div class="sketch-workspace__section sketch-workspace__section--primitives">
          <div class="sketch-primitive-list">
            {#each [...strokes, ...(activeStroke ? [activeStroke] : [])] as stroke (stroke.primitiveId)}
              <div class="sketch-token">{stroke.primitiveId} / {stroke.view} / {stroke.closed ? 'closed' : 'open'}</div>
            {:else}
              <div class="sketch-token">NO PROFILE</div>
            {/each}
          </div>
        </div>
      </SketchInspectorSection>

      <SketchInspectorSection title="DRAFT MODE" ariaLabel="Draft mode">
        {#snippet summaryExtra()}
          <span>{draftSceneRow?.status?.toUpperCase() ?? draftModeSummary.label}</span>
        {/snippet}
        <div class="sketch-draft-mode">
          {#if draftSceneRow}
            <div class="sketch-token">MESH DRAFT {draftSceneRow.status.toUpperCase()}</div>
            <div class="sketch-token">{draftSceneRow.detail}</div>
            {#if draftSceneRow.action}
              <button
                class="btn btn-xs btn-primary"
                type="button"
                onclick={() => runWorkspaceSceneAction('draft')}
                disabled={generating || brepCandidateLoading || brepCandidateAcceptingSolutionId !== null}
              >
                {workspaceSceneActionLabel(draftSceneRow.action, {
                  generating,
                  acceptingSolutionId: brepCandidateAcceptingSolutionId,
                })}
              </button>
            {/if}
          {/if}
          <div class="sketch-token">{draftModeSummary.detail}</div>
        </div>
      </SketchInspectorSection>

	      {#if dimensionSummary}
	        <div class="sketch-dimensions" aria-label="Dimensions and constraints">
          <div class="sketch-workspace__section-title">DIMENSIONS/CONSTRAINTS</div>
          <div class="sketch-dimensions__grid">
            <div class="sketch-dimensions__cell">
              <span>WIDTH</span>
              <strong>{formatNumber(dimensionSummary.width)}MM</strong>
            </div>
            <div class="sketch-dimensions__cell">
              <span>HEIGHT</span>
              <strong>{formatNumber(dimensionSummary.height)}MM</strong>
            </div>
            <div class="sketch-dimensions__cell">
              <span>DEPTH</span>
              <strong>{formatNumber(dimensionSummary.depth)}MM</strong>
            </div>
          </div>
          <div class="sketch-dimensions__constraints">
            {#each dimensionSummary.constraints as constraint}
              <span>{constraint} CONSTRAINT</span>
            {/each}
          </div>
        </div>
      {/if}

      {#if dimensionConstraintSummary}
        <div class="sketch-dimension-constraints" aria-label="Dimension constraints">
          <div class="sketch-workspace__section-title">DIMENSION CONSTRAINTS</div>
          <div class="sketch-dimension-constraints__rows">
            <div class="sketch-dimension-constraints__row" data-locked={dimensionConstraintSummary.widthLocked}>
              <span>WIDTH</span>
              <strong>{dimensionConstraintSummary.width}MM</strong>
              <em>{dimensionConstraintSummary.widthLocked ? 'LOCKED' : 'UNLOCKED'}</em>
            </div>
            <div class="sketch-dimension-constraints__row" data-locked={dimensionConstraintSummary.heightLocked}>
              <span>HEIGHT</span>
              <strong>{dimensionConstraintSummary.height}MM</strong>
              <em>{dimensionConstraintSummary.heightLocked ? 'LOCKED' : 'UNLOCKED'}</em>
            </div>
          </div>
        </div>
      {/if}

      {#if sourceFitSeed}
        <div class="sketch-source-fit" aria-label="Source fit report">
          <div class="sketch-source-fit__header">
            <div class="sketch-workspace__section-title">SOURCE FIT REPORT</div>
            <span>SOURCE-BACKED / {sourceFitSeed.status.toUpperCase()}</span>
          </div>
          <div class="sketch-source-fit__rows">
            {#each sourceFitSeed.rows as row (row.id)}
              <div class="sketch-source-fit__row" data-status={row.status}>
                <span class="sketch-source-fit__label">{row.label.toUpperCase()}</span>
                <span class="sketch-source-fit__status">{row.status.toUpperCase()}</span>
                <span class="sketch-source-fit__message">{row.message}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <SketchInspectorSection title="STEP LEDGER" ariaLabel="Step ledger">
        {#snippet summaryExtra()}
          <span>{previewStep.label}</span>
        {/snippet}
        <div class="sketch-ledger">
        <div class="sketch-ledger__rows">
          <details class="sketch-ledger__row" data-state={profileLedgerState} data-step-ledger-row open={profileLedgerState === 'failed'}>
            <summary class="sketch-ledger__row-summary">
              <span class="sketch-ledger__dot"></span>
              <span class="sketch-ledger__label">PROFILE</span>
              <span class="sketch-ledger__state">{profileLedgerState}</span>
              <span class="sketch-ledger__summary-detail">{compactInspectorDetail(profileLedgerDetail)}</span>
            </summary>
            <div class="sketch-ledger__detail">{profileLedgerDetail}</div>
          </details>
          <details class="sketch-ledger__row" data-state={previewStep.state} data-step-ledger-row open={previewStep.state === 'failed'}>
            <summary class="sketch-ledger__row-summary">
              <span class="sketch-ledger__dot"></span>
              <span class="sketch-ledger__label">PREVIEW</span>
              <span class="sketch-ledger__state">{previewStep.label}</span>
              <span class="sketch-ledger__summary-detail">
                {compactInspectorDetail(`${autoPreviewPrimitiveId ? `${autoPreviewPrimitiveId} / ` : ''}${previewStep.detail}`)}
              </span>
            </summary>
            <div class="sketch-ledger__detail">
              {#if autoPreviewPrimitiveId}{autoPreviewPrimitiveId} / {/if}{previewStep.detail}
            </div>
          </details>
          <details class="sketch-ledger__row" data-state={suggestionLedgerState} data-step-ledger-row open={suggestionLedgerState === 'failed'}>
            <summary class="sketch-ledger__row-summary">
              <span class="sketch-ledger__dot"></span>
              <span class="sketch-ledger__label">FEATURE</span>
              <span class="sketch-ledger__state">{suggestionLedgerLabel}</span>
              <span class="sketch-ledger__summary-detail">{compactInspectorDetail(suggestionLedgerDetail)}</span>
            </summary>
            <div class="sketch-ledger__detail">{suggestionLedgerDetail}</div>
          </details>
        </div>
        </div>
      </SketchInspectorSection>

      <SketchInspectorSection title="POINT EDITOR" ariaLabel="Point editor">
        {#snippet summaryExtra()}
          <span>{selectedPoint ? `${selectedPoint.primitiveId} / ${selectedPoint.pointIndex}` : 'NO POINT'}</span>
        {/snippet}
        <div class="sketch-point-editor">
          <label class="sketch-point-editor__field">
            <span>POINT X</span>
            <input
              class="sketch-point-editor__input"
              type="text"
              inputmode="decimal"
              aria-label="POINT X"
              value={selectedPointX}
              oninput={(event) => {
                selectedPointX = event.currentTarget.value;
              }}
              disabled={generating || !selectedPoint}
            />
          </label>
          <label class="sketch-point-editor__field">
            <span>POINT Y</span>
            <input
              class="sketch-point-editor__input"
              type="text"
              inputmode="decimal"
              aria-label="POINT Y"
              value={selectedPointY}
              oninput={(event) => {
                selectedPointY = event.currentTarget.value;
              }}
              disabled={generating || !selectedPoint}
            />
          </label>
          <button class="btn btn-xs btn-primary" type="button" onclick={applySelectedPointCoordinates} disabled={generating || !selectedPoint}>
            APPLY POINT
          </button>
        </div>
      </SketchInspectorSection>

      <SketchInspectorSection title="PROFILE SIZE" ariaLabel="Profile size">
        {#snippet summaryExtra()}
          <span>{profileSizeTarget ? profileSizeTarget.primitiveId : 'NO PROFILE'}</span>
        {/snippet}
        <div class="sketch-profile-size-editor">
          <label class="sketch-profile-size-editor__field">
            <span>PROFILE X</span>
            <input
              class="sketch-profile-size-editor__input"
              type="text"
              inputmode="decimal"
              aria-label="PROFILE X"
              value={profileX}
              oninput={(event) => {
                profileX = event.currentTarget.value;
              }}
              disabled={generating || !profileSizeTarget}
            />
          </label>
          <label class="sketch-profile-size-editor__field">
            <span>PROFILE Y</span>
            <input
              class="sketch-profile-size-editor__input"
              type="text"
              inputmode="decimal"
              aria-label="PROFILE Y"
              value={profileY}
              oninput={(event) => {
                profileY = event.currentTarget.value;
              }}
              disabled={generating || !profileSizeTarget}
            />
          </label>
          <label class="sketch-profile-size-editor__field">
            <span>PROFILE WIDTH</span>
            <input
              class="sketch-profile-size-editor__input"
              type="text"
              inputmode="decimal"
              aria-label="PROFILE WIDTH"
              value={profileWidth}
              oninput={(event) => {
                profileWidth = event.currentTarget.value;
              }}
              disabled={generating || !profileSizeTarget}
            />
          </label>
          <label class="sketch-profile-size-editor__field">
            <span>PROFILE HEIGHT</span>
            <input
              class="sketch-profile-size-editor__input"
              type="text"
              inputmode="decimal"
              aria-label="PROFILE HEIGHT"
              value={profileHeight}
              oninput={(event) => {
                profileHeight = event.currentTarget.value;
              }}
              disabled={generating || !profileSizeTarget}
            />
          </label>
          <button class="btn btn-xs btn-primary" type="button" onclick={applyProfileSize} disabled={generating || !profileSizeTarget}>
            APPLY SIZE
          </button>
          <button class="btn btn-xs btn-primary" type="button" onclick={applyProfilePosition} disabled={generating || !profileSizeTarget}>
            APPLY POSITION
          </button>
          <button
            class="btn btn-xs btn-secondary"
            type="button"
            onclick={toggleProfileDimensionLocks}
            disabled={generating || !profileSizeTarget}
          >
            {dimensionConstraintSummary?.allLocked ? 'UNLOCK DIMENSIONS' : 'LOCK DIMENSIONS'}
          </button>
        </div>
      </SketchInspectorSection>

      {#if errorText}
        <div class="sketch-error" role="alert">{errorText}</div>
      {/if}

      {#if cleanupEvidenceText}
        <div class="sketch-cleanup-evidence" aria-label="Cleanup evidence">{cleanupEvidenceText}</div>
      {/if}

      {#if importRepairEvidenceText}
        <div class="sketch-import-repair" aria-label="Import repair">
          <div>{importRepairEvidenceText}</div>
          <button class="btn btn-xs btn-secondary" type="button" onclick={repairSketchDocumentImport} disabled={generating}>
            REPAIR IMPORT
          </button>
        </div>
      {/if}

      {#if sourcePatchEntries.length}
        <SketchInspectorSection title="SOURCE PATCH LEDGER" ariaLabel="Source patch ledger" className="sketch-source-patch-ledger">
          <div class="sketch-source-patch-ledger__rows">
            {#each sourcePatchEntries as entry (entry.patchId)}
              <div class="sketch-source-patch-ledger__row">
                <span>{entry.action}</span>
                <strong>{entry.primitiveId}</strong>
                <em>{entry.detail}</em>
              </div>
            {/each}
          </div>
        </SketchInspectorSection>
      {/if}

      {#if brepTopologyRepairProposals.length}
        <div class="sketch-topology-repair" aria-label="BRep topology repair proposals">
          <div class="sketch-workspace__section-title">TOPOLOGY REPAIR PROPOSALS</div>
          <div class="sketch-topology-repair__rows">
            {#each brepTopologyRepairProposals as proposal}
              <div class="sketch-topology-repair-proposal" data-brep-topology-repair-proposal={proposal.proposalId}>
                <span>{proposal.kind.toUpperCase()} {proposal.view?.toUpperCase() ?? 'UNKNOWN'} / {proposal.primitiveId ?? 'UNBOUND'} / {proposal.reason}</span>
                <button class="btn btn-xs btn-primary" type="button" onclick={() => applyBrepTopologyRepair(proposal)} disabled={generating}>
                  APPLY REDRAW SEED
                </button>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <SketchInspectorSection title="VALIDATION LEDGER" ariaLabel="Validation ledger" className="sketch-validation-ledger">
        <div class="sketch-validation-ledger__rows">
          {#each visibleValidationRows as row (row.id)}
            <details class="sketch-validation-ledger__row validation-row" data-status={row.status} data-validation-ledger-row open={row.status === 'fail'}>
              <summary class="sketch-validation-ledger__row-summary">
                <span class="sketch-validation-ledger__label">{row.label.toUpperCase()}</span>
                <span class="sketch-validation-ledger__status">{validationStatusLabel(row)}</span>
                <span class="sketch-validation-ledger__summary-detail">{compactInspectorDetail(row.detail)}</span>
              </summary>
              <div class="sketch-validation-ledger__detail">{row.detail}</div>
            </details>
          {/each}
        </div>
      </SketchInspectorSection>

      <SketchInspectorSection title="SKETCH DOCUMENT / SKETCH IR" ariaLabel="Sketch document source" className="sketch-document-source">
        {#snippet summaryExtra()}
          <span class="sketch-document-source__status">{sketchDocumentStatus}</span>
        {/snippet}

        {#if !sketchDocumentSourceSummary.error}
          <div class="sketch-token sketch-document-source__summary-line">{sketchDocumentSourceSummary.summary}</div>
          <div class="sketch-document-source__summary" aria-label="Sketch document summary">
            {#each sketchDocumentSourceSummary.rows as row (row.id)}
              <div class="sketch-document-source__summary-row">
                <span class="sketch-document-source__label">{row.id}</span>
                <span class="sketch-document-source__value">{row.value}</span>
              </div>
            {/each}
          </div>
          <details class="sketch-source-details sketch-document-source__details">
            <summary>SKETCH DOCUMENT / SKETCH IR JSON</summary>
            <pre class="sketch-source sketch-document-source__json">{sketchDocumentJson}</pre>
          </details>
        {:else}
          <div class="sketch-token">
            {openProfileCount > 0 ? 'CLOSE PROFILE TO BUILD IR' : 'DRAW CLOSED PROFILE TO BUILD IR'}
          </div>
        {/if}
      </SketchInspectorSection>

      <SketchInspectorSection title="SKETCH SOURCE" ariaLabel="Sketch import tools" className="sketch-document-import-shell">
        <div class="sketch-document-import" aria-label="Sketch document import">
          <div class="sketch-document-import__header">
            <div class="sketch-workspace__section-title">SKETCH DOCUMENT / IR</div>
            <button class="btn btn-xs btn-primary" type="button" onclick={importSketchDocumentSource} disabled={generating}>APPLY</button>
          </div>
          <textarea
            class="sketch-document-import__editor"
            aria-label="Sketch document or ecky source"
            spellcheck="false"
            bind:value={sketchDocumentImportText}
            oninput={() => {
              sketchDocumentEditorDirty = true;
            }}
          ></textarea>
        </div>
      </SketchInspectorSection>

      {#if showSuggestionPanel}
        <SketchInspectorSection title="SUGGESTED FEATURES" ariaLabel="Suggested features" className="sketch-suggestions">
          {#snippet summaryExtra()}
            {#if suggestingFeatures}
              <span class="sketch-suggestions__status">PENDING</span>
            {/if}
          {/snippet}
          {#if suggestionErrorText}
            <div class="sketch-suggestions__error" role="alert">{suggestionErrorText}</div>
          {/if}

          {#if featureSuggestions.length}
            <div class="sketch-suggestions__list">
              {#each featureSuggestions as suggestion (suggestion.suggestionId)}
                <div class="sketch-suggestion" class:sketch-suggestion--accepted={acceptedSuggestionId === suggestion.suggestionId}>
                  <div class="sketch-suggestion__top">
                    <span>{formatSuggestionLabel(suggestion)}</span>
                    <button
                      class="btn btn-xs btn-primary sketch-suggestion__action"
                      type="button"
                      aria-label={`Apply ${formatSuggestionLabel(suggestion)} suggestion`}
                      disabled={generating || acceptingSuggestionId !== null}
                      onclick={() => acceptSuggestion(suggestion)}
                    >
                      {acceptingSuggestionId === suggestion.suggestionId
                        ? 'APPLYING...'
                        : acceptedSuggestionId === suggestion.suggestionId
                          ? 'APPLIED'
                          : 'APPLY'}
                    </button>
                    <span class="sketch-suggestion__confidence">CONFIDENCE {formatConfidence(suggestion.confidence)}</span>
                  </div>
                  <div class="sketch-suggestion__reason">{suggestion.reason}</div>
                  {#if acceptedSuggestionId === suggestion.suggestionId}
                    <div class="sketch-suggestion__accepted">ACCEPTED INTO PREVIEW</div>
                  {/if}
                  {#if suggestion.warnings?.length}
                    <div class="sketch-suggestion__warnings">
                      {#each suggestion.warnings as warning}
                        <div>{warning}</div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {:else if suggestingFeatures}
            <div class="sketch-token">FETCHING SUGGESTIONS...</div>
          {/if}

          {#if suggestionWarnings.length}
            <div class="sketch-suggestion__warnings">
              {#each suggestionWarnings as warning}
                <div>{warning}</div>
              {/each}
            </div>
          {/if}
        </SketchInspectorSection>
      {/if}

      {#if !draft}
        <div class="sketch-coach">
          <div class="sketch-workspace__section-title">COACH</div>
          <div>Close profile, then preview.</div>
        </div>
      {/if}

      {#if draft}
        <SketchInspectorSection title={learningLens.title} ariaLabel="Learning lens" className="sketch-learning-lens-shell" open={false}>
          <div class="sketch-learning-lens">
            <div class="sketch-learning-lens__grid">
              <div class="sketch-learning-lens__diagram" aria-hidden="true">
                <svg viewBox="0 0 120 70" class="sketch-learning-lens__svg">
                  <polygon class="sketch-learning-lens__depth" points="42,16 78,28 78,56 42,44" />
                  <polygon class="sketch-learning-lens__profile" points="22,12 58,24 58,52 22,40" />
                  <line class="sketch-learning-lens__axis" x1="58" y1="24" x2="78" y2="28" />
                  <line class="sketch-learning-lens__axis" x1="58" y1="52" x2="78" y2="56" />
                  <path class="sketch-learning-lens__motion" d="M24 58 H78" />
                </svg>
              </div>
              <div class="sketch-learning-lens__copy">
                <div class="sketch-token">{learningLens.operationLabel}</div>
                <div>{learningLens.explanation}</div>
                <div class="sketch-learning-lens__math">{learningLens.formula}</div>
                <div class="sketch-learning-lens__math">{learningLens.domain}</div>
              </div>
            </div>
          </div>
        </SketchInspectorSection>

        {#if projections.length}
          <SketchInspectorSection title="PROJECTIONS" ariaLabel="Projection panel" className="sketch-projections">
            <div class="sketch-projections__list">
              {#each projections as projection (projection.view)}
                <div class="sketch-projection" aria-label={`${projection.view.toUpperCase()} projection`}>
                  <svg class="sketch-projection__svg" viewBox="0 0 100 100" role="img" aria-label={`${projection.view.toUpperCase()} mini view`}>
                    <rect class="sketch-projection__frame" x="8" y="8" width="84" height="84" />
                    {#if projection.role === 'source' && projection.path}
                      <path class="sketch-projection__profile" d={projection.path} />
                    {:else if projection.bounds}
                      <path class="sketch-projection__depth" d={projectionBoundsPath(projection.bounds)} />
                      <path class="sketch-projection__depth-axis" d={projectionDepthPath(projection.bounds)} />
                    {/if}
                  </svg>
                  <div class="sketch-projection__copy">
                    <div class="sketch-projection__view">{projection.view.toUpperCase()}</div>
                    <div class="sketch-projection__role">{projectionRoleLabel(projection)}</div>
                  </div>
                </div>
              {/each}
            </div>
          </SketchInspectorSection>
        {/if}

        {#if brepCandidateLoading || brepCandidateResponse || brepCandidateErrorText}
          <div class="sketch-brep-candidates" aria-label="BRep candidate graph">
            <div class="sketch-workspace__section-title">BREP CANDIDATE GRAPH</div>
            {#if exactSceneRow}
              <div class="sketch-token">EXACT MODEL {exactSceneRow.status.toUpperCase()}</div>
              <div class="sketch-token">{exactSceneRow.detail}</div>
              {#if exactSceneRow.action}
                <div class="sketch-brep-candidates__actions">
                  <button
                    class="btn btn-xs btn-primary"
                    type="button"
                    onclick={() => runWorkspaceSceneAction('exact')}
                    disabled={generating || brepCandidateLoading || brepCandidateAcceptingSolutionId !== null}
                  >
                    {workspaceSceneActionLabel(exactSceneRow.action, {
                      generating,
                      acceptingSolutionId: brepCandidateAcceptingSolutionId,
                    })}
                  </button>
                </div>
              {/if}
            {/if}
            {#if brepCandidateLoading}
              <div class="sketch-token">ANALYZING...</div>
            {/if}
            {#if brepCandidateErrorText}
              <div class="sketch-suggestions__error" role="alert">{brepCandidateErrorText}</div>
            {/if}
            {#if brepCandidateResponse}
              <div class="sketch-dimensions__grid">
                <div class="sketch-dimensions__cell">
                  <span>VERTICES</span>
                  <strong>{brepCandidateResponse.graph.vertices?.length ?? 0}</strong>
                </div>
                <div class="sketch-dimensions__cell">
                  <span>EDGES</span>
                  <strong>{brepCandidateResponse.graph.edges?.length ?? 0}</strong>
                </div>
                <div class="sketch-dimensions__cell">
                  <span>CELLS</span>
                  <strong>{brepCandidateResponse.search?.cells?.length ?? 0}</strong>
                </div>
                <div class="sketch-dimensions__cell">
                  <span>SOLUTIONS</span>
                  <strong>{brepCandidateResponse.search?.solutions?.length ?? 0}</strong>
                </div>
              </div>
              <div class="sketch-token">
                CANDIDATE SEARCH {(brepCandidateResponse.search?.solutions?.length ?? 0) > 0 ? 'READY' : 'PENDING'}
              </div>
              {#if brepCandidateResponse.validation.passed && brepCandidateResponse.search?.solutions?.length}
                <div class="sketch-brep-candidates__actions">
                  {#each brepCandidateResponse.search.solutions as solution (solution.solutionId)}
                    <button
                      class="btn btn-xs btn-primary"
                      type="button"
                      aria-label={`Accept candidate ${solution.solutionId}`}
                      disabled={generating || brepCandidateLoading || brepCandidateAcceptingSolutionId !== null || !brepCandidateDocument}
                      onclick={() => acceptBrepCandidateSolution(solution.solutionId)}
                    >
                      {brepCandidateAcceptingSolutionId === solution.solutionId
                        ? 'ACCEPTING...'
                        : brepCandidateAcceptedSolutionId === solution.solutionId
                          ? 'ACCEPTED'
                          : 'ACCEPT CANDIDATE'}
                    </button>
                  {/each}
                </div>
              {/if}
              {#if brepCandidateAcceptErrorText}
                <div class="sketch-suggestions__error" role="alert">{brepCandidateAcceptErrorText}</div>
              {/if}
              {#if brepCandidateAcceptedSolutionId}
                <div class="sketch-token">ACCEPTED BREP {brepCandidateAcceptedSolutionId}</div>
                <div class="sketch-token">{acceptedBrepTopologySummary()}</div>
                <div class="sketch-brep-candidates__actions">
                  <button
                    class="btn btn-xs btn-primary"
                    type="button"
                    aria-label="Create reusable package"
                    disabled={generating || brepComponentPackageLoading || !brepCandidateDocument || !acceptedBrepStepSourceRef()}
                    onclick={createAcceptedBrepComponentPackage}
                  >
                    {brepComponentPackageLoading ? 'PACKAGING...' : 'CREATE REUSABLE PACKAGE'}
                  </button>
                </div>
              {/if}
              {#if brepComponentPackageErrorText}
                <div class="sketch-suggestions__error" role="alert">{brepComponentPackageErrorText}</div>
              {/if}
              {#if brepComponentPackage}
                <div class="sketch-component-package" aria-label="Accepted BRep component package">
                  <div class="sketch-workspace__section-title">REUSABLE PACKAGE</div>
                  <div class="sketch-token">PACKAGE {brepComponentPackage.packageId}</div>
                  <div class="sketch-token">VERSION {brepComponentPackage.version}</div>
                  {#each brepComponentPackage.components ?? [] as component (component.componentId)}
                    <div class="sketch-token">COMPONENT {component.componentId}</div>
                    <div class="sketch-token">SKETCHES {component.sketches?.length ?? 0}</div>
                    <div class="sketch-token">SOURCE {component.sourceRef ? basename(component.sourceRef) : 'none'}</div>
                    {#each component.ports ?? [] as port (port.portId)}
                      <div class="sketch-token">PORT {port.portId}</div>
                      <div class="sketch-token">TYPE {port.typeId}</div>
                    {/each}
                  {/each}
                </div>
              {/if}
              {#if brepCandidateAcceptEvidence.length}
                <div class="sketch-validation-ledger__rows">
                  {#each brepCandidateAcceptEvidence as evidence}
                    <div class="sketch-token">{evidence}</div>
                  {/each}
                </div>
              {/if}
              {#if brepCandidateResponse.search?.evidence?.length}
                <div class="sketch-validation-ledger__rows">
                  {#each brepCandidateResponse.search.evidence as evidence}
                    <div class="sketch-token">{evidence}</div>
                  {/each}
                </div>
              {/if}
              <div class="sketch-token">
                PROJECTION REPLAY {brepCandidateResponse.validation.passed ? 'PASS' : 'FAIL'}
              </div>
              {#if brepCandidateResponse.validation.evidence?.length}
                <div class="sketch-validation-ledger__rows">
                  {#each brepCandidateResponse.validation.evidence as evidence}
                    <div class="sketch-token">{evidence}</div>
                  {/each}
                </div>
              {/if}
              {#if brepCandidateResponse.validation.issues?.length}
                <div class="sketch-suggestion__warnings">
                  {#each brepCandidateResponse.validation.issues as issue}
                    <div>{summarizeSketchValidationIssue(issue)}</div>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/if}

        {#if hiddenLineLoading || hiddenLineResponse || hiddenLineErrorText}
          <div class="sketch-hidden-line" aria-label="OCCT hidden-line projection">
            <div class="sketch-workspace__section-title">OCCT HIDDEN-LINE PROJECTION</div>
            {#if hiddenLineLoading}
              <div class="sketch-token">EXTRACTING...</div>
            {/if}
            {#if hiddenLineErrorText}
              <div class="sketch-suggestions__error" role="alert">{hiddenLineErrorText}</div>
            {/if}
            {#if hiddenLineResponse}
              <div class="sketch-validation-ledger__rows">
                {#each hiddenLineResponse.views as view}
                  <div class="sketch-token">{hiddenLineViewSummary(view)}</div>
                {/each}
              </div>
              <div class="sketch-token">SOURCE {basename(hiddenLineResponse.sourceArtifactPath)}</div>
              {#if brepDerivedSketch && !('error' in brepDerivedSketch)}
                <div class="sketch-derived-brep" aria-label="Derived BRep sketches">
                  <div class="sketch-workspace__section-title">DERIVED BREP SKETCHES</div>
                  <div class="sketch-token">DERIVED FROM BREP / NOT AUTHORING HISTORY</div>
                  <div class="sketch-validation-ledger__rows">
                    {#each brepDerivedSketch.views as view}
                      <div class="sketch-token">{view.toUpperCase()} editable seed</div>
                    {/each}
                  </div>
                  <button class="btn btn-xs btn-primary" type="button" onclick={convertDerivedBrepSketches} disabled={generating}>
                    CONVERT DERIVED SKETCHES
                  </button>
                </div>
              {:else if brepDerivedSketch && 'error' in brepDerivedSketch}
                <div class="sketch-token">{brepDerivedSketch.error}</div>
              {/if}
              {#if brepHiddenLineWarningMessages(hiddenLineResponse).length}
                <div class="sketch-suggestion__warnings">
                  {#each brepHiddenLineWarningMessages(hiddenLineResponse) as warning}
                    <div>{warning}</div>
                  {/each}
                </div>
              {/if}
              {#if hiddenLineResponse.validation}
                <div class="sketch-token">BREP/SKETCH {hiddenLineResponse.validation.passed ? 'PASS' : 'FAIL'}</div>
                {#if hiddenLineResponse.validation.evidence?.length}
                  <div class="sketch-validation-ledger__rows">
                    {#each hiddenLineResponse.validation.evidence as evidence}
                      <div class="sketch-token">{evidence}</div>
                    {/each}
                  </div>
                {/if}
                {#if hiddenLineResponse.validation.issues?.length}
                  <div class="sketch-suggestion__warnings">
                    {#each hiddenLineResponse.validation.issues as issue}
                      <div>{summarizeSketchValidationIssue(issue)}</div>
                    {/each}
                  </div>
                {/if}
                {#if brepSketchRepairTargets.length}
                  <div class="sketch-validation-ledger__rows" aria-label="BRep repair targets">
                    <div class="sketch-workspace__section-title">REPAIR TARGETS</div>
                    {#each brepSketchRepairTargets as target}
                      <div class="sketch-token" data-brep-repair-target={target.targetId}>
                        {target.severity.toUpperCase()} {target.label}{target.edgeId ? ` / ${target.edgeId}` : ''} / {target.reason}
                      </div>
                    {/each}
                  </div>
                {/if}
              {/if}
            {/if}
          </div>
        {/if}

        {#if artifactBundle}
          <div class="sketch-preview-summary" aria-label="Preview artifact summary">
            <div class="sketch-token">{basename(artifactBundle.previewStlPath)}</div>
            <div class="sketch-token">{artifactBundle.viewerAssets?.length ?? 0} assets</div>
            {#if draft.warnings?.length}
              {#each draft.warnings as warning}
                <div class="sketch-preview-summary__warning">{warning}</div>
              {/each}
            {/if}
          </div>
        {/if}

        <SketchInspectorSection title="ARTIFACT EVIDENCE / SOURCE STATUS" ariaLabel="Sketch artifact evidence" className="sketch-workspace__section" open={false}>
          <div class="sketch-token">
            {draft.sourceLanguage} / {draft.geometryBackend} / {sourceLineCount(draft.source)} lines
          </div>
          <details class="sketch-source-details">
            <summary>VIEW SOURCE</summary>
            <pre class="sketch-source">{draft.source}</pre>
          </details>
          {#if draft.warnings?.length}
            <div class="sketch-warnings">
              {#each draft.warnings as warning}
                <div>{warning}</div>
              {/each}
            </div>
          {/if}
          {#if artifactBundle}
            <div class="sketch-preview">
              <div class="sketch-workspace__section-title">MESH PREVIEW</div>
              <div class="sketch-token">model {artifactBundle.modelId}</div>
              <div class="sketch-token">{basename(artifactBundle.previewStlPath)}</div>
              <div class="sketch-token">{artifactBundle.viewerAssets?.length ?? 0} assets</div>
            </div>
          {/if}
        </SketchInspectorSection>
      {/if}
    </div>
  </div>
</div>

<style>
  .sketch-workspace {
    flex: 1 1 auto;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    overflow: hidden;
    background: var(--bg);
  }

  .sketch-workspace__header {
    flex: 0 0 auto;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-200) 88%, black 12%);
    overflow: hidden;
  }

  .sketch-workspace__header > div:first-child {
    flex: 0 1 240px;
    min-width: 160px;
    overflow: hidden;
  }

  .sketch-workspace__header h2 {
    margin: 0;
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    letter-spacing: 0.1em;
  }

  .sketch-workspace__actions {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    overflow: hidden;
  }

  .sketch-workspace__primary-action {
    order: -1;
    flex: 0 0 auto;
  }

  .sketch-grid-control {
    flex: 0 0 auto;
    display: inline-grid;
    grid-template-columns: auto 54px;
    align-items: center;
    gap: 5px;
    min-width: 0;
    padding: 4px 6px;
    border: 1px solid var(--bg-300);
    color: var(--text-dim);
    background: var(--bg-200);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.06em;
    overflow: hidden;
  }

  .sketch-grid-control__input {
    min-width: 0;
    width: 54px;
    border: 1px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    color: var(--text);
    background: color-mix(in srgb, var(--bg) 82%, black 18%);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    line-height: 1.2;
    padding: 2px 4px;
    outline: none;
  }

  .sketch-grid-control__input:focus {
    border-color: var(--secondary);
  }

  .sketch-workspace__meta,
  .sketch-workspace__section-title {
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.08em;
  }

  .sketch-workspace__body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) clamp(300px, 22vw, 420px);
    overflow: hidden;
  }

  .sketch-workspace__panes {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    grid-template-rows: repeat(2, minmax(0, 1fr));
    gap: 1px;
    background: var(--bg-300);
    overflow: hidden;
  }

  .sketch-pane {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    touch-action: none;
    cursor: crosshair;
    background:
      linear-gradient(rgba(255, 255, 255, 0.04) 1px, transparent 1px),
      linear-gradient(90deg, rgba(255, 255, 255, 0.04) 1px, transparent 1px),
      color-mix(in srgb, var(--bg-100) 84%, black 16%);
    background-size: 20px 20px, 20px 20px, auto;
  }

  .sketch-pane--front {
    grid-row: span 2;
  }

  .sketch-pane__label {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    letter-spacing: 0.12em;
  }

  .sketch-pane__label-action {
    appearance: none;
    border: 1px solid color-mix(in srgb, var(--secondary) 48%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 84%, black 16%);
    color: color-mix(in srgb, var(--secondary) 82%, white 18%);
    font-family: var(--font-mono);
    font-size: 0.54rem;
    line-height: 1;
    letter-spacing: 0.12em;
    padding: 3px 6px;
    min-height: 18px;
    text-transform: uppercase;
    cursor: pointer;
  }

  .sketch-pane__label-action:hover {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 10%, var(--bg-100));
  }

  .sketch-pane__label-action:focus-visible {
    outline: 1px solid color-mix(in srgb, var(--secondary) 72%, transparent);
    outline-offset: 1px;
  }

  .sketch-pane__drawing {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    pointer-events: none;
  }

  .sketch-pane__drawing polyline {
    stroke: var(--secondary);
    stroke-width: 1.5;
    stroke-linejoin: miter;
    stroke-linecap: square;
    vector-effect: non-scaling-stroke;
  }

  .sketch-pane__drawing .sketch-pane__stroke--closed {
    stroke: var(--primary);
  }

  .sketch-pane__drawing .sketch-pane__brep-overlay {
    pointer-events: none;
    opacity: 0.9;
  }

  .sketch-pane__drawing .sketch-pane__brep-edge {
    stroke-width: 1.2;
    stroke-linejoin: miter;
    stroke-linecap: square;
    vector-effect: non-scaling-stroke;
  }

  .sketch-pane__drawing .sketch-pane__brep-edge--visible {
    stroke: color-mix(in srgb, var(--primary) 62%, white 38%);
  }

  .sketch-pane__drawing .sketch-pane__brep-edge--hidden {
    stroke: var(--secondary);
    stroke-dasharray: 4 3;
  }

  .sketch-pane__drawing .sketch-pane__brep-overlay--fail .sketch-pane__brep-edge {
    stroke: #ff6b5f;
  }

  .sketch-pane__drawing .sketch-pane__brep-overlay--warn .sketch-pane__brep-edge {
    stroke: color-mix(in srgb, var(--secondary) 82%, white 18%);
  }

  .sketch-pane__drawing .sketch-point-handle {
    fill: color-mix(in srgb, var(--primary) 82%, white 18%);
    stroke: var(--bg);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
    pointer-events: all;
    cursor: move;
    transition:
      fill 120ms ease,
      stroke 120ms ease;
  }

  .sketch-pane__drawing .sketch-point-handle--active {
    fill: var(--secondary);
    stroke: var(--primary);
  }

  .sketch-workspace__inspect {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border-left: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 88%, black 12%);
    overflow: auto;
  }

  .sketch-workspace__section {
    flex: 0 0 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }

  .sketch-workspace__section--primitives {
    flex: 0 0 auto;
    max-height: none;
  }

  .sketch-primitive-list {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }

  .sketch-token {
    padding: 6px 8px;
    border: 1px solid var(--bg-300);
    color: var(--text);
    background: var(--bg-200);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-source-details {
    min-height: 0;
    overflow: hidden;
  }

  .sketch-source-details summary {
    padding: 6px 8px;
    border: 1px solid var(--bg-300);
    color: var(--secondary);
    background: var(--bg-200);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 0.68rem;
    overflow: hidden;
  }

  .sketch-source {
    flex: 1;
    max-height: none;
    margin: 0;
    padding: 10px;
    border: 1px solid color-mix(in srgb, var(--primary) 35%, var(--bg-300));
    color: var(--text);
    background: color-mix(in srgb, var(--bg) 84%, black 16%);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow: visible;
  }

  .sketch-warnings,
  .sketch-error,
  .sketch-cleanup-evidence,
  .sketch-import-repair,
  .sketch-brep-candidates,
  .sketch-hidden-line,
  .sketch-preview,
  .sketch-coach,
  .sketch-preview-summary,
  .sketch-source-fit,
  .sketch-dimension-constraints,
  .sketch-document-import,
  .sketch-learning-lens {
    padding: 8px;
    border: 1px solid color-mix(in srgb, var(--secondary) 40%, var(--bg-300));
    color: var(--secondary);
    background: color-mix(in srgb, var(--bg-200) 84%, black 16%);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow: hidden;
  }

  .sketch-preview-summary {
    flex: 0 0 auto;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-preview-summary__warning {
    grid-column: 1 / -1;
    min-width: 0;
    padding: 5px 6px;
    border: 1px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    color: var(--primary);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-coach {
    color: var(--text-dim);
    overflow: hidden;
  }

  .sketch-source-fit {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-source-fit__header {
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    overflow: hidden;
  }

  .sketch-source-fit__header span {
    min-width: 0;
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.56rem;
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-source-fit__rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .sketch-source-fit__row {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(74px, 1fr) 42px minmax(0, 1.2fr);
    gap: 6px;
    align-items: start;
    padding: 4px 5px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-source-fit__label,
  .sketch-source-fit__status,
  .sketch-source-fit__message {
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.56rem;
    letter-spacing: 0.05em;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-source-fit__status {
    color: var(--secondary);
  }

  .sketch-source-fit__row[data-status='fail'] .sketch-source-fit__status {
    color: #ef8a8a;
  }

  .sketch-dimension-constraints {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-dimension-constraints__rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .sketch-dimension-constraints__row {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(58px, 0.8fr) minmax(52px, 0.8fr) minmax(70px, 1fr);
    gap: 6px;
    align-items: center;
    padding: 4px 5px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-dimension-constraints__row span,
  .sketch-dimension-constraints__row strong,
  .sketch-dimension-constraints__row em {
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.58rem;
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-style: normal;
  }

  .sketch-dimension-constraints__row strong {
    color: var(--secondary);
  }

  .sketch-dimension-constraints__row em {
    color: var(--text-dim);
  }

  .sketch-dimension-constraints__row[data-locked='true'] em {
    color: var(--primary);
  }

  .sketch-validation-ledger__rows,
  .sketch-topology-repair__rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .sketch-topology-repair {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-topology-repair-proposal {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px;
    align-items: center;
    padding: 6px 8px;
    border: 1px solid var(--bg-300);
    color: var(--text);
    background: var(--bg-200);
    overflow: hidden;
  }

  .sketch-topology-repair-proposal span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-validation-ledger__row {
    min-width: 0;
    display: block;
    padding: 0;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-validation-ledger__label,
  .sketch-validation-ledger__status,
  .sketch-validation-ledger__summary-detail,
  .sketch-validation-ledger__detail {
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.58rem;
    line-height: 1.25;
    overflow: hidden;
  }

  .sketch-validation-ledger__label {
    color: var(--text);
    letter-spacing: 0.06em;
  }

  .sketch-validation-ledger__status {
    color: var(--secondary);
  }

  .sketch-validation-ledger__row-summary {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px;
    align-items: start;
    padding: 5px 6px;
    cursor: pointer;
    overflow: hidden;
  }

  .sketch-validation-ledger__row-summary::-webkit-details-marker {
    color: var(--primary);
  }

  .sketch-validation-ledger__summary-detail {
    grid-column: 1 / -1;
    color: var(--text-dim);
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .sketch-validation-ledger__detail {
    padding: 0 6px 6px;
    color: var(--text-dim);
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .sketch-validation-ledger__row[data-status='pass'] {
    color: #9ee6b3;
  }

  .sketch-validation-ledger__row[data-status='pass'] .sketch-validation-ledger__status {
    color: #9ee6b3;
  }

  .sketch-validation-ledger__row[data-status='fail'] {
    border-color: color-mix(in srgb, #ff6b5f 48%, var(--bg-300));
  }

  .sketch-validation-ledger__row[data-status='fail'] .sketch-validation-ledger__status,
  .sketch-validation-ledger__row[data-status='fail'] .sketch-validation-ledger__summary-detail,
  .sketch-validation-ledger__row[data-status='fail'] .sketch-validation-ledger__detail {
    color: #ffb1aa;
  }

  .sketch-validation-ledger__row[data-status='pending'] {
    color: var(--text-dim);
  }

  .sketch-document-source__status {
    min-width: 0;
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    letter-spacing: 0.08em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-document-source__summary {
    min-width: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 4px;
    overflow: hidden;
  }

  .sketch-document-source__summary-row {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
    gap: 5px;
    padding: 4px 5px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-document-source__label,
  .sketch-document-source__value {
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.56rem;
    line-height: 1.2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-document-source__label {
    color: var(--text-dim);
  }

  .sketch-document-source__value {
    color: var(--text);
  }

  .sketch-document-source__details {
    flex: 0 1 auto;
  }

  .sketch-document-source__json {
    max-height: none;
    font-size: 0.62rem;
    line-height: 1.35;
    white-space: pre;
  }

  .sketch-document-import {
    flex: 0 0 auto;
    max-height: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    white-space: normal;
    overflow: hidden;
  }

  .sketch-document-import__header {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    overflow: hidden;
  }

  .sketch-document-import__editor {
    min-height: 76px;
    resize: none;
    padding: 8px;
    border: 1px solid var(--bg-300);
    color: var(--text);
    background: color-mix(in srgb, var(--bg) 84%, black 16%);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    line-height: 1.35;
    outline: none;
    overflow: hidden;
  }

  .sketch-document-import__editor:focus {
    border-color: color-mix(in srgb, var(--primary) 60%, var(--bg-300));
  }

  .sketch-ledger {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-draft-mode {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-draft-mode .sketch-token,
  .sketch-ledger__detail,
  .sketch-validation-ledger__detail {
    text-overflow: clip;
  }

  .sketch-draft-mode .sketch-token {
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .sketch-point-editor,
  .sketch-profile-size-editor {
    flex: 0 0 auto;
    display: grid;
    gap: 6px;
    align-items: end;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-point-editor {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
  }

  .sketch-profile-size-editor {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .sketch-profile-size-editor > .btn {
    min-width: 0;
    width: 100%;
  }

  .sketch-profile-size-editor > .btn-secondary {
    grid-column: 1 / -1;
  }

  .sketch-point-editor__field,
  .sketch-profile-size-editor__field {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.56rem;
    letter-spacing: 0.06em;
    overflow: hidden;
  }

  .sketch-point-editor__input,
  .sketch-profile-size-editor__input {
    min-width: 0;
    width: 100%;
    border: 1px solid var(--bg-300);
    color: var(--text);
    background: color-mix(in srgb, var(--bg) 82%, black 18%);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    line-height: 1.2;
    padding: 4px 5px;
    outline: none;
    overflow: hidden;
  }

  .sketch-point-editor__input:focus,
  .sketch-profile-size-editor__input:focus {
    border-color: var(--secondary);
  }

  .sketch-ledger__rows {
    display: flex;
    flex-direction: column;
    gap: 5px;
    overflow: hidden;
  }

  .sketch-ledger__row {
    min-width: 0;
    display: block;
    padding: 0;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 76%, black 24%);
    overflow: hidden;
  }

  .sketch-ledger__dot {
    width: 7px;
    height: 7px;
    border: 1px solid currentColor;
    background: currentColor;
  }

  .sketch-ledger__label,
  .sketch-ledger__state,
  .sketch-ledger__summary-detail,
  .sketch-ledger__detail {
    min-width: 0;
    overflow: hidden;
  }

  .sketch-ledger__label,
  .sketch-ledger__state {
    font-size: 0.6rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .sketch-ledger__state {
    color: var(--secondary);
  }

  .sketch-ledger__row-summary {
    min-width: 0;
    display: grid;
    grid-template-columns: 8px minmax(64px, 0.9fr) auto;
    gap: 6px;
    align-items: start;
    padding: 5px 6px;
    cursor: pointer;
    overflow: hidden;
  }

  .sketch-ledger__row-summary::-webkit-details-marker {
    color: var(--primary);
  }

  .sketch-ledger__summary-detail {
    grid-column: 2 / -1;
    color: var(--text-dim);
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .sketch-ledger__detail {
    padding: 0 6px 6px 20px;
    color: var(--text-dim);
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .sketch-ledger__row[data-state='blocked'],
  .sketch-ledger__row[data-state='idle'] {
    color: var(--text-dim);
  }

  .sketch-ledger__row[data-state='queued'] {
    color: var(--secondary);
  }

  .sketch-ledger__row[data-state='generating'] {
    color: var(--primary);
  }

  .sketch-ledger__row[data-state='accepted'] {
    color: #9ee6b3;
  }

  .sketch-ledger__row[data-state='failed'] {
    color: #ffb1aa;
  }

	  .sketch-error {
	    border-color: color-mix(in srgb, #ff6b5f 48%, var(--bg-300));
	    color: #ffb1aa;
	  }

	  .sketch-cleanup-evidence {
	    border-color: color-mix(in srgb, var(--primary) 48%, var(--bg-300));
	    color: #9ee6b3;
	  }

	  .sketch-import-repair {
	    display: flex;
	    flex-direction: column;
	    gap: 6px;
	    border-color: color-mix(in srgb, var(--primary) 48%, var(--bg-300));
	    color: #9ee6b3;
	  }

	  .sketch-source-patch-ledger__rows {
	    display: flex;
	    flex-direction: column;
	    gap: 4px;
	    overflow: hidden;
	  }

	  .sketch-source-patch-ledger__row {
	    min-width: 0;
	    display: grid;
	    grid-template-columns: minmax(72px, 0.7fr) minmax(0, 1fr);
	    gap: 4px 6px;
	    padding: 4px 5px;
	    border: 1px solid var(--bg-300);
	    background: color-mix(in srgb, var(--bg) 78%, black 22%);
	    overflow: hidden;
	  }

	  .sketch-source-patch-ledger__row span,
	  .sketch-source-patch-ledger__row strong,
	  .sketch-source-patch-ledger__row em {
	    min-width: 0;
	    font-family: var(--font-mono);
	    font-size: 0.58rem;
	    line-height: 1.25;
	    overflow: hidden;
	    text-overflow: ellipsis;
	  }

	  .sketch-source-patch-ledger__row span {
	    color: var(--primary);
	  }

	  .sketch-source-patch-ledger__row strong {
	    color: var(--secondary);
	    font-weight: 700;
	  }

	  .sketch-source-patch-ledger__row em {
	    grid-column: 1 / -1;
	    color: var(--text-dim);
	    font-style: normal;
	    white-space: nowrap;
	  }

  .sketch-preview {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .sketch-suggestion__top {
    display: grid;
    gap: 8px;
    align-items: center;
    overflow: hidden;
  }

  .sketch-suggestion__top {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .sketch-suggestions__status,
  .sketch-suggestion__top span {
    min-width: 0;
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.08em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-suggestion__confidence {
    grid-column: 1 / -1;
  }

  .sketch-suggestion__action {
    min-width: 64px;
    padding: 4px 6px;
    font-size: 0.58rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-suggestions__list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }

  .sketch-suggestion {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 6px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-suggestion--accepted {
    border-color: color-mix(in srgb, #9ee6b3 58%, var(--bg-300));
    background: color-mix(in srgb, var(--bg) 72%, #102015 28%);
  }

  .sketch-suggestion__reason {
    color: var(--text);
    font-size: 0.64rem;
    line-height: 1.35;
    overflow: hidden;
  }

  .sketch-suggestion__accepted {
    padding: 4px 6px;
    border: 1px solid color-mix(in srgb, #9ee6b3 52%, var(--bg-300));
    color: #9ee6b3;
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    letter-spacing: 0.08em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-suggestion__warnings,
  .sketch-suggestions__error {
    padding: 5px 6px;
    border: 1px solid color-mix(in srgb, #ffb65f 42%, var(--bg-300));
    color: #ffd49a;
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    font-size: 0.62rem;
    line-height: 1.35;
    overflow: hidden;
  }

  .sketch-suggestions__error {
    border-color: color-mix(in srgb, #ff6b5f 48%, var(--bg-300));
    color: #ffb1aa;
    white-space: pre-wrap;
  }

  .sketch-brep-candidates {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-brep-candidates__actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    overflow: hidden;
  }

  .sketch-component-package {
    display: flex;
    flex-direction: column;
    gap: 5px;
    overflow: hidden;
  }

  .sketch-hidden-line {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-dimensions {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-dimensions__grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 5px;
    overflow: hidden;
  }

  .sketch-dimensions__cell {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 5px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-dimensions__cell span,
  .sketch-dimensions__cell strong {
    min-width: 0;
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-dimensions__cell span {
    color: var(--text-dim);
    font-size: 0.54rem;
    letter-spacing: 0.06em;
  }

  .sketch-dimensions__cell strong {
    color: var(--secondary);
    font-size: 0.64rem;
  }

  .sketch-dimensions__constraints {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    overflow: hidden;
  }

  .sketch-dimensions__constraints span {
    padding: 3px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 38%, var(--bg-300));
    color: var(--primary);
    background: color-mix(in srgb, var(--bg) 82%, black 18%);
    font-family: var(--font-mono);
    font-size: 0.56rem;
    letter-spacing: 0.05em;
  }

  .sketch-projections__list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }

  .sketch-projection {
    min-width: 0;
    display: grid;
    grid-template-columns: 58px minmax(0, 1fr);
    gap: 8px;
    align-items: center;
    padding: 5px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-projection__svg {
    width: 58px;
    height: 42px;
    border: 1px solid var(--bg-300);
    background:
      linear-gradient(rgba(255, 255, 255, 0.035) 1px, transparent 1px),
      linear-gradient(90deg, rgba(255, 255, 255, 0.035) 1px, transparent 1px),
      color-mix(in srgb, var(--bg-100) 74%, black 26%);
    background-size: 16px 16px, 16px 16px, auto;
    overflow: hidden;
  }

  .sketch-projection__frame {
    fill: none;
    stroke: color-mix(in srgb, var(--bg-300) 80%, var(--secondary) 20%);
    stroke-width: 1;
  }

  .sketch-projection__profile {
    fill: color-mix(in srgb, var(--primary) 16%, transparent);
    stroke: var(--primary);
    stroke-width: 2;
    stroke-linejoin: miter;
    vector-effect: non-scaling-stroke;
  }

  .sketch-projection__depth {
    fill: color-mix(in srgb, var(--secondary) 12%, transparent);
    stroke: var(--secondary);
    stroke-width: 2;
    stroke-linejoin: miter;
    vector-effect: non-scaling-stroke;
  }

  .sketch-projection__depth-axis {
    fill: none;
    stroke: var(--primary);
    stroke-width: 1.4;
    stroke-dasharray: 4 3;
    vector-effect: non-scaling-stroke;
  }

  .sketch-projection__copy {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .sketch-projection__view {
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    letter-spacing: 0.08em;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-projection__role {
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sketch-learning-lens {
    color: var(--text);
    overflow: hidden;
  }

  .sketch-learning-lens__grid {
    display: grid;
    grid-template-columns: 74px minmax(0, 1fr);
    gap: 8px;
    align-items: center;
    overflow: hidden;
  }

  .sketch-learning-lens__diagram {
    width: 74px;
    height: 56px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 72%, black 28%);
    overflow: hidden;
  }

  .sketch-learning-lens__svg {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .sketch-learning-lens__profile {
    fill: color-mix(in srgb, var(--primary) 18%, transparent);
    stroke: var(--primary);
    stroke-width: 2;
    stroke-linejoin: miter;
    animation: profile-pulse 1.7s ease-in-out infinite;
  }

  .sketch-learning-lens__depth {
    fill: color-mix(in srgb, var(--secondary) 18%, transparent);
    stroke: var(--secondary);
    stroke-width: 1.5;
    stroke-linejoin: miter;
    animation: depth-grow 1.7s ease-in-out infinite;
    transform-origin: 42px 35px;
  }

  .sketch-learning-lens__axis,
  .sketch-learning-lens__motion {
    fill: none;
    stroke: var(--secondary);
    stroke-width: 1;
    stroke-dasharray: 4 3;
  }

  .sketch-learning-lens__motion {
    stroke: var(--primary);
    stroke-width: 1.4;
    animation: motion-dash 1.7s linear infinite;
  }

  .sketch-learning-lens__copy {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
    overflow: hidden;
  }

  .sketch-learning-lens__copy .sketch-token {
    padding: 4px 6px;
  }

  .sketch-learning-lens__math {
    padding: 4px 6px;
    border: 1px solid var(--bg-300);
    color: var(--secondary);
    background: var(--bg);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes depth-grow {
    0%,
    100% {
      opacity: 0.45;
      transform: translateX(-8px);
    }

    50% {
      opacity: 1;
      transform: translateX(0);
    }
  }

  @keyframes profile-pulse {
    0%,
    100% {
      opacity: 0.75;
    }

    50% {
      opacity: 1;
    }
  }

  @keyframes motion-dash {
    to {
      stroke-dashoffset: -14;
    }
  }

  @media (max-width: 900px) {
    .sketch-workspace__body {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(0, 1fr) minmax(180px, 220px);
    }

    .sketch-workspace__inspect {
      border-left: 0;
      border-top: 1px solid var(--bg-300);
    }
  }
</style>
