<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import {
    analyzeSketchBrepCandidates,
    extractBrepHiddenLineProjections,
    formatBackendError,
    generateSketchDraftPreview,
    generateSketchPreviewHull,
    suggestSketchFeatures,
  } from './tauri/client';
  import type {
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
    summarizeSketchDraftMode,
    type SketchPoint,
    type SketchStroke,
  } from './sketchWorkspaceState';
  import { extrudeLearningLens } from './sketchLearningLens';
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
  import { buildSketchBrepProjectionValidationSummary } from './sketchBrepProjectionValidation';
  import { cleanupSketchStrokes } from './sketchCleanup';
  import { repairSketchDocumentDimensionConstraints } from './sketchConstraintValidation';
  import { buildSketchPreviewHullRequest, shouldUseSketchPreviewHull } from './sketchPreviewHull';
  import {
    appendSketchSourcePatch,
    compactRepairDetail,
    type SketchSourcePatchEntry,
  } from './sketchSourcePatchLedger';
  import type { ArtifactBundle } from './types/domain';

  type PreviewResult = { draft: SketchDraftSource; artifactBundle: ArtifactBundle } | null;
  type ProjectionRect = { x: number; y: number; width: number; height: number };
  type PreviewMode = 'manual' | 'auto';
  type PointDragState = {
    primitiveId: string;
    pointIndex: number;
    pointerId: number;
    view: SketchView;
    originalPoint: SketchPoint;
  };
  type SelectedPointState = {
    primitiveId: string;
    pointIndex: number;
    view: SketchView;
  };

  const EXTRUDE_AMOUNT = 12;
  const AUTO_PREVIEW_DEBOUNCE_MS = 650;
  const DEFAULT_SNAP_GRID_SIZE = '10';

  let {
    onPreviewResult = null,
    onGhostPreviewChange = null,
  }: {
    onPreviewResult?: ((result: PreviewResult) => void) | null;
    onGhostPreviewChange?: ((result: SketchGhostPreviewState | null) => void) | null;
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
  let pointDrag = $state<PointDragState | null>(null);
  let selectedPoint = $state<SelectedPointState | null>(null);
  let snapToGrid = $state(false);
  let sketchGridSize = $state(DEFAULT_SNAP_GRID_SIZE);
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
  let hiddenLineResponse = $state<BrepHiddenLineProjectionResponse | null>(null);
  let hiddenLineErrorText = $state('');
  let hiddenLineLoading = $state(false);
  const frontHiddenLineOverlay = $derived(hiddenLineResponse?.views?.find((view) => view.view === 'front') ?? null);
  const topHiddenLineOverlay = $derived(hiddenLineResponse?.views?.find((view) => view.view === 'top') ?? null);
  const sideHiddenLineOverlay = $derived(hiddenLineResponse?.views?.find((view) => view.view === 'side') ?? null);
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
  const learningLens = $derived.by(() => extrudeLearningLens(draftDepth));
  const projections = $derived.by(() => (projectionProfile ? buildSketchProjections(projectionProfile, draftDepth) : []));
  const dimensionSummary = $derived.by(() => (projectionProfile ? buildSketchDimensionSummary(projectionProfile, draftDepth) : null));
  const dimensionConstraintSummary = $derived.by(() => summarizeDimensionConstraints(profileSizeTarget));
  const sourceFitSeed = $derived.by(() =>
    projectionProfile
      ? buildSketchFitValidationSeed({
          profilePoints: projectionProfile.points.map(([x, y]) => ({ x, y })),
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
    }),
  );
  const brepSketchValidationSummary = $derived.by(() =>
    sketchDocumentSource && hiddenLineResponse
      ? buildSketchBrepProjectionValidationSummary(sketchDocumentSource, hiddenLineResponse)
      : null,
  );
  const visibleValidationRows = $derived.by(() => [
    ...validationRows,
    ...(brepSketchValidationSummary ? [brepSketchValidationLedgerRow(brepSketchValidationSummary)] : []),
  ]);
  const sketchDocumentSource = $derived.by(() => {
    const request = buildSketchSuggestionRequest(strokes);
    return 'error' in request ? null : request.document;
  });
  const sketchDocumentSourceSummary = $derived.by(() => buildSketchDocumentSourceSummary(sketchDocumentSource));
  const sketchDocumentJson = $derived.by(() => formatSketchDocumentSource(sketchDocumentSource));
  const sketchDocumentStatus = $derived.by(() => {
    if (!sketchDocumentSourceSummary.error) return 'READY';
    if (openProfileCount > 0) return 'PROFILE OPEN';
    return 'WAITING';
  });
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
    if (suggestingFeatures) return 'pending';
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
    if (sketchDocumentSource) {
      sketchDocumentSnapshot = sketchDocumentSource;
    }
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

  function beginStroke(event: PointerEvent, view: SketchView) {
    if (generating) return;

    const pointResult = pointForEditEvent(event, view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      activeStroke = null;
      return;
    }
    const point = pointResult.point;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    activeStroke = {
      primitiveId: `primitive-${view}-${++primitiveSequence}`,
      view,
      points: [point],
      closed: false,
    };
    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearImportRepair();
    clearSourcePatchLedger();
    clearPreviewResult();
    cleanupEvidenceText = '';
    errorText = '';
  }

  function extendStroke(event: PointerEvent) {
    if (!activeStroke || !event.buttons) return;

    const pointResult = pointForEditEvent(event, activeStroke.view);
    if ('error' in pointResult) {
      errorText = pointResult.error;
      activeStroke = null;
      clearPreviewResult();
      return;
    }
    const point = pointResult.point;
    const last = activeStroke.points[activeStroke.points.length - 1];
    if (last && Math.hypot(point[0] - last[0], point[1] - last[1]) < 0.8) return;

    activeStroke = {
      ...activeStroke,
      points: [...activeStroke.points, point],
    };
  }

  function endStroke(event: PointerEvent) {
    if (!activeStroke) return;

    const target = event.currentTarget as HTMLElement;
    if (target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    const finished = finishStroke(activeStroke);
    if (finished.points.length > 1) {
      strokes = [...strokes, finished];
      if (finished.closed) {
        queueAutoPreview(finished);
        requestFeatureSuggestions(strokes);
      }
    }
    activeStroke = null;
    clearSelectedPoint();
  }

  function beginPointDrag(event: PointerEvent, stroke: SketchStroke, pointIndex: number) {
    if (generating) return;

    event.preventDefault();
    event.stopPropagation();
    const target = event.currentTarget as Element;
    target.setPointerCapture(event.pointerId);
    pointDrag = {
      primitiveId: stroke.primitiveId,
      pointIndex,
      pointerId: event.pointerId,
      view: stroke.view,
      originalPoint: stroke.points[pointIndex],
    };
    selectPoint(stroke, pointIndex);
    clearAutoPreviewQueue();
    clearFeatureSuggestions();
    clearAcceptedSuggestion();
    clearImportRepair();
    clearPreviewResult();
    cleanupEvidenceText = '';
    errorText = '';
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

    if (isSamePoint(pointResult.point, pointDrag.originalPoint)) {
      pointDrag = null;
      return;
    }

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
    const target = event.currentTarget as Element;
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
    if (!pointDrag) return { error: 'Sketch point target missing.' };

    let movedStroke: SketchStroke | null = null;
    const nextStrokes = strokes.map((stroke) => {
      if (stroke.primitiveId !== pointDrag?.primitiveId || stroke.view !== pointDrag.view) return stroke;

      movedStroke = moveClosedStrokePointWithDimensionLocks(stroke, pointDrag.pointIndex, point);
      assertLockedDimensionsPreserved(stroke, movedStroke);
      return movedStroke;
    });

    if (!movedStroke) return { error: 'Sketch point target missing.' };

    strokes = nextStrokes;
    syncSelectedPointInputs(movedStroke, pointDrag.pointIndex);
    return { stroke: movedStroke, strokes: nextStrokes };
  }

  function isSamePoint(left: SketchPoint, right: SketchPoint): boolean {
    return Math.hypot(left[0] - right[0], left[1] - right[1]) < 0.01;
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
    ];
  }

  function closeOpenProfiles() {
    const hadOpenProfile = Boolean(activeStroke && !activeStroke.closed) || strokes.some((stroke) => !stroke.closed);
    strokes = strokes.map((stroke) => closeStroke(stroke));
    activeStroke = activeStroke ? closeStroke(activeStroke) : null;
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

    const replay = sketchDocumentToStrokes(parsed.document);
    if ('error' in replay) {
      errorText = replay.error;
      cleanupEvidenceText = '';
      primeImportRepair(parsed.document);
      clearPreviewResult();
      return;
    }

    strokes = replay.strokes;
    activeStroke = null;
    clearSelectedPoint();
    primitiveSequence = nextPrimitiveSequenceFromStrokes(replay.strokes);
    sketchDocumentSnapshot = parsed.document;
    sketchDocumentImportText = formatSketchDocumentSource(parsed.document);
    errorText = '';
    cleanupEvidenceText = '';
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

  async function generateDraft(mode: PreviewMode = 'manual', currentStrokes: SketchStroke[] = strokes) {
    clearAutoPreviewQueue();
    clearAcceptedSuggestion();
    const openError = currentStrokes.some((stroke) => !stroke.closed) || (activeStroke && !activeStroke.closed) ? 'Close profile before preview.' : '';
    if (openError) {
      errorText = openError;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    const request = buildSketchDraftRequest(currentStrokes);
    if ('error' in request) {
      errorText = request.error;
      cleanupEvidenceText = '';
      clearPreviewResult();
      return;
    }

    if (!suggestingFeatures && !suggestionResponse) {
      requestFeatureSuggestions(currentStrokes);
    }
    generating = true;
    autoQueued = mode === 'auto';
    autoPreviewPrimitiveId = request.sketch.primitives?.[0]?.primitiveId ?? null;
    errorText = '';
    clearPreviewResult();
    const runId = ++autoPreviewRunId;
    try {
      const usePreviewHull = shouldUseSketchPreviewHull(currentStrokes);
      const previewHullRequest = usePreviewHull ? assertPreviewHullRequest(currentStrokes) : null;
      const result = previewHullRequest
        ? await generateSketchPreviewHull(previewHullRequest)
        : await generateSketchDraftPreview(request);
      if (runId !== autoPreviewRunId) return;
      draft = result.draft;
      artifactBundle = result.artifactBundle;
      previewProfile = previewProfileFor(request.sketch.view, currentStrokes);
      syncSketchDocumentEnvelope(result.draft.source);
      autoQueued = false;
      publishPreviewResult(result);
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
  }

  async function loadHiddenLineProjection(bundle: ArtifactBundle, document: SketchDocument, runId: number) {
    hiddenLineResponse = null;
    hiddenLineErrorText = '';
    if (bundle.geometryBackend !== 'freecad' || !bundle.fcstdPath) {
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

  function hiddenLineProjectionStatus(view: SketchView): 'pass' | 'fail' {
    return hiddenLineViewHasIssue(view) ? 'fail' : 'pass';
  }

  function hiddenLineViewHasIssue(view: SketchView): boolean {
    const key = view.toLowerCase();
    const issues = hiddenLineResponse?.validation?.issues ?? [];
    if (issues.some((issue) => [issue.sketchId, issue.primitiveId ?? '', issue.message].some((text) => text.toLowerCase().includes(key)))) {
      return true;
    }
    if (hiddenLineResponse?.validation && !hiddenLineResponse.validation.passed && issues.length === 0) {
      return true;
    }
    return Boolean(hiddenLineResponse?.warnings?.some((warning) => warning.toLowerCase().includes(key)));
  }

  function hiddenLineEdgePoints(edge: BrepProjectedEdge2d): string {
    return pointsToSvg(edge.points ?? []);
  }

  function brepSketchValidationLedgerRow(summary: ReturnType<typeof buildSketchBrepProjectionValidationSummary>): SketchValidationRow {
    const backendValidation = hiddenLineResponse?.validation;
    const backendEvidence = backendValidation?.evidence?.filter(Boolean).join('; ') ?? '';
    const backendIssue = backendValidation?.issues
      ?.map((issue) => issue.message)
      .filter(Boolean)
      .join('; ') ?? '';
    const failingRow = summary.rows.find((row) => row.status === 'fail');
    const warning = hiddenLineResponse?.warnings?.find((item) => item.toLowerCase().includes('brep/sketch'));
    const viewEvidence = summary.viewSummaries
      .map((view) => `${view.view} ${view.visibleEdgeCount} visible / ${view.hiddenEdgeCount} hidden`)
      .join('; ');
    if (backendValidation) {
      if (!backendValidation.passed || backendIssue || warning) {
        return {
          id: 'brepSketchValidation',
          label: 'BRep/sketch validation',
          status: 'fail',
          detail: warning ?? backendIssue ?? backendEvidence ?? 'BRep/sketch validation failed.',
        };
      }
      return {
        id: 'brepSketchValidation',
        label: 'BRep/sketch validation',
        status: 'pass',
        detail: backendEvidence || viewEvidence || 'BRep/sketch validation passed.',
      };
    }
    if (failingRow || warning) {
      return {
        id: 'brepSketchValidation',
        label: 'BRep/sketch validation',
        status: 'fail',
        detail: warning ?? failingRow?.evidence ?? 'BRep/sketch validation failed.',
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

  function validationStatusLabel(row: SketchValidationRow): string {
    return row.status.toUpperCase();
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

<div class="sketch-workspace">
  <header class="sketch-workspace__header">
    <div>
      <h2>SKETCH WORKSPACE</h2>
      <div class="sketch-workspace__meta">ORTHOGRAPHIC SKETCH / EXTRUDE 12MM</div>
    </div>
    <div class="sketch-workspace__actions">
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
      <button class="btn btn-xs btn-primary" onclick={() => generateDraft('manual')} disabled={generating}>
        {generating ? 'GENERATING...' : 'PREVIEW NOW'}
      </button>
    </div>
  </header>

  <div class="sketch-workspace__body">
    <div class="sketch-workspace__panes">
      <div
        class="sketch-pane sketch-pane--front"
        role="application"
        aria-label="Front sketch pane"
        onpointerdown={(event) => beginStroke(event, 'front')}
        onpointermove={(event) => (pointDrag ? dragPoint(event) : extendStroke(event))}
        onpointerup={(event) => (pointDrag ? endPointDrag(event) : endStroke(event))}
        onpointercancel={(event) => (pointDrag ? endPointDrag(event) : endStroke(event))}
      >
        <div class="sketch-pane__label">FRONT</div>
        <svg bind:this={frontSvg} class="sketch-pane__drawing" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          {#each visibleStrokes('front') as stroke (stroke.primitiveId)}
            <polyline class:sketch-pane__stroke--closed={stroke.closed} points={pointsToSvg(stroke.points)} fill="none" />
            {#if stroke.closed}
              {#each editablePointIndices(stroke) as pointIndex (`${stroke.primitiveId}-${pointIndex}`)}
                {@const point = stroke.points[pointIndex]}
                <circle
                  class="sketch-point-handle"
                  class:sketch-point-handle--active={(pointDrag?.primitiveId === stroke.primitiveId && pointDrag.pointIndex === pointIndex) ||
                    (selectedPoint?.primitiveId === stroke.primitiveId && selectedPoint.pointIndex === pointIndex)}
                  cx={point[0]}
                  cy={point[1]}
                  r="2.8"
                  role="button"
                  tabindex="0"
                  aria-label={`Edit ${stroke.primitiveId} point ${pointIndex}`}
                  data-sketch-point-handle
                  data-point-handle
                  onpointerdown={(event) => beginPointDrag(event, stroke, pointIndex)}
                  onpointermove={dragPoint}
                  onpointerup={endPointDrag}
                  onpointercancel={endPointDrag}
                />
              {/each}
            {/if}
          {/each}
          {#if frontHiddenLineOverlay}
            <g
              class="sketch-pane__brep-overlay"
              class:sketch-pane__brep-overlay--fail={hiddenLineViewHasIssue('front')}
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
      <div
        class="sketch-pane"
        role="application"
        aria-label="Top sketch pane"
        onpointerdown={(event) => beginStroke(event, 'top')}
        onpointermove={(event) => (pointDrag ? dragPoint(event) : extendStroke(event))}
        onpointerup={(event) => (pointDrag ? endPointDrag(event) : endStroke(event))}
        onpointercancel={(event) => (pointDrag ? endPointDrag(event) : endStroke(event))}
      >
        <div class="sketch-pane__label">TOP</div>
        <svg bind:this={topSvg} class="sketch-pane__drawing" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          {#each visibleStrokes('top') as stroke (stroke.primitiveId)}
            <polyline class:sketch-pane__stroke--closed={stroke.closed} points={pointsToSvg(stroke.points)} fill="none" />
            {#if stroke.closed}
              {#each editablePointIndices(stroke) as pointIndex (`${stroke.primitiveId}-${pointIndex}`)}
                {@const point = stroke.points[pointIndex]}
                <circle
                  class="sketch-point-handle"
                  class:sketch-point-handle--active={(pointDrag?.primitiveId === stroke.primitiveId && pointDrag.pointIndex === pointIndex) ||
                    (selectedPoint?.primitiveId === stroke.primitiveId && selectedPoint.pointIndex === pointIndex)}
                  cx={point[0]}
                  cy={point[1]}
                  r="2.8"
                  role="button"
                  tabindex="0"
                  aria-label={`Edit ${stroke.primitiveId} point ${pointIndex}`}
                  data-sketch-point-handle
                  data-point-handle
                  onpointerdown={(event) => beginPointDrag(event, stroke, pointIndex)}
                  onpointermove={dragPoint}
                  onpointerup={endPointDrag}
                  onpointercancel={endPointDrag}
                />
              {/each}
            {/if}
          {/each}
          {#if topHiddenLineOverlay}
            <g
              class="sketch-pane__brep-overlay"
              class:sketch-pane__brep-overlay--fail={hiddenLineViewHasIssue('top')}
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
      <div
        class="sketch-pane"
        role="application"
        aria-label="Side sketch pane"
        onpointerdown={(event) => beginStroke(event, 'side')}
        onpointermove={(event) => (pointDrag ? dragPoint(event) : extendStroke(event))}
        onpointerup={(event) => (pointDrag ? endPointDrag(event) : endStroke(event))}
        onpointercancel={(event) => (pointDrag ? endPointDrag(event) : endStroke(event))}
      >
        <div class="sketch-pane__label">SIDE</div>
        <svg bind:this={sideSvg} class="sketch-pane__drawing" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          {#each visibleStrokes('side') as stroke (stroke.primitiveId)}
            <polyline class:sketch-pane__stroke--closed={stroke.closed} points={pointsToSvg(stroke.points)} fill="none" />
            {#if stroke.closed}
              {#each editablePointIndices(stroke) as pointIndex (`${stroke.primitiveId}-${pointIndex}`)}
                {@const point = stroke.points[pointIndex]}
                <circle
                  class="sketch-point-handle"
                  class:sketch-point-handle--active={(pointDrag?.primitiveId === stroke.primitiveId && pointDrag.pointIndex === pointIndex) ||
                    (selectedPoint?.primitiveId === stroke.primitiveId && selectedPoint.pointIndex === pointIndex)}
                  cx={point[0]}
                  cy={point[1]}
                  r="2.8"
                  role="button"
                  tabindex="0"
                  aria-label={`Edit ${stroke.primitiveId} point ${pointIndex}`}
                  data-sketch-point-handle
                  data-point-handle
                  onpointerdown={(event) => beginPointDrag(event, stroke, pointIndex)}
                  onpointermove={dragPoint}
                  onpointerup={endPointDrag}
                  onpointercancel={endPointDrag}
                />
              {/each}
            {/if}
          {/each}
          {#if sideHiddenLineOverlay}
            <g
              class="sketch-pane__brep-overlay"
              class:sketch-pane__brep-overlay--fail={hiddenLineViewHasIssue('side')}
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
      <div class="sketch-workspace__section sketch-workspace__section--primitives">
        <div class="sketch-workspace__section-title">PRIMITIVES</div>
        <div class="sketch-primitive-list">
          {#each strokes as stroke (stroke.primitiveId)}
            <div class="sketch-token">{stroke.primitiveId} / {stroke.view} / {stroke.closed ? 'closed' : 'open'}</div>
          {:else}
            <div class="sketch-token">NO PROFILE</div>
          {/each}
        </div>
	      </div>

	      <div class="sketch-draft-mode" aria-label="Draft mode">
	        <div class="sketch-workspace__section-title">DRAFT MODE</div>
	        <div class="sketch-token">{draftModeSummary.label} / {draftModeSummary.detail}</div>
	      </div>

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

      <div class="sketch-ledger" aria-label="Step ledger">
        <div class="sketch-workspace__section-title">STEP LEDGER</div>
        <div class="sketch-ledger__rows">
          <div class="sketch-ledger__row" data-state={profileLedgerState}>
            <span class="sketch-ledger__dot"></span>
            <span class="sketch-ledger__label">PROFILE</span>
            <span class="sketch-ledger__state">{profileLedgerState}</span>
            <span class="sketch-ledger__detail">{profileLedgerDetail}</span>
          </div>
          <div class="sketch-ledger__row" data-state={previewStep.state}>
            <span class="sketch-ledger__dot"></span>
            <span class="sketch-ledger__label">PREVIEW</span>
            <span class="sketch-ledger__state">{previewStep.label}</span>
            <span class="sketch-ledger__detail">
              {#if autoPreviewPrimitiveId}{autoPreviewPrimitiveId} / {/if}{previewStep.detail}
            </span>
          </div>
          <div class="sketch-ledger__row" data-state={suggestionLedgerState}>
            <span class="sketch-ledger__dot"></span>
            <span class="sketch-ledger__label">FEATURE</span>
            <span class="sketch-ledger__state">{suggestionLedgerLabel}</span>
            <span class="sketch-ledger__detail">{suggestionLedgerDetail}</span>
          </div>
        </div>
      </div>

      <div class="sketch-point-editor" aria-label="Point editor">
        <div class="sketch-workspace__section-title">POINT EDITOR</div>
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

      <div class="sketch-profile-size-editor" aria-label="Profile size editor">
        <div class="sketch-workspace__section-title">PROFILE SIZE</div>
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
        <div class="sketch-source-patch-ledger" aria-label="Source patch ledger">
          <div class="sketch-workspace__section-title">SOURCE PATCH LEDGER</div>
          <div class="sketch-source-patch-ledger__rows">
            {#each sourcePatchEntries as entry (entry.patchId)}
              <div class="sketch-source-patch-ledger__row">
                <span>{entry.action}</span>
                <strong>{entry.primitiveId}</strong>
                <em>{entry.detail}</em>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <div class="sketch-validation-ledger" aria-label="Validation ledger">
        <div class="sketch-workspace__section-title">VALIDATION LEDGER</div>
        <div class="sketch-validation-ledger__rows">
          {#each visibleValidationRows as row (row.id)}
            <div class="sketch-validation-ledger__row validation-row" data-status={row.status} data-validation-ledger-row>
              <span class="sketch-validation-ledger__label">{row.label.toUpperCase()}</span>
              <span class="sketch-validation-ledger__status">{validationStatusLabel(row)}</span>
              <span class="sketch-validation-ledger__detail">{row.detail}</span>
            </div>
          {/each}
        </div>
      </div>

      <div class="sketch-document-source" aria-label="Sketch document source">
        <div class="sketch-document-source__header">
          <div class="sketch-workspace__section-title">SKETCH DOCUMENT / SKETCH IR</div>
          <span class="sketch-document-source__status">{sketchDocumentStatus}</span>
        </div>

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
      </div>

      <div class="sketch-document-import" aria-label="Sketch document import">
        <div class="sketch-document-import__header">
          <div class="sketch-workspace__section-title">SKETCH DOCUMENT / ECKY IMPORT</div>
          <button class="btn btn-xs btn-primary" type="button" onclick={importSketchDocumentSource} disabled={generating}>IMPORT</button>
        </div>
        <textarea
          class="sketch-document-import__editor"
          aria-label="Sketch document or ecky source"
          spellcheck="false"
          bind:value={sketchDocumentImportText}
        ></textarea>
      </div>

      {#if showSuggestionPanel}
        <div class="sketch-suggestions" aria-label="Suggested features">
          <div class="sketch-suggestions__header">
            <div class="sketch-workspace__section-title">SUGGESTED FEATURES</div>
            {#if suggestingFeatures}
              <div class="sketch-suggestions__status">PENDING</div>
            {/if}
          </div>

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
        </div>
      {/if}

      {#if !draft}
        <div class="sketch-coach">
          <div class="sketch-workspace__section-title">COACH</div>
          <div>Close profile, then preview.</div>
        </div>
      {/if}

      {#if draft}
        <div class="sketch-learning-lens" aria-label="Learning lens">
          <div class="sketch-workspace__section-title">{learningLens.title}</div>
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

        {#if projections.length}
          <div class="sketch-projections" aria-label="Projection panel">
            <div class="sketch-workspace__section-title">PROJECTIONS</div>
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
          </div>
        {/if}

        {#if brepCandidateLoading || brepCandidateResponse || brepCandidateErrorText}
          <div class="sketch-brep-candidates" aria-label="BRep candidate graph">
            <div class="sketch-workspace__section-title">BREP CANDIDATE GRAPH</div>
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
              </div>
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
                    <div>{issue.message}</div>
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
              {#if hiddenLineResponse.warnings?.length}
                <div class="sketch-suggestion__warnings">
                  {#each hiddenLineResponse.warnings as warning}
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
                      <div>{issue.message}</div>
                    {/each}
                  </div>
                {/if}
              {/if}
            {/if}
          </div>
        {/if}

        <div class="sketch-workspace__section">
          <div class="sketch-workspace__section-title">SOURCE STATUS</div>
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
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .sketch-workspace {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg);
  }

  .sketch-workspace__header {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-200) 88%, black 12%);
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
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
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
    grid-template-columns: minmax(0, 1fr) minmax(190px, 260px);
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
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    letter-spacing: 0.12em;
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

  .sketch-pane__drawing .sketch-point-handle {
    fill: color-mix(in srgb, var(--primary) 82%, white 18%);
    stroke: var(--bg);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
    pointer-events: all;
    cursor: move;
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
    overflow: hidden;
  }

  .sketch-workspace__section {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }

  .sketch-workspace__section--primitives {
    flex: 0 0 auto;
    max-height: 88px;
  }

  .sketch-primitive-list {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: auto;
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
    max-height: 180px;
    margin: 0;
    padding: 10px;
    border: 1px solid color-mix(in srgb, var(--primary) 35%, var(--bg-300));
    color: var(--text);
    background: color-mix(in srgb, var(--bg) 84%, black 16%);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow: auto;
  }

	  .sketch-warnings,
	  .sketch-error,
	  .sketch-cleanup-evidence,
	  .sketch-import-repair,
	  .sketch-source-patch-ledger,
	  .sketch-draft-mode,
  .sketch-brep-candidates,
  .sketch-hidden-line,
	  .sketch-preview,
  .sketch-coach,
  .sketch-validation-ledger,
  .sketch-source-fit,
  .sketch-dimension-constraints,
  .sketch-document-source,
  .sketch-document-import,
  .sketch-ledger,
  .sketch-point-editor,
  .sketch-profile-size-editor,
  .sketch-learning-lens,
  .sketch-projections,
  .sketch-suggestions {
    padding: 8px;
    border: 1px solid color-mix(in srgb, var(--secondary) 40%, var(--bg-300));
    color: var(--secondary);
    background: color-mix(in srgb, var(--bg-200) 84%, black 16%);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow: auto;
  }

  .sketch-coach {
    color: var(--text-dim);
    overflow: hidden;
  }

  .sketch-validation-ledger {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
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

  .sketch-validation-ledger__rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .sketch-validation-ledger__row {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(88px, 1fr) 46px minmax(0, 1.2fr);
    gap: 6px;
    align-items: start;
    padding: 4px 5px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, black 22%);
    overflow: hidden;
  }

  .sketch-validation-ledger__label,
  .sketch-validation-ledger__status,
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

  .sketch-validation-ledger__detail {
    color: var(--text-dim);
    white-space: nowrap;
    text-overflow: ellipsis;
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
  .sketch-validation-ledger__row[data-status='fail'] .sketch-validation-ledger__detail {
    color: #ffb1aa;
  }

  .sketch-validation-ledger__row[data-status='fail'] .sketch-validation-ledger__detail {
    max-height: 54px;
    white-space: pre-wrap;
    overflow: auto;
    text-overflow: clip;
  }

  .sketch-validation-ledger__row[data-status='pending'] {
    color: var(--text-dim);
  }

  .sketch-document-source {
    flex: 0 0 auto;
    max-height: 178px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    white-space: normal;
    overflow: hidden;
  }

  .sketch-document-source:has(.sketch-document-source__details[open]) {
    max-height: 320px;
  }

  .sketch-document-source__header {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    overflow: hidden;
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
    max-height: 132px;
    font-size: 0.62rem;
    line-height: 1.35;
    white-space: pre;
  }

  .sketch-document-import {
    flex: 0 0 auto;
    max-height: 170px;
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
    overflow: auto;
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

  .sketch-point-editor .sketch-workspace__section-title,
  .sketch-profile-size-editor .sketch-workspace__section-title {
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
    display: grid;
    grid-template-columns: 8px minmax(50px, 0.7fr) minmax(58px, 0.7fr) minmax(0, 1.4fr);
    gap: 6px;
    align-items: center;
    padding: 5px 6px;
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
  .sketch-ledger__detail {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .sketch-ledger__detail {
    color: var(--text-dim);
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

	  .sketch-source-patch-ledger {
	    display: flex;
	    flex-direction: column;
	    gap: 6px;
	    overflow: hidden;
	  }

	  .sketch-source-patch-ledger__rows {
	    display: flex;
	    flex-direction: column;
	    gap: 4px;
	    overflow: auto;
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

  .sketch-suggestions {
    flex: 0 0 auto;
    max-height: 135px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-suggestions__header,
  .sketch-suggestion__top {
    display: grid;
    gap: 8px;
    align-items: center;
    overflow: hidden;
  }

  .sketch-suggestions__header {
    grid-template-columns: minmax(0, 1fr) auto;
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
    overflow: auto;
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
    overflow: auto;
  }

  .sketch-suggestions__error {
    border-color: color-mix(in srgb, #ff6b5f 48%, var(--bg-300));
    color: #ffb1aa;
    white-space: pre-wrap;
  }

  .sketch-projections {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
    overflow: hidden;
  }

  .sketch-brep-candidates {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text);
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

  @media (max-width: 760px) {
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
