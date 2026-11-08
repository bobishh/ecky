<script lang="ts">
  import LivePaperPanel from './LivePaperPanel.svelte';
  import LivePaperOverlay from './LivePaperOverlay.svelte';
  import { prepareLivePaperProfile, livePaperStateLabel, livePaperScopeToken, type LivePaperSnapshot } from './livePaper';
  import {
    normalizeProfileLoops,
    profileBounds,
    profileGeometrySettingsMatch,
    profileLoopError,
    profileLoopsForApply,
    profilePlacementForOperation,
    profileRequestIsCurrent,
    finishProfileViewRequest,
    profileReferenceForApply,
    profilePointFromPointer,
    profileDrawingBounds,
    profileCirclePoints,
    profileDisplayCurveArcPath,
    removeProfileLoop,
    profileSketchLoopsForApply,
    type ProfileApplyInput,
    type ProfileApplyOutcome,
    type ProfileDeriveInput,
    type ProfileDrawingInput,
    type ProfileDrawingProjectionResponse,
    type ProfileDisplayCurve,
    type ProfileDrawingVisibility,
    type ProfileGeometryOperation,
    type ProfileLoop,
    type ProfileLoopRole,
    type ProfilePlane,
    type ProfileGeometryResponse,
    type ProfilePoint,
  } from './profileWorkspace';
  import {
    calibratedDistanceMm,
    cloneDrawingPrimitive,
    drawingCandidateIsSelectable,
    drawingCandidatesForApply,
    validateDrawingCalibration,
    type DrawingPoint,
  } from './drawingPhoto';
  import type {
    DrawingImageIdentity,
    DrawingPrimitive,
    DrawingProfileCandidate,
    DrawingProfileReviewResponse,
    RecognizeDrawingProfileRequest as DrawingRecognitionRequest,
    DrawingRecognitionResponse,
    DrawingRegion,
    DrawingRectification,
    DrawingScaleCalibration,
    PreparedDrawingImage,
    ReviewDrawingProfileRequest,
  } from './tauri/contracts';
  import { formatBackendError } from './tauri/client';

  type ProfileView = 'front' | 'top' | 'side';
  type PaneCamera = { zoom: number; centerX: number; centerY: number };
  const MIN_CAMERA_ZOOM = 0.001;
  const MAX_CAMERA_ZOOM = 100_000;
  const clampCameraZoom = (zoom: number) => Math.min(MAX_CAMERA_ZOOM, Math.max(MIN_CAMERA_ZOOM, zoom));
  const PROFILE_VIEWS: ProfileView[] = ['front', 'top', 'side'];
  const PLANE_VIEW: Record<ProfilePlane, ProfileView> = { xy: 'top', xz: 'front', yz: 'side' };
  const VIEW_PLANE: Record<ProfileView, ProfilePlane> = { front: 'xz', top: 'xy', side: 'yz' };
  type ProfileEditorState = {
    scopeKey: string;
    loops: ProfileLoop[];
    undoItems: ProfileLoop[][];
    selectedLoopIndex: number;
    selectedPointIndex: number | null;
    draftLoopIndex: number | null;
    drawing: boolean;
    drawingTool: 'polyline' | 'rectangle' | 'circle';
    convertedExact: boolean;
    placementPlane: ProfilePlane;
    placementOffset: string;
  };

  type PhotoViewState = {
    scopeKey: string;
    prepared: PreparedDrawingImage;
    region: DrawingRegion;
    rectificationPoints: DrawingPoint[];
    aspectRatio: string;
    rotationDegrees: string;
    calibrationPoints: DrawingPoint[];
    knownDistanceMm: string;
    imageOpacity: number;
    recognition: DrawingRecognitionResponse | null;
    primitives: DrawingPrimitive[];
    selectedPrimitiveId: string | null;
    profiles: DrawingProfileCandidate[];
    selectedProfileIds: string[];
    profilesNeedReview: boolean;
    reviewDiagnostics: string[];
    drawing: boolean;
    correctionTool: 'line' | 'polyline';
    draftPointsMm: DrawingPoint[];
    accepted: boolean;
    pending: boolean;
    error: string;
  };

  type Props = {
    scopeKey: string;
    sourceAvailable: boolean;
    exactCadAvailable: boolean;
    sourceAccuracyHint?: string | null;
    disabledReason?: string | null;
    onDerive: (input: ProfileDeriveInput) => Promise<ProfileGeometryResponse>;
    onProjectDrawing: (input: ProfileDrawingInput) => Promise<ProfileDrawingProjectionResponse>;
    onApply: (input: ProfileApplyInput) => Promise<ProfileApplyOutcome>;
    onChooseDrawing: () => Promise<PreparedDrawingImage | null>;
    onRecognizeDrawing: (input: DrawingRecognitionRequest) => Promise<DrawingRecognitionResponse>;
    onReviewDrawing: (input: ReviewDrawingProfileRequest) => Promise<DrawingProfileReviewResponse>;
  };

  let {
    scopeKey,
    sourceAvailable,
    exactCadAvailable,
    sourceAccuracyHint = null,
    disabledReason = null,
    onDerive,
    onProjectDrawing,
    onApply,
    onChooseDrawing,
    onRecognizeDrawing,
    onReviewDrawing,
  }: Props = $props();

  let profileEdits = $state<Record<ProfileView, ProfileEditorState>>({
    front: createProfileEditor(''), top: createProfileEditor(''), side: createProfileEditor(''),
  });
  let geometryEvidence = $state<Record<ProfileView, { scopeKey: string; response: ProfileGeometryResponse } | null>>({ front: null, top: null, side: null });
  let projectionDrawings = $state<Record<ProfileView, { scopeKey: string; response: ProfileDrawingProjectionResponse } | null>>({ front: null, top: null, side: null });
  let viewPending = $state<Record<ProfileView, { scopeKey: string; requestId: number; kind: 'drawing' | 'profile' } | null>>({ front: null, top: null, side: null });
  let viewErrors = $state<Record<ProfileView, { scopeKey: string; message: string } | null>>({ front: null, top: null, side: null });
  let selectedView = $state<ProfileView>('front');
  let paneCameras = $state<Record<ProfileView, PaneCamera>>({ front: { zoom: 4, centerX: 0, centerY: 0 }, top: { zoom: 4, centerX: 0, centerY: 0 }, side: { zoom: 4, centerX: 0, centerY: 0 } });
  let paneElements = $state<Record<ProfileView, HTMLElement | null>>({ front: null, top: null, side: null });
  let paneSizes = $state<Record<ProfileView, { width: number; height: number }>>({ front: { width: 0, height: 0 }, top: { width: 0, height: 0 }, side: { width: 0, height: 0 } });
  let activePan = $state<{ view: ProfileView; pointerId: number; clientX: number; clientY: number; camera: PaneCamera } | null>(null);
  let viewRequestIds = $state<Record<ProfileView, number>>({ front: 0, top: 0, side: 0 });
  let deriveMode = $state<'exactProjection' | 'exactSection' | 'meshSection'>('exactProjection');
  let sectionPlane = $state<ProfilePlane>('xy');
  let sectionOffset = $state('0');
  let viewLayout = $state<'single' | 'allThree'>('single');
  let tolerance = $state('0.5');
  let projectionVisibility = $state<ProfileDrawingVisibility>('visibleEdges');
  let clearance = $state('0');
  let depth = $state('10');
  let partName = $state('profile-feature');
  let drawRole = $state<ProfileLoopRole>('outer');
  let drawParentIndex = $state<number | null>(null);
  let activeTool = $state<'polyline' | 'rectangle' | 'circle'>('polyline');
  let snapToGrid = $state(false);
  let sketchGridSize = $state('1');
  let errorText = $state('');
  let errorScopeKey = $state<string | null>(null);
  let statusText = $state('');
  let statusScopeKey = $state<string | null>(null);
  let pending = $state<'section' | 'apply' | null>(null);
  let pendingScopeKey = $state<string | null>(null);
  let requestSequence = 0;
  let activeRequestId = $state<number | null>(null);
  let drag = $state<{ loopIndex: number; pointIndex: number; pointerId: number; view: ProfileView } | null>(null);
  let photoVertexDrag = $state<{ primitiveId: string; pointIndex: number; pointerId: number; view: ProfileView } | null>(null);
  let photoStates = $state<Record<ProfileView, PhotoViewState | null>>({ front: null, top: null, side: null });
  let photoPickMode = $state<'region' | 'rectification' | 'calibration' | null>(null);
  let photoRegionStart = $state<DrawingPoint | null>(null);
  let photoRegionDraft = $state<DrawingRegion | null>(null);
  let photoPickOriginalPoints = $state<DrawingPoint[] | null>(null);
  let photoImportPending = $state(false);
  let photoRequestIds = $state<Record<ProfileView, number>>({ front: 0, top: 0, side: 0 });
  let selectedProfilePrimitiveId = $state<string | null>(null);
  let photoRequestSequence = 0;
  let photoImportRequestId = 0;
  let photoPrimitiveSequence = 0;
  let livePaper = $state.raw<LivePaperSnapshot | null>(null);
  let liveSelectedId = $state<string | null>(null);
  let liveScopeBinding = $state.raw<{ sourceScope: string; token: string } | null>(null);
  let liveScopeToken = $derived(liveScopeBinding?.sourceScope === scopeKey ? liveScopeBinding.token : '');

  // WebCrypto is asynchronous; retain the source scope to discard obsolete digests.
  $effect(() => {
    const sourceScope = scopeKey;
    let current = true;
    void livePaperScopeToken(sourceScope).then((token) => {
      if (!current) return;
      liveScopeBinding = { sourceScope, token };
      if (livePaper?.scopeKey === token) bindLivePaperView();
    }).catch((cause) => {
      if (current) setError(`Could not bind live paper to this model: ${formatBackendError(cause)}`);
    });
    return () => { current = false; };
  });

  function livePaperForView(view: ProfileView): LivePaperSnapshot | null {
    return liveScopeToken && livePaper?.view === view && livePaper.scopeKey === liveScopeToken ? livePaper : null;
  }

  function livePaperProfileView(): ProfileView | null {
    const view = livePaper?.view;
    return view === 'front' || view === 'top' || view === 'side' ? view : null;
  }

  function bindLivePaperView(): void {
    const view = livePaperProfileView();
    if (!view || !liveScopeToken || livePaper?.scopeKey !== liveScopeToken) return;
    selectedView = view;
    resetPaneCamera(view);
  }

  async function useLivePaperProfile(): Promise<void> {
    if (!livePaper || !liveScopeToken || livePaper.scopeKey !== liveScopeToken) throw new Error('Stop capture, then start live paper for the current model before using its profile.');
    const requestScope = scopeKey;
    const view = livePaperProfileView();
    if (!view) throw new Error('Choose FRONT, TOP, or SIDE for live paper.');
    const profile = await prepareLivePaperProfile(livePaper.sessionId, liveScopeToken);
    if (requestScope !== scopeKey) return;
    selectedView = view;
    const loops = profile.loops.map((loop, index) => {
      const geometry = profile.sketchLoops?.[index]?.geometry;
      return geometry?.kind === 'circle'
        ? { ...loop, points: profileCirclePoints(geometry.centerMm, geometry.radiusMm), analyticCircle: geometry }
        : loop;
    });
    geometryEvidence = { ...geometryEvidence, [view]: null };
    photoStates = { ...photoStates, [view]: null };
    replaceLoops(loops);
    patchProfileEditor(view, { placementPlane: VIEW_PLANE[view], placementOffset: '0', convertedExact: true });
    setError('');
    setStatus(`Live paper profile ready for normal Apply. Image-derived · check dimensions.${profile.diagnostics?.length ? ` ${profile.diagnostics.join(' ')}` : ''}`);
  }

  $effect(() => {
    const entries = PROFILE_VIEWS.flatMap((view) => paneElements[view] ? [[view, paneElements[view]!] as const] : []);
    if (!entries.length || typeof ResizeObserver === 'undefined') return;
    const viewsByElement = new Map(entries.map(([view, element]) => [element, view]));
    const observer = new ResizeObserver((observed) => {
      const nextSizes = { ...paneSizes };
      for (const entry of observed) {
        const view = viewsByElement.get(entry.target as HTMLElement);
        if (view) nextSizes[view] = { width: entry.contentRect.width, height: entry.contentRect.height };
      }
      paneSizes = nextSizes;
    });
    for (const [, element] of entries) observer.observe(element);
    return () => observer.disconnect();
  });

  let currentEdit = $derived(profileEditorFor(selectedView));
  let currentLoops = $derived(currentEdit.loops);
  let selectedLoopIndex = $derived(currentEdit.selectedLoopIndex);
  let selectedPointIndex = $derived(currentEdit.selectedPointIndex);
  let selectedLoop = $derived(currentLoops[selectedLoopIndex] ?? null);
  let draftLoopIndex = $derived(currentEdit.draftLoopIndex);
  let drawing = $derived(currentEdit.drawing);
  let visibleViews = $derived(viewLayout === 'allThree' ? PROFILE_VIEWS : [selectedView]);
  let currentPhoto = $derived(photoStates[selectedView]?.scopeKey === scopeKey ? photoStates[selectedView] : null);
  let photoDrawing = $derived(Boolean(currentPhoto?.drawing));
  let currentEvidence = $derived(currentPhoto?.recognition ? null : geometryEvidence[selectedView]?.scopeKey === scopeKey ? geometryEvidence[selectedView]?.response ?? null : null);
  let currentProjectionDrawing = $derived(projectionDrawingForView(selectedView));
  let acceptedPhotoPrimitives = $derived(currentPhoto?.accepted && currentPhoto.recognition
    ? drawingCandidatesForApply(currentPhoto.profiles, currentPhoto.selectedProfileIds, currentPhoto.primitives)
    : null);
  let photoProfileReady = $derived(Boolean(currentPhoto?.accepted && acceptedPhotoPrimitives?.length));
  let calibratedPreviewMm = $derived(currentPhoto?.recognition && currentPhoto.calibrationPoints.length === 2
    ? calibratedDistanceMm({
      startPx: currentPhoto.calibrationPoints[0]!,
      endPx: currentPhoto.calibrationPoints[1]!,
      distanceMm: Number(currentPhoto.knownDistanceMm),
    }, currentPhoto.recognition)
    : null);
  let selectedViewError = $derived(viewErrorFor(selectedView));
  let convertedSelectedEvidence = $derived(currentEdit.convertedExact);
  let isExactProfile = $derived(
    currentEvidence?.accuracy === 'exactBrep'
      && Boolean(currentEvidence.profileAsset)
      && !convertedSelectedEvidence,
  );
  let isConvertedPolygon = $derived(convertedSelectedEvidence && currentEvidence?.accuracy === 'exactBrep');
  let currentPending = $derived(pendingScopeKey === scopeKey ? pending : null);
  let recognitionReadinessReason = $derived(drawingRecognitionReadiness(
    currentPhoto,
    photoPickMode,
    photoImportPending,
    currentPending,
    tolerance,
  ));
  let currentViewsPending = $derived(PROFILE_VIEWS.some(viewPendingFor));
  let currentErrorText = $derived(errorScopeKey === scopeKey ? errorText : '');
  let currentStatusText = $derived(statusScopeKey === scopeKey ? statusText : '');
  let canUndo = $derived(currentEdit.undoItems.length > 0 && currentPending === null);
  let loopError = $derived(isExactProfile ? null : profileLoopError(currentLoops));

  function drawingRecognitionReadiness(
    photo: PhotoViewState | null,
    pickMode: typeof photoPickMode,
    photoOpening: boolean,
    profileOperation: string | null,
    toleranceText: string,
  ): string | null {
    if (!photo) return 'Add a drawing photo first.';
    if (photo.pending) return 'Recognition is running.';
    if (photoOpening) return 'Wait for the drawing photo to finish opening.';
    if (profileOperation) return 'Wait for the current profile operation to finish.';
    if (pickMode === 'region') return 'Finish the image crop, or cancel photo picking.';
    if (pickMode === 'rectification') {
      return `Pick rectangle corner ${Math.min(photo.rectificationPoints.length + 1, 4)} of 4, or cancel perspective correction.`;
    }
    if (pickMode === 'calibration') {
      return `Pick scale point ${Math.min(photo.calibrationPoints.length + 1, 2)} of 2, or cancel photo picking.`;
    }
    if (photo.calibrationPoints.length !== 2) {
      return `Pick 2 scale points on the image (${photo.calibrationPoints.length}/2 picked).`;
    }
    const knownDistanceMm = Number(photo.knownDistanceMm);
    if (!Number.isFinite(knownDistanceMm) || knownDistanceMm <= 0) {
      return 'Enter a positive known length in millimeters.';
    }
    const calibration = {
      startPx: photo.calibrationPoints[0]!,
      endPx: photo.calibrationPoints[1]!,
      distanceMm: knownDistanceMm,
    };
    const calibrationError = validateDrawingCalibration(calibration);
    if (calibrationError) return calibrationError;
    if (photo.rectificationPoints.length > 0 && photo.rectificationPoints.length !== 4) {
      return `Pick all 4 rectangle corners, or cancel perspective correction (${photo.rectificationPoints.length}/4 picked).`;
    }
    if (photo.rectificationPoints.length === 4) {
      const aspectRatio = Number(photo.aspectRatio);
      if (!Number.isFinite(aspectRatio) || aspectRatio <= 0) {
        return 'Enter a positive width-to-height ratio, or pick four corners again.';
      }
    }
    if (photo.region.widthPx < 4 || photo.region.heightPx < 4) {
      return 'Select an image crop at least 4 pixels wide and high.';
    }
    const toleranceMm = Number(toleranceText);
    if (!Number.isFinite(toleranceMm) || toleranceMm <= 0) {
      return 'Enter a positive tolerance in millimeters.';
    }
    return null;
  }
  let deriveOperation = $derived<ProfileGeometryOperation>({ kind: 'section', plane: sectionPlane, offsetMm: Number(sectionOffset) });
  let deriveAvailable = $derived(
    deriveMode === 'meshSection' ? sourceAvailable : exactCadAvailable,
  );
  let deriveSettingsMatch = $derived(
    !currentEvidence
      || (
        currentEvidence.toleranceMm === Number(tolerance)
        && (deriveMode === 'exactProjection'
          ? currentEvidence.operation.kind === 'projection' && currentEvidence.accuracy === 'exactBrep'
          : currentEvidence.operation.kind === 'section'
            && profileGeometrySettingsMatch(currentEvidence, deriveOperation, Number(tolerance))
            && currentEvidence.accuracy === (deriveMode === 'meshSection' ? 'approximateMesh' : 'exactBrep'))
      ),
  );
  let selectedVertex = $derived(
    selectedPointIndex === null ? null : currentLoops[selectedLoopIndex]?.points[selectedPointIndex] ?? null,
  );
  let sourceWarning = $derived(
    deriveAvailable ? '' : deriveMode === 'meshSection'
      ? 'Load a mesh artifact to extract an approximate section.'
      : 'Exact projection and section need a STEP model. FCStd-only sources need a STEP export.',
  );
  let staleEvidence = $derived(PROFILE_VIEWS.some((view) => geometryEvidence[view]?.scopeKey !== undefined && geometryEvidence[view]?.scopeKey !== scopeKey));
  let applyDisabled = $derived(
    Boolean(disabledReason)
      || currentPending !== null
      || viewPendingFor(selectedView)
      || drawing
      || photoDrawing
      || (!photoProfileReady && loopError !== null)
      || (currentPhoto?.pending ?? false)
      || (Boolean(currentPhoto) && !photoProfileReady)
      || !deriveSettingsMatch
      || !scopeKey
      || !Number.isFinite(Number(depth))
      || Number(depth) <= 0
      || !Number.isFinite(Number(clearance))
      || Number(clearance) < 0
      || partName.trim().length === 0,
  );

  function createProfileEditor(targetScope: string): ProfileEditorState {
    return {
      scopeKey: targetScope,
      loops: [],
      undoItems: [],
      selectedLoopIndex: 0,
      selectedPointIndex: null,
      draftLoopIndex: null,
      drawing: false,
      drawingTool: 'polyline',
      convertedExact: false,
      placementPlane: 'xy',
      placementOffset: '0',
    };
  }

  function profileEditorFor(view: ProfileView): ProfileEditorState {
    const editor = profileEdits[view];
    return editor.scopeKey === scopeKey ? editor : { ...createProfileEditor(scopeKey), placementPlane: VIEW_PLANE[view] };
  }

  function patchProfileEditor(view: ProfileView, patch: Partial<ProfileEditorState>): void {
    profileEdits = { ...profileEdits, [view]: { ...profileEditorFor(view), ...patch, scopeKey } };
  }

  function startEdit(): ProfileLoop[] {
    return currentEdit.loops;
  }

  function setError(message: string): void {
    errorText = message;
    errorScopeKey = scopeKey;
  }

  function setStatus(message: string): void {
    statusText = message;
    statusScopeKey = scopeKey;
  }

  function viewPendingFor(view: ProfileView): boolean {
    return viewPending[view]?.scopeKey === scopeKey;
  }

  function viewPendingKindFor(view: ProfileView): 'drawing' | 'profile' | null {
    const pendingView = viewPending[view];
    return pendingView?.scopeKey === scopeKey ? pendingView.kind : null;
  }

  function viewErrorFor(view: ProfileView): string {
    const error = viewErrors[view];
    return error?.scopeKey === scopeKey ? error.message : '';
  }

  function photoForView(view: ProfileView): PhotoViewState | null {
    const photo = photoStates[view];
    return photo?.scopeKey === scopeKey ? photo : null;
  }

  function updatePhoto(view: ProfileView, update: Partial<PhotoViewState>, invalidate = false): void {
    const current = photoForView(view);
    if (!current) return;
    const next: PhotoViewState = { ...current, ...update, accepted: invalidate ? false : update.accepted ?? current.accepted };
    if (invalidate) {
      next.recognition = null;
      next.primitives = [];
      next.profiles = [];
      next.selectedProfileIds = [];
      next.selectedPrimitiveId = null;
      next.profilesNeedReview = false;
      next.reviewDiagnostics = [];
      next.drawing = false;
      next.draftPointsMm = [];
      next.error = '';
      next.accepted = false;
      photoRequestIds = { ...photoRequestIds, [view]: ++photoRequestSequence };
    }
    photoStates = { ...photoStates, [view]: next };
  }

  async function chooseDrawingPhoto(): Promise<void> {
    const view = selectedView;
    const requestScope = scopeKey;
    const requestId = ++photoImportRequestId;
    photoImportPending = true;
    setError('');
    setStatus(`Opening a drawing photo for ${view.toUpperCase()}…`);
    try {
      const prepared = await onChooseDrawing();
      if (requestScope !== scopeKey || requestId !== photoImportRequestId || !prepared) return;
      const image = prepared.image;
      photoStates = {
        ...photoStates,
        [view]: {
          scopeKey: requestScope,
          prepared,
          region: { leftPx: 0, topPx: 0, widthPx: image.widthPx, heightPx: image.heightPx, rotationDegrees: 0 },
          rectificationPoints: [],
          aspectRatio: '',
          rotationDegrees: '0',
          calibrationPoints: [],
          knownDistanceMm: '',
          imageOpacity: 0.28,
          recognition: null,
          primitives: [],
          selectedPrimitiveId: null,
          profiles: [],
          selectedProfileIds: [],
          profilesNeedReview: false,
          reviewDiagnostics: [],
          drawing: false,
          correctionTool: 'polyline',
          draftPointsMm: [],
          accepted: false,
          pending: false,
          error: '',
        },
      };
      resetPaneCamera(view);
      selectedView = view;
      patchProfileEditor(view, { placementPlane: VIEW_PLANE[view], placementOffset: '0' });
      photoPickMode = null;
      photoPickOriginalPoints = null;
      setStatus(`Photo assigned to ${view.toUpperCase()}. Drag to crop, then set its scale.`);
    } catch (error) {
      if (requestScope === scopeKey && requestId === photoImportRequestId) {
        setError(formatBackendError(error));
        setStatus('Drawing photo could not be opened. Current profiles remain available.');
      }
    } finally {
      if (requestId === photoImportRequestId) photoImportPending = false;
    }
  }

  function sourcePointFromPaneEvent(event: PointerEvent | MouseEvent, view: ProfileView): DrawingPoint | null {
    const photo = photoForView(view);
    if (!photo || photo.recognition) return null;
    const element = event.currentTarget as HTMLElement;
    const point = profilePointFromPointer(
      event.clientX,
      event.clientY,
      element.getBoundingClientRect(),
      cameraBounds(view, boundsForView(view)),
    );
    return [point[0], photo.prepared.image.heightPx - point[1]];
  }

  function startPhotoRegion(event: PointerEvent, view: ProfileView): void {
    if (photoPickMode !== 'region' || !photoForView(view) || selectedView !== view) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    photoRegionStart = sourcePointFromPaneEvent(event, view);
    photoRegionDraft = null;
  }

  function movePhotoRegion(event: PointerEvent, view: ProfileView): void {
    if (photoPickMode !== 'region' || !photoRegionStart || selectedView !== view) return;
    const photo = photoForView(view);
    const point = sourcePointFromPaneEvent(event, view);
    if (!photo || !point) return;
    photoRegionDraft = {
      leftPx: Math.min(photoRegionStart[0], point[0]),
      topPx: Math.min(photoRegionStart[1], point[1]),
      widthPx: Math.abs(point[0] - photoRegionStart[0]),
      heightPx: Math.abs(point[1] - photoRegionStart[1]),
      rotationDegrees: Number(photo.rotationDegrees) || 0,
    };
  }

  function finishPhotoRegion(event: PointerEvent, view: ProfileView): void {
    if (photoPickMode !== 'region' || !photoRegionStart || selectedView !== view) return;
    movePhotoRegion(event, view);
    const region = photoRegionDraft;
    if (region && region.widthPx >= 4 && region.heightPx >= 4) {
      updatePhoto(view, { region, rectificationPoints: [], calibrationPoints: [] }, true);
      setStatus(`Image region selected: ${Math.round(region.widthPx)} × ${Math.round(region.heightPx)} pixels.`);
    } else {
      setError('Crop needs a visible rectangle at least four pixels wide and high.');
    }
    photoRegionStart = null;
    photoRegionDraft = null;
    photoPickMode = null;
    photoPickOriginalPoints = null;
  }

  function pickPhotoPoint(event: MouseEvent, view: ProfileView): void {
    if (!photoPickMode || photoPickMode === 'region' || selectedView !== view) return;
    const point = sourcePointFromPaneEvent(event, view);
    const photo = photoForView(view);
    if (!photo || !point) return;
    if (photoPickMode === 'rectification') {
      const points = [...photo.rectificationPoints, point].slice(0, 4);
      updatePhoto(view, { rectificationPoints: points }, true);
      if (points.length === 4) {
        photoPickMode = null;
        photoPickOriginalPoints = null;
        setStatus('Four rectangle corners set. Enter its known width-to-height ratio.');
      }
    } else {
      const points = [...photo.calibrationPoints, point].slice(0, 2);
      updatePhoto(view, { calibrationPoints: points }, true);
      if (points.length === 2) {
        photoPickMode = null;
        photoPickOriginalPoints = null;
        setStatus('Scale points set. Enter their known distance in millimeters.');
      }
    }
  }

  function beginPhotoPick(mode: 'region' | 'rectification' | 'calibration'): void {
    if (photoPickMode) cancelPhotoPicking();
    const photo = photoForView(selectedView);
    if (!photo) return;
    photoPickOriginalPoints = mode === 'rectification'
      ? photo.rectificationPoints.map(([x, y]) => [x, y])
      : mode === 'calibration'
        ? photo.calibrationPoints.map(([x, y]) => [x, y])
        : null;
    if (photo.recognition) {
      updatePhoto(selectedView, {}, true);
      resetPaneCamera(selectedView);
    }
    photoPickMode = mode;
    photoRegionStart = null;
    photoRegionDraft = null;
    if (mode === 'region') setStatus(`Drag a crop on the ${selectedView.toUpperCase()} image pane.`);
    else if (mode === 'rectification') updatePhoto(selectedView, { rectificationPoints: [] }, true);
    else updatePhoto(selectedView, { calibrationPoints: [] }, true);
  }

  function cancelPhotoPicking(): void {
    const mode = photoPickMode;
    if (!mode) return;
    const previousPoints = photoPickOriginalPoints?.map(([x, y]) => [x, y] as DrawingPoint) ?? [];
    if (mode === 'rectification') updatePhoto(selectedView, { rectificationPoints: previousPoints }, true);
    else if (mode === 'calibration') updatePhoto(selectedView, { calibrationPoints: previousPoints }, true);
    photoPickMode = null;
    photoPickOriginalPoints = null;
    photoRegionStart = null;
    photoRegionDraft = null;
    setStatus('Incomplete photo pick discarded. Existing crop and completed measurements remain.');
  }

  function setPhotoRotation(value: string): void {
    const rotationDegrees = Number(value);
    if (![0, 90, 180, 270].includes(rotationDegrees)) {
      setError('Rotation must be 0, 90, 180, or 270 degrees.');
      return;
    }
    updatePhoto(selectedView, {
      rotationDegrees: String(rotationDegrees),
      region: { ...currentPhoto!.region, rotationDegrees },
    }, true);
  }

  function clearDrawingPhoto(): void {
    photoStates = { ...photoStates, [selectedView]: null };
    photoPickMode = null;
    photoPickOriginalPoints = null;
    photoRegionStart = null;
    photoRegionDraft = null;
    resetPaneCamera(selectedView);
    setStatus('Drawing photo removed from this view.');
  }

  async function recognizeDrawing(): Promise<void> {
    const photo = currentPhoto;
    if (!photo) return;
    if (recognitionReadinessReason) {
      setError(recognitionReadinessReason);
      return;
    }
    const toleranceMm = Number(tolerance);
    const calibrationPoints = photo.calibrationPoints;
    const knownDistanceMm = Number(photo.knownDistanceMm);
    const calibration: DrawingScaleCalibration = {
      startPx: calibrationPoints[0]!,
      endPx: calibrationPoints[1]!,
      distanceMm: knownDistanceMm,
    };
    let rectification: DrawingRectification | undefined;
    if (photo.rectificationPoints.length === 4) {
      const aspectRatio = Number(photo.aspectRatio);
      rectification = {
        cornersPx: photo.rectificationPoints as DrawingRectification['cornersPx'],
        aspectRatio,
      };
    }

    const view = selectedView;
    const requestScope = scopeKey;
    const requestId = ++photoRequestSequence;
    photoRequestIds = { ...photoRequestIds, [view]: requestId };
    updatePhoto(view, { pending: true, error: '' }, false);
    setError('');
    setStatus(`Recognizing linework in ${view.toUpperCase()}…`);
    const request: DrawingRecognitionRequest = {
      image: photo.prepared.image,
      view,
      region: { ...photo.region, rotationDegrees: Number(photo.rotationDegrees) || 0 },
      rectification: rectification ?? null,
      calibration,
      toleranceMm,
    };
    try {
      const response = await onRecognizeDrawing(request);
      if (requestScope !== scopeKey || photoRequestIds[view] !== requestId || photoForView(view)?.prepared.image.digest !== photo.prepared.image.digest) return;
      const selectable = response.profiles.filter((candidate) => drawingCandidateIsSelectable(candidate, response.primitives));
      const selectedProfileIds = selectable.map((candidate) => candidate.id);
      const next: PhotoViewState = {
        ...photo,
        pending: false,
        error: '',
        recognition: response,
        primitives: response.primitives.map(cloneDrawingPrimitive),
        profiles: response.profiles,
        selectedProfileIds,
        profilesNeedReview: false,
        reviewDiagnostics: [],
        drawing: false,
        correctionTool: 'polyline',
        draftPointsMm: [],
        selectedPrimitiveId: response.primitives[0]?.id ?? null,
        accepted: false,
      };
      photoStates = { ...photoStates, [view]: next };
      resetPaneCamera(view);
      setStatus(response.profiles.length
        ? `${response.profiles.length} profile candidates found. Review and accept closed contours.`
        : 'No closed profile candidates found. Adjust the crop or repair the linework.');
    } catch (error) {
      if (requestScope === scopeKey && photoRequestIds[view] === requestId) {
        updatePhoto(view, { pending: false, error: formatBackendError(error) }, false);
        setError(formatBackendError(error));
        setStatus(`${view.toUpperCase()} recognition failed. Other views remain available.`);
      }
    } finally {
      const previous = photoStates[view];
      if (photoRequestIds[view] === requestId && previous?.scopeKey === requestScope) {
        photoStates = { ...photoStates, [view]: { ...previous, pending: false } };
      }
    }
  }

  function toggleDrawingCandidate(candidate: DrawingProfileCandidate, checked: boolean): void {
    if (!currentPhoto) return;
    const selectedProfileIds = new Set(currentPhoto.selectedProfileIds);
    if (checked) selectedProfileIds.add(candidate.id);
    else selectedProfileIds.delete(candidate.id);
    updatePhoto(selectedView, { selectedProfileIds: [...selectedProfileIds], accepted: false });
  }

  async function acceptDrawingProfile(): Promise<void> {
    const photo = currentPhoto;
    if (!photo?.recognition || photo.pending) return;
    if (photo.profilesNeedReview) {
      const view = selectedView;
      const requestScope = scopeKey;
      const requestId = ++photoRequestSequence;
      const evidenceId = photo.recognition.evidenceId;
      const digest = photo.prepared.image.digest;
      photoRequestIds = { ...photoRequestIds, [view]: requestId };
      updatePhoto(view, { pending: true, error: '' }, false);
      setError('');
      setStatus('Checking corrected contours…');
      try {
        const response = await onReviewDrawing({ evidenceId, primitives: photo.primitives });
        const latest = photoForView(view);
        if (requestScope !== scopeKey || photoRequestIds[view] !== requestId || latest?.prepared.image.digest !== digest) return;
        if (response.evidenceId !== evidenceId) throw new Error('Photo evidence changed during contour review. Review the current image again.');
        const selectableIds = new Set(response.profiles
          .filter((candidate) => drawingCandidateIsSelectable(candidate, photo.primitives))
          .map((candidate) => candidate.id));
        photoStates = {
          ...photoStates,
          [view]: {
            ...latest,
            pending: false,
            error: '',
            profiles: response.profiles,
            selectedProfileIds: photo.selectedProfileIds.filter((id) => selectableIds.has(id)),
            profilesNeedReview: false,
            reviewDiagnostics: response.diagnostics,
            accepted: false,
          },
        };
        setStatus(response.profiles.length
          ? 'Corrections checked. Review eligible contours, then accept the profile.'
          : 'No closed profile remains. Correct the linework or select a different image region.');
      } catch (error) {
        if (requestScope === scopeKey && photoRequestIds[view] === requestId) {
          updatePhoto(view, { pending: false, error: formatBackendError(error) }, false);
          setError(formatBackendError(error));
          setStatus('Contour review failed. Your edits remain available.');
        }
      } finally {
        const latest = photoForView(view);
        if (photoRequestIds[view] === requestId && latest?.prepared.image.digest === digest && latest.pending) {
          updatePhoto(view, { pending: false }, false);
        }
      }
      return;
    }
    const accepted = drawingCandidatesForApply(photo.profiles, photo.selectedProfileIds, photo.primitives);
    if (!accepted) {
      setError('Select a closed outer profile and any contained holes before accepting.');
      return;
    }
    updatePhoto(selectedView, { accepted: true }, false);
    setError('');
    setStatus('Image-derived profile accepted. Check dimensions before applying.');
  }

  function selectDrawingPrimitive(id: string): void {
    selectedProfilePrimitiveId = id;
    updatePhoto(selectedView, { selectedPrimitiveId: id, accepted: false }, false);
  }

  function updateDrawingPrimitiveNumber(id: string, field: 'x' | 'y' | 'x2' | 'y2' | 'radius', value: string): void {
    const number = Number(value);
    if (!Number.isFinite(number) || !currentPhoto) return;
    const primitives = currentPhoto.primitives.map((primitive) => {
      if (primitive.id !== id) return cloneDrawingPrimitive(primitive);
      if (primitive.kind === 'circle') {
        if (field === 'radius') return { ...primitive, radiusMm: number };
        const centerMm = [...primitive.centerMm] as DrawingPoint;
        centerMm[field === 'x' ? 0 : 1] = number;
        return { ...primitive, centerMm };
      }
      if (primitive.kind === 'line') {
        if (field === 'radius') return primitive;
        const startMm = [...primitive.startMm] as DrawingPoint;
        const endMm = [...primitive.endMm] as DrawingPoint;
        if (field === 'x') startMm[0] = number;
        if (field === 'y') startMm[1] = number;
        if (field === 'x2') endMm[0] = number;
        if (field === 'y2') endMm[1] = number;
        return { ...primitive, startMm, endMm };
      }
      return primitive;
    });
    updatePhoto(selectedView, { primitives, profilesNeedReview: true, accepted: false });
  }

  function updateDrawingPolylineVertex(id: string, pointIndex: number, axis: 0 | 1, value: string): void {
    const number = Number(value);
    if (!Number.isFinite(number) || !currentPhoto) return;
    const primitives = currentPhoto.primitives.map((primitive) => {
      if (primitive.id !== id || primitive.kind !== 'polyline') return cloneDrawingPrimitive(primitive);
      const pointsMm = primitive.pointsMm.map(([x, y]) => [x, y] as DrawingPoint);
      if (pointsMm[pointIndex]) pointsMm[pointIndex]![axis] = number;
      return { ...primitive, pointsMm };
    });
    updatePhoto(selectedView, { primitives, profilesNeedReview: true, accepted: false });
  }

  function removeDrawingPrimitive(id: string): void {
    if (!currentPhoto) return;
    const primitives = currentPhoto.primitives.filter((primitive) => primitive.id !== id);
    updatePhoto(selectedView, { primitives, selectedPrimitiveId: primitives[0]?.id ?? null, profilesNeedReview: true, accepted: false });
  }

  function replaceLoops(loops: ProfileLoop[]): void {
    rememberUndo();
    const normalized = normalizeProfileLoops(loops);
    patchProfileEditor(selectedView, {
      loops: normalized,
      selectedLoopIndex: selectedLoopIndex >= normalized.length ? Math.max(0, normalized.length - 1) : selectedLoopIndex,
      selectedPointIndex: null,
    });
  }

  function rememberUndo(): void {
    const editor = profileEditorFor(selectedView);
    const previous = editor.loops;
    const items = [...editor.undoItems];
    items.push(normalizeProfileLoops(previous));
    if (items.length > 40) items.shift();
    patchProfileEditor(selectedView, { undoItems: items });
  }

  function undoLastEdit(): void {
    const editor = profileEditorFor(selectedView);
    if (editor.undoItems.length === 0 || currentPending) return;
    const items = [...editor.undoItems];
    const previous = items.pop() ?? [];
    const loops = normalizeProfileLoops(previous);
    patchProfileEditor(selectedView, {
      undoItems: items,
      loops,
      selectedPointIndex: null,
      selectedLoopIndex: Math.max(0, loops.length - 1),
      drawing: false,
      draftLoopIndex: null,
    });
    setError('');
    setStatus('Last profile edit undone.');
  }

  function finiteValue(value: string, label: string): number | null {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) {
      setError(`${label} must be a finite number.`);
      return null;
    }
    return parsed;
  }

  async function deriveProfile(): Promise<void> {
    if (!deriveAvailable || currentPending || drawing) return;
    if (deriveMode === 'exactProjection') return;
    let offsetMm = 0;
    const parsedOffset = finiteValue(sectionOffset, 'Section offset');
    if (parsedOffset === null) return;
    offsetMm = parsedOffset;
    const toleranceMm = finiteValue(tolerance, 'Tolerance');
    if (toleranceMm === null) return;
    if (toleranceMm <= 0) {
      setError('Tolerance must be greater than zero.');
      return;
    }

    const operation: ProfileGeometryOperation = { kind: 'section', plane: sectionPlane, offsetMm };
    const requestScope = scopeKey;
    const requestId = ++requestSequence;
    activeRequestId = requestId;
    pendingScopeKey = requestScope;
    pending = 'section';
    setError('');
    setStatus(deriveMode === 'meshSection' ? 'Extracting approximate mesh section…' : 'Extracting exact CAD section…');
    try {
      const response = await onDerive({
        operation,
        toleranceMm,
        source: deriveMode === 'meshSection' ? 'mesh' : 'step',
      });
      if (!profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)) return;
      const view = PLANE_VIEW[response.operation.kind === 'section' ? response.operation.plane : 'xy'];
      geometryEvidence = { ...geometryEvidence, [view]: { scopeKey: requestScope, response } };
      viewErrors = { ...viewErrors, [view]: null };
      selectedView = view;
      const placement = profilePlacementForOperation(response.operation);
      const loops = response.accuracy === 'exactBrep' && response.profileAsset ? [] : response.loops;
      const outerIndex = loops.findIndex((loop) => loop.role === 'outer');
      profileEdits = {
        ...profileEdits,
        [view]: {
          ...createProfileEditor(requestScope),
          loops: normalizeProfileLoops(loops),
          selectedLoopIndex: Math.max(0, outerIndex),
          placementPlane: placement.plane,
          placementOffset: String(placement.offsetMm),
        },
      };
      resetPaneCamera(view);
      photoPickMode = null;
      setStatus(response.accuracy === 'exactBrep'
        ? 'Exact CAD outline ready. Convert it to a polygon before editing vertices.'
        : response.loops.length
          ? 'Approximate mesh profile ready. Edit vertices, then apply the profile.'
          : 'Section contained no closed profile loops.');
    } catch (error) {
      if (profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)) {
        setError(formatBackendError(error));
        setStatus('Profile derivation failed. Current profile remains available.');
      }
    } finally {
      if (activeRequestId === requestId) {
        pending = null;
        pendingScopeKey = null;
        activeRequestId = null;
      }
    }
  }

  async function projectAllDrawings(): Promise<void> {
    const toleranceMm = finiteValue(tolerance, 'Tolerance');
    if (toleranceMm === null) return;
    if (toleranceMm <= 0) {
      setError('Tolerance must be greater than zero.');
      return;
    }
    const requestScope = scopeKey;
    const visibility = projectionVisibility;
    setError('');
    setStatus(`Projecting ${visibility === 'allEdges' ? 'all edges' : 'visible edges'} into front, top, and side views…`);
    const tasks = PROFILE_VIEWS.map(async (view) => {
      const requestId = ++requestSequence;
      viewRequestIds = { ...viewRequestIds, [view]: requestId };
      viewPending = { ...viewPending, [view]: { scopeKey: requestScope, requestId, kind: 'drawing' } };
      viewErrors = { ...viewErrors, [view]: { scopeKey: requestScope, message: '' } };
      try {
        const response = await onProjectDrawing({ view, toleranceMm, visibility });
        if (requestScope !== scopeKey || viewRequestIds[view] !== requestId) return;
        projectionDrawings = { ...projectionDrawings, [view]: { scopeKey: requestScope, response } };
        viewErrors = { ...viewErrors, [view]: null };
        resetPaneCamera(view);
      } catch (error) {
        if (requestScope === scopeKey && viewRequestIds[view] === requestId) {
          viewErrors = { ...viewErrors, [view]: { scopeKey: requestScope, message: formatBackendError(error) } };
        }
      } finally {
        if (viewRequestIds[view] === requestId) {
          viewPending = { ...viewPending, [view]: finishProfileViewRequest(viewPending[view], requestId) };
        }
      }
    });
    await Promise.all(tasks);
    if (requestScope === scopeKey) {
      const failed = PROFILE_VIEWS.filter((view) => Boolean(viewErrorFor(view)));
      if (failed.length > 0) {
        setError(failed.map((view) => `${view.toUpperCase()}: ${viewErrorFor(view)}`).join('\n'));
      }
      setStatus(failed.length ? `${3 - failed.length} of 3 projections ready; failed views remain available for retry.` : 'Front, top, and side projected drawings ready. Build a closed profile in a view before applying it.');
    }
  }

  async function buildClosedProfile(): Promise<void> {
    const drawing = projectionDrawingForView(selectedView);
    if (!drawing || photoForView(selectedView) || !exactCadAvailable || currentPending !== null || currentViewsPending) return;
    const toleranceMm = finiteValue(tolerance, 'Tolerance');
    if (toleranceMm === null) return;
    if (toleranceMm <= 0) {
      setError('Tolerance must be greater than zero.');
      return;
    }
    const view = selectedView;
    const requestScope = scopeKey;
    const requestId = ++requestSequence;
    activeRequestId = requestId;
    pendingScopeKey = requestScope;
    pending = 'section';
    viewRequestIds = { ...viewRequestIds, [view]: requestId };
    viewPending = { ...viewPending, [view]: { scopeKey: requestScope, requestId, kind: 'profile' } };
    viewErrors = { ...viewErrors, [view]: null };
    setError('');
    setStatus(`Building a closed ${view.toUpperCase()} profile from the current model…`);
    try {
      const response = await onDerive({
        operation: { kind: 'projection', view },
        toleranceMm,
        source: 'step',
      });
      if (!profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)
        || viewRequestIds[view] !== requestId) return;
      geometryEvidence = { ...geometryEvidence, [view]: { scopeKey: requestScope, response } };
      const placement = profilePlacementForOperation(response.operation);
      const loops = response.accuracy === 'exactBrep' && response.profileAsset ? [] : response.loops;
      const outerIndex = loops.findIndex((loop) => loop.role === 'outer');
      profileEdits = {
        ...profileEdits,
        [view]: {
          ...createProfileEditor(requestScope),
          loops: normalizeProfileLoops(loops),
          selectedLoopIndex: Math.max(0, outerIndex),
          placementPlane: placement.plane,
          placementOffset: String(placement.offsetMm),
        },
      };
      resetPaneCamera(view);
      setError('');
      setStatus(`Exact ${view.toUpperCase()} closed profile ready. Projected drawing remains visible.`);
    } catch (error) {
      if (profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)
        && viewRequestIds[view] === requestId) {
        const message = formatBackendError(error);
        viewErrors = { ...viewErrors, [view]: { scopeKey: requestScope, message } };
        setError(message);
        setStatus(`Could not build a closed ${view.toUpperCase()} profile. Projected drawing remains available.`);
      }
    } finally {
      if (viewRequestIds[view] === requestId) {
        viewPending = { ...viewPending, [view]: finishProfileViewRequest(viewPending[view], requestId) };
      }
      if (activeRequestId === requestId) {
        pending = null;
        pendingScopeKey = null;
        activeRequestId = null;
      }
    }
  }

  function convertExactProfileToPolygon(): void {
    if (!currentEvidence || !isExactProfile) return;
    replaceLoops(currentEvidence.loops);
    patchProfileEditor(selectedView, { convertedExact: true });
    setError('');
    setStatus('Converted to an editable approximate polygon.');
  }

  function beginDrawing(): void {
    if (currentPhoto?.recognition) {
      if (currentPhoto.drawing) return;
      updatePhoto(selectedView, { drawing: true, draftPointsMm: [], accepted: false });
      setError('');
      setStatus(currentPhoto.correctionTool === 'line'
        ? 'Click two endpoints to add a line to the photo profile.'
        : 'Click three or more vertices, then close the added photo contour.');
      return;
    }
    if (currentPhoto) {
      setError('Set scale and recognize the photo before adding geometry.');
      return;
    }
    const loops = startEdit();
    if (drawing) return;
    const outerIndices = loops.flatMap((loop, index) => loop.role === 'outer' ? [index] : []);
    if (drawRole === 'hole' && !outerIndices.includes(drawParentIndex ?? -1)) {
      drawParentIndex = outerIndices[0] ?? null;
    }
    if (drawRole === 'hole' && drawParentIndex === null) {
      setError('Add an outer profile before drawing a hole.');
      return;
    }
    const nextLoops = [...loops, {
      points: [],
      role: drawRole,
      parentIndex: drawRole === 'hole' ? drawParentIndex : null,
    }];
    const nextDraftLoopIndex = nextLoops.length - 1;
    replaceLoops(nextLoops);
    patchProfileEditor(selectedView, {
      draftLoopIndex: nextDraftLoopIndex,
      selectedLoopIndex: nextDraftLoopIndex,
      drawing: true,
      drawingTool: activeTool,
    });
    setError('');
    setStatus(activeTool === 'polyline'
      ? 'Click three or more points in the selected view, then close the loop.'
      : activeTool === 'circle'
        ? 'Click the circle center, then a point on its radius. Circle remains analytic.'
        : 'Click two opposite corners to create a rectangle.');
  }

  function addDrawingPoint(event: MouseEvent, view: ProfileView): void {
    if (!drawing || currentPending) {
      selectView(view);
      return;
    }
    const element = event.currentTarget as HTMLElement;
    const point = snapPoint(profilePointFromPointer(
      event.clientX,
      event.clientY,
      element.getBoundingClientRect(),
      cameraBounds(view, boundsForView(view)),
    ));
    const loops = [...startEdit()];
    const draftIndex = currentEdit.draftLoopIndex;
    const tool = currentEdit.drawingTool;
    const draft = draftIndex === null ? null : loops[draftIndex];
    if (!draft || draftIndex === null) return;
    if (tool === 'polyline' || draft.points.length === 0) {
      loops[draftIndex] = { ...draft, points: [...draft.points, point] };
    } else if (tool === 'rectangle') {
      const [x1, y1] = draft.points[0]!;
      loops[draftIndex] = { ...draft, points: [[x1, y1], [point[0], y1], [point[0], point[1]], [x1, point[1]]] };
      patchProfileEditor(selectedView, { drawing: false, draftLoopIndex: null });
    } else {
      const [cx, cy] = draft.points[0]!;
      const radius = Math.hypot(point[0] - cx, point[1] - cy);
      if (radius <= 0) {
        setError('Circle radius must be greater than zero.');
        return;
      }
      const points = profileCirclePoints([cx, cy], radius);
      loops[draftIndex] = {
        ...draft,
        points,
        analyticCircle: { kind: 'circle', centerMm: [cx, cy] as ProfilePoint, radiusMm: radius },
      };
      patchProfileEditor(selectedView, { drawing: false, draftLoopIndex: null });
    }
    replaceLoops(loops);
    patchProfileEditor(selectedView, { selectedLoopIndex: currentEdit.draftLoopIndex ?? loops.length - 1 });
    selectView(view);
  }

  function addPhotoDrawingPoint(event: MouseEvent, view: ProfileView): void {
    const photo = photoForView(view);
    const pane = paneElements[view];
    if (!photo?.recognition || !photo.drawing || !pane || currentPending) return;
    const point = snapPoint(profilePointFromPointer(
      event.clientX,
      event.clientY,
      pane.getBoundingClientRect(),
      cameraBounds(view, boundsForView(view)),
    ));
    const draftPointsMm = [...photo.draftPointsMm, point];
    if (photo.correctionTool === 'line' && draftPointsMm.length === 2) {
      const id = `user-line-${++photoPrimitiveSequence}`;
      const primitive: DrawingPrimitive = {
        kind: 'line', id, startMm: draftPointsMm[0]!, endMm: draftPointsMm[1]!, residualMm: 0,
      };
      updatePhoto(view, {
        primitives: [...photo.primitives, primitive], selectedPrimitiveId: id,
        drawing: false, draftPointsMm: [], profilesNeedReview: true, accepted: false,
      });
      setStatus('Line added. Review corrections to check the edited profile.');
      return;
    }
    updatePhoto(view, { draftPointsMm });
  }

  function closeLoop(): void {
    if (currentPhoto?.recognition && currentPhoto.drawing) {
      if (currentPhoto.correctionTool !== 'polyline' || currentPhoto.draftPointsMm.length < 3) {
        setError('A photo contour needs at least three vertices before it can be closed.');
        return;
      }
      const id = `user-polyline-${++photoPrimitiveSequence}`;
      const primitive: DrawingPrimitive = {
        kind: 'polyline', id, pointsMm: currentPhoto.draftPointsMm.map(([x, y]) => [x, y]), closed: true, residualMm: 0,
      };
      updatePhoto(selectedView, {
        primitives: [...currentPhoto.primitives, primitive], selectedPrimitiveId: id,
        drawing: false, draftPointsMm: [], profilesNeedReview: true, accepted: false,
      });
      setError('');
      setStatus('Closed contour added. Review corrections to check the edited profile.');
      return;
    }
    if (!drawing) return;
    const current = currentEdit.draftLoopIndex === null ? null : startEdit()[currentEdit.draftLoopIndex];
    if (!current || current.points.length < 3) {
      setError('A closed profile needs at least three points.');
      return;
    }
    patchProfileEditor(selectedView, { drawing: false, draftLoopIndex: null });
    setError('');
    setStatus('Profile loop closed. Drag vertices or edit their coordinates.');
  }

  function cancelPhotoDrawing(): void {
    if (!currentPhoto?.drawing) return;
    updatePhoto(selectedView, { drawing: false, draftPointsMm: [] });
    setError('');
    setStatus('Photo correction cancelled. Recognized geometry remains unchanged.');
  }

  function selectVertex(loopIndex: number, pointIndex: number): void {
    patchProfileEditor(selectedView, { selectedLoopIndex: loopIndex, selectedPointIndex: pointIndex });
  }

  function selectView(view: ProfileView): void {
    if (view === selectedView) return;
    if (photoPickMode) cancelPhotoPicking();
    selectedView = view;
    activePan = null;
    drag = null;
  }

  function showSingleView(view: ProfileView): void {
    selectView(view);
    viewLayout = 'single';
  }

  function showAllViews(): void {
    viewLayout = 'allThree';
  }

  function boundsForView(view: ProfileView) {
    const live = livePaperForView(view);
    if (live) {
      const padding = Math.max(live.widthMm, live.heightMm) * 0.08;
      return { minX: -padding, minY: -padding, width: live.widthMm + padding * 2, height: live.heightMm + padding * 2 };
    }
    const photo = photoForView(view);
    if (photo?.recognition) {
      const width = photo.recognition.previewWidthPx * photo.recognition.millimetersPerPixel;
      const height = photo.recognition.previewHeightPx * photo.recognition.millimetersPerPixel;
      const padding = Math.max(width, height, 10) * 0.12;
      return { minX: -padding, minY: -padding, width: width + padding * 2, height: height + padding * 2 };
    }
    if (photo) return { minX: 0, minY: 0, width: photo.prepared.image.widthPx, height: photo.prepared.image.heightPx };
    const evidence = geometryEvidence[view]?.scopeKey === scopeKey ? geometryEvidence[view]?.response : null;
    const loops = evidence?.loops ?? (view === selectedView ? currentLoops : profileEditorFor(view).loops);
    const profileBoundsForView = loops.length ? profileBounds(loops) : null;
    const drawing = projectionDrawingForView(view);
    const drawingBoundsForView = drawing ? profileDrawingBounds(
      [...drawing.displayEdges, ...drawing.hiddenDisplayEdges],
      [...drawing.displayCurves, ...drawing.hiddenDisplayCurves],
    ) : null;
    if (profileBoundsForView && drawingBoundsForView) {
      const minX = Math.min(profileBoundsForView.minX, drawingBoundsForView.minX);
      const minY = Math.min(profileBoundsForView.minY, drawingBoundsForView.minY);
      const maxX = Math.max(profileBoundsForView.minX + profileBoundsForView.width, drawingBoundsForView.minX + drawingBoundsForView.width);
      const maxY = Math.max(profileBoundsForView.minY + profileBoundsForView.height, drawingBoundsForView.minY + drawingBoundsForView.height);
      return { minX, minY, width: maxX - minX, height: maxY - minY };
    }
    if (profileBoundsForView) return profileBoundsForView;
    if (drawingBoundsForView) return drawingBoundsForView;
    return { minX: -50, minY: -50, width: 100, height: 100 };
  }

  function paneSizeFor(view: ProfileView): { width: number; height: number } {
    return paneSizes[view];
  }

  function cameraBounds(view: ProfileView, base: ReturnType<typeof profileBounds>) {
    const camera = paneCameras[view];
    const paneSize = paneSizeFor(view);
    const width = (paneSize.width > 0 ? paneSize.width : base.width * camera.zoom) / camera.zoom;
    const height = (paneSize.height > 0 ? paneSize.height : base.height * camera.zoom) / camera.zoom;
    return { minX: camera.centerX - width / 2, minY: camera.centerY - height / 2, width, height };
  }

  function viewBox(view: ProfileView): string {
    const viewBounds = cameraBounds(view, boundsForView(view));
    return `${viewBounds.minX} ${-viewBounds.minY - viewBounds.height} ${viewBounds.width} ${viewBounds.height}`;
  }

  function resetPaneCamera(view: ProfileView): void {
    const bounds = boundsForView(view);
    const paneSize = paneSizeFor(view);
    const fitZoom = paneSize.width > 0 && paneSize.height > 0
      ? Math.min(paneSize.width / bounds.width, paneSize.height / bounds.height) * 0.88
      : paneCameras[view].zoom;
    paneCameras = { ...paneCameras, [view]: { zoom: clampCameraZoom(fitZoom), centerX: bounds.minX + bounds.width / 2, centerY: bounds.minY + bounds.height / 2 } };
  }

  function handlePaneWheel(event: WheelEvent, view: ProfileView): void {
    event.preventDefault();
    const camera = paneCameras[view];
    const zoom = clampCameraZoom(camera.zoom * Math.exp(-event.deltaY * 0.001));
    paneCameras = { ...paneCameras, [view]: { ...camera, zoom } };
  }

  function handlePanePointerDown(event: PointerEvent, view: ProfileView): void {
    if (view !== selectedView || !photoPickMode) selectView(view);
    if (photoPickMode === 'region' && view === selectedView) {
      startPhotoRegion(event, view);
      return;
    }
    if (event.button !== 1 && !event.shiftKey) return;
    event.preventDefault();
    const element = event.currentTarget as HTMLElement;
    element.setPointerCapture(event.pointerId);
    activePan = { view, pointerId: event.pointerId, clientX: event.clientX, clientY: event.clientY, camera: paneCameras[view] };
  }

  function handlePanePointerMove(event: PointerEvent, view: ProfileView): void {
    if (photoRegionStart && photoPickMode === 'region') {
      movePhotoRegion(event, view);
      return;
    }
    if (photoVertexDrag && photoVertexDrag.view === view && photoVertexDrag.pointerId === event.pointerId) {
      movePhotoVertex(event, view);
      return;
    }
    if (!activePan || activePan.view !== view || activePan.pointerId !== event.pointerId) return;
    const zoom = activePan.camera.zoom;
    const centerX = activePan.camera.centerX - (event.clientX - activePan.clientX) / zoom;
    const centerY = activePan.camera.centerY + (event.clientY - activePan.clientY) / zoom;
    paneCameras = { ...paneCameras, [view]: { ...activePan.camera, centerX, centerY } };
  }

  function handlePanePointerUp(event: PointerEvent, view: ProfileView): void {
    if (photoRegionStart && photoPickMode === 'region') finishPhotoRegion(event, view);
    if (photoVertexDrag?.pointerId === event.pointerId) photoVertexDrag = null;
    if (activePan?.pointerId === event.pointerId) activePan = null;
  }

  function snapPoint(point: ProfilePoint): ProfilePoint {
    if (!snapToGrid) return point;
    const grid = Number(sketchGridSize);
    if (!Number.isFinite(grid) || grid <= 0) return point;
    return [Math.round(point[0] / grid) * grid, Math.round(point[1] / grid) * grid];
  }

  function evidenceForView(view: ProfileView): ProfileGeometryResponse | null {
    return geometryEvidence[view]?.scopeKey === scopeKey ? geometryEvidence[view]?.response ?? null : null;
  }

  function projectionDrawingForView(view: ProfileView): ProfileDrawingProjectionResponse | null {
    const drawing = projectionDrawings[view];
    return drawing?.scopeKey === scopeKey ? drawing.response : null;
  }

  function exactView(view: ProfileView): boolean {
    if (photoForView(view)?.recognition) return false;
    const evidence = evidenceForView(view);
    return evidence?.accuracy === 'exactBrep'
      && Boolean(evidence.profileAsset)
      && !profileEditorFor(view).convertedExact;
  }

  function loopsForView(view: ProfileView): ProfileLoop[] {
    if (photoForView(view)?.recognition) return view === selectedView ? currentLoops : profileEditorFor(view).loops;
    const evidence = evidenceForView(view);
    if (evidence?.accuracy === 'exactBrep' && exactView(view)) return evidence.loops;
    return view === selectedView ? currentLoops : profileEditorFor(view).loops;
  }

  function updateVertex(axis: 0 | 1, value: string): void {
    const parsed = Number(value);
    if (!Number.isFinite(parsed) || selectedPointIndex === null) return;
    const loops = [...startEdit()];
    const loop = loops[selectedLoopIndex];
    const point = loop?.points[selectedPointIndex];
    if (!loop || !point) return;
    const points = loop.points.map((item) => [...item] as ProfilePoint);
    const changed = points[selectedPointIndex];
    if (!changed) return;
    rememberUndo();
    changed[axis] = parsed;
    loops[selectedLoopIndex] = { ...loop, points };
    patchProfileEditor(selectedView, { loops: normalizeProfileLoops(loops) });
  }

  function updateCircleGeometry(field: 'x' | 'y' | 'radius', value: string): void {
    const parsed = Number(value);
    if (!Number.isFinite(parsed) || (field === 'radius' && parsed <= 0)) return;
    const loops = [...startEdit()];
    const loop = loops[selectedLoopIndex];
    if (!loop?.analyticCircle) return;
    const centerMm: ProfilePoint = [...loop.analyticCircle.centerMm];
    let radiusMm = loop.analyticCircle.radiusMm;
    if (field === 'x') centerMm[0] = parsed;
    else if (field === 'y') centerMm[1] = parsed;
    else radiusMm = parsed;
    rememberUndo();
    loops[selectedLoopIndex] = {
      ...loop,
      points: profileCirclePoints(centerMm, radiusMm),
      analyticCircle: { kind: 'circle', centerMm, radiusMm },
    };
    patchProfileEditor(selectedView, { loops: normalizeProfileLoops(loops), selectedPointIndex: null });
  }

  function removeSelectedLoop(): void {
    const loops = [...startEdit()];
    if (!loops[selectedLoopIndex]) return;
    replaceLoops(removeProfileLoop(loops, selectedLoopIndex));
    setStatus('Profile loop removed.');
  }

  function pointerCoordinates(event: PointerEvent, view: ProfileView): ProfilePoint {
    const element = event.currentTarget as HTMLElement;
    return snapPoint(profilePointFromPointer(event.clientX, event.clientY, element.getBoundingClientRect(), cameraBounds(view, boundsForView(view))));
  }

  function beginVertexDrag(event: PointerEvent, loopIndex: number, pointIndex: number, view: ProfileView): void {
    if (drawing || currentPending) return;
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as SVGCircleElement).setPointerCapture(event.pointerId);
    rememberUndo();
    drag = { loopIndex, pointIndex, pointerId: event.pointerId, view };
    selectVertex(loopIndex, pointIndex);
  }

  function moveVertex(event: PointerEvent, view: ProfileView): void {
    if (!drag || drag.pointerId !== event.pointerId || drag.view !== view) return;
    const loops = [...profileEditorFor(view).loops];
    const loop = loops[drag.loopIndex];
    if (!loop) return;
    const points = loop.points.map((point) => [...point] as ProfilePoint);
    points[drag.pointIndex] = pointerCoordinates(event, view);
    loops[drag.loopIndex] = { ...loop, points };
    profileEdits = { ...profileEdits, [view]: { ...profileEditorFor(view), loops } };
  }

  function finishVertexDrag(event: PointerEvent): void {
    if (!drag || drag.pointerId !== event.pointerId) return;
    drag = null;
  }

  function startPhotoVertexDrag(event: PointerEvent, view: ProfileView, primitiveId: string, pointIndex: number): void {
    if (event.button !== 0 || view !== selectedView) return;
    event.preventDefault();
    event.stopPropagation();
    paneElements[view]?.setPointerCapture(event.pointerId);
    photoVertexDrag = { primitiveId, pointIndex, pointerId: event.pointerId, view };
    selectDrawingPrimitive(primitiveId);
  }

  function movePhotoVertex(event: PointerEvent, view: ProfileView): void {
    const active = photoVertexDrag;
    const photo = photoForView(view);
    if (!active || !photo || active.view !== view || active.pointerId !== event.pointerId) return;
    const pane = paneElements[view];
    if (!pane) return;
    const point = profilePointFromPointer(
      event.clientX,
      event.clientY,
      pane.getBoundingClientRect(),
      cameraBounds(view, boundsForView(view)),
    );
    const primitives = photo.primitives.map((primitive) => {
      if (primitive.id !== active.primitiveId || primitive.kind !== 'polyline') return cloneDrawingPrimitive(primitive);
      const pointsMm = primitive.pointsMm.map(([x, y]) => [x, y] as DrawingPoint);
      if (pointsMm[active.pointIndex]) pointsMm[active.pointIndex] = point;
      return { ...primitive, pointsMm };
    });
    updatePhoto(view, { primitives, profilesNeedReview: true, accepted: false });
  }

  async function applyProfile(): Promise<void> {
    if (applyDisabled || currentPending) return;
    const clearanceMm = finiteValue(clearance, 'Clearance');
    const depthMm = finiteValue(depth, 'Depth');
    const offsetMm = finiteValue(currentEdit.placementOffset, 'Placement offset');
    if (clearanceMm === null || depthMm === null || offsetMm === null) return;
    if (clearanceMm < 0) {
      setError('Clearance must not be negative.');
      return;
    }
    if (depthMm <= 0) {
      setError('Depth must be greater than zero.');
      return;
    }
    if (!deriveSettingsMatch) {
      setError('Derivation settings changed. Derive the profile again before applying.');
      return;
    }
    const drawingProfile = photoProfileReady && currentPhoto?.recognition && acceptedPhotoPrimitives
      ? { evidenceId: currentPhoto.recognition.evidenceId, primitives: acceptedPhotoPrimitives }
      : undefined;
    const sketchLoops = drawingProfile ? undefined : profileSketchLoopsForApply(currentLoops);
    const loops = drawingProfile || sketchLoops ? undefined : profileLoopsForApply(
      currentEvidence,
      convertedSelectedEvidence,
      currentLoops,
    );
    const validation = drawingProfile ? null : profileLoopError(currentLoops);
    if (validation) {
      setError(validation);
      return;
    }

    const reference = sketchLoops ? null : profileReferenceForApply(
      currentEvidence,
      convertedSelectedEvidence,
    );

    const requestScope = scopeKey;
    const requestId = ++requestSequence;
    activeRequestId = requestId;
    pendingScopeKey = requestScope;
    pending = 'apply';
    setError('');
    setStatus('Validating and saving profile…');
    try {
      const outcome = await onApply({
        ...(loops ? { loops } : {}),
        ...(sketchLoops ? { sketchLoops } : {}),
        plane: currentEdit.placementPlane,
        offsetMm,
        clearanceMm,
        depthMm,
        partName: partName.trim(),
        reference,
        ...(drawingProfile ? { drawingProfile } : {}),
      });
      if (profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)) {
        if (outcome.status === 'error') {
          setError(`${outcome.message} The failed version is saved; choose it from history to inspect or edit its source.`);
          setStatus('The failed version was saved. Choose it from history to inspect or edit its source.');
        } else {
          setStatus('Profile submitted as a model version.');
        }
      }
    } catch (error) {
      if (profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)) {
        setError(formatBackendError(error));
        setStatus('Profile could not be saved. Your edits remain available.');
      }
    } finally {
      if (activeRequestId === requestId) {
        pending = null;
        pendingScopeKey = null;
        activeRequestId = null;
      }
    }
  }
</script>

{#snippet projectedGeometry(curves: ProfileDisplayCurve[], edges: ProfilePoint[][])}
  {#each curves as curve, curveIndex (curveIndex)}
    {#if curve.kind === 'line'}
      <line class="projected-line" data-testid="projected-line" x1={curve.startMm[0]} y1={-curve.startMm[1]} x2={curve.endMm[0]} y2={-curve.endMm[1]} />
    {:else if curve.kind === 'circle'}
      <circle class="projected-circle" data-testid="projected-circle" cx={curve.centerMm[0]} cy={-curve.centerMm[1]} r={curve.radiusMm} />
    {:else if curve.kind === 'circularArc'}
      <path class="projected-circular-arc" data-testid="projected-circular-arc" d={profileDisplayCurveArcPath(curve)} />
    {:else if curve.kind === 'ellipse'}
      {@const rotationDegrees = (-curve.rotationRad * 180) / Math.PI}
      <ellipse class="projected-ellipse" data-testid="projected-ellipse" cx={curve.centerMm[0]} cy={-curve.centerMm[1]} rx={curve.majorRadiusMm} ry={curve.minorRadiusMm} transform={`rotate(${rotationDegrees} ${curve.centerMm[0]} ${-curve.centerMm[1]})`} />
    {:else}
      <path class="projected-elliptical-arc" data-testid="projected-elliptical-arc" d={profileDisplayCurveArcPath(curve)} />
    {/if}
  {/each}
  {#each edges as edge, edgeIndex (edgeIndex)}
    {@const edgePoints = edge.map(([x, y]) => `${x},${-y}`).join(' ')}
    <polyline class="projected-edge" data-testid="projected-edge" points={edgePoints} />
  {/each}
{/snippet}

<section class="profile-workspace" data-testid="profile-workspace" aria-label="Sketch workspace">
  <div class="workspace-grid">
    <aside class="controls-pane" aria-label="Profile controls">
      <section class="control-section">
        <h3>Derive from geometry</h3>
        <label>
          Geometry source
          <select aria-label="Derivation mode" bind:value={deriveMode} disabled={currentPending !== null || drawing}>
            <option value="exactProjection" disabled={!exactCadAvailable}>Exact CAD projection</option>
            <option value="exactSection" disabled={!exactCadAvailable}>Exact CAD section</option>
            <option value="meshSection" disabled={!sourceAvailable}>Approximate mesh section</option>
          </select>
        </label>
        {#if !exactCadAvailable}
          <p class="inline-hint">A supported STEP model is needed for exact projection. FCStd-only sources need a STEP export; STL sections remain approximate.</p>
        {/if}
        {#if deriveMode !== 'exactProjection'}
          <label>
            Section plane
            <select aria-label="Section plane" bind:value={sectionPlane} disabled={currentPending !== null}>
              <option value="xy">XY</option>
              <option value="xz">XZ</option>
              <option value="yz">YZ</option>
            </select>
          </label>
          <label>
            Section offset (mm)
            <input aria-label="Section offset (mm)" type="number" step="0.1" bind:value={sectionOffset} disabled={currentPending !== null} />
          </label>
        {/if}
        <label>
          Tolerance (mm)
          <input aria-label="Tolerance (mm)" type="number" min="0.001" step="0.1" bind:value={tolerance} disabled={currentPending !== null} />
        </label>
        {#if deriveMode === 'exactProjection'}
          <label>
            Projection edges
            <select aria-label="Projection edges" bind:value={projectionVisibility} disabled={currentPending !== null || currentViewsPending}>
              <option value="allEdges">Visible + hidden edges</option>
              <option value="visibleEdges">Visible edges</option>
            </select>
          </label>
          <p class="inline-hint">Visible edges are solid. Hidden edges, when requested, are thin and dashed.</p>
          <button class="action-button action-button--secondary" type="button" onclick={projectAllDrawings} disabled={!deriveAvailable || currentPending !== null || drawing || currentViewsPending}>
            {currentViewsPending ? 'Projecting…' : 'Project all three views'}
          </button>
          {#if currentProjectionDrawing && !currentPhoto}
            <button class="action-button action-button--secondary" type="button" onclick={buildClosedProfile} disabled={!deriveAvailable || currentPending !== null || drawing || currentViewsPending}>
              {viewPendingKindFor(selectedView) === 'profile' ? 'Building profile…' : 'Build closed profile'}
            </button>
            <p class="inline-hint">Projected edges are view-only. Build a closed profile before applying.</p>
          {:else if currentProjectionDrawing && currentPhoto}
            <p class="inline-hint">A drawing photo is assigned to this view. Select another view to build its CAD profile.</p>
          {/if}
        {:else}
          <button class="action-button action-button--secondary" type="button" onclick={deriveProfile} disabled={!deriveAvailable || currentPending !== null || drawing || currentViewsPending}>
            {currentPending === 'section'
              ? 'Deriving…'
              : deriveMode === 'exactSection'
                ? 'Extract exact section'
                : 'Extract approximate section'}
          </button>
        {/if}
        {#if sourceWarning}<p class="inline-hint">{sourceWarning}</p>{/if}
      </section>

      <LivePaperPanel
        bind:snapshot={livePaper} bind:selectedId={liveSelectedId}
        viewLabel={selectedView} scopeKey={liveScopeToken}
        onStarted={bindLivePaperView} onUseProfile={useLivePaperProfile}
        disabled={currentPending !== null || Boolean(disabledReason) || !liveScopeToken}
      />

      <section class="control-section drawing-photo-controls" aria-label="Drawing photo">
        <h3>Drawing photo · {selectedView.toUpperCase()}</h3>
        <p class="inline-hint">Crop and perspective correction are optional. Full image works as-is; scale calibration is required.</p>
        <button class="action-button action-button--secondary" type="button" onclick={chooseDrawingPhoto} disabled={photoImportPending || currentPending !== null}>
          {photoImportPending ? 'Opening photo…' : currentPhoto ? 'Replace drawing photo' : 'Drawing photo'}
        </button>
        {#if currentPhoto}
          <p class="inline-hint">Photo is assigned to {selectedView.toUpperCase()}. Select another pane before importing its view.</p>
          <div class="photo-tool-row">
            <button class="action-button action-button--secondary" type="button" onclick={() => beginPhotoPick('region')} disabled={currentPhoto.pending}>Select image region</button>
            <label class="compact-field">
              Rotate
              <select aria-label="Drawing photo rotation" value={currentPhoto.rotationDegrees} onchange={(event) => setPhotoRotation(event.currentTarget.value)} disabled={currentPhoto.pending}>
                <option value="0">0°</option>
                <option value="90">90°</option>
                <option value="180">180°</option>
                <option value="270">270°</option>
              </select>
            </label>
          </div>
          <p class="inline-hint">Crop: {Math.round(currentPhoto.region.widthPx)} × {Math.round(currentPhoto.region.heightPx)} px · drag on the image pane.</p>
          <div class="photo-tool-row">
            <button class="action-button action-button--secondary" type="button" onclick={() => beginPhotoPick('rectification')} disabled={currentPhoto.pending}>Pick four corners</button>
            <label class="compact-field">
              Width / height
              <input aria-label="Known rectangle aspect ratio" type="number" min="0.001" step="0.01" value={currentPhoto.aspectRatio} oninput={(event) => updatePhoto(selectedView, { aspectRatio: event.currentTarget.value }, true)} disabled={currentPhoto.pending} />
            </label>
          </div>
          <p class="inline-hint">{currentPhoto.rectificationPoints.length}/4 corners picked. Supply the known ratio to rectify a photographed rectangle.</p>
          <div class="photo-tool-row">
            <button class="action-button action-button--secondary" type="button" onclick={() => beginPhotoPick('calibration')} disabled={currentPhoto.pending}>Pick scale points</button>
            <label class="compact-field">
              Known length (mm)
              <input aria-label="Known length (mm)" type="number" min="0.001" step="0.1" value={currentPhoto.knownDistanceMm} oninput={(event) => updatePhoto(selectedView, { knownDistanceMm: event.currentTarget.value }, true)} disabled={currentPhoto.pending} />
            </label>
          </div>
          <p class="inline-hint">{currentPhoto.calibrationPoints.length}/2 scale points picked. Calibration uses one uniform scale.</p>
          {#if currentPhoto.recognition}
            <p class="inline-hint" aria-label="Calibrated distance (mm)">
              Recovered distance: {calibratedPreviewMm === null ? 'unavailable' : `${calibratedPreviewMm.toFixed(2)} mm`} · image-derived, check dimensions.
            </p>
          {:else if currentPhoto.knownDistanceMm}
            <p class="inline-hint" aria-label="Calibrated distance (mm)">Reference distance: {currentPhoto.knownDistanceMm} mm</p>
          {/if}
          <label class="snap-control">
            <span>Source image opacity</span>
            <input aria-label="Source image opacity" type="range" min="0.05" max="0.9" step="0.05" value={currentPhoto.imageOpacity} oninput={(event) => updatePhoto(selectedView, { imageOpacity: Number(event.currentTarget.value) })} />
          </label>
          <div class="photo-tool-row">
            <button class="action-button" type="button" onclick={recognizeDrawing} disabled={recognitionReadinessReason !== null}>
              {currentPhoto.pending ? 'Recognizing…' : 'Recognize drawing'}
            </button>
            <button class="text-button" type="button" onclick={clearDrawingPhoto}>Clear photo</button>
            {#if photoPickMode}
              <button class="text-button" type="button" onclick={cancelPhotoPicking}>Cancel photo picking</button>
            {/if}
          </div>
          <p class="inline-hint" data-testid="drawing-recognition-readiness" aria-live="polite">{recognitionReadinessReason ?? 'Ready to recognize.'}</p>
          {#if currentPhoto.error}<p class="status-error" role="alert">{currentPhoto.error}</p>{/if}
          {#if currentPhoto.recognition}
            <div class="drawing-candidates" data-testid="drawing-profile-candidates" aria-label="Recognized profile candidates">
              <h4>Recognized profiles</h4>
              {#each currentPhoto.profiles as candidate (candidate.id)}
                {@const selectable = drawingCandidateIsSelectable(candidate, currentPhoto.primitives)}
                <label class="candidate-row">
                  <input
                    type="checkbox"
                    aria-label={`Use ${candidate.role} profile ${candidate.id}`}
                    checked={currentPhoto.selectedProfileIds.includes(candidate.id)}
                    onchange={(event) => toggleDrawingCandidate(candidate, event.currentTarget.checked)}
                    disabled={!selectable || currentPhoto.pending || currentPhoto.profilesNeedReview}
                  />
                  <span><strong>{candidate.id}</strong> · {candidate.role} · residual {candidate.residualMm.toFixed(2)} mm</span>
                </label>
                {#if !candidate.closed || candidate.issues.length || !selectable}
                  <p class="inline-hint candidate-issue">{candidate.issues.join('; ') || 'Open or incomplete contour; repair before accepting.'}</p>
                {/if}
              {/each}
              {#if currentPhoto.profilesNeedReview}
                <p class="inline-hint">Contour candidates may have changed. Review corrections before selecting or accepting a profile.</p>
              {/if}
              {#if currentPhoto.primitives.length}
                <label>
                  Edit recognized geometry
                  <select aria-label="Recognized primitive" value={currentPhoto.selectedPrimitiveId ?? ''} onchange={(event) => selectDrawingPrimitive(event.currentTarget.value)}>
                    {#each currentPhoto.primitives as primitive (primitive.id)}
                      <option value={primitive.id}>{primitive.kind} · {primitive.id}</option>
                    {/each}
                  </select>
                </label>
                {@const primitive = currentPhoto.primitives.find((item) => item.id === currentPhoto.selectedPrimitiveId)}
                {#if primitive?.kind === 'circle'}
                  <div class="vertex-fields" aria-label="Edit circle geometry">
                    <label>Center X (mm)<input aria-label="Circle center X (mm)" type="number" step="0.01" value={primitive.centerMm[0]} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'x', event.currentTarget.value)} /></label>
                    <label>Center Y (mm)<input aria-label="Circle center Y (mm)" type="number" step="0.01" value={primitive.centerMm[1]} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'y', event.currentTarget.value)} /></label>
                    <label>Radius (mm)<input aria-label="Circle radius (mm)" type="number" min="0.001" step="0.01" value={primitive.radiusMm} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'radius', event.currentTarget.value)} /></label>
                  </div>
                {:else if primitive?.kind === 'line'}
                  <div class="vertex-fields" aria-label="Edit line geometry">
                    <label>Start X (mm)<input aria-label="Line start X (mm)" type="number" step="0.01" value={primitive.startMm[0]} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'x', event.currentTarget.value)} /></label>
                    <label>Start Y (mm)<input aria-label="Line start Y (mm)" type="number" step="0.01" value={primitive.startMm[1]} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'y', event.currentTarget.value)} /></label>
                    <label>End X (mm)<input aria-label="Line end X (mm)" type="number" step="0.01" value={primitive.endMm[0]} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'x2', event.currentTarget.value)} /></label>
                    <label>End Y (mm)<input aria-label="Line end Y (mm)" type="number" step="0.01" value={primitive.endMm[1]} oninput={(event) => updateDrawingPrimitiveNumber(primitive.id, 'y2', event.currentTarget.value)} /></label>
                  </div>
                {:else if primitive?.kind === 'polyline'}
                  <div class="drawing-polyline-vertices" aria-label="Edit recognized polyline vertices">
                    {#each primitive.pointsMm as point, pointIndex (`${primitive.id}-${pointIndex}`)}
                      <div class="vertex-fields" aria-label={`Polyline vertex ${pointIndex + 1}`}>
                        <label>Vertex {pointIndex + 1} X (mm)<input aria-label={`Polyline vertex ${pointIndex + 1} X (mm)`} type="number" step="0.01" value={point[0]} oninput={(event) => updateDrawingPolylineVertex(primitive.id, pointIndex, 0, event.currentTarget.value)} /></label>
                        <label>Vertex {pointIndex + 1} Y (mm)<input aria-label={`Polyline vertex ${pointIndex + 1} Y (mm)`} type="number" step="0.01" value={point[1]} oninput={(event) => updateDrawingPolylineVertex(primitive.id, pointIndex, 1, event.currentTarget.value)} /></label>
                      </div>
                    {/each}
                  </div>
                {/if}
                {#if primitive}
                  <button class="text-button" type="button" onclick={() => removeDrawingPrimitive(primitive.id)} disabled={currentPhoto.pending}>Remove selected geometry</button>
                {/if}
              {/if}
              {#if currentPhoto.recognition.diagnostics.length || currentPhoto.reviewDiagnostics.length}
                <ul class="warning-list">{#each [...currentPhoto.recognition.diagnostics, ...currentPhoto.reviewDiagnostics] as diagnostic, index (`${index}-${diagnostic}`)}<li>{diagnostic}</li>{/each}</ul>
              {/if}
              <button class="action-button action-button--primary" type="button" onclick={acceptDrawingProfile} disabled={currentPhoto.pending || (!currentPhoto.profilesNeedReview && !currentPhoto.selectedProfileIds.length)}>
                {currentPhoto.profilesNeedReview ? 'Review corrections' : currentPhoto.accepted ? 'Profile accepted' : 'Accept selected profile'}
              </button>
            </div>
          {/if}
        {/if}
      </section>

      <section class="control-section">
        <h3>Profile</h3>
        {#if currentEvidence?.accuracy === 'exactBrep' && sourceAccuracyHint}
          <p class="inline-hint">{sourceAccuracyHint}</p>
        {/if}
        {#if isExactProfile}
          <p class="inline-hint">Exact curves stay intact. Select a view to apply its profile.</p>
          <div class="tool-row">
            <button class="action-button action-button--secondary" type="button" onclick={convertExactProfileToPolygon} disabled={currentPending !== null}>
              Convert to editable approximate polygon
            </button>
          </div>
        {:else if currentPhoto?.recognition}
          <label>
            Photo correction tool
            <select aria-label="Photo correction tool" bind:value={currentPhoto.correctionTool} disabled={currentPhoto.drawing || currentPhoto.pending}>
              <option value="line">Line segment</option>
              <option value="polyline">Closed contour</option>
            </select>
          </label>
          <div class="tool-row">
            {#if currentPhoto.drawing}
              <button class="action-button action-button--secondary" type="button" onclick={cancelPhotoDrawing}>Cancel correction</button>
              {#if currentPhoto.correctionTool === 'polyline'}
                <button class="action-button" type="button" onclick={closeLoop}>Close contour</button>
              {/if}
            {:else}
              <button class="action-button" type="button" onclick={beginDrawing} disabled={currentPending !== null || currentPhoto.pending}>
                Draw correction
              </button>
            {/if}
          </div>
          <p class="inline-hint">Add straight linework or a closed polygon. Existing circles stay analytic.</p>
        {:else if currentPhoto}
          <p class="inline-hint">Set scale and recognize the photo before adding geometry. Clear the photo to draw a separate manual profile.</p>
        {:else}
          <div class="tool-row">
            <button class="action-button" type="button" onclick={beginDrawing} disabled={currentPending !== null || drawing}>
              Draw profile
            </button>
            <button class="action-button action-button--secondary" type="button" onclick={undoLastEdit} disabled={!canUndo}>Undo</button>
            {#if drawing}
              <button class="action-button action-button--secondary" type="button" onclick={closeLoop}>Close loop</button>
            {/if}
          </div>
          <label>
            New loop type
            <select aria-label="New loop type" bind:value={drawRole} disabled={drawing || currentPending !== null || Boolean(currentPhoto)}>
              <option value="outer">Outer profile</option>
              <option value="hole">Hole</option>
            </select>
          </label>
          <label>
            Drawing tool
            <select aria-label="Drawing tool" bind:value={activeTool} disabled={drawing || currentPending !== null || Boolean(currentPhoto)}>
              <option value="polyline">Polyline</option>
              <option value="rectangle">Rectangle</option>
              <option value="circle">Circle</option>
            </select>
          </label>
          <label class="snap-control">
            <span>Snap to grid</span>
            <input aria-label="Snap to grid" type="checkbox" bind:checked={snapToGrid} disabled={drawing || currentPending !== null || Boolean(currentPhoto)} />
          </label>
          {#if snapToGrid}
            <label>
              Grid size (mm)
              <input aria-label="Grid size (mm)" type="number" min="0.001" step="0.1" bind:value={sketchGridSize} disabled={drawing || currentPending !== null || Boolean(currentPhoto)} />
            </label>
          {/if}
          {#if drawRole === 'hole' && currentLoops.some((loop) => loop.role === 'outer')}
            <label>
              Hole parent outer loop
              <select aria-label="Hole parent outer loop" bind:value={drawParentIndex} disabled={drawing || currentPending !== null}>
                {#each currentLoops as loop, index (`parent-${index}-${loop.role}`)}
                  {#if loop.role === 'outer'}
                    <option value={index}>Outer {index + 1}</option>
                  {/if}
                {/each}
              </select>
            </label>
          {/if}
          {#if currentLoops.length > 0}
            <label>
              Selected loop
              <select aria-label="Selected loop" value={selectedLoopIndex} onchange={(event) => patchProfileEditor(selectedView, { selectedLoopIndex: Number(event.currentTarget.value), selectedPointIndex: null })} disabled={currentPending !== null || drawing}>
                {#each currentLoops as loop, index (`${index}-${loop.role}`)}
                  <option value={index}>{loop.role === 'hole' ? 'Hole' : 'Outer'} {index + 1}{loop.analyticCircle ? ` · circle r=${loop.analyticCircle.radiusMm.toFixed(2)} mm` : ` · ${loop.points.length} pts`}</option>
                {/each}
              </select>
            </label>
            <button class="text-button" type="button" onclick={removeSelectedLoop} disabled={currentPending !== null || drawing}>Remove selected loop</button>
          {/if}
          {#if selectedLoop?.analyticCircle}
            <div class="vertex-fields" aria-label="Edit analytic circle">
              <label>Center X (mm)<input aria-label="Circle center X (mm)" type="number" step="0.01" value={selectedLoop.analyticCircle.centerMm[0]} onchange={(event) => updateCircleGeometry('x', event.currentTarget.value)} disabled={currentPending !== null} /></label>
              <label>Center Y (mm)<input aria-label="Circle center Y (mm)" type="number" step="0.01" value={selectedLoop.analyticCircle.centerMm[1]} onchange={(event) => updateCircleGeometry('y', event.currentTarget.value)} disabled={currentPending !== null} /></label>
              <label>Radius (mm)<input aria-label="Circle radius (mm)" type="number" min="0.001" step="0.01" value={selectedLoop.analyticCircle.radiusMm} onchange={(event) => updateCircleGeometry('radius', event.currentTarget.value)} disabled={currentPending !== null} /></label>
            </div>
            <p class="inline-hint">Circle stays analytic in the applied feature.</p>
          {/if}
          {#if selectedVertex}
            <div class="vertex-fields" aria-label="Selected vertex coordinates">
              <label>
                X (mm)
                <input aria-label="Vertex X (mm)" type="number" step="0.1" value={selectedVertex[0].toFixed(2)} onchange={(event) => updateVertex(0, event.currentTarget.value)} disabled={currentPending !== null} />
              </label>
              <label>
                Y (mm)
                <input aria-label="Vertex Y (mm)" type="number" step="0.1" value={selectedVertex[1].toFixed(2)} onchange={(event) => updateVertex(1, event.currentTarget.value)} disabled={currentPending !== null} />
              </label>
            </div>
          {/if}
          <p class="inline-hint">Click to place vertices. Close the loop, then drag handles or edit coordinates.</p>
        {/if}
      </section>

      <section class="control-section">
        <h3>Profile placement</h3>
        <label>
          Profile plane
          <select aria-label="Profile plane" value={currentEdit.placementPlane} onchange={(event) => patchProfileEditor(selectedView, { placementPlane: event.currentTarget.value as ProfilePlane })} disabled={currentPending !== null || currentEvidence !== null || !!currentPhoto?.recognition}>
            <option value="xy">XY</option>
            <option value="xz">XZ</option>
            <option value="yz">YZ</option>
          </select>
        </label>
        <label>
          Profile plane offset (mm)
          <input aria-label="Profile plane offset (mm)" type="number" step="0.1" value={currentEdit.placementOffset} oninput={(event) => patchProfileEditor(selectedView, { placementOffset: event.currentTarget.value })} disabled={currentPending !== null || currentEvidence !== null} />
        </label>
        {#if currentEvidence}
          <p class="inline-hint">Plane {currentEdit.placementPlane.toUpperCase()} · offset {Number(currentEdit.placementOffset).toFixed(2)} mm follows the derived profile.</p>
        {/if}
      </section>

    </aside>

    <main class="editor-pane">
      <nav class="view-layout-switcher" aria-label="Sketch views">
        {#each PROFILE_VIEWS as view (view)}
          <button type="button" aria-label={`Show ${view} view`} aria-pressed={viewLayout === 'single' && selectedView === view} onclick={() => showSingleView(view)}>{view.toUpperCase()}</button>
        {/each}
        <button type="button" aria-label="All three views" aria-pressed={viewLayout === 'allThree'} onclick={showAllViews}>ALL THREE</button>
      </nav>
      <div class={['sketch-workspace__panes', viewLayout === 'single' && 'sketch-workspace__panes--single']} data-testid="profile-projection-views">
        {#each visibleViews as view (view)}
          {@const viewLoops = loopsForView(view)}
          {@const viewBounds = boundsForView(view)}
          {@const visibleBounds = cameraBounds(view, viewBounds)}
          {@const isViewExact = exactView(view)}
          {@const photo = photoForView(view)}
          {@const live = livePaperForView(view)}
          {@const projectionDrawing = projectionDrawingForView(view)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div
            class={['sketch-pane', `sketch-pane--${view}`, selectedView === view && 'sketch-pane--selected', (drawing || photoDrawing) && selectedView === view && 'draw-mode']}
            aria-label={`${view[0]!.toUpperCase()}${view.slice(1)} projection`}
            data-testid={`profile-view-${view}`}
            bind:this={paneElements[view]}
            role="application"
            tabindex="0"
            onpointerdown={(event) => handlePanePointerDown(event, view)}
            onpointermove={(event) => {
              handlePanePointerMove(event, view);
              if (drag) moveVertex(event, view);
            }}
            onpointerup={(event) => handlePanePointerUp(event, view)}
            onpointercancel={(event) => handlePanePointerUp(event, view)}
            onclick={(event) => {
              if (photoPickMode && selectedView === view) pickPhotoPoint(event, view);
              else if (photoForView(view)?.recognition && photoForView(view)?.drawing && selectedView === view) addPhotoDrawingPoint(event, view);
              else if (drawing && selectedView === view) addDrawingPoint(event, view);
              else selectView(view);
            }}
            onwheel={(event) => handlePaneWheel(event, view)}
          >
            <div class="sketch-pane__label">
              <button class="view-select" type="button" aria-label={`Select ${view} projection`} aria-pressed={selectedView === view} onclick={(event) => { event.stopPropagation(); selectView(view); }}>{view.toUpperCase()}</button>
              <span>{live ? livePaperStateLabel(live.state).toUpperCase() : photoForView(view)?.pending ? 'RECOGNIZING…' : photoForView(view)?.error ? 'ERROR' : viewPendingFor(view) ? (viewPendingKindFor(view) === 'profile' ? 'BUILDING PROFILE…' : 'PROJECTING…') : viewErrorFor(view) ? 'ERROR' : photoForView(view)?.accepted ? 'PROFILE ACCEPTED' : projectionDrawing && !photo ? `${projectionDrawing.visibility === 'allEdges' ? 'VISIBLE + HIDDEN' : 'VISIBLE EDGES'} · PROJECTED` : viewLoops.length ? `${viewLoops.length} LOOPS` : photoForView(view) ? 'DRAWING PHOTO' : 'NO PROFILE'}</span>
              <button class="sketch-pane__label-action" type="button" onclick={(event) => { event.stopPropagation(); resetPaneCamera(view); }}>FIT VIEW</button>
            </div>
            {#if photoForView(view)?.pending}
              <span class="pane-message">Recognizing drawing lines…</span>
            {:else if photoForView(view)?.error}
              <span class="pane-message pane-message--error">{photoForView(view)?.error}</span>
            {:else if viewPendingFor(view)}
              <span class="pane-message">{viewPendingKindFor(view) === 'profile' ? 'Building a closed profile…' : 'Projecting current model…'}</span>
            {:else if viewErrorFor(view)}
              <span class="pane-message pane-message--error">{viewErrorFor(view)}</span>
            {:else if photoPickMode && selectedView === view}
              <span class="pane-message">{photoPickMode === 'region' ? 'Drag a rectangle to crop the drawing.' : photoPickMode === 'calibration' ? `Click scale point ${photoForView(view)?.calibrationPoints.length === 0 ? '1' : '2'} of 2.` : `Click rectangle corner ${(photoForView(view)?.rectificationPoints.length ?? 0) + 1} of 4.`}</span>
            {:else if photoForView(view)?.drawing}
              <span class="pane-message">{photoForView(view)?.correctionTool === 'line' ? `Click endpoint ${photoForView(view)?.draftPointsMm.length === 0 ? '1' : '2'} of 2.` : `${photoForView(view)?.draftPointsMm.length ?? 0} contour points · click Close contour when ready.`}</span>
            {:else if viewLoops.length === 0 && !projectionDrawing && !photoForView(view)?.recognition && !livePaperForView(view)}
              <span class="pane-message">Select a view, draw a profile, or add a drawing photo.</span>
            {/if}
            <svg class="sketch-pane__drawing" viewBox={viewBox(view)} preserveAspectRatio="xMidYMid meet" aria-label={`${view} sketch canvas`}>
              {#if live?.previewDataUrl}
                <image data-testid="live-paper-image" href={live.previewDataUrl} x="0" y={-live.heightMm} width={live.widthMm} height={live.heightMm} opacity="0.65" preserveAspectRatio="none" />
              {/if}
              {#if !photo || photo.recognition}
                <defs>
                  <pattern id={`profile-grid-${view}`} width="10" height="10" patternUnits="userSpaceOnUse" patternTransform="scale(1,-1)">
                    <path d="M 10 0 L 0 0 0 10" class="grid-line" />
                  </pattern>
                </defs>
                <rect x={visibleBounds.minX} y={-visibleBounds.minY - visibleBounds.height} width={visibleBounds.width} height={visibleBounds.height} fill={`url(#profile-grid-${view})`} />
                <line x1={0} y1={-visibleBounds.minY - visibleBounds.height} x2={0} y2={-visibleBounds.minY} class="axis-line" />
                <line x1={visibleBounds.minX} y1={0} x2={visibleBounds.minX + visibleBounds.width} y2={0} class="axis-line" />
              {/if}
              {#if live}
                <LivePaperOverlay snapshot={live} selectedId={liveSelectedId} onSelect={(id) => liveSelectedId = id} />
              {/if}
              {#if photo?.recognition}
                {@const imageWidthMm = photo.recognition.previewWidthPx * photo.recognition.millimetersPerPixel}
                {@const imageHeightMm = photo.recognition.previewHeightPx * photo.recognition.millimetersPerPixel}
                <image href={photo.recognition.transformedPreviewDataUrl} x="0" y={-imageHeightMm} width={imageWidthMm} height={imageHeightMm} opacity={photo.imageOpacity} preserveAspectRatio="none" />
                {#each photo.primitives as primitive (primitive.id)}
                  {#if primitive.kind === 'line'}
                    <line
                      class={['drawing-primitive', photo.selectedPrimitiveId === primitive.id && 'drawing-primitive--selected']}
                      x1={primitive.startMm[0]} y1={-primitive.startMm[1]} x2={primitive.endMm[0]} y2={-primitive.endMm[1]}
                      role="button" tabindex="0" aria-label={`Select line ${primitive.id}`}
                      onclick={(event) => { event.stopPropagation(); selectDrawingPrimitive(primitive.id); }}
                      onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectDrawingPrimitive(primitive.id); } }}
                    />
                  {:else if primitive.kind === 'circle'}
                    <circle
                      class={['drawing-primitive', 'drawing-circle', photo.selectedPrimitiveId === primitive.id && 'drawing-primitive--selected']}
                      cx={primitive.centerMm[0]} cy={-primitive.centerMm[1]} r={primitive.radiusMm}
                      role="button" tabindex="0" aria-label={`Select circle ${primitive.id}`}
                      onclick={(event) => { event.stopPropagation(); selectDrawingPrimitive(primitive.id); }}
                      onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectDrawingPrimitive(primitive.id); } }}
                    />
                  {:else}
                    {@const points = primitive.pointsMm.map(([x, y]) => `${x},${-y}`).join(' ')}
                    {#if primitive.closed}
                      <polygon class={['drawing-primitive', photo.selectedPrimitiveId === primitive.id && 'drawing-primitive--selected']} points={points} role="button" tabindex="0" aria-label={`Select closed linework ${primitive.id}`} onclick={(event) => { event.stopPropagation(); selectDrawingPrimitive(primitive.id); }} onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectDrawingPrimitive(primitive.id); } }} />
                    {:else}
                      <polyline class={['drawing-primitive', photo.selectedPrimitiveId === primitive.id && 'drawing-primitive--selected']} points={points} role="button" tabindex="0" aria-label={`Select open linework ${primitive.id}`} onclick={(event) => { event.stopPropagation(); selectDrawingPrimitive(primitive.id); }} onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectDrawingPrimitive(primitive.id); } }} />
                    {/if}
                    {#if selectedView === view && photo.selectedPrimitiveId === primitive.id}
                      {#each primitive.pointsMm as point, pointIndex (`${primitive.id}-handle-${pointIndex}`)}
                        <circle
                          class="drawing-vertex-handle"
                          data-testid={`drawing-vertex-${pointIndex}`}
                          cx={point[0]} cy={-point[1]} r={Math.max(0.2, 5 / paneCameras[view].zoom)}
                          role="button" tabindex="0"
                          aria-label={`${primitive.id} vertex ${pointIndex + 1}, ${point[0].toFixed(2)} by ${point[1].toFixed(2)} mm`}
                          onpointerdown={(event) => startPhotoVertexDrag(event, view, primitive.id, pointIndex)}
                          onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectDrawingPrimitive(primitive.id); } }}
                        />
                      {/each}
                    {/if}
                  {/if}
                {/each}
                {#if photo.drawing && photo.draftPointsMm.length}
                  {@const draftPoints = photo.draftPointsMm.map(([x, y]) => `${x},${-y}`).join(' ')}
                  {#if photo.correctionTool === 'line' && photo.draftPointsMm.length === 1}
                    <circle class="drawing-primitive drawing-primitive--draft" cx={photo.draftPointsMm[0]![0]} cy={-photo.draftPointsMm[0]![1]} r={Math.max(0.2, 5 / paneCameras[view].zoom)} />
                  {:else if photo.correctionTool === 'line'}
                    <line class="drawing-primitive drawing-primitive--draft" x1={photo.draftPointsMm[0]![0]} y1={-photo.draftPointsMm[0]![1]} x2={photo.draftPointsMm[1]![0]} y2={-photo.draftPointsMm[1]![1]} />
                  {:else}
                    <polyline class="drawing-primitive drawing-primitive--draft" points={draftPoints} />
                  {/if}
                {/if}
              {:else if photo}
                <image data-testid="drawing-photo-source" href={photo.prepared.previewDataUrl} x="0" y={-photo.prepared.image.heightPx} width={photo.prepared.image.widthPx} height={photo.prepared.image.heightPx} opacity={photo.imageOpacity} preserveAspectRatio="none" />
                {@const region = photoRegionDraft ?? photo.region}
                <rect class="drawing-region" x={region.leftPx} y={-(region.topPx + region.heightPx)} width={region.widthPx} height={region.heightPx} />
                {#each photo.rectificationPoints as point, index (`rectification-${index}`)}
                  <circle class="drawing-pick-point" cx={point[0]} cy={-(photo.prepared.image.heightPx - point[1])} r={Math.max(photo.prepared.image.widthPx, photo.prepared.image.heightPx) * 0.008} />
                {/each}
                {#each photo.calibrationPoints as point, index (`calibration-${index}`)}
                  <circle class="drawing-calibration-point" cx={point[0]} cy={-(photo.prepared.image.heightPx - point[1])} r={Math.max(photo.prepared.image.widthPx, photo.prepared.image.heightPx) * 0.009} />
                {/each}
              {/if}
              {#if projectionDrawing && !photo}
                {#if projectionDrawing.visibility === 'allEdges'}
                  <g class="projected-layer projected-layer--hidden" data-projection-visibility="hidden">
                    {@render projectedGeometry(projectionDrawing.hiddenDisplayCurves, projectionDrawing.hiddenDisplayEdges)}
                  </g>
                {/if}
                <g class="projected-layer" data-projection-visibility="visible">
                  {@render projectedGeometry(projectionDrawing.displayCurves, projectionDrawing.displayEdges)}
                </g>
              {/if}
              {#each viewLoops as loop, loopIndex (`${view}-${loopIndex}-${loop.points.length}-${loop.role}`)}
                {@const pathPoints = loop.points.map(([x, y]) => `${x},${-y}`).join(' ')}
                {#if loop.analyticCircle}
                  <circle class={['analytic-profile-circle', loop.role === 'hole' && 'hole', profileEditorFor(view).selectedLoopIndex === loopIndex && 'selected-loop']} data-testid="analytic-profile-circle" cx={loop.analyticCircle.centerMm[0]} cy={-loop.analyticCircle.centerMm[1]} r={loop.analyticCircle.radiusMm} />
                {:else}
                  <polygon points={pathPoints} class={[loop.role, loop.role === 'hole' && 'hole', profileEditorFor(view).selectedLoopIndex === loopIndex && 'selected-loop', profileEditorFor(view).drawing && loopIndex === profileEditorFor(view).draftLoopIndex && 'open-draft']} fill-rule="evenodd" />
                {/if}
                {#if selectedView === view && !isViewExact && !drawing && !loop.analyticCircle}
                  {#each loop.points as point, pointIndex (`${view}-${loopIndex}-${pointIndex}`)}
                    <circle
                      cx={point[0]}
                      cy={-point[1]}
                      r={Math.max(viewBounds.width, viewBounds.height) * 0.009}
                      class={['vertex-handle', profileEditorFor(view).selectedLoopIndex === loopIndex && profileEditorFor(view).selectedPointIndex === pointIndex && 'selected-point']}
                      role="button"
                      tabindex="0"
                      aria-label={`Loop ${loopIndex + 1} vertex ${pointIndex + 1}, ${point[0].toFixed(2)} millimeters, ${point[1].toFixed(2)} millimeters`}
                      onpointerdown={(event) => beginVertexDrag(event, loopIndex, pointIndex, view)}
                      onclick={(event) => { event.stopPropagation(); selectVertex(loopIndex, pointIndex); }}
                      onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectVertex(loopIndex, pointIndex); } }}
                    />
                  {/each}
                {/if}
              {/each}
            </svg>
            <span class="pane-camera-hint">Wheel to zoom · Shift-drag to pan</span>
          </div>
        {/each}
      </div>
      <div class="evidence-row">
        {#if currentPhoto?.recognition}
          <span class="evidence-tag">IMAGE-DERIVED</span>
          <span>{selectedView.toUpperCase()} · scale {calibratedPreviewMm?.toFixed(2) ?? 'unavailable'} mm · check dimensions · tolerance {currentPhoto.recognition.evidence.toleranceMm.toFixed(2)} mm</span>
        {:else if currentPhoto}
          <span class="evidence-tag evidence-tag--manual">DRAWING PHOTO</span>
          <span>Image coordinates in pixels · set scale and recognize before applying.</span>
        {:else if currentProjectionDrawing && !currentEvidence}
          <span class="evidence-tag exact">EXACT CAD PROJECTION</span>
          <span>{selectedView.toUpperCase()} · {currentProjectionDrawing.displayEdges.length + currentProjectionDrawing.displayCurves.length} visible edges{#if currentProjectionDrawing.visibility === 'allEdges'} · {currentProjectionDrawing.hiddenDisplayEdges.length + currentProjectionDrawing.hiddenDisplayCurves.length} hidden edges{/if} · preview only · build a closed profile before applying</span>
        {:else if currentEvidence}
          <span class={['evidence-tag', { exact: isExactProfile }]}>{isExactProfile ? 'EXACT CAD PROFILE' : 'APPROXIMATE PROFILE'}</span>
          <span>
            {#if currentEvidence.operation.kind === 'section'}
              {currentEvidence.operation.plane.toUpperCase()} section · offset {currentEvidence.operation.offsetMm.toFixed(2)} mm
            {:else}
              {currentEvidence.operation.view.toUpperCase()} projection
            {/if}
            · tolerance {currentEvidence.toleranceMm.toFixed(2)} mm
          </span>
        {:else}
          <span class="evidence-tag evidence-tag--manual">MANUAL PROFILE</span>
          <span>No derived geometry used.</span>
        {/if}
      </div>
      {#if currentEvidence?.warnings.length}
        <ul class="warning-list">
          {#each currentEvidence.warnings as warning (warning)}<li>{warning}</li>{/each}
        </ul>
      {/if}
      {#if currentProjectionDrawing?.warnings.length}
        <ul class="warning-list">
          {#each currentProjectionDrawing.warnings as warning (warning)}<li>{warning}</li>{/each}
        </ul>
      {/if}
      {#if staleEvidence}<p class="inline-hint stale-hint">Model version changed. Previous section has been cleared from this edit.</p>{/if}
      {#if selectedViewError}<p class="inline-hint stale-hint">{selectedViewError}</p>{/if}
      <div class="status-slot" data-testid="profile-status" aria-live="polite" aria-atomic="true">
        {#if currentErrorText}
          <p class="status-error">{currentErrorText}</p>
        {:else if currentEvidence && !deriveSettingsMatch}
          <p>Derivation settings changed. Derive again before applying.</p>
        {:else if currentStatusText}
          <p>{currentStatusText}</p>
        {:else if currentProjectionDrawing && !currentEvidence}
          <p>Projected edges are display-only. Build a closed profile to enable Apply.</p>
        {:else if loopError}
          <p>{loopError}</p>
        {:else}
          <p>{disabledReason ?? 'Edit a closed profile and apply it as a normal model version.'}</p>
        {/if}
      </div>
    </main>
  </div>

  <footer class="feature-footer" aria-label="Apply profile feature">
    <label>
      Profile name
      <input aria-label="Profile name" type="text" maxlength="64" bind:value={partName} disabled={currentPending !== null} />
    </label>
    <label>
      Clearance (mm)
      <input aria-label="Clearance (mm)" type="number" min="0" step="0.1" bind:value={clearance} disabled={currentPending !== null} />
    </label>
    <label>
      Depth (mm)
      <input aria-label="Depth (mm)" type="number" min="0.01" step="0.1" bind:value={depth} disabled={currentPending !== null} />
    </label>
    <button class="action-button action-button--primary" type="button" onclick={applyProfile} disabled={applyDisabled}>
      {currentPending === 'apply' ? 'Saving…' : 'Apply profile'}
    </button>
  </footer>
</section>

<style>
  .profile-workspace {
    --profile-bg: var(--background, #111923);
    --profile-panel: var(--surface, #18232d);
    --profile-border: var(--border, #38434b);
    --profile-text: var(--foreground, #e7e4dc);
    --profile-muted: var(--muted-foreground, #a6aaab);
    --profile-primary: var(--primary, #b58446);
    --profile-secondary: var(--secondary, #d1a04f);
    color: var(--profile-text);
    background: var(--profile-bg);
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .canvas-heading,
  .evidence-row,
  .tool-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .evidence-tag {
    color: var(--profile-secondary);
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  h3 { margin: 0 0 0.65rem; font-size: 0.78rem; letter-spacing: 0.08em; text-transform: uppercase; }

  .workspace-grid {
    display: grid;
    grid-template-columns: minmax(15rem, 18rem) minmax(0, 1fr);
    gap: 0.8rem;
    padding: 0.8rem;
    min-height: 0;
    flex: 1;
  }

  .editor-pane {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding: 0.55rem;
    overflow: hidden;
  }

  .view-layout-switcher {
    display: flex;
    flex: 0 0 auto;
    gap: 0.25rem;
  }
  .view-layout-switcher button {
    min-height: 1.7rem;
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--profile-border);
    background: transparent;
    color: var(--profile-muted);
    font-size: 0.6rem;
    font-weight: 700;
    letter-spacing: 0.06em;
  }
  .view-layout-switcher button[aria-pressed="true"] {
    border-color: var(--profile-secondary);
    color: var(--profile-secondary);
  }

  .sketch-workspace__panes {
    display: grid;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    grid-template-rows: repeat(2, minmax(0, 1fr));
    gap: 1px;
    background: var(--profile-border);
    overflow: hidden;
  }

  .sketch-workspace__panes--single { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); }

  .sketch-pane {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    touch-action: none;
    cursor: crosshair;
    background: color-mix(in srgb, var(--profile-panel) 84%, black 16%);
  }

  .sketch-pane--front { grid-row: span 2; }
  .sketch-pane--selected { outline: 1px solid var(--profile-secondary); outline-offset: -1px; }
  .sketch-pane__label {
    position: absolute;
    z-index: 2;
    inset: 0.35rem 0.35rem auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.25rem;
    color: var(--profile-secondary);
    font-family: var(--font-mono, monospace);
    font-size: 0.6rem;
    letter-spacing: 0.08em;
  }
  .view-select { padding: 0; border: 0; background: transparent; color: inherit; font: inherit; letter-spacing: inherit; cursor: pointer; }
  .view-select[aria-pressed="true"] { color: var(--profile-text); }
  .sketch-pane__label-action { padding: 0.12rem 0.22rem; border: 1px solid color-mix(in srgb, var(--profile-secondary) 48%, var(--profile-border)); background: color-mix(in srgb, var(--profile-panel) 84%, black 16%); color: var(--profile-secondary); font-size: 0.52rem; letter-spacing: 0.05em; }
  .sketch-pane__drawing { position: absolute; inset: 0; width: 100%; height: 100%; overflow: hidden; pointer-events: none; }
  .pane-message { position: absolute; top: 50%; left: 0.5rem; right: 0.5rem; z-index: 1; color: var(--profile-muted); font-size: 0.58rem; text-align: center; }
  .pane-message--error { color: #e89b8e; }
  .pane-camera-hint { position: absolute; bottom: 0.2rem; left: 0.3rem; color: var(--profile-muted); font-size: 0.48rem; }

  .feature-footer {
    display: flex;
    flex: 0 0 auto;
    align-items: end;
    gap: 0.6rem;
    padding: 0 0.8rem 0.8rem;
    border-top: 1px solid var(--profile-border);
    background: var(--profile-bg);
  }

  .feature-footer label {
    flex: 1 1 5rem;
    min-width: 0;
    margin: 0;
  }

  .feature-footer label:first-child {
    flex: 2 1 10rem;
  }

  .feature-footer .action-button {
    flex: 1 1 8rem;
    min-width: 7.5rem;
    width: auto;
  }

  .controls-pane,
  .editor-pane {
    min-height: 0;
    border: 1px solid var(--profile-border);
    background: var(--profile-panel);
  }

  .controls-pane {
    overflow: auto;
    display: flex;
    flex-direction: column;
  }

  .control-section { padding: 0.8rem; border-bottom: 1px solid var(--profile-border); }
  .control-section:last-child { border-bottom: 0; }
  label { display: flex; flex-direction: column; gap: 0.3rem; margin-bottom: 0.65rem; color: var(--profile-muted); font-size: 0.72rem; }
  input, select {
    box-sizing: border-box;
    width: 100%;
    min-height: 2rem;
    padding: 0.35rem 0.45rem;
    border: 1px solid var(--profile-border);
    border-radius: 0;
    background: #101820;
    color: var(--profile-text);
    font: inherit;
  }

  button { cursor: pointer; }
  button:disabled { cursor: not-allowed; opacity: 0.48; }
  .action-button, .text-button {
    min-height: 2.1rem;
    padding: 0.42rem 0.62rem;
    border: 1px solid var(--profile-border);
    border-radius: 0;
    color: var(--profile-text);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .action-button { background: #24313c; }
  .action-button--secondary { border-color: var(--profile-secondary); color: var(--profile-secondary); background: transparent; }
  .action-button--primary { width: 100%; border-color: var(--profile-primary); color: #100e0a; background: var(--profile-primary); }
  .text-button { border: 0; background: transparent; color: var(--profile-muted); }
  .tool-row { justify-content: flex-start; flex-wrap: wrap; }
  .inline-hint { min-height: 1.2rem; margin: 0.4rem 0 0; color: var(--profile-muted); font-size: 0.68rem; line-height: 1.45; }
  .vertex-fields { display: grid; grid-template-columns: 1fr 1fr; gap: 0.55rem; }

  .grid-line { fill: none; stroke: #29343d; stroke-width: 0.55; vector-effect: non-scaling-stroke; }
  .axis-line { stroke: #59636a; stroke-width: 0.8; vector-effect: non-scaling-stroke; }
  .sketch-pane polygon,
  .analytic-profile-circle { fill: color-mix(in srgb, var(--profile-primary) 22%, transparent); stroke: var(--profile-muted); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .sketch-pane polygon.selected-loop { stroke: var(--profile-secondary); stroke-width: 1.5; }
  .analytic-profile-circle.selected-loop { stroke: var(--profile-secondary); stroke-width: 1.5; }
  .sketch-pane polygon.hole,
  .analytic-profile-circle.hole { fill: color-mix(in srgb, #aa5b50 20%, transparent); stroke: #c78274; }
  .projected-edge { fill: none; stroke: #82b1a2; stroke-width: 1.05; vector-effect: non-scaling-stroke; pointer-events: none; }
  .projected-line, .projected-circle, .projected-circular-arc, .projected-ellipse, .projected-elliptical-arc { fill: none; stroke: #82b1a2; stroke-width: 1.05; vector-effect: non-scaling-stroke; pointer-events: none; }
  .projected-layer--hidden :is(line, circle, path, ellipse, polyline) {
    stroke: #607d78;
    stroke-width: 0.7;
    stroke-dasharray: 5 4;
    stroke-linecap: butt;
  }
  .sketch-pane polygon.open-draft { fill: none; stroke-dasharray: 4 3; }
  .drawing-region { fill: color-mix(in srgb, var(--profile-secondary) 16%, transparent); stroke: var(--profile-secondary); stroke-width: 1.2; stroke-dasharray: 5 3; vector-effect: non-scaling-stroke; pointer-events: none; }
  .drawing-pick-point { fill: #f1c879; stroke: #17120a; stroke-width: 1; vector-effect: non-scaling-stroke; pointer-events: none; }
  .drawing-vertex-handle { fill: #f1c879; stroke: #241b0f; stroke-width: 1; vector-effect: non-scaling-stroke; pointer-events: all; cursor: move; }
  .drawing-calibration-point { fill: #ef7d64; stroke: #20120f; stroke-width: 1; vector-effect: non-scaling-stroke; pointer-events: none; }
  .drawing-primitive { fill: none; stroke: #f0c572; stroke-width: 1.3; vector-effect: non-scaling-stroke; pointer-events: stroke; cursor: pointer; }
  .drawing-primitive--selected { stroke: #fff0c6; stroke-width: 2; }
  .drawing-circle { fill: color-mix(in srgb, #d76857 16%, transparent); }
  .vertex-handle { fill: var(--profile-panel); stroke: var(--profile-secondary); stroke-width: 1.5; vector-effect: non-scaling-stroke; cursor: grab; pointer-events: all; }
  .vertex-handle.selected-point { fill: var(--profile-secondary); stroke: #f0d6a5; }
  .evidence-row { justify-content: flex-start; flex-wrap: wrap; min-height: 2.3rem; color: var(--profile-muted); font-size: 0.68rem; }
  .evidence-tag { padding: 0.25rem 0.35rem; border: 1px solid #776044; }
  .evidence-tag--manual { color: #b9c0c2; border-color: var(--profile-border); }
  .warning-list { margin: 0.15rem 0 0.5rem; padding-left: 1.2rem; color: #e2bc80; font-size: 0.68rem; }
  .stale-hint { color: #e2bc80; }
  .status-slot {
    box-sizing: border-box;
    height: 3.2rem;
    flex: 0 0 3.2rem;
    margin-top: 0.5rem;
    padding: 0.55rem 0.65rem;
    border: 1px solid var(--profile-border);
    color: var(--profile-muted);
    font-size: 0.7rem;
    line-height: 1.4;
    overflow: auto;
  }
  .status-slot p { margin: 0; }
  .status-error { color: #e89b8e; }

  @media (max-width: 760px) {
    .workspace-grid { grid-template-columns: minmax(0, 1fr); overflow: auto; }
    .controls-pane { overflow: visible; }
    .editor-pane { min-height: 22rem; }
    .sketch-workspace__panes { min-height: 18rem; }
  }

  @media (max-width: 560px) {
    .feature-footer { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
    .feature-footer label:first-child { grid-column: 1 / -1; }
    .feature-footer .action-button { grid-column: 1 / -1; }
  }
</style>
