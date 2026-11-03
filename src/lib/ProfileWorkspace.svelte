<script lang="ts">
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
    removeProfileLoop,
    type ProfileApplyInput,
    type ProfileApplyOutcome,
    type ProfileDeriveInput,
    type ProfileGeometryOperation,
    type ProfileLoop,
    type ProfileLoopRole,
    type ProfilePlane,
    type ProfileGeometryResponse,
    type ProfilePoint,
  } from './profileWorkspace';

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

  type Props = {
    scopeKey: string;
    sourceAvailable: boolean;
    exactCadAvailable: boolean;
    sourceAccuracyHint?: string | null;
    disabledReason?: string | null;
    onDerive: (input: ProfileDeriveInput) => Promise<ProfileGeometryResponse>;
    onApply: (input: ProfileApplyInput) => Promise<ProfileApplyOutcome>;
  };

  let {
    scopeKey,
    sourceAvailable,
    exactCadAvailable,
    sourceAccuracyHint = null,
    disabledReason = null,
    onDerive,
    onApply,
  }: Props = $props();

  let profileEdits = $state<Record<ProfileView, ProfileEditorState>>({
    front: createProfileEditor(''), top: createProfileEditor(''), side: createProfileEditor(''),
  });
  let geometryEvidence = $state<Record<ProfileView, { scopeKey: string; response: ProfileGeometryResponse } | null>>({ front: null, top: null, side: null });
  let viewPending = $state<Record<ProfileView, { scopeKey: string; requestId: number } | null>>({ front: null, top: null, side: null });
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
  let draftLoopIndex = $derived(currentEdit.draftLoopIndex);
  let drawing = $derived(currentEdit.drawing);
  let visibleViews = $derived(viewLayout === 'allThree' ? PROFILE_VIEWS : [selectedView]);
  let currentEvidence = $derived(geometryEvidence[selectedView]?.scopeKey === scopeKey ? geometryEvidence[selectedView]?.response ?? null : null);
  let selectedViewError = $derived(viewErrorFor(selectedView));
  let convertedSelectedEvidence = $derived(currentEdit.convertedExact);
  let isExactProfile = $derived(
    currentEvidence?.accuracy === 'exactBrep'
      && Boolean(currentEvidence.profileAsset)
      && !convertedSelectedEvidence,
  );
  let isConvertedPolygon = $derived(convertedSelectedEvidence && currentEvidence?.accuracy === 'exactBrep');
  let currentPending = $derived(pendingScopeKey === scopeKey ? pending : null);
  let currentViewsPending = $derived(PROFILE_VIEWS.some(viewPendingFor));
  let currentErrorText = $derived(errorScopeKey === scopeKey ? errorText : '');
  let currentStatusText = $derived(statusScopeKey === scopeKey ? statusText : '');
  let canUndo = $derived(currentEdit.undoItems.length > 0 && currentPending === null);
  let loopError = $derived(isExactProfile ? null : profileLoopError(currentLoops));
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
      || drawing
      || loopError !== null
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
      scopeKey: targetScope, loops: [], undoItems: [], selectedLoopIndex: 0,
      selectedPointIndex: null, draftLoopIndex: null, drawing: false,
      drawingTool: 'polyline', convertedExact: false, placementPlane: 'xy', placementOffset: '0',
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

  function viewErrorFor(view: ProfileView): string {
    const error = viewErrors[view];
    return error?.scopeKey === scopeKey ? error.message : '';
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
      undoItems: items, loops, selectedPointIndex: null,
      selectedLoopIndex: Math.max(0, loops.length - 1), drawing: false, draftLoopIndex: null,
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
    if (deriveMode === 'exactProjection') {
      await deriveAllProjections();
      return;
    }
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
          ...createProfileEditor(requestScope), loops: normalizeProfileLoops(loops),
          selectedLoopIndex: Math.max(0, outerIndex), placementPlane: placement.plane,
          placementOffset: String(placement.offsetMm),
        },
      };
      resetPaneCamera(view);
      setStatus(response.accuracy === 'exactBrep'
        ? 'Exact CAD outline ready. Convert it to a polygon before editing vertices.'
        : response.loops.length
          ? 'Approximate mesh profile ready. Edit vertices, then apply the profile.'
          : 'Section contained no closed profile loops.');
    } catch (error) {
      if (profileRequestIsCurrent(requestScope, requestId, scopeKey, activeRequestId)) {
        setError(error instanceof Error ? error.message : String(error));
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

  async function deriveAllProjections(): Promise<void> {
    const toleranceMm = finiteValue(tolerance, 'Tolerance');
    if (toleranceMm === null) return;
    if (toleranceMm <= 0) {
      setError('Tolerance must be greater than zero.');
      return;
    }
    const requestScope = scopeKey;
    setError('');
    setStatus('Projecting the current model into front, top, and side views…');
    const tasks = PROFILE_VIEWS.map(async (view) => {
      const requestId = ++requestSequence;
      viewRequestIds = { ...viewRequestIds, [view]: requestId };
      viewPending = { ...viewPending, [view]: { scopeKey: requestScope, requestId } };
      viewErrors = { ...viewErrors, [view]: { scopeKey: requestScope, message: '' } };
      try {
        const response = await onDerive({
          operation: { kind: 'projection', view },
          toleranceMm,
          source: 'step',
        });
        if (requestScope !== scopeKey || viewRequestIds[view] !== requestId) return;
        geometryEvidence = { ...geometryEvidence, [view]: { scopeKey: requestScope, response } };
        const placement = profilePlacementForOperation(response.operation);
        profileEdits = { ...profileEdits, [view]: { ...createProfileEditor(requestScope), placementPlane: placement.plane, placementOffset: String(placement.offsetMm) } };
        resetPaneCamera(view);
      } catch (error) {
        if (requestScope === scopeKey && viewRequestIds[view] === requestId) {
          viewErrors = { ...viewErrors, [view]: { scopeKey: requestScope, message: error instanceof Error ? error.message : String(error) } };
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
      setStatus(failed.length ? `${3 - failed.length} of 3 projections ready; failed views remain available for retry.` : 'Front, top, and side projections ready.');
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
    patchProfileEditor(selectedView, { draftLoopIndex: nextDraftLoopIndex, selectedLoopIndex: nextDraftLoopIndex, drawing: true, drawingTool: activeTool });
    setError('');
    setStatus(activeTool === 'polyline'
      ? 'Click three or more points in the selected view, then close the loop.'
      : `Click two points in the selected view to create an approximate ${activeTool}.`);
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
    const draftTool = currentEdit.drawingTool;
    const draft = draftIndex === null ? null : loops[draftIndex];
    if (!draft || draftIndex === null) return;
    if (draftTool === 'polyline' || draft.points.length === 0) {
      loops[draftIndex] = { ...draft, points: [...draft.points, point] };
    } else if (draftTool === 'rectangle') {
      const [x1, y1] = draft.points[0]!;
      loops[draftIndex] = { ...draft, points: [[x1, y1], [point[0], y1], [point[0], point[1]], [x1, point[1]]] };
      patchProfileEditor(selectedView, { drawing: false, draftLoopIndex: null });
    } else {
      const [cx, cy] = draft.points[0]!;
      const radius = Math.hypot(point[0] - cx, point[1] - cy);
      const points = Array.from({ length: 48 }, (_, index) => {
        const angle = (2 * Math.PI * index) / 48;
        return [cx + radius * Math.cos(angle), cy + radius * Math.sin(angle)] as ProfilePoint;
      });
      loops[draftIndex] = { ...draft, points };
      patchProfileEditor(selectedView, { drawing: false, draftLoopIndex: null });
    }
    replaceLoops(loops);
    patchProfileEditor(selectedView, { selectedLoopIndex: currentEdit.draftLoopIndex ?? loops.length - 1 });
    selectView(view);
  }

  function closeLoop(): void {
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

  function selectVertex(loopIndex: number, pointIndex: number): void {
    patchProfileEditor(selectedView, { selectedLoopIndex: loopIndex, selectedPointIndex: pointIndex });
  }

  function selectView(view: ProfileView): void {
    if (view === selectedView) return;
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
    const evidence = geometryEvidence[view]?.scopeKey === scopeKey ? geometryEvidence[view]?.response : null;
    if (evidence) return profileBounds(evidence.loops);
    const loops = view === selectedView ? currentLoops : profileEditorFor(view).loops;
    if (loops.length) return profileBounds(loops);
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
    selectView(view);
    if (event.button !== 1 && !event.shiftKey) return;
    event.preventDefault();
    const element = event.currentTarget as HTMLElement;
    element.setPointerCapture(event.pointerId);
    activePan = { view, pointerId: event.pointerId, clientX: event.clientX, clientY: event.clientY, camera: paneCameras[view] };
  }

  function handlePanePointerMove(event: PointerEvent, view: ProfileView): void {
    if (!activePan || activePan.view !== view || activePan.pointerId !== event.pointerId) return;
    const zoom = activePan.camera.zoom;
    const centerX = activePan.camera.centerX - (event.clientX - activePan.clientX) / zoom;
    const centerY = activePan.camera.centerY + (event.clientY - activePan.clientY) / zoom;
    paneCameras = { ...paneCameras, [view]: { ...activePan.camera, centerX, centerY } };
  }

  function handlePanePointerUp(event: PointerEvent): void {
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

  function exactView(view: ProfileView): boolean {
    const evidence = evidenceForView(view);
    return evidence?.accuracy === 'exactBrep'
      && Boolean(evidence.profileAsset)
      && !profileEditorFor(view).convertedExact;
  }

  function loopsForView(view: ProfileView): ProfileLoop[] {
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
    const loops = profileLoopsForApply(
      currentEvidence,
      convertedSelectedEvidence,
      currentLoops,
    );
    const validation = loops ? profileLoopError(loops) : null;
    if (validation) {
      setError(validation);
      return;
    }

    const reference = profileReferenceForApply(
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
        plane: currentEdit.placementPlane,
        offsetMm,
        clearanceMm,
        depthMm,
        partName: partName.trim(),
        reference,
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
        setError(error instanceof Error ? error.message : String(error));
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
        <button class="action-button action-button--secondary" type="button" onclick={deriveProfile} disabled={!deriveAvailable || currentPending !== null || drawing || currentViewsPending}>
          {deriveMode === 'exactProjection' && currentViewsPending
            ? 'Projecting…'
            : currentPending === 'section'
            ? 'Deriving…'
            : deriveMode === 'exactProjection'
              ? 'Project all three views'
              : deriveMode === 'exactSection'
                ? 'Extract exact section'
                : 'Extract approximate section'}
        </button>
        {#if sourceWarning}<p class="inline-hint">{sourceWarning}</p>{/if}
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
            <select aria-label="New loop type" bind:value={drawRole} disabled={drawing || currentPending !== null}>
              <option value="outer">Outer profile</option>
              <option value="hole">Hole</option>
            </select>
          </label>
          <label>
            Drawing tool
            <select aria-label="Drawing tool" bind:value={activeTool} disabled={drawing || currentPending !== null}>
              <option value="polyline">Polyline</option>
              <option value="rectangle">Rectangle</option>
              <option value="circle">Circle · approximate</option>
            </select>
          </label>
          <label class="snap-control">
            <span>Snap to grid</span>
            <input aria-label="Snap to grid" type="checkbox" bind:checked={snapToGrid} disabled={drawing || currentPending !== null} />
          </label>
          {#if snapToGrid}
            <label>
              Grid size (mm)
              <input aria-label="Grid size (mm)" type="number" min="0.001" step="0.1" bind:value={sketchGridSize} disabled={drawing || currentPending !== null} />
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
              <select aria-label="Selected loop" bind:value={selectedLoopIndex} disabled={currentPending !== null || drawing}>
                {#each currentLoops as loop, index (`${index}-${loop.role}`)}
                  <option value={index}>{loop.role === 'hole' ? 'Hole' : 'Outer'} {index + 1} · {loop.points.length} pts</option>
                {/each}
              </select>
            </label>
            <button class="text-button" type="button" onclick={removeSelectedLoop} disabled={currentPending !== null || drawing}>Remove selected loop</button>
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
          <select aria-label="Profile plane" value={currentEdit.placementPlane} onchange={(event) => patchProfileEditor(selectedView, { placementPlane: event.currentTarget.value as ProfilePlane })} disabled={currentPending !== null || currentEvidence !== null}>
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
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div
            class={['sketch-pane', `sketch-pane--${view}`, selectedView === view && 'sketch-pane--selected', drawing && selectedView === view && 'draw-mode']}
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
            onpointerup={handlePanePointerUp}
            onpointercancel={handlePanePointerUp}
            onclick={(event) => {
              if (drawing && selectedView === view) addDrawingPoint(event, view);
              else selectView(view);
            }}
            onwheel={(event) => handlePaneWheel(event, view)}
          >
            <div class="sketch-pane__label">
              <button class="view-select" type="button" aria-label={`Select ${view} projection`} aria-pressed={selectedView === view} onclick={(event) => { event.stopPropagation(); selectView(view); }}>{view.toUpperCase()}</button>
              <span>{viewPendingFor(view) ? 'PROJECTING…' : viewErrorFor(view) ? 'ERROR' : viewLoops.length ? `${viewLoops.length} LOOPS` : 'NO PROFILE'}</span>
              <button class="sketch-pane__label-action" type="button" onclick={(event) => { event.stopPropagation(); resetPaneCamera(view); }}>FIT VIEW</button>
            </div>
            {#if viewPendingFor(view)}
              <span class="pane-message">Projecting current model…</span>
            {:else if viewErrorFor(view)}
              <span class="pane-message pane-message--error">{viewErrorFor(view)}</span>
            {:else if viewLoops.length === 0}
              <span class="pane-message">Select this view, draw a profile, or project all views.</span>
            {/if}
            <svg class="sketch-pane__drawing" viewBox={viewBox(view)} preserveAspectRatio="none" aria-hidden="true">
              <defs>
                <pattern id={`profile-grid-${view}`} width="10" height="10" patternUnits="userSpaceOnUse" patternTransform="scale(1,-1)">
                  <path d="M 10 0 L 0 0 0 10" class="grid-line" />
                </pattern>
              </defs>
              <rect x={visibleBounds.minX} y={-visibleBounds.minY - visibleBounds.height} width={visibleBounds.width} height={visibleBounds.height} fill={`url(#profile-grid-${view})`} />
              <line x1={0} y1={-visibleBounds.minY - visibleBounds.height} x2={0} y2={-visibleBounds.minY} class="axis-line" />
              <line x1={visibleBounds.minX} y1={0} x2={visibleBounds.minX + visibleBounds.width} y2={0} class="axis-line" />
              {#each viewLoops as loop, loopIndex (`${view}-${loopIndex}-${loop.points.length}-${loop.role}`)}
                {@const pathPoints = loop.points.map(([x, y]) => `${x},${-y}`).join(' ')}
                <polygon points={pathPoints} class={[loop.role, loop.role === 'hole' && 'hole', profileEditorFor(view).selectedLoopIndex === loopIndex && 'selected-loop', profileEditorFor(view).drawing && loopIndex === profileEditorFor(view).draftLoopIndex && 'open-draft']} fill-rule="evenodd" />
                {#if selectedView === view && !isViewExact && !drawing}
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
        {#if currentEvidence}
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
      {#if staleEvidence}<p class="inline-hint stale-hint">Model version changed. Previous section has been cleared from this edit.</p>{/if}
      {#if selectedViewError}<p class="inline-hint stale-hint">{selectedViewError}</p>{/if}
      <div class="status-slot" data-testid="profile-status" aria-live="polite" aria-atomic="true">
        {#if currentErrorText}
          <p class="status-error">{currentErrorText}</p>
        {:else if currentEvidence && !deriveSettingsMatch}
          <p>Derivation settings changed. Derive again before applying.</p>
        {:else if currentStatusText}
          <p>{currentStatusText}</p>
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

  .view-layout-switcher { display: flex; flex: 0 0 auto; gap: 0.25rem; }
  .view-layout-switcher button { min-height: 1.7rem; padding: 0.25rem 0.5rem; border: 1px solid var(--profile-border); background: transparent; color: var(--profile-muted); font-size: 0.6rem; font-weight: 700; letter-spacing: 0.06em; }
  .view-layout-switcher button[aria-pressed="true"] { border-color: var(--profile-secondary); color: var(--profile-secondary); }

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
  .sketch-workspace__panes--single > .sketch-pane--front { grid-row: auto; }

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
  .sketch-pane polygon { fill: color-mix(in srgb, var(--profile-primary) 22%, transparent); stroke: var(--profile-muted); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .sketch-pane polygon.selected-loop { stroke: var(--profile-secondary); stroke-width: 1.5; }
  .sketch-pane polygon.hole { fill: color-mix(in srgb, #aa5b50 20%, transparent); stroke: #c78274; }
  .sketch-pane polygon.open-draft { fill: none; stroke-dasharray: 4 3; }
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
