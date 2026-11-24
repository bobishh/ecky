<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { onDestroy, onMount, untrack } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
  import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';
  import { ThreeMFLoader } from 'three/examples/jsm/loaders/3MFLoader.js';
  import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js';
  import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
  import ViewportTransmutation from './ViewportTransmutation.svelte';
  import { estimateBase64Bytes, profileLog } from './debug/profiler';
  import type {
    Advisory,
    ParamValue,
    PartBinding,
    UiField,
    ViewerAsset,
    ViewerEdgeTarget,
    ViewerFaceTarget,
    ViewportCameraState,
  } from './types/domain';
  import {
    shouldDisplayViewportControlList,
    type MeasurementControlFocus,
    type ResolvedMeasurementCallout,
    type ContextSelectionTarget,
  } from './modelRuntime/contextualEditing';
  import type { ImportedPreviewTransform } from './modelRuntime/importedRuntime';
  import type { MaterializedSemanticControl } from './modelRuntime/semanticControls';
  import {
    cycleTopologyMode,
    meshTopologyOpacity,
    meshTopologyVisible,
    topologyModeLabel,
    type TopologyMode,
  } from './viewerDisplayMode';
  import { resolveViewerTone, type ViewerTone } from './viewerLook';
  import { resolveViewerAssetUrl } from './viewerAssetUrl';
  import { resolveViewerClipPlanes } from './viewerCameraPolicy';
  import { materializeViewerTopology } from './viewerTopologyBudget';
  import { shouldHandleSelectionClick, shouldHandleViewerClick } from './viewerInteraction';
  import { prepareStlDisplayGeometry } from './viewerStlNormals';
  import {
    captureSurfaceAnchorFromIntersection,
    type CaptureSurfaceAnchorValue,
  } from './capture/captureSurfaceAnchor';
  import { buildCaptureGuideOverlayPrimitives } from './capture/captureGuideOverlay';
  import { buildCaptureDeviationDisplayPoints } from './capture/captureDeviationOverlay';
  import type {
    CaptureObservedDeviationReport,
    CaptureReconstructionGuide,
    SurfaceTrimCapPreview,
    SurfaceTrimLoopSegmentPreview,
  } from './tauri/contracts';
  import type { FemMeshPreviewResponse, FemRunResponse } from './tauri/client';
  import { femColorRamp, normalizeFemField, type FemDisplayOptions } from './femDisplay';

  type ViewportBusyPhase = 'generating' | 'repairing' | 'rendering' | 'committing' | null;
  type PositionMm = [number, number, number];

  let {
    modelKey = null,
    stlUrl = null,
    viewerAssets = [],
    manifestParts = [],
    edgeTargets = [],
    faceTargets = [],
    selectionTargets = [],
    selectedTarget = null,
    searchQuery = '',
    outlineEnabled = true,
    selectedPartId = null,
    overlayPartLabel = null,
    overlayPartEditable = false,
    overlayPreviewOnly = false,
    showContextOverlay = true,
    overlayControls = [],
    overlayAdvisories = [],
    activeMeasurementCallout = null,
    previewTransforms = {},
    parameterGhostStlUrl = null,
    onParameterGhostLoad,
    isGenerating = false,
    retainModelWhileLoading = false,
    hideModelWhileBusy = false,
    busyPhase = null,
    busyText = null,
    topologyMode = 'mesh',
    viewerMode = 'orbit',
    persistedCameraState = null,
    cropBoxEnabled = false,
    cropBoxMode = 'translate',
    cropBounds = null,
    componentPlacement = null,
    captureLandmarkMode = false,
    captureSourceMeshContentDigest = null,
    capturePlaneAnchors = [],
    surfaceTrimActive = false,
    surfaceTrimAnchors = [],
    surfaceTrimKeepSeed = null,
    surfaceTrimLoopSegments = [],
    surfaceTrimRetainedTriangleIndices = [],
    surfaceTrimCapPreview = null,
    surfaceTrimSelectedAnchorIndex = null,
    captureGuide = null,
    captureSelectedLandmarkId = null,
    captureComparisonStlUrl = null,
    captureDeviation = null,
    captureDeviationVisible = true,
    captureReferenceVisible = true,
    captureReferenceOpacity = 0.28,
    captureGeneratedVisible = true,
    captureGeneratedOpacity = 1,
    femResult = null,
    femMeshPreview = null,
    femDisplay = null,
    onSearchQueryChange,
    onSelectTarget,
    onOverlayChange,
    onControlFocusChange,
    onCameraStateChange,
    onModelLoaded,
    onModelLoadError,
    onCropBoundsChange,
    onComponentPlacementMove,
    onComponentPlacementLoad,
    onComponentPlacementError,
    onCaptureSurfaceAnchor,
    onCaptureSurfaceHover,
    onCaptureSurfaceAnchorError,
    onCaptureSelectLandmark,
    onSurfaceTrimPointSelect,
  }: {
    modelKey?: string | null;
    stlUrl?: string | null;
    viewerAssets?: ViewerAsset[];
    manifestParts?: PartBinding[];
    edgeTargets?: ViewerEdgeTarget[];
    faceTargets?: ViewerFaceTarget[];
    selectionTargets?: ContextSelectionTarget[];
    selectedTarget?: ContextSelectionTarget | null;
    searchQuery?: string;
    outlineEnabled?: boolean;
    selectedPartId?: string | null;
    overlayPartLabel?: string | null;
    overlayPartEditable?: boolean;
    overlayPreviewOnly?: boolean;
    showContextOverlay?: boolean;
    overlayControls?: MaterializedSemanticControl[];
    overlayAdvisories?: Advisory[];
    activeMeasurementCallout?: ResolvedMeasurementCallout | null;
    previewTransforms?: Record<string, ImportedPreviewTransform>;
    parameterGhostStlUrl?: string | null;
    onParameterGhostLoad?: (url: string, error?: string) => void;
    isGenerating?: boolean;
    retainModelWhileLoading?: boolean;
    hideModelWhileBusy?: boolean;
    busyPhase?: ViewportBusyPhase;
    busyText?: string | null;
    topologyMode?: TopologyMode;
    viewerMode?: 'orbit' | 'select' | 'measure';
    persistedCameraState?: ViewportCameraState | null;
    cropBoxEnabled?: boolean;
    cropBoxMode?: 'translate' | 'scale';
    cropBounds?: { min: [number, number, number]; max: [number, number, number] } | null;
    componentPlacement?: { stlUrl: string; positionMm: PositionMm; disabled?: boolean } | null;
    captureLandmarkMode?: boolean;
    captureSourceMeshContentDigest?: string | null;
    capturePlaneAnchors?: CaptureSurfaceAnchorValue[];
    surfaceTrimActive?: boolean;
    surfaceTrimAnchors?: CaptureSurfaceAnchorValue[];
    surfaceTrimKeepSeed?: CaptureSurfaceAnchorValue | null;
    surfaceTrimLoopSegments?: SurfaceTrimLoopSegmentPreview[];
    surfaceTrimRetainedTriangleIndices?: number[];
    surfaceTrimCapPreview?: SurfaceTrimCapPreview | null;
    surfaceTrimSelectedAnchorIndex?: number | null;
    captureGuide?: CaptureReconstructionGuide | null;
    captureSelectedLandmarkId?: string | null;
    captureComparisonStlUrl?: string | null;
    captureDeviation?: CaptureObservedDeviationReport | null;
    captureDeviationVisible?: boolean;
    captureReferenceVisible?: boolean;
    captureReferenceOpacity?: number;
    captureGeneratedVisible?: boolean;
    captureGeneratedOpacity?: number;
    femResult?: FemRunResponse | null;
    femMeshPreview?: FemMeshPreviewResponse | null;
    femDisplay?: FemDisplayOptions | null;
    onSearchQueryChange?: (query: string) => void;
    onSelectTarget?: (target: ContextSelectionTarget | null) => void;
    onOverlayChange?: (primitiveId: string, value: ParamValue) => Promise<void> | void;
    onControlFocusChange?: (focus: MeasurementControlFocus | null) => void;
    onCameraStateChange?: (camera: ViewportCameraState) => void;
    onModelLoaded?: () => void;
    onModelLoadError?: (message: string) => void;
    onCropBoundsChange?: (bounds: { min: [number, number, number]; max: [number, number, number] }) => void;
    onComponentPlacementMove?: (positionMm: PositionMm) => void;
    onComponentPlacementLoad?: (stlUrl: string) => void;
    onComponentPlacementError?: (stlUrl: string, message: string) => void;
    onCaptureSurfaceAnchor?: (anchor: CaptureSurfaceAnchorValue) => void;
    onCaptureSurfaceHover?: (anchor: CaptureSurfaceAnchorValue | null) => void;
    onCaptureSurfaceAnchorError?: (message: string) => void;
    onCaptureSelectLandmark?: (landmarkId: string) => void;
    onSurfaceTrimPointSelect?: (index: number) => void;
  } = $props();

  type RuntimeMesh = {
    partId: string | null;
    baseBounds: THREE.Box3 | null;
    outline: THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial> | null;
    mesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>;
    sourcePickMesh?: THREE.Mesh<THREE.BufferGeometry, THREE.MeshBasicMaterial>;
    topology: THREE.LineSegments<THREE.WireframeGeometry, THREE.LineBasicMaterial> | null;
    tone: ViewerTone;
    captureLayer?: 'reference' | 'generated';
  };

  type RuntimeEdge = {
    targetId: string;
    durableTargetId?: string | null;
    canonicalTargetId?: string | null;
    aliasIds: string[];
    partId: string;
    line: THREE.Line<THREE.BufferGeometry, THREE.LineBasicMaterial>;
  };

  type RuntimeFace = {
    targetId: string;
    durableTargetId?: string | null;
    canonicalTargetId?: string | null;
    aliasIds: string[];
    partId: string;
    basePosition: THREE.Vector3;
    mesh: THREE.Mesh<THREE.CircleGeometry, THREE.MeshBasicMaterial>;
  };

  let viewerHost: HTMLDivElement;
  let scene: THREE.Scene | null = null;
  let camera: THREE.PerspectiveCamera | null = null;
  let renderer: THREE.WebGLRenderer | null = null;
  let controls: OrbitControls | null = null;
  let cropTransformControls: TransformControls | null = null;
  let cropTransformHelper: THREE.Object3D | null = null;
  let cropBoxMesh: THREE.Mesh<THREE.BoxGeometry, THREE.MeshBasicMaterial> | null = null;
  let appliedCropBoundsSignature = '';
  let viewerGrid: THREE.GridHelper | null = null;
  let componentPlacementPreview: {
    frame: THREE.Group;
    mesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>;
    transform: TransformControls;
  } | null = null;
  let componentPlacementReady = $state(false);
  let componentPlacementPositionChosen = false;
  let parameterGhostMesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial> | null = null;
  let parameterGhostReady = $state(false);
  let modelRoot: THREE.Group | null = null;
  let runtimeMeshes: RuntimeMesh[] = [];
  let runtimeEdges: RuntimeEdge[] = [];
  let runtimeFaces: RuntimeFace[] = [];
  let animationFrameId: number | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let loadToken = 0;
  let modelStatus = $state<'empty' | 'loaded'>('empty');
  let overlayLeft = $state(24);
  let overlayTop = $state(24);
  let overlayVisible = $state(false);
  let overlayFallback = $state(true);
  let hoveredPartId = $state<string | null>(null);
  let hoveredTargetId = $state<string | null>(null);
  let inspectedPartId = $state<string | null>(null);
  let orbitDraggedSincePointerDown = $state(false);
  let dimensionFrame = $state<{ bottom: number; height: number; left: number; right: number; top: number; width: number } | null>(null);
  let measurementOverlay = $state<{
    badgeLeft: number;
    badgeTop: number;
    lineSegments: Array<{ x1: number; y1: number; x2: number; y2: number }>;
    label: string;
    explanation: string | null;
  } | null>(null);
  let captureGuideProjected = $state<{
    points: Array<{
      landmarkId: string;
      ordinal: number;
      label: string;
      role: string;
      x: number;
      y: number;
    }>;
    segments: Array<{ key: string; kind: 'profile' | 'axis'; x1: number; y1: number; x2: number; y2: number }>;
    planePolygons: Array<{ key: string; points: string }>;
  }>({ points: [], segments: [], planePolygons: [] });
  let capturePlaneProjected = $state<Array<{ x: number; y: number }>>([]);
  let capturePlaneNormalProjected = $state<{
    x1: number;
    y1: number;
    x2: number;
    y2: number;
  } | null>(null);
  let surfaceTrimProjected = $state<{
    points: Array<{ x: number; y: number }>;
    keepSeed: { x: number; y: number } | null;
    segments: Array<{ key: string; x1: number; y1: number; x2: number; y2: number }>;
  }>({ points: [], keepSeed: null, segments: [] });
  let surfaceTrimRegionOverlay: THREE.Mesh<THREE.BufferGeometry, THREE.MeshBasicMaterial> | null = null;
  let surfaceTrimCapOverlay: THREE.Mesh<THREE.BufferGeometry, THREE.MeshBasicMaterial> | null = null;
  let captureComparisonLoaded = $state(false);
  let captureDeviationOverlay: THREE.Points<THREE.BufferGeometry, THREE.PointsMaterial> | null = null;
  let captureDeviationPointCount = $state(0);
  let femOverlay = $state.raw<THREE.Group | null>(null);
  let femOverlayKind = $state<'mesh' | 'result' | null>(null);
  let femLoadToken = 0;
  let femOverlayError = $state('');
  let femLegend = $state<{ label: string; minimum: number; maximum: number; unit: string } | null>(null);
  const viewerAssetSignature = $derived.by(() =>
    viewerAssets.map((asset) => `${asset.partId}:${asset.nodeId}:${asset.path}`).join('|'),
  );
  const manifestPartSignature = $derived.by(() =>
    manifestParts.map((part) => `${part.partId}:${part.label}:${part.kind}:${part.semanticRole ?? ''}`).join('|'),
  );
  const topologyMaterialization = $derived(
    materializeViewerTopology(edgeTargets.length, faceTargets.length),
  );
  const edgeTargetSignature = $derived.by(() =>
    !topologyMaterialization.materialize
      ? `query-only:${edgeTargets.length}`
      :
    edgeTargets
      .map((target) => [
        target.targetId,
        target.durableTargetId ?? '',
        target.canonicalTargetId ?? '',
        target.partId,
        target.start.x,
        target.start.y,
        target.start.z,
        target.end.x,
        target.end.y,
        target.end.z,
      ].join(':'))
      .join('|'),
  );
  const faceTargetSignature = $derived.by(() =>
    !topologyMaterialization.materialize
      ? `query-only:${faceTargets.length}`
      :
    faceTargets
      .map((target) => [
        target.targetId,
        target.durableTargetId ?? '',
        target.canonicalTargetId ?? '',
        target.partId,
        target.center.x,
        target.center.y,
        target.center.z,
        target.normal?.join(',') ?? '',
        target.area ?? '',
      ].join(':'))
      .join('|'),
  );
  const modelLoadSignature = $derived.by(
    () => `${modelKey ?? ''}::${stlUrl ?? ''}::${captureComparisonStlUrl ?? ''}::${viewerAssetSignature}::${manifestPartSignature}`,
  );
  const showEditableCallouts = $derived(false);
  const selectionMode = $derived.by(() => viewerMode === 'select');
  const componentPlacementUrl = $derived(componentPlacement?.stlUrl ?? null);
  const capturePickingMode = $derived.by(() => captureLandmarkMode && Boolean(captureSourceMeshContentDigest));
  const captureGuidePrimitives = $derived.by(() =>
    captureGuide ? buildCaptureGuideOverlayPrimitives(captureGuide) : null,
  );
  const captureGuideSignature = $derived.by(() => captureGuide
    ? [
        captureGuide.guideId,
        captureGuide.revision,
        captureGuide.canonicalDigest,
        ...captureGuide.landmarks.map(item => `${item.landmarkId}:${item.role}:${item.anchor.sourcePosition.join(',')}`),
        ...captureGuide.profiles.map(item => `${item.profileId}:${item.kind}:${item.landmarkIds.join(',')}`),
        ...captureGuide.axes.map(item => `${item.axisId}:${item.landmarkIds.join(',')}`),
        ...captureGuide.planes.map(item => `${item.planeId}:${item.landmarkIds.join(',')}`),
      ].join('|')
    : '');
  const capturePlaneSignature = $derived.by(() => capturePlaneAnchors
    .map(anchor => `${anchor.triangleIndex}:${anchor.barycentric.join(',')}:${anchor.sourcePosition.join(',')}`)
    .join('|'));
  const surfaceTrimSignature = $derived.by(() => [
    surfaceTrimActive ? 'active' : 'inactive',
    ...surfaceTrimAnchors.map(anchor => `${anchor.triangleIndex}:${anchor.barycentric.join(',')}:${anchor.sourcePosition.join(',')}`),
    surfaceTrimKeepSeed ? `seed:${surfaceTrimKeepSeed.triangleIndex}:${surfaceTrimKeepSeed.sourcePosition.join(',')}` : 'no-seed',
    ...surfaceTrimLoopSegments.flatMap(segment => segment.continuousPolyline.map((point, index) => `${segment.segmentIndex}:${index}:${point.sourcePosition.join(',')}`)),
    `retained:${surfaceTrimRetainedTriangleIndices.join(',')}`,
    surfaceTrimCapPreview
      ? `cap:${surfaceTrimCapPreview.vertices.map(vertex => vertex.join(',')).join(';')}:${surfaceTrimCapPreview.triangles.map(triangle => triangle.join(',')).join(';')}`
      : 'no-cap',
  ].join('|'));
  const showPartOverlay = $derived.by(
    () => Boolean(selectedTarget && shouldDisplayViewportControlList(selectedTarget)),
  );

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let pointerDownAt: { x: number; y: number } | null = null;
  let isOrbitDragging = false;
  let lastCaptureHoverAt = 0;

  function currentCameraState(): ViewportCameraState | null {
    if (!camera || !controls) return null;
    return {
      position: [camera.position.x, camera.position.y, camera.position.z],
      target: [controls.target.x, controls.target.y, controls.target.z],
      zoom: Number.isFinite(camera.zoom) ? camera.zoom : null,
      fov: Number.isFinite(camera.fov) ? camera.fov : null,
    };
  }

  function applyCameraState(nextState: ViewportCameraState | null | undefined) {
    if (!camera || !controls || !nextState) return;
    camera.position.set(...nextState.position);
    camera.zoom = typeof nextState.zoom === 'number' ? nextState.zoom : 1;
    camera.fov = typeof nextState.fov === 'number' ? nextState.fov : 45;
    camera.updateProjectionMatrix();
    controls.target.set(...nextState.target);
    controls.update();
    updateCameraClipPlanes();
    updateOverlayAnchor();
  }

  function updateCameraClipPlanes() {
    if (!camera || (!modelRoot && !componentPlacementPreview)) return;
    const bounds = new THREE.Box3();
    if (modelRoot) bounds.expandByObject(modelRoot);
    if (componentPlacementPreview) {
      componentPlacementPreview.frame.updateMatrixWorld(true);
      bounds.expandByObject(componentPlacementPreview.mesh);
    }
    if (parameterGhostMesh?.visible) bounds.expandByObject(parameterGhostMesh);
    if (bounds.isEmpty()) return;
    const clipPlanes = resolveViewerClipPlanes(bounds, camera.position);
    if (!Number.isFinite(clipPlanes.near) || !Number.isFinite(clipPlanes.far)) return;
    camera.near = clipPlanes.near;
    camera.far = clipPlanes.far;
    camera.updateProjectionMatrix();
  }

  function emitCameraStateChange() {
    const nextCamera = currentCameraState();
    if (nextCamera) {
      onCameraStateChange?.(nextCamera);
    }
  }

  async function notifyModelLoaded(token: number) {
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (token !== loadToken) return;
    onModelLoaded?.();
  }

  function notifyModelLoadError(token: number, context: string, error: unknown) {
    if (token !== loadToken) return;
    const message = error instanceof Error ? error.message : String(error);
    onModelLoadError?.(`${context}: ${message}`);
  }

  function loadStlGeometry(loader: STLLoader, url: string): Promise<THREE.BufferGeometry> {
    const resolvedUrl = resolveViewerAssetUrl(url, modelKey);
    const timeoutMs = 30000;
    let timer: ReturnType<typeof setTimeout> | undefined;
    return Promise.race([
      loader.loadAsync(resolvedUrl),
      new Promise<THREE.BufferGeometry>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error(`STL load timed out after ${timeoutMs}ms: ${resolvedUrl}`)),
          timeoutMs,
        );
      }),
    ]).finally(() => {
      if (timer) clearTimeout(timer);
    });
  }

  function loadObjObject(loader: OBJLoader, url: string): Promise<THREE.Object3D> {
    return loader.loadAsync(resolveViewerAssetUrl(url, modelKey));
  }

  function loadThreeMfObject(loader: ThreeMFLoader, url: string): Promise<THREE.Object3D> {
    return loader.loadAsync(resolveViewerAssetUrl(url, modelKey));
  }

  export function getCameraState(): ViewportCameraState | null {
    return currentCameraState();
  }

  export function setCameraState(nextState: ViewportCameraState | null = null) {
    applyCameraState(nextState);
  }

  function renderModelForCapture() {
    if (!renderer || !scene || !camera) return;
    const preview = componentPlacementPreview;
    const helper = preview?.transform.getHelper();
    const gridPosition = preview ? viewerGrid?.position.clone() : null;
    const ghost = parameterGhostMesh;
    const ghostWasVisible = ghost?.visible ?? false;
    if (preview && helper) {
      preview.frame.visible = false;
      helper.visible = false;
      viewerGrid?.position.set(0, 0, 0);
    }
    if (ghost) ghost.visible = false;
    try {
      renderer.render(scene, camera);
    } finally {
      if (preview) preview.frame.visible = true;
      if (helper) helper.visible = true;
      if (ghost) ghost.visible = ghostWasVisible;
      if (gridPosition) viewerGrid?.position.copy(gridPosition);
    }
  }

  export function captureScreenshotDetails(
    overlayCanvas: HTMLCanvasElement | null = null,
  ): { dataUrl: string; width: number; height: number; camera: ViewportCameraState } | null {
    if (!renderer || !scene || !camera) return null;
    renderModelForCapture();
    const source = renderer.domElement;
    const effectiveCamera = currentCameraState();
    if (!effectiveCamera) return null;
    let dataUrl = null;
    if (overlayCanvas) {
      const offscreen = document.createElement('canvas');
      offscreen.width = source.width;
      offscreen.height = source.height;
      const ctx = offscreen.getContext('2d');
      if (!ctx) return null;
      ctx.drawImage(source, 0, 0);
      ctx.drawImage(
        overlayCanvas,
        0,
        0,
        overlayCanvas.width,
        overlayCanvas.height,
        0,
        0,
        offscreen.width,
        offscreen.height,
      );
      dataUrl = offscreen.toDataURL('image/jpeg', 0.8);
    } else {
      dataUrl = source.toDataURL('image/jpeg', 0.8);
    }
    profileLog('viewer.capture_screenshot', {
      sourceW: source.width,
      sourceH: source.height,
      outputMb: Number((estimateBase64Bytes(dataUrl) / (1024 * 1024)).toFixed(2)),
      withOverlay: !!overlayCanvas,
    });
    return {
      dataUrl,
      width: source.width,
      height: source.height,
      camera: effectiveCamera,
    };
  }

  export function captureScreenshot(overlayCanvas: HTMLCanvasElement | null = null): string | null {
    return captureScreenshotDetails(overlayCanvas)?.dataUrl ?? null;
  }

  /**
   * Capture the current model from N standard angles for vision verification.
   * Saves and restores the camera state so the user sees no change.
   *
   * Angles (normalized direction vectors from model center):
   *   0 – isometric front-right  (1, -1,  0.7)
   *   1 – isometric back-left   (-1,  1,  0.7)
   *   2 – front                  (0, -1,  0.2)
   *   3 – top-down               (0,  0,   1 )
   */
  export function captureMultiAngleScreenshots(): string[] {
    if (!renderer || !scene || !camera || !controls) return [];
    const savedState = currentCameraState();
    if (!savedState) return [];

    const cx = controls.target.x;
    const cy = controls.target.y;
    const cz = controls.target.z;
    const dist = camera.position.distanceTo(controls.target);

    // [dx, dy, dz] — direction from center to camera, will be normalised
    const directions: [number, number, number][] = [
      [ 1, -1,  0.7],
      [-1,  1,  0.7],
      [ 0, -1,  0.2],
      [ 0,  0,  1.0],
    ];

    const results: string[] = [];
    for (const [dx, dy, dz] of directions) {
      const len = Math.sqrt(dx * dx + dy * dy + dz * dz);
      camera.position.set(
        cx + (dx / len) * dist,
        cy + (dy / len) * dist,
        cz + (dz / len) * dist,
      );
      controls.update();
      renderModelForCapture();
      results.push(renderer.domElement.toDataURL('image/jpeg', 0.75));
    }

    // Restore original view
    applyCameraState(savedState);
    renderer.render(scene, camera);
    return results;
  }

  function asNumber(value: ParamValue | undefined, fallback = 0): number {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : fallback;
  }

  function parseOptionalNumber(value: number | undefined): number | undefined {
    return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
  }

  function getRangeProps(field: Extract<UiField, { type: 'range' | 'number' }>, value: ParamValue) {
    const rawValue = Number(value);
    const currentValue = Number.isFinite(rawValue) ? rawValue : 0;
    let min = parseOptionalNumber(field.min) ?? 0;
    let max = parseOptionalNumber(field.max) ?? Math.max(200, currentValue * 4);
    if (max < min) max = min;
    if (max === min) max = min + 1;
    const stepCandidate = parseOptionalNumber(field.step) ?? (max - min > 50 ? 1 : 0.1);
    const step = Number.isFinite(stepCandidate) && stepCandidate > 0 ? stepCandidate : 1;
    return { min, max, step };
  }

  function getSelectValue(value: ParamValue): string | number | null {
    return typeof value === 'string' || typeof value === 'number' ? value : null;
  }

  function firstSelectedPath(selected: string | string[] | null): string | null {
    if (Array.isArray(selected)) {
      return typeof selected[0] === 'string' ? selected[0] : null;
    }
    return typeof selected === 'string' ? selected : null;
  }

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
  }

  function setFocusedControl(primitiveId: string | null, parameterKey: string | null) {
    onControlFocusChange?.({ primitiveId, parameterKey });
  }

  function clearFocusedControl(event: MouseEvent | FocusEvent) {
    const current = event.currentTarget as HTMLElement | null;
    const related = (event as FocusEvent).relatedTarget as Node | null;
    if (current && related && current.contains(related)) return;
    onControlFocusChange?.(null);
  }

  function updateOverlayParam(primitiveId: string, value: ParamValue) {
    if (hideModelWhileBusy) return;
    onOverlayChange?.(primitiveId, value);
  }

  async function pickOverlayImage(primitiveId: string) {
    try {
      const selected = firstSelectedPath(
        await open({
          multiple: false,
          filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }],
        }),
      );
      if (selected) {
        updateOverlayParam(primitiveId, selected);
      }
    } catch (error) {
      console.error('Failed to pick overlay image:', error);
    }
  }

  function overlayFieldTone(field: UiField | null) {
    if (!field) return 'x';
    const signature = `${field.key} ${field.label}`.toLowerCase();
    if (signature.includes('height') || signature.includes('vertical') || signature.includes('z')) {
      return 'z';
    }
    if (
      signature.includes('depth') ||
      signature.includes('length') ||
      signature.includes('offset') ||
      signature.includes('front') ||
      signature.includes('back') ||
      signature.includes('y')
    ) {
      return 'y';
    }
    if (
      signature.includes('angle') ||
      signature.includes('tilt') ||
      signature.includes('rotate') ||
      signature.includes('yaw') ||
      signature.includes('pitch')
    ) {
      return 'angle';
    }
    return 'x';
  }

  function runtimeLocalBounds(): THREE.Box3 | null {
    const bounds = new THREE.Box3();
    let found = false;
    for (const runtime of runtimeMeshes) {
      if (!runtime.baseBounds || runtime.baseBounds.isEmpty()) continue;
      bounds.union(runtime.baseBounds);
      found = true;
    }
    return found ? bounds : null;
  }

  function normalizedCropBounds() {
    const fullBounds = runtimeLocalBounds();
    if (!fullBounds) return null;
    return cropBounds ?? {
      min: fullBounds.min.toArray() as [number, number, number],
      max: fullBounds.max.toArray() as [number, number, number],
    };
  }

  function emitCropBoxBounds() {
    if (!cropBoxMesh) return;
    const halfSize = cropBoxMesh.scale.clone();
    halfSize.set(Math.abs(halfSize.x), Math.abs(halfSize.y), Math.abs(halfSize.z)).multiplyScalar(0.5);
    const minimum = cropBoxMesh.position.clone().sub(halfSize);
    const maximum = cropBoxMesh.position.clone().add(halfSize);
    onCropBoundsChange?.({
      min: minimum.toArray() as [number, number, number],
      max: maximum.toArray() as [number, number, number],
    });
  }

  function syncOrbitEnabled() {
    if (!controls) return;
    controls.enabled = !selectionMode && !hideModelWhileBusy
      && !cropTransformControls?.dragging && !componentPlacementPreview?.transform.dragging;
  }

  function handleCropMouseUp() {
    if (!cropBoxMesh) return;
    const fullBounds = runtimeLocalBounds();
    const minimumSize = fullBounds
      ? Math.max(fullBounds.getSize(new THREE.Vector3()).length() * 0.002, 1.0e-6)
      : 1.0e-6;
    cropBoxMesh.scale.set(
      Math.max(Math.abs(cropBoxMesh.scale.x), minimumSize),
      Math.max(Math.abs(cropBoxMesh.scale.y), minimumSize),
      Math.max(Math.abs(cropBoxMesh.scale.z), minimumSize),
    );
    emitCropBoxBounds();
  }

  function disposeCropBox() {
    if (cropTransformControls) {
      cropTransformControls.removeEventListener('dragging-changed', syncOrbitEnabled);
      cropTransformControls.removeEventListener('mouseUp', handleCropMouseUp);
      cropTransformControls.detach();
      cropTransformControls.dispose();
    }
    if (cropTransformHelper) scene?.remove(cropTransformHelper);
    if (cropBoxMesh) {
      cropBoxMesh.parent?.remove(cropBoxMesh);
      cropBoxMesh.geometry.dispose();
      cropBoxMesh.material.dispose();
    }
    cropTransformControls = null;
    cropTransformHelper = null;
    cropBoxMesh = null;
    appliedCropBoundsSignature = '';
    syncOrbitEnabled();
  }

  function syncCropBox() {
    if (!cropBoxEnabled || !scene || !camera || !renderer || !modelRoot) {
      disposeCropBox();
      return;
    }
    const bounds = normalizedCropBounds();
    if (!bounds) return;
    if (!cropBoxMesh) {
      cropBoxMesh = new THREE.Mesh(
        new THREE.BoxGeometry(1, 1, 1),
        new THREE.MeshBasicMaterial({
          color: 0xc89a58,
          wireframe: true,
          transparent: true,
          opacity: 0.9,
          depthTest: false,
        }),
      );
      cropBoxMesh.renderOrder = 20;
      cropBoxMesh.userData.ignoreRaycast = true;
      modelRoot.add(cropBoxMesh);

      cropTransformControls = new TransformControls(camera, renderer.domElement);
      cropTransformControls.setSpace('local');
      cropTransformControls.size = 0.8;
      cropTransformControls.addEventListener('dragging-changed', syncOrbitEnabled);
      cropTransformControls.addEventListener('mouseUp', handleCropMouseUp);
      cropTransformHelper = cropTransformControls.getHelper();
      scene.add(cropTransformHelper);
      cropTransformControls.attach(cropBoxMesh);
    }
    cropTransformControls?.setMode(cropBoxMode);
    const signature = JSON.stringify(bounds);
    if (signature !== appliedCropBoundsSignature) {
      const minimum = new THREE.Vector3(...bounds.min);
      const maximum = new THREE.Vector3(...bounds.max);
      cropBoxMesh.position.copy(minimum).add(maximum).multiplyScalar(0.5);
      cropBoxMesh.scale.copy(maximum).sub(minimum);
      appliedCropBoundsSignature = signature;
    }
    if (!cropBounds) emitCropBoxBounds();
  }

  function syncComponentPlacementFrame() {
    const preview = componentPlacementPreview;
    if (!preview) return;
    // The viewer recenters rendered meshes. Match that offset while keeping the pose in CAD coordinates.
    if (modelRoot) {
      preview.frame.position.copy(modelRoot.position);
      preview.frame.quaternion.copy(modelRoot.quaternion);
    } else {
      preview.frame.position.set(0, 0, 0);
      preview.frame.rotation.set(-Math.PI / 2, 0, 0);
    }
    viewerGrid?.position.copy(preview.frame.position);
    preview.frame.updateMatrixWorld(true);
  }

  function emitComponentPlacementMove(position: THREE.Vector3) {
    if (componentPlacement?.disabled || hideModelWhileBusy) return;
    const [x, y, z] = position.toArray();
    const mm: PositionMm = [Number(x.toFixed(2)), Number(y.toFixed(2)), Number(z.toFixed(2))];
    if (!mm.every(Number.isFinite)) return;
    componentPlacementPositionChosen = true;
    onComponentPlacementMove?.(mm);
  }

  function handleComponentPlacementObjectChange() {
    if (componentPlacementPreview) emitComponentPlacementMove(componentPlacementPreview.mesh.position);
  }

  function syncComponentPlacementControls() {
    const preview = componentPlacementPreview;
    if (!preview) return;
    preview.transform.enabled = preview.transform.dragging || (!componentPlacement?.disabled && !hideModelWhileBusy);
    const position = componentPlacement?.positionMm;
    if (!position || !position.every(Number.isFinite) || preview.transform.dragging) return;
    if (position.some((value, index) => value !== preview.mesh.position.getComponent(index))) {
      componentPlacementPositionChosen = true;
      preview.mesh.position.set(...position);
      updateCameraClipPlanes();
    }
  }

  function handleComponentPlacementDraggingChanged() {
    syncOrbitEnabled();
    syncComponentPlacementControls();
    if (!componentPlacementPreview?.transform.dragging) updateCameraClipPlanes();
  }

  function disposeComponentPlacement() {
    const preview = componentPlacementPreview;
    if (!preview) return;
    preview.transform.removeEventListener('dragging-changed', handleComponentPlacementDraggingChanged);
    preview.transform.removeEventListener('objectChange', handleComponentPlacementObjectChange);
    preview.transform.getHelper().removeFromParent();
    preview.transform.detach();
    const canvas = renderer?.domElement;
    const touchAction = canvas?.style.touchAction;
    preview.transform.dispose();
    if (canvas && touchAction !== undefined) canvas.style.touchAction = touchAction;
    preview.frame.removeFromParent();
    preview.mesh.geometry.dispose();
    preview.mesh.material.dispose();
    componentPlacementPreview = null;
    componentPlacementReady = false;
    componentPlacementPositionChosen = false;
    viewerGrid?.position.set(0, 0, 0);
    syncOrbitEnabled();
    updateCameraClipPlanes();
  }

  async function loadComponentPlacement(stlUrl: string, cancelled: () => boolean) {
    try {
      const geometry = await loadStlGeometry(new STLLoader(), stlUrl);
      if (cancelled() || !scene || !camera || !renderer || componentPlacementUrl !== stlUrl) {
        geometry.dispose();
        return;
      }
      geometry.computeBoundingBox();
      if (!geometry.boundingBox || geometry.boundingBox.isEmpty()) {
        geometry.dispose();
        throw new Error('The component STL has no geometry.');
      }
      const frame = new THREE.Group();
      const mesh = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
        color: 0xdca85c,
        transparent: true,
        opacity: 0.46,
        depthTest: false,
        depthWrite: false,
      }));
      mesh.renderOrder = 3;
      mesh.position.set(...(componentPlacement?.positionMm ?? [0, 0, 0]));
      frame.add(mesh);
      const transform = new TransformControls(camera, renderer.domElement);
      transform.setMode('translate');
      transform.setSpace('local');
      transform.size = 0.8;
      transform.attach(mesh);
      transform.addEventListener('dragging-changed', handleComponentPlacementDraggingChanged);
      transform.addEventListener('objectChange', handleComponentPlacementObjectChange);
      componentPlacementPreview = { frame, mesh, transform };
      scene.add(frame, transform.getHelper());
      syncComponentPlacementFrame();
      syncComponentPlacementControls();
      if (!modelRoot) frameModel(mesh);
      else updateCameraClipPlanes();
      componentPlacementReady = true;
      onComponentPlacementLoad?.(stlUrl);
    } catch (error) {
      if (cancelled() || componentPlacementUrl !== stlUrl) return;
      disposeComponentPlacement();
      onComponentPlacementError?.(stlUrl, error instanceof Error ? error.message : String(error));
    }
  }

  function alignParameterGhost() {
    if (!parameterGhostMesh) return;
    parameterGhostMesh.visible = Boolean(modelRoot);
    if (modelRoot) {
      parameterGhostMesh.position.copy(modelRoot.position);
      parameterGhostMesh.quaternion.copy(modelRoot.quaternion);
    }
  }

  function disposeParameterGhost() {
    parameterGhostMesh?.removeFromParent();
    parameterGhostMesh?.geometry.dispose();
    parameterGhostMesh?.material.dispose();
    parameterGhostMesh = null;
  }

  function clearParameterGhostMesh() {
    disposeParameterGhost();
    parameterGhostReady = false;
    updateCameraClipPlanes();
  }

  async function loadParameterGhost(stlUrl: string, cancelled: () => boolean) {
    try {
      const geometry = await loadStlGeometry(new STLLoader(), stlUrl);
      if (cancelled() || !scene || parameterGhostStlUrl !== stlUrl) {
        geometry.dispose();
        return;
      }
      geometry.computeBoundingBox();
      if (!geometry.boundingBox || geometry.boundingBox.isEmpty()) {
        geometry.dispose();
        throw new Error('The parameter preview STL has no geometry.');
      }
      const mesh = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
        color: 0xdca85c,
        transparent: true,
        opacity: 0.42,
        depthTest: false,
        depthWrite: false,
      }));
      mesh.renderOrder = 3;
      disposeParameterGhost();
      parameterGhostMesh = mesh;
      alignParameterGhost();
      scene.add(mesh);
      updateCameraClipPlanes();
      parameterGhostReady = true;
      onParameterGhostLoad?.(stlUrl);
    } catch (error) {
      if (!cancelled() && parameterGhostStlUrl === stlUrl) {
        onParameterGhostLoad?.(stlUrl, error instanceof Error ? error.message : String(error));
      }
    }
  }

  onMount(() => {
    setupViewer();

    resizeObserver = new ResizeObserver(() => {
      onResize();
    });
    resizeObserver.observe(viewerHost);
    requestAnimationFrame(() => {
      onResize();
      void loadCurrentModel();
    });
  });

  onDestroy(() => {
    if (animationFrameId) cancelAnimationFrame(animationFrameId);
    if (viewerHost) {
      viewerHost.removeEventListener('pointerdown', handlePointerDown);
      viewerHost.removeEventListener('pointermove', handlePointerMove);
      viewerHost.removeEventListener('pointerleave', handlePointerLeave);
      viewerHost.removeEventListener('pointerup', handlePointerUp);
      viewerHost.removeEventListener('pointercancel', handlePointerCancel);
    }
    controls?.removeEventListener?.('start', handleOrbitStart);
    controls?.removeEventListener?.('end', handleOrbitEnd);
    controls?.removeEventListener?.('change', handleControlsChange);
    disposeComponentPlacement();
    disposeParameterGhost();
    viewerGrid?.dispose();
    viewerGrid = null;
    if (!retainModelWhileLoading || !modelRoot) disposeModel();
    controls?.dispose?.();
    if (renderer) {
      (renderer as THREE.WebGLRenderer & { renderLists?: { dispose?: () => void } }).renderLists?.dispose?.();
      renderer.dispose();
      renderer.forceContextLoss?.();
      const canvas = renderer.domElement;
      if (canvas.parentNode) {
        canvas.parentNode.removeChild(canvas);
      }
    }
    renderer = null;
    controls = null;
    camera = null;
    scene = null;
    resizeObserver?.disconnect();
  });

  $effect(() => {
    const reloadSignature = modelLoadSignature;
    if (!scene) return;
    void reloadSignature;
    void untrack(() => loadCurrentModel());
  });

  $effect(() => {
    if (retainModelWhileLoading || stlUrl || viewerAssets.length > 0 || !scene) return;
    disposeModel();
  });

  $effect(() => {
    void cropBoxEnabled;
    void cropBoxMode;
    void cropBounds;
    syncCropBox();
  });

  $effect(() => {
    const stlUrl = componentPlacementUrl;
    if (!stlUrl || !scene) return;
    let cancelled = false;
    void untrack(() => loadComponentPlacement(stlUrl, () => cancelled));
    return () => {
      cancelled = true;
      disposeComponentPlacement();
    };
  });

  $effect(() => {
    if (componentPlacementReady) syncComponentPlacementControls();
  });

  $effect(() => {
    const stlUrl = parameterGhostStlUrl;
    if (!stlUrl) {
      untrack(clearParameterGhostMesh);
      return;
    }
    if (!scene) return;
    let cancelled = false;
    void untrack(() => loadParameterGhost(stlUrl, () => cancelled));
    return () => { cancelled = true; };
  });

  $effect(() => {
    const root = modelRoot;
    const targetSignature = `${edgeTargetSignature}::${faceTargetSignature}`;
    void targetSignature;
    if (!root) return;
    untrack(() => {
      attachEdgeTargets(root);
      attachFaceTargets(root);
      applySelectionStyles();
      updateOverlayAnchor();
    });
  });

  $effect(() => {
    applySelectionStyles();
    updateOverlayAnchor();
  });

  $effect(() => {
    applyPreviewTransforms();
    updateOverlayAnchor();
    updateCaptureGuideOverlay();
  });

  $effect(() => {
    const signature = captureGuideSignature;
    const planeSignature = capturePlaneSignature;
    const trimSignature = surfaceTrimSignature;
    void signature;
    void planeSignature;
    void trimSignature;
    untrack(applyCaptureComparisonState);
    untrack(updateCaptureGuideOverlay);
    untrack(syncSurfaceTrimRegionOverlay);
  });

  $effect(() => {
    void captureReferenceVisible;
    void captureReferenceOpacity;
    void captureGeneratedVisible;
    void captureGeneratedOpacity;
    untrack(applyCaptureComparisonState);
  });

  $effect(() => {
    void captureDeviation?.contentDigest;
    void captureDeviationVisible;
    untrack(syncCaptureDeviationOverlay);
  });

  $effect(() => {
    void femResult?.resultDigest;
    void femMeshPreview?.meshContentDigest;
    void femDisplay?.field;
    void femDisplay?.deformationScale;
    void femDisplay?.showMesh;
    void femDisplay?.showOutline;
    void femDisplay?.clipFraction;
    if (!modelRoot) return;
    void untrack(syncFemOverlay);
  });

  $effect(() => {
    void outlineEnabled;
    void topologyMode;
    void viewerMode;
    void surfaceTrimActive;
    if (controls) {
      syncOrbitEnabled();
      controls.mouseButtons.LEFT = surfaceTrimActive
        ? (-1 as THREE.MOUSE)
        : THREE.MOUSE.ROTATE;
      controls.mouseButtons.RIGHT = surfaceTrimActive
        ? THREE.MOUSE.ROTATE
        : THREE.MOUSE.PAN;
    }
    applySelectionStyles();
  });

  $effect(() => {
    if (!hideModelWhileBusy) return;
    if (hoveredPartId !== null) {
      hoveredPartId = null;
      applySelectionStyles();
    }
    if (renderer) {
      renderer.domElement.style.cursor = 'progress';
    }
  });

  function disposeFemOverlay() {
    femLoadToken += 1;
    if (femOverlay) {
      femOverlay.parent?.remove(femOverlay);
      disposeDetachedGroup(femOverlay);
    }
    femOverlay = null;
    femOverlayKind = null;
    femLegend = null;
    femOverlayError = '';
  }

  async function readFemArray(path: string, scalarType: string): Promise<Float64Array | Uint32Array> {
    const response = await fetch(convertFileSrc(path));
    if (!response.ok) throw new Error(`FEM array '${path}' returned HTTP ${response.status}.`);
    const buffer = await response.arrayBuffer();
    const view = new DataView(buffer);
    if (scalarType === 'float64Le') {
      if (buffer.byteLength % 8 !== 0) throw new Error(`FEM Float64 array '${path}' is truncated.`);
      const values = new Float64Array(buffer.byteLength / 8);
      for (let index = 0; index < values.length; index += 1) values[index] = view.getFloat64(index * 8, true);
      return values;
    }
    if (scalarType === 'uint32Le') {
      if (buffer.byteLength % 4 !== 0) throw new Error(`FEM Uint32 array '${path}' is truncated.`);
      const values = new Uint32Array(buffer.byteLength / 4);
      for (let index = 0; index < values.length; index += 1) values[index] = view.getUint32(index * 4, true);
      return values;
    }
    throw new Error(`Unsupported FEM scalar type '${scalarType}'.`);
  }

  async function syncFemOverlay() {
    disposeFemOverlay();
    if ((!femResult && !femMeshPreview) || !femDisplay || !modelRoot) return;
    if (!femResult && femMeshPreview) {
      await syncFemMeshPreviewOverlay(femMeshPreview);
      return;
    }
    if (!femResult) return;
    const token = femLoadToken;
    const byName = new Map(femResult.arrays.map((array) => [array.name, array]));
    const requiredNames = ['nodesMm', 'boundaryTriangles', 'displacementMm', 'nodalDisplayVonMisesMpa'];
    for (const name of requiredNames) {
      if (!byName.has(name)) {
        femOverlayError = `FEM result array '${name}' is missing.`;
        return;
      }
    }
    try {
      const [nodes, triangles, displacement, nodalStress] = await Promise.all(requiredNames.map((name) => {
        const array = byName.get(name)!;
        return readFemArray(array.path, array.scalarType);
      }));
      if (token !== femLoadToken || !modelRoot) return;
      if (!(nodes instanceof Float64Array)
        || !(triangles instanceof Uint32Array)
        || !(displacement instanceof Float64Array)
        || !(nodalStress instanceof Float64Array)) {
        throw new Error('FEM result array scalar types do not match manifest roles.');
      }
      if (nodes.length % 3 !== 0 || displacement.length !== nodes.length || nodalStress.length !== nodes.length / 3 || triangles.length % 3 !== 0) {
        throw new Error('FEM result array shapes disagree.');
      }
      const nodeCount = nodes.length / 3;
      const positions = new Float32Array(nodes.length);
      const fieldValues = new Float64Array(nodeCount);
      let minimumX = Number.POSITIVE_INFINITY;
      let maximumX = Number.NEGATIVE_INFINITY;
      for (let node = 0; node < nodeCount; node += 1) {
        const offset = node * 3;
        const dx = displacement[offset];
        const dy = displacement[offset + 1];
        const dz = displacement[offset + 2];
        positions[offset] = nodes[offset] + dx * femDisplay.deformationScale;
        positions[offset + 1] = nodes[offset + 1] + dy * femDisplay.deformationScale;
        positions[offset + 2] = nodes[offset + 2] + dz * femDisplay.deformationScale;
        fieldValues[node] = femDisplay.field === 'displacement' ? Math.hypot(dx, dy, dz) : nodalStress[node];
        minimumX = Math.min(minimumX, nodes[offset]);
        maximumX = Math.max(maximumX, nodes[offset]);
      }
      const fieldMinimum = fieldValues.reduce((value, next) => Math.min(value, next), Number.POSITIVE_INFINITY);
      const fieldMaximum = fieldValues.reduce((value, next) => Math.max(value, next), Number.NEGATIVE_INFINITY);
      const colors = new Float32Array(nodeCount * 3);
      for (let node = 0; node < nodeCount; node += 1) {
        colors.set(femColorRamp(normalizeFemField(fieldValues[node], fieldMinimum, fieldMaximum)), node * 3);
      }
      const clipX = minimumX + (maximumX - minimumX) * Math.max(0, Math.min(1, femDisplay.clipFraction));
      const visibleTriangles: number[] = [];
      for (let index = 0; index < triangles.length; index += 3) {
        const a = triangles[index];
        const b = triangles[index + 1];
        const c = triangles[index + 2];
        if (a >= nodeCount || b >= nodeCount || c >= nodeCount) throw new Error('FEM boundary triangle references an out-of-range node.');
        const centroidX = (nodes[a * 3] + nodes[b * 3] + nodes[c * 3]) / 3;
        if (centroidX <= clipX) visibleTriangles.push(a, b, c);
      }
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
      geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
      geometry.setIndex(visibleTriangles);
      geometry.computeVertexNormals();
      const overlay = new THREE.Group();
      overlay.userData.previewOnlyFemOverlay = true;
      const surface = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
        vertexColors: true,
        metalness: 0,
        roughness: 0.72,
        polygonOffset: true,
        polygonOffsetFactor: -1,
        polygonOffsetUnits: -1,
      }));
      surface.userData.ignoreRaycast = true;
      overlay.add(surface);
      if (femDisplay.showMesh) {
        const wireframe = new THREE.LineSegments(
          new THREE.WireframeGeometry(geometry),
          new THREE.LineBasicMaterial({ color: 0x101722, transparent: true, opacity: 0.5 }),
        );
        wireframe.userData.ignoreRaycast = true;
        overlay.add(wireframe);
      }
      if (femDisplay.showOutline) {
        const undeformed = new THREE.BufferGeometry();
        undeformed.setAttribute('position', new THREE.BufferAttribute(Float32Array.from(nodes), 3));
        undeformed.setIndex(visibleTriangles);
        const outline = new THREE.LineSegments(
          new THREE.EdgesGeometry(undeformed, 28),
          new THREE.LineBasicMaterial({ color: 0xc89a58, transparent: true, opacity: 0.7 }),
        );
        undeformed.dispose();
        outline.userData.ignoreRaycast = true;
        overlay.add(outline);
      }
      femOverlay = overlay;
      femOverlayKind = 'result';
      modelRoot.add(overlay);
      femLegend = {
        label: femDisplay.field === 'displacement' ? 'DISPLACEMENT' : 'VON MISES',
        minimum: fieldMinimum,
        maximum: fieldMaximum,
        unit: femDisplay.field === 'displacement' ? 'mm' : 'MPa',
      };
    } catch (error) {
      femOverlayError = error instanceof Error ? error.message : String(error);
    }
  }

  async function syncFemMeshPreviewOverlay(preview: FemMeshPreviewResponse) {
    const token = femLoadToken;
    const byName = new Map(preview.arrays.map((array) => [array.name, array]));
    const nodesAsset = byName.get('nodesMm');
    const trianglesAsset = byName.get('boundaryTriangles');
    if (!nodesAsset || !trianglesAsset) {
      femOverlayError = 'FEM mesh preview requires nodesMm and boundaryTriangles arrays.';
      return;
    }
    try {
      const [nodes, triangles] = await Promise.all([
        readFemArray(nodesAsset.path, nodesAsset.scalarType),
        readFemArray(trianglesAsset.path, trianglesAsset.scalarType),
      ]);
      if (token !== femLoadToken || !modelRoot) return;
      if (!(nodes instanceof Float64Array) || !(triangles instanceof Uint32Array)) {
        throw new Error('FEM mesh preview scalar types do not match manifest roles.');
      }
      if (nodes.length % 3 !== 0 || triangles.length % 3 !== 0) {
        throw new Error('FEM mesh preview array shapes disagree.');
      }
      const nodeCount = nodes.length / 3;
      for (const node of triangles) {
        if (node >= nodeCount) throw new Error('FEM mesh boundary references an out-of-range node.');
      }
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(Float32Array.from(nodes), 3));
      geometry.setIndex(Array.from(triangles));
      geometry.computeVertexNormals();
      const overlay = new THREE.Group();
      overlay.userData.previewOnlyFemMesh = true;
      const surface = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
        color: 0xc89a58,
        transparent: true,
        opacity: 0.28,
        metalness: 0,
        roughness: 0.78,
        polygonOffset: true,
        polygonOffsetFactor: -1,
        polygonOffsetUnits: -1,
      }));
      surface.userData.ignoreRaycast = true;
      overlay.add(surface);
      const wireframe = new THREE.LineSegments(
        new THREE.WireframeGeometry(geometry),
        new THREE.LineBasicMaterial({ color: 0xe1ba7d, transparent: true, opacity: 0.8 }),
      );
      wireframe.userData.ignoreRaycast = true;
      overlay.add(wireframe);
      femOverlay = overlay;
      femOverlayKind = 'mesh';
      modelRoot.add(overlay);
    } catch (error) {
      femOverlayError = error instanceof Error ? error.message : String(error);
    }
  }

  function setupViewer() {
    if (renderer) return;
    scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0b0f1a);

    const { width, height } = hostSize();
    camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 10);
    camera.position.set(140, 120, 140);

    renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.08;
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height);
    viewerHost.appendChild(renderer.domElement);
    viewerHost.addEventListener('pointerdown', handlePointerDown);
    viewerHost.addEventListener('pointermove', handlePointerMove);
    viewerHost.addEventListener('pointerleave', handlePointerLeave);
    viewerHost.addEventListener('pointerup', handlePointerUp);
    viewerHost.addEventListener('pointercancel', handlePointerCancel);

    controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.autoRotate = false;
    controls.autoRotateSpeed = 0;
    syncOrbitEnabled();
    controls.mouseButtons.LEFT = surfaceTrimActive
      ? (-1 as THREE.MOUSE)
      : THREE.MOUSE.ROTATE;
    controls.mouseButtons.RIGHT = surfaceTrimActive
      ? THREE.MOUSE.ROTATE
      : THREE.MOUSE.PAN;
    controls.addEventListener('start', handleOrbitStart);
    controls.addEventListener('end', handleOrbitEnd);
    controls.addEventListener('change', handleControlsChange);

    const hemi = new THREE.HemisphereLight(0xbfd4ff, 0x182032, 0.78);
    scene.add(hemi);

    const key = new THREE.DirectionalLight(0xfff2dc, 1.55);
    key.position.set(140, 190, 155);
    scene.add(key);

    const fill = new THREE.DirectionalLight(0x9ec8ff, 0.72);
    fill.position.set(-120, 120, -90);
    scene.add(fill);

    const rim = new THREE.DirectionalLight(0xf6d39d, 0.38);
    rim.position.set(-40, 160, 180);
    scene.add(rim);

    const grid = new THREE.GridHelper(250, 24, 0x24314f, 0x18203a);
    grid.position.y = 0;
    const gridMaterial = grid.material as THREE.Material | THREE.Material[];
    for (const material of Array.isArray(gridMaterial) ? gridMaterial : [gridMaterial]) {
      if ('transparent' in material) material.transparent = true;
      if ('opacity' in material) material.opacity = 0.22;
    }
    scene.add(grid);
    viewerGrid = grid;

    animate();
  }

  function animate() {
    if (!controls || !renderer || !scene || !camera) return;
    animationFrameId = requestAnimationFrame(animate);
    controls.update();
    if (showContextOverlay && selectionMode && !isOrbitDragging) {
      updateOverlayAnchor();
    }
    renderer.render(scene, camera);
  }

  function hostSize() {
    return {
      width: Math.max(1, viewerHost?.clientWidth ?? 1),
      height: Math.max(1, viewerHost?.clientHeight ?? 1),
    };
  }

  function projectCaptureSourcePosition(
    position: [number, number, number],
  ): { x: number; y: number } | null {
    if (!camera || !viewerHost) return null;
    const sourceMesh = runtimeMeshes.find(entry => entry.sourcePickMesh)?.sourcePickMesh;
    if (!sourceMesh) return null;
    sourceMesh.updateMatrixWorld(true);
    const projected = sourceMesh
      .localToWorld(new THREE.Vector3(...position))
      .project(camera);
    if (!Number.isFinite(projected.x) || !Number.isFinite(projected.y) || projected.z < -1 || projected.z > 1) {
      return null;
    }
    const { width, height } = hostSize();
    return {
      x: (projected.x * 0.5 + 0.5) * width,
      y: (-projected.y * 0.5 + 0.5) * height,
    };
  }

  function updateSurfaceTrimProjection() {
    if (!surfaceTrimActive) {
      surfaceTrimProjected = { points: [], keepSeed: null, segments: [] };
      return;
    }
    const points = surfaceTrimAnchors.flatMap(anchor => {
      const projected = projectCaptureSourcePosition(anchor.sourcePosition);
      return projected ? [projected] : [];
    });
    const keepSeed = surfaceTrimKeepSeed
      ? projectCaptureSourcePosition(surfaceTrimKeepSeed.sourcePosition)
      : null;
    const segments = surfaceTrimLoopSegments.flatMap(segment =>
      segment.continuousPolyline.slice(1).flatMap((point, index) => {
        const from = projectCaptureSourcePosition(
          segment.continuousPolyline[index].sourcePosition,
        );
        const to = projectCaptureSourcePosition(point.sourcePosition);
        return from && to
          ? [{
              key: `${segment.segmentIndex}:${index}`,
              x1: from.x,
              y1: from.y,
              x2: to.x,
              y2: to.y,
            }]
          : [];
      }),
    );
    surfaceTrimProjected = { points, keepSeed, segments };
  }

  function disposeSurfaceTrimRegionOverlay() {
    if (surfaceTrimRegionOverlay) {
      surfaceTrimRegionOverlay.parent?.remove(surfaceTrimRegionOverlay);
      surfaceTrimRegionOverlay.geometry.dispose();
      surfaceTrimRegionOverlay.material.dispose();
      surfaceTrimRegionOverlay = null;
    }
    if (surfaceTrimCapOverlay) {
      surfaceTrimCapOverlay.parent?.remove(surfaceTrimCapOverlay);
      surfaceTrimCapOverlay.geometry.dispose();
      surfaceTrimCapOverlay.material.dispose();
      surfaceTrimCapOverlay = null;
    }
  }

  function syncSurfaceTrimRegionOverlay() {
    disposeSurfaceTrimRegionOverlay();
    if (!surfaceTrimActive || !modelRoot) return;
    const sourceMesh = runtimeMeshes.find(entry => entry.sourcePickMesh)?.sourcePickMesh;
    const sourceGeometry = sourceMesh?.geometry;
    const sourcePositions = sourceGeometry?.getAttribute('position');
    if (!sourceMesh || !sourceGeometry || !sourcePositions) return;
    const sourceIndex = sourceGeometry.getIndex();
    const positions: number[] = [];
    for (const triangleIndex of surfaceTrimRetainedTriangleIndices) {
      if (!Number.isInteger(triangleIndex) || triangleIndex < 0) continue;
      const cornerBase = triangleIndex * 3;
      for (let corner = 0; corner < 3; corner += 1) {
        const vertexIndex = sourceIndex?.getX(cornerBase + corner) ?? cornerBase + corner;
        if (vertexIndex < 0 || vertexIndex >= sourcePositions.count) continue;
        positions.push(
          sourcePositions.getX(vertexIndex),
          sourcePositions.getY(vertexIndex),
          sourcePositions.getZ(vertexIndex),
        );
      }
    }
    if (positions.length > 0) {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
      geometry.computeVertexNormals();
      const material = new THREE.MeshBasicMaterial({
        color: 0x4fa36f,
        opacity: 0.38,
        transparent: true,
        depthTest: true,
        depthWrite: false,
        side: THREE.DoubleSide,
      });
      surfaceTrimRegionOverlay = new THREE.Mesh(geometry, material);
      surfaceTrimRegionOverlay.name = 'surface-trim-retained-region-preview';
      surfaceTrimRegionOverlay.renderOrder = 900;
      surfaceTrimRegionOverlay.userData.previewDiagnosticOnly = true;
      modelRoot.add(surfaceTrimRegionOverlay);
    }

    if (surfaceTrimCapPreview && surfaceTrimCapPreview.triangles.length > 0) {
      const capPositions: number[] = [];
      for (const triangle of surfaceTrimCapPreview.triangles) {
        for (const vertexIndex of triangle) {
          const vertex = surfaceTrimCapPreview.vertices[vertexIndex];
          if (!vertex || !vertex.every(Number.isFinite)) continue;
          capPositions.push(...vertex);
        }
      }
      if (capPositions.length > 0) {
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.Float32BufferAttribute(capPositions, 3));
        geometry.computeVertexNormals();
        const material = new THREE.MeshBasicMaterial({
          color: 0xc89a58,
          opacity: 0.64,
          transparent: true,
          depthTest: true,
          depthWrite: false,
          side: THREE.DoubleSide,
        });
        surfaceTrimCapOverlay = new THREE.Mesh(geometry, material);
        surfaceTrimCapOverlay.name = 'surface-trim-cap-preview';
        surfaceTrimCapOverlay.renderOrder = 910;
        surfaceTrimCapOverlay.userData.previewDiagnosticOnly = true;
        modelRoot.add(surfaceTrimCapOverlay);
      }
    }
  }

  function updateCaptureGuideOverlay() {
    updateSurfaceTrimProjection();
    capturePlaneProjected = capturePlaneAnchors.flatMap(anchor => {
      const projected = projectCaptureSourcePosition(anchor.sourcePosition);
      return projected ? [projected] : [];
    });
    capturePlaneNormalProjected = null;
    if (capturePlaneAnchors.length === 3) {
      const [a, b, c] = capturePlaneAnchors.map(anchor => anchor.sourcePosition);
      const ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]] as [number, number, number];
      const ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]] as [number, number, number];
      const raw = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
      ] as [number, number, number];
      const magnitude = Math.hypot(...raw);
      const edgeScale = Math.max(Math.hypot(...ab), Math.hypot(...ac)) * 0.7;
      if (magnitude > 1e-9 && edgeScale > 0) {
        const origin = [
          (a[0] + b[0] + c[0]) / 3,
          (a[1] + b[1] + c[1]) / 3,
          (a[2] + b[2] + c[2]) / 3,
        ] as [number, number, number];
        const endpoint = origin.map((value, axis) =>
          value + raw[axis] / magnitude * edgeScale,
        ) as [number, number, number];
        const start = projectCaptureSourcePosition(origin);
        const end = projectCaptureSourcePosition(endpoint);
        if (start && end) {
          capturePlaneNormalProjected = { x1: start.x, y1: start.y, x2: end.x, y2: end.y };
        }
      }
    }
    const primitives = captureGuidePrimitives;
    if (!primitives) {
      captureGuideProjected = { points: [], segments: [], planePolygons: [] };
      return;
    }
    const pointById = new Map<string, { x: number; y: number }>();
    const points = primitives.landmarks.flatMap(item => {
      const projected = projectCaptureSourcePosition(item.sourcePosition);
      if (!projected) return [];
      pointById.set(item.landmarkId, projected);
      return [{ ...item, ...projected }];
    });
    const segments = [...primitives.profileSegments, ...primitives.axisSegments].flatMap(segment => {
      const first = pointById.get(segment.fromLandmarkId);
      const second = pointById.get(segment.toLandmarkId);
      return first && second ? [{
        key: segment.key,
        kind: segment.kind,
        x1: first.x,
        y1: first.y,
        x2: second.x,
        y2: second.y,
      }] : [];
    });
    const planePolygons = primitives.planeLoops.flatMap(plane => {
      const polygon = plane.landmarkIds.map(id => pointById.get(id));
      return polygon.every((point): point is { x: number; y: number } => Boolean(point))
        ? [{ key: plane.planeId, points: polygon.map(point => `${point.x},${point.y}`).join(' ') }]
        : [];
    });
    captureGuideProjected = { points, segments, planePolygons };
  }

  function onResize() {
    if (!viewerHost || !camera || !renderer) return;
    const { width, height } = hostSize();
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height);
    updateOverlayAnchor();
    updateCaptureGuideOverlay();
  }

  async function loadCurrentModel() {
    if (!scene || !camera) return;
    const token = ++loadToken;
    console.warn('[CAD_FLOW][viewer.load]', {
      modelKey,
      stlUrl,
      viewerAssetCount: viewerAssets.length,
      retainModelWhileLoading,
      hasMountedModel: Boolean(modelRoot),
    });

    if (viewerAssets.length > 0) {
      await loadMultipartAssets(token, viewerAssets);
      return;
    }

    if (stlUrl) {
      await loadSingleStl(token, stlUrl);
      return;
    }

    if (!retainModelWhileLoading || !modelRoot) disposeModel();
  }

  async function loadMultipartAssets(token: number, assets: ViewerAsset[]) {
    if (!scene || !camera) return;
    const stlLoader = new STLLoader();
    const objLoader = new OBJLoader();
    const threeMfLoader = new ThreeMFLoader();
    const nextRoot = new THREE.Group();
    nextRoot.rotation.x = -Math.PI / 2;
    const nextMeshes: RuntimeMesh[] = [];

    try {
      for (const asset of assets) {
        const loaded = await loadViewerAsset(asset, stlLoader, objLoader, threeMfLoader);
        if (token !== loadToken) {
          disposeDetachedGroup(loaded);
          return;
        }

        const tone = resolveViewerTone(asset.partId, manifestParts);
        nextMeshes.push(...prepareLoadedAssetMeshes(loaded, asset, tone));
        nextRoot.add(loaded);
      }

      if (token !== loadToken) {
        disposeDetachedGroup(nextRoot);
        return;
      }

      disposeModel();
      modelRoot = nextRoot;
      modelStatus = 'loaded';
      runtimeMeshes = nextMeshes;
      applyPreviewTransforms();
      scene.add(modelRoot);
      void syncFemOverlay();
      frameModel(modelRoot);
      attachEdgeTargets(modelRoot);
      attachFaceTargets(modelRoot);
      applyPreviewTransforms();
      applyCameraState(persistedCameraState);
      applySelectionStyles();
      updateOverlayAnchor();
      emitCameraStateChange();
      syncCropBox();
      updateCaptureGuideOverlay();
      syncSurfaceTrimRegionOverlay();
      await notifyModelLoaded(token);
    } catch (error) {
      console.error('Failed to load multipart STL assets:', error);
      disposeDetachedGroup(nextRoot);
      if (stlUrl) {
        await loadSingleStl(token, stlUrl);
        return;
      }
      notifyModelLoadError(token, 'Failed to load multipart STL assets', error);
    }
  }

  async function loadViewerAsset(
    asset: ViewerAsset,
    stlLoader: STLLoader,
    objLoader: OBJLoader,
    threeMfLoader: ThreeMFLoader,
  ): Promise<THREE.Object3D> {
    if (asset.format === 'stl') {
      const geometry = await loadStlGeometry(stlLoader, asset.path);
      return new THREE.Mesh(geometry);
    }
    if (asset.format === 'obj') {
      return loadObjObject(objLoader, asset.path);
    }
    if (asset.format === '3mf') {
      return loadThreeMfObject(threeMfLoader, asset.path);
    }
    throw new Error(`Unsupported viewer asset format: ${asset.format}`);
  }

  function prepareLoadedAssetMeshes(
    object: THREE.Object3D,
    asset: ViewerAsset,
    tone: ViewerTone,
  ): RuntimeMesh[] {
    const meshes: RuntimeMesh[] = [];
    object.traverse((child) => {
      const mesh = child as THREE.Mesh;
      if (!mesh.isMesh || !(mesh.geometry instanceof THREE.BufferGeometry)) return;
      const geometry = prepareDisplayGeometry(mesh.geometry, asset.format === 'stl');
      if (geometry !== mesh.geometry) {
        mesh.geometry.dispose();
        mesh.geometry = geometry;
      }
      geometry.computeBoundingBox();
      mesh.material = createMaterial(tone, asset.partId === selectedPartId);
      const outline = createOutline(geometry, tone, asset.partId === selectedPartId);
      const topology = createTopologyOverlay(geometry, tone);
      if (outline) {
        mesh.add(outline);
      }
      if (topology) {
        mesh.add(topology);
      }
      mesh.userData.partId = asset.partId;
      mesh.userData.nodeId = asset.nodeId;
      const runtimeMesh = mesh as THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>;
      meshes.push({
        partId: asset.partId,
        baseBounds: geometry.boundingBox?.clone() ?? null,
        outline,
        mesh: runtimeMesh,
        topology,
        tone,
      });
    });
    return meshes;
  }

  function captureSourceToLocalMatrix(): THREE.Matrix4 {
    if (!captureComparisonStlUrl || !captureGuide) return new THREE.Matrix4();
    const scaleMm = captureGuide.calibration.millimetresPerSourceUnit;
    const { originMm, xAxis, yAxis, zAxis } = captureGuide.reconstructionFrame;
    const offset = (axis: [number, number, number]) =>
      -(axis[0] * originMm[0] + axis[1] * originMm[1] + axis[2] * originMm[2]);
    return new THREE.Matrix4().set(
      xAxis[0] * scaleMm, xAxis[1] * scaleMm, xAxis[2] * scaleMm, offset(xAxis),
      yAxis[0] * scaleMm, yAxis[1] * scaleMm, yAxis[2] * scaleMm, offset(yAxis),
      zAxis[0] * scaleMm, zAxis[1] * scaleMm, zAxis[2] * scaleMm, offset(zAxis),
      0, 0, 0, 1,
    );
  }

  function applyCaptureComparisonState() {
    const comparing = Boolean(captureComparisonStlUrl);
    const referenceMatrix = captureSourceToLocalMatrix();
    const referenceOpacity = Math.max(0.02, Math.min(1, captureReferenceOpacity));
    for (const entry of runtimeMeshes) {
      if (entry.captureLayer === 'reference') {
        entry.mesh.visible = !comparing || captureReferenceVisible;
        entry.mesh.matrixAutoUpdate = false;
        entry.mesh.matrix.copy(referenceMatrix);
        entry.mesh.matrixWorldNeedsUpdate = true;
        if (entry.sourcePickMesh) {
          entry.sourcePickMesh.matrixAutoUpdate = false;
          entry.sourcePickMesh.matrix.copy(referenceMatrix);
          entry.sourcePickMesh.matrixWorldNeedsUpdate = true;
        }
        entry.mesh.material.transparent = comparing;
        entry.mesh.material.opacity = comparing ? referenceOpacity : 1;
        entry.mesh.material.depthWrite = !comparing || referenceOpacity >= 0.98;
        if (entry.outline) {
          entry.outline.material.transparent = comparing;
          entry.outline.material.opacity = comparing ? Math.min(0.72, referenceOpacity + 0.18) : 1;
        }
      } else if (entry.captureLayer === 'generated') {
        entry.mesh.visible = captureGeneratedVisible;
        const generatedOpacity = Math.max(0.02, Math.min(1, captureGeneratedOpacity));
        entry.mesh.material.transparent = generatedOpacity < 0.98;
        entry.mesh.material.opacity = generatedOpacity;
        entry.mesh.material.depthWrite = generatedOpacity >= 0.98;
      }
    }
    modelRoot?.updateMatrixWorld(true);
    updateCaptureGuideOverlay();
  }

  function syncCaptureDeviationOverlay() {
    if (captureDeviationOverlay) {
      captureDeviationOverlay.removeFromParent();
      captureDeviationOverlay.geometry.dispose();
      captureDeviationOverlay.material.dispose();
      captureDeviationOverlay = null;
    }
    captureDeviationPointCount = 0;
    if (!modelRoot || !captureComparisonStlUrl || !captureDeviation) return;
    const displayPoints = buildCaptureDeviationDisplayPoints(
      captureDeviation.displaySamples ?? [],
      captureDeviation.outlierThresholdMm,
    );
    if (displayPoints.length === 0) return;
    const positions = new Float32Array(displayPoints.length * 3);
    const colors = new Float32Array(displayPoints.length * 3);
    displayPoints.forEach((point, index) => {
      positions.set(point.localPositionMm, index * 3);
      const color = new THREE.Color(point.color);
      colors.set([color.r, color.g, color.b], index * 3);
    });
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
    const material = new THREE.PointsMaterial({
      depthTest: false,
      depthWrite: false,
      opacity: 0.92,
      size: 6,
      sizeAttenuation: false,
      transparent: true,
      vertexColors: true,
    });
    captureDeviationOverlay = new THREE.Points(geometry, material);
    captureDeviationOverlay.name = 'capture-deviation-display-only';
    captureDeviationOverlay.renderOrder = 1000;
    captureDeviationOverlay.visible = captureDeviationVisible;
    captureDeviationOverlay.userData.previewDiagnosticOnly = true;
    modelRoot.add(captureDeviationOverlay);
    captureDeviationPointCount = displayPoints.length;
  }

  async function loadSingleStl(token: number, url: string) {
    if (!scene || !camera) return;
    const loader = new STLLoader();
    const nextRoot = new THREE.Group();
    nextRoot.rotation.x = -Math.PI / 2;

    try {
      const geometry = await loadStlGeometry(loader, url);
      if (token !== loadToken) {
        geometry.dispose();
        return;
      }

      const displayGeometry = prepareDisplayGeometry(geometry, true);
      displayGeometry.computeBoundingBox();
      const tone = resolveViewerTone(null, manifestParts);
      const material = createMaterial(tone, false);
      const mesh = new THREE.Mesh(displayGeometry, material);
      const sourcePickMesh = new THREE.Mesh(
        geometry,
        new THREE.MeshBasicMaterial({ visible: false }),
      );
      sourcePickMesh.userData.captureSourceGeometry = true;
      const outline = createOutline(displayGeometry, tone, false);
      const topology = createTopologyOverlay(displayGeometry, tone);
      if (outline) {
        mesh.add(outline);
      }
      if (topology) {
        mesh.add(topology);
      }
      nextRoot.add(mesh);
      nextRoot.add(sourcePickMesh);

      const captureMeshes: RuntimeMesh[] = [{
        partId: null,
        baseBounds: displayGeometry.boundingBox?.clone() ?? null,
        outline,
        mesh,
        sourcePickMesh,
        topology,
        tone,
        captureLayer: 'reference',
      }];
      if (captureComparisonStlUrl) {
        const generatedGeometry = await loadStlGeometry(loader, captureComparisonStlUrl);
        if (token !== loadToken) {
          generatedGeometry.dispose();
          disposeDetachedGroup(nextRoot);
          return;
        }
        const generatedDisplayGeometry = prepareDisplayGeometry(generatedGeometry, true);
        generatedDisplayGeometry.computeBoundingBox();
        const generatedTone = resolveViewerTone('generated-brep', manifestParts);
        const generatedMaterial = createMaterial(generatedTone, false);
        const generatedMesh = new THREE.Mesh(generatedDisplayGeometry, generatedMaterial);
        const generatedOutline = createOutline(generatedDisplayGeometry, generatedTone, false);
        const generatedTopology = createTopologyOverlay(generatedDisplayGeometry, generatedTone);
        if (generatedOutline) generatedMesh.add(generatedOutline);
        if (generatedTopology) generatedMesh.add(generatedTopology);
        nextRoot.add(generatedMesh);
        captureMeshes.push({
          partId: null,
          baseBounds: generatedDisplayGeometry.boundingBox?.clone() ?? null,
          outline: generatedOutline,
          mesh: generatedMesh,
          topology: generatedTopology,
          tone: generatedTone,
          captureLayer: 'generated',
        });
      }
      disposeModel();
      modelRoot = nextRoot;
      modelStatus = 'loaded';
      runtimeMeshes = captureMeshes;
      captureComparisonLoaded = captureMeshes.some((entry) => entry.captureLayer === 'generated');
      syncCaptureDeviationOverlay();
      applyCaptureComparisonState();
      applyPreviewTransforms();
      scene.add(modelRoot);
      void syncFemOverlay();
      frameModel(modelRoot);
      attachEdgeTargets(modelRoot);
      attachFaceTargets(modelRoot);
      applyPreviewTransforms();
      applyCameraState(persistedCameraState);
      updateOverlayAnchor();
      emitCameraStateChange();
      syncCropBox();
      updateCaptureGuideOverlay();
      await notifyModelLoaded(token);
    } catch (error) {
      console.error('Failed to load STL:', error);
      if (modelRoot === nextRoot) {
        disposeModel();
      } else {
        disposeDetachedGroup(nextRoot);
      }
      notifyModelLoadError(token, 'Failed to load STL', error);
    }
  }

  function prepareDisplayGeometry(geometry: THREE.BufferGeometry, smoothStlSeams: boolean): THREE.BufferGeometry {
    if (!smoothStlSeams) {
      geometry.computeVertexNormals();
      return geometry;
    }

    return prepareStlDisplayGeometry(geometry);
  }

  function frameModel(object: THREE.Object3D) {
    if (!scene || !camera || !controls) return;
    object.updateMatrixWorld(true);
    const box = new THREE.Box3().setFromObject(object);
    if (box.isEmpty()) return;

    const center = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const maxDim = Math.max(size.x, size.y, size.z, 1);

    if (object === modelRoot) {
      object.position.x -= center.x;
      object.position.z -= center.z;
      object.position.y -= box.min.y;
      object.updateMatrixWorld(true);
      syncComponentPlacementFrame();
      alignParameterGhost();
      center.set(0, maxDim * 0.35, 0);
      camera.position.set(maxDim * 1.3, maxDim * 1.1, maxDim * 1.3);
    } else {
      camera.position.copy(center).add(new THREE.Vector3(1.3, 1.1, 1.3).multiplyScalar(maxDim));
    }
    controls.target.copy(center);
    updateCameraClipPlanes();
    controls.update();
  }

  function createMaterial(tone: ViewerTone, isSelected: boolean) {
    return new THREE.MeshStandardMaterial({
      color: isSelected ? 0xe5ca88 : tone.color,
      emissive: isSelected ? tone.emissive : 0x000000,
      emissiveIntensity: isSelected ? 0.38 : 0,
      metalness: 0.04,
      roughness: 0.54,
    });
  }

  function createOutline(
    geometry: THREE.BufferGeometry,
    tone: ViewerTone,
    isSelected: boolean,
  ): THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial> | null {
    const outlineGeometry = new THREE.EdgesGeometry(geometry, 32);
    if (outlineGeometry.getAttribute('position')?.count === 0) {
      outlineGeometry.dispose();
      return null;
    }
    const outline = new THREE.LineSegments(
      outlineGeometry,
      new THREE.LineBasicMaterial({
        color: isSelected ? 0xe5ca88 : tone.edge,
        transparent: true,
        opacity: isSelected ? 0.95 : 0.26,
        depthTest: true,
        depthWrite: false,
      }),
    );
    outline.renderOrder = 2;
    outline.userData.ignoreRaycast = true;
    return outline;
  }

  function createTopologyOverlay(
    geometry: THREE.BufferGeometry,
    tone: ViewerTone,
  ): THREE.LineSegments<THREE.WireframeGeometry, THREE.LineBasicMaterial> | null {
    const topologyGeometry = new THREE.WireframeGeometry(geometry);
    if (topologyGeometry.getAttribute('position')?.count === 0) {
      topologyGeometry.dispose();
      return null;
    }
    const topology = new THREE.LineSegments(
      topologyGeometry,
      new THREE.LineBasicMaterial({
        color: tone.topology,
        transparent: true,
        opacity: 0,
        depthTest: false,
        depthWrite: false,
      }),
    );
    topology.renderOrder = 3;
    topology.userData.ignoreRaycast = true;
    return topology;
  }

  function createEdgeMaterial(isSelected: boolean, isHovered: boolean) {
    return new THREE.LineBasicMaterial({
      color: isSelected ? 0xe5ca88 : isHovered ? 0x78c0a8 : 0x405371,
      transparent: true,
      opacity: isSelected ? 1 : isHovered ? 0.95 : 0,
    });
  }

  function createFaceMaterial(isSelected: boolean, isHovered: boolean) {
    return new THREE.MeshBasicMaterial({
      color: isSelected ? 0xe5ca88 : isHovered ? 0x78c0a8 : 0xb78a4b,
      transparent: true,
      opacity: isSelected ? 0.36 : isHovered ? 0.3 : 0,
      depthTest: false,
      depthWrite: false,
      side: THREE.DoubleSide,
    });
  }

  function faceTargetRadius(target: ViewerFaceTarget): number {
    const area = typeof target.area === 'number' && Number.isFinite(target.area) ? target.area : null;
    if (!area || area <= 0) return 6;
    return Math.max(2, Math.min(18, Math.sqrt(area / Math.PI) * 0.18));
  }

  function faceTargetNormal(target: ViewerFaceTarget): THREE.Vector3 {
    const [x, y, z] = target.normal ?? [0, 0, 1];
    const normal = new THREE.Vector3(x, y, z);
    if (normal.lengthSq() < 0.000001) return new THREE.Vector3(0, 0, 1);
    return normal.normalize();
  }

  function disposeRuntimeEdges(root: THREE.Group | null) {
    if (!root) {
      runtimeEdges = [];
      return;
    }
    for (const entry of runtimeEdges) {
      root.remove(entry.line);
      entry.line.geometry?.dispose?.();
      entry.line.material?.dispose?.();
    }
    runtimeEdges = [];
  }

  function disposeRuntimeFaces(root: THREE.Group | null) {
    if (!root) {
      runtimeFaces = [];
      return;
    }
    for (const entry of runtimeFaces) {
      root.remove(entry.mesh);
      entry.mesh.geometry?.dispose?.();
      entry.mesh.material?.dispose?.();
    }
    runtimeFaces = [];
  }

  function attachEdgeTargets(root: THREE.Group) {
    disposeRuntimeEdges(root);
    if (!topologyMaterialization.materialize || edgeTargets.length === 0) return;

    runtimeEdges = edgeTargets.map((target) => {
      const geometry = new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(target.start.x, target.start.y, target.start.z),
        new THREE.Vector3(target.end.x, target.end.y, target.end.z),
      ]);
      const line = new THREE.Line(
        geometry,
        createEdgeMaterial(false, false),
      );
      line.userData.partId = target.partId;
      line.userData.viewerNodeId = target.viewerNodeId;
      line.userData.selectionTargetId = target.targetId;
      line.userData.selectionTargetIds = [
        target.targetId,
        ...(target.durableTargetId ? [target.durableTargetId] : []),
        ...(target.canonicalTargetId ? [target.canonicalTargetId] : []),
        ...(target.aliasIds || []),
      ];
      root.add(line);
      return {
        targetId: target.targetId,
        durableTargetId: target.durableTargetId,
        canonicalTargetId: target.canonicalTargetId,
        aliasIds: target.aliasIds || [],
        partId: target.partId,
        line,
      };
    });
  }

  function attachFaceTargets(root: THREE.Group) {
    disposeRuntimeFaces(root);
    if (!topologyMaterialization.materialize || faceTargets.length === 0) return;

    runtimeFaces = faceTargets.map((target) => {
      const radius = faceTargetRadius(target);
      const geometry = new THREE.CircleGeometry(radius, 32);
      const normal = faceTargetNormal(target);
      const mesh = new THREE.Mesh(
        geometry,
        createFaceMaterial(false, false),
      );
      const basePosition = new THREE.Vector3(
        target.center.x + normal.x * Math.min(0.5, radius * 0.04),
        target.center.y + normal.y * Math.min(0.5, radius * 0.04),
        target.center.z + normal.z * Math.min(0.5, radius * 0.04),
      );
      mesh.position.copy(basePosition);
      mesh.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), normal);
      mesh.renderOrder = 4;
      mesh.userData.partId = target.partId;
      mesh.userData.viewerNodeId = target.viewerNodeId;
      mesh.userData.selectionTargetId = target.targetId;
      mesh.userData.selectionTargetIds = [
        target.targetId,
        ...(target.durableTargetId ? [target.durableTargetId] : []),
        ...(target.canonicalTargetId ? [target.canonicalTargetId] : []),
        ...(target.aliasIds || []),
      ];
      root.add(mesh);
      return {
        targetId: target.targetId,
        durableTargetId: target.durableTargetId,
        canonicalTargetId: target.canonicalTargetId,
        aliasIds: target.aliasIds || [],
        partId: target.partId,
        basePosition,
        mesh,
      };
    });
  }

  function applySelectionStyles() {
    const measurementPartIds = new Set(activeMeasurementCallout?.partIds || []);
    const measurementTargetIds = new Set(activeMeasurementCallout?.targetIds || []);

    for (const entry of runtimeMeshes) {
      const isSelected = !!selectedPartId && entry.partId === selectedPartId;
      const isInspected = !isSelected && !selectedPartId && !!entry.partId && inspectedPartId === entry.partId;
      const isActive = isSelected || isInspected;
      const isHovered = !isSelected && !!hoveredPartId && entry.partId === hoveredPartId;
      const isMeasured =
        !isSelected && !isHovered && !!entry.partId && measurementPartIds.has(entry.partId);
      entry.mesh.material.color.setHex(
        isActive ? 0xe5ca88 : isHovered ? entry.tone.hoverColor : isMeasured ? entry.tone.measuredColor : entry.tone.color,
      );
      entry.mesh.material.emissive.setHex(
        isActive ? entry.tone.emissive : isHovered ? entry.tone.hoverEmissive : isMeasured ? entry.tone.measuredEmissive : 0x000000,
      );
      entry.mesh.material.emissiveIntensity = isSelected ? 0.38 : isInspected ? 0.2 : isHovered ? 0.24 : isMeasured ? 0.18 : 0;
      if (entry.outline) {
        entry.outline.visible = outlineEnabled || topologyMode === 'feature' || isInspected;
        entry.outline.material.color.setHex(
          isActive || (topologyMode === 'feature' && isHovered) ? 0xe5ca88 : entry.tone.edge,
        );
        entry.outline.material.opacity = !entry.outline.visible
          ? 0
          : isActive
            ? 0.95
            : topologyMode === 'feature' && isHovered
              ? 0.72
              : isHovered
                ? 0.4
                : isMeasured
                  ? 0.34
                  : 0.26;
      }
      if (entry.topology) {
        const topologyActive = isActive || isHovered || isMeasured;
        entry.topology.visible = meshTopologyVisible(topologyMode, topologyActive);
        entry.topology.material.opacity = meshTopologyOpacity(topologyMode, topologyActive);
      }
    }

    for (const entry of runtimeEdges) {
      const isSelected =
        selectedTarget?.kind === 'edge' &&
        runtimeEdgeMatchesTargetId(entry.targetId, selectedTarget.targetId);
      const isHovered = !isSelected && runtimeEdgeMatchesTargetId(entry.targetId, hoveredTargetId);
      const isMeasured =
        !isSelected &&
        !isHovered &&
        [...measurementTargetIds].some((targetId) => runtimeEdgeMatchesTargetId(entry.targetId, targetId));
      const showFeatureTopology = topologyMode === 'feature';
      entry.line.visible = showFeatureTopology || isSelected || isHovered || isMeasured;
      entry.line.material.color.setHex(
        isSelected ? 0xe5ca88 : isHovered ? 0x78c0a8 : isMeasured ? 0x9ad8c5 : 0x405371,
      );
      entry.line.material.opacity = isSelected
        ? 1
        : isHovered
          ? 0.95
          : isMeasured
            ? 0.88
            : showFeatureTopology
              ? 0.46
              : 0;
    }

    for (const entry of runtimeFaces) {
      const isSelected =
        selectedTarget?.kind === 'face' &&
        runtimeFaceMatchesTargetId(entry.targetId, selectedTarget.targetId);
      const isHovered = !isSelected && runtimeFaceMatchesTargetId(entry.targetId, hoveredTargetId);
      const isMeasured =
        !isSelected &&
        !isHovered &&
        [...measurementTargetIds].some((targetId) => runtimeFaceMatchesTargetId(entry.targetId, targetId));
      entry.mesh.visible = selectionMode || isSelected || isHovered || isMeasured;
      entry.mesh.material.color.setHex(
        isSelected ? 0xe5ca88 : isHovered ? 0x78c0a8 : isMeasured ? 0x9ad8c5 : 0xb78a4b,
      );
      entry.mesh.material.opacity = isSelected ? 0.36 : isHovered ? 0.3 : isMeasured ? 0.24 : 0;
    }
  }

  function applyPreviewTransforms() {
    for (const entry of runtimeMeshes) {
      if (!entry.partId || !entry.baseBounds) {
        entry.mesh.scale.set(1, 1, 1);
        entry.mesh.position.set(0, 0, 0);
        continue;
      }

      const preview = previewTransforms[entry.partId];
      if (!preview) {
        entry.mesh.scale.set(1, 1, 1);
        entry.mesh.position.set(0, 0, 0);
        continue;
      }

      const { scale, anchor, translate } = preview;
      entry.mesh.scale.set(scale.x, scale.y, scale.z);
      entry.mesh.position.set(
        (1 - scale.x) * anchor.x + (translate?.x ?? 0),
        (1 - scale.y) * anchor.y + (translate?.y ?? 0),
        (1 - scale.z) * anchor.z + (translate?.z ?? 0),
      );
    }

    for (const entry of runtimeEdges) {
      const preview = previewTransforms[entry.partId];
      if (!preview) {
        entry.line.scale.set(1, 1, 1);
        entry.line.position.set(0, 0, 0);
        continue;
      }

      const { scale, anchor, translate } = preview;
      entry.line.scale.set(scale.x, scale.y, scale.z);
      entry.line.position.set(
        (1 - scale.x) * anchor.x + (translate?.x ?? 0),
        (1 - scale.y) * anchor.y + (translate?.y ?? 0),
        (1 - scale.z) * anchor.z + (translate?.z ?? 0),
      );
    }

    for (const entry of runtimeFaces) {
      const preview = previewTransforms[entry.partId];
      if (!preview) {
        entry.mesh.scale.set(1, 1, 1);
        entry.mesh.position.copy(entry.basePosition);
        continue;
      }

      const { scale, anchor, translate } = preview;
      entry.mesh.scale.set(scale.x, scale.y, scale.z);
      entry.mesh.position.set(
        entry.basePosition.x + (1 - scale.x) * anchor.x + (translate?.x ?? 0),
        entry.basePosition.y + (1 - scale.y) * anchor.y + (translate?.y ?? 0),
        entry.basePosition.z + (1 - scale.z) * anchor.z + (translate?.z ?? 0),
      );
    }
  }

  function projectMeshPoint(
    mesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>,
    mode: 'center' | 'top',
  ) {
    if (!camera || !renderer || !viewerHost) return null;

    const box = new THREE.Box3().setFromObject(mesh);
    if (box.isEmpty()) return null;

    const point = new THREE.Vector3(
      (box.min.x + box.max.x) * 0.5,
      mode === 'top' ? box.max.y : (box.min.y + box.max.y) * 0.5,
      (box.min.z + box.max.z) * 0.5,
    );
    point.project(camera);
    if (point.z < -1 || point.z > 1) return null;

    const width = renderer.domElement.clientWidth || viewerHost.clientWidth;
    const height = renderer.domElement.clientHeight || viewerHost.clientHeight;
    return {
      x: ((point.x + 1) * 0.5) * width,
      y: ((1 - point.y) * 0.5) * height,
    };
  }

  function projectMeshFrame(mesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>) {
    if (!camera || !renderer || !viewerHost) return null;

    const box = new THREE.Box3().setFromObject(mesh);
    if (box.isEmpty()) return null;

    const corners = [
      new THREE.Vector3(box.min.x, box.min.y, box.min.z),
      new THREE.Vector3(box.min.x, box.min.y, box.max.z),
      new THREE.Vector3(box.min.x, box.max.y, box.min.z),
      new THREE.Vector3(box.min.x, box.max.y, box.max.z),
      new THREE.Vector3(box.max.x, box.min.y, box.min.z),
      new THREE.Vector3(box.max.x, box.min.y, box.max.z),
      new THREE.Vector3(box.max.x, box.max.y, box.min.z),
      new THREE.Vector3(box.max.x, box.max.y, box.max.z),
    ];

    const width = renderer.domElement.clientWidth || viewerHost.clientWidth;
    const height = renderer.domElement.clientHeight || viewerHost.clientHeight;
    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;

    for (const corner of corners) {
      corner.project(camera);
      if (corner.z < -1 || corner.z > 1) continue;
      const x = ((corner.x + 1) * 0.5) * width;
      const y = ((1 - corner.y) * 0.5) * height;
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }

    if (!Number.isFinite(minX) || !Number.isFinite(minY)) return null;

    return {
      left: minX,
      right: maxX,
      top: minY,
      bottom: maxY,
      width: Math.max(0, maxX - minX),
      height: Math.max(0, maxY - minY),
    };
  }

  function projectWorldPoint(point: [number, number, number]) {
    if (!camera || !renderer || !viewerHost) return null;
    const projected = new THREE.Vector3(point[0], point[1], point[2]).project(camera);
    if (projected.z < -1 || projected.z > 1) return null;
    const width = renderer.domElement.clientWidth || viewerHost.clientWidth;
    const height = renderer.domElement.clientHeight || viewerHost.clientHeight;
    return {
      x: ((projected.x + 1) * 0.5) * width,
      y: ((1 - projected.y) * 0.5) * height,
    };
  }

  function selectionTargetMatchesId(
    target: ContextSelectionTarget | null | undefined,
    requestedTargetId: string | null | undefined,
  ) {
    return Boolean(
      target &&
        requestedTargetId &&
        (target.targetId === requestedTargetId || target.aliasIds.includes(requestedTargetId)),
    );
  }

  function resolveSelectionTargetByAnyId(targetId: string | null | undefined) {
    if (!targetId) return null;
    return selectionTargets.find((target) => selectionTargetMatchesId(target, targetId)) ?? null;
  }

  function runtimeEdgeMatchesTargetId(
    runtimeTargetId: string | null | undefined,
    requestedTargetId: string | null | undefined,
  ) {
    if (!runtimeTargetId || !requestedTargetId) return false;
    const selectionTarget = resolveSelectionTargetByAnyId(runtimeTargetId);
    if (!selectionTarget) {
      const runtimeEdge = runtimeEdges.find((entry) => entry.targetId === runtimeTargetId);
      if (!runtimeEdge) return runtimeTargetId === requestedTargetId;
      return (
        runtimeTargetId === requestedTargetId ||
        runtimeEdge.durableTargetId === requestedTargetId ||
        runtimeEdge.canonicalTargetId === requestedTargetId ||
        runtimeEdge.aliasIds.includes(requestedTargetId)
      );
    }
    return selectionTargetMatchesId(selectionTarget, requestedTargetId);
  }

  function runtimeFaceMatchesTargetId(
    runtimeTargetId: string | null | undefined,
    requestedTargetId: string | null | undefined,
  ) {
    if (!runtimeTargetId || !requestedTargetId) return false;
    const selectionTarget = resolveSelectionTargetByAnyId(runtimeTargetId);
    if (!selectionTarget) {
      const runtimeFace = runtimeFaces.find((entry) => entry.targetId === runtimeTargetId);
      if (!runtimeFace) return runtimeTargetId === requestedTargetId;
      return (
        runtimeTargetId === requestedTargetId ||
        runtimeFace.durableTargetId === requestedTargetId ||
        runtimeFace.canonicalTargetId === requestedTargetId ||
        runtimeFace.aliasIds.includes(requestedTargetId)
      );
    }
    return selectionTargetMatchesId(selectionTarget, requestedTargetId);
  }

  function projectEdgeMidpoint(targetId: string) {
    const edge = runtimeEdges.find((entry) => runtimeEdgeMatchesTargetId(entry.targetId, targetId))?.line;
    if (!edge) return null;
    const position = edge.geometry.getAttribute('position');
    if (!position || position.count < 2) return null;
    const start = new THREE.Vector3().fromBufferAttribute(position, 0);
    const end = new THREE.Vector3().fromBufferAttribute(position, position.count - 1);
    start.applyMatrix4(edge.matrixWorld);
    end.applyMatrix4(edge.matrixWorld);
    return projectWorldPoint([
      (start.x + end.x) * 0.5,
      (start.y + end.y) * 0.5,
      (start.z + end.z) * 0.5,
    ]);
  }

  function projectRuntimeFaceCenter(face: RuntimeFace) {
    const center = face.basePosition.clone().applyMatrix4(face.mesh.parent?.matrixWorld ?? face.mesh.matrixWorld);
    return projectWorldPoint([center.x, center.y, center.z]);
  }

  function fallbackMeasurementPoint(): { x: number; y: number } | null {
    for (const targetId of activeMeasurementCallout?.targetIds || []) {
      const edgePoint = projectEdgeMidpoint(targetId);
      if (edgePoint) return edgePoint;
    }

    for (const partId of activeMeasurementCallout?.partIds || []) {
      const point = runtimeMeshes
        .filter((entry) => entry.partId === partId)
        .map((entry) => projectMeshPoint(entry.mesh, 'top'))
        .find(Boolean);
      if (point) return point;
    }

    if (selectedPartId) {
      const selectedPoint = runtimeMeshes
        .filter((entry) => entry.partId === selectedPartId)
        .map((entry) => projectMeshPoint(entry.mesh, 'top'))
        .find(Boolean);
      if (selectedPoint) return selectedPoint;
    }

    return null;
  }

  function updateMeasurementOverlay() {
    if (!activeMeasurementCallout || hideModelWhileBusy) {
      measurementOverlay = null;
      return;
    }

    const lineSegments: Array<{ x1: number; y1: number; x2: number; y2: number }> = [];
    let badgePoint: { x: number; y: number } | null = null;

    if (activeMeasurementCallout.guide && activeMeasurementCallout.guide.points.length > 0) {
      const screenPoints = activeMeasurementCallout.guide.points
        .map((point) => projectWorldPoint(point))
        .filter((point): point is { x: number; y: number } => Boolean(point));
      for (let index = 1; index < screenPoints.length; index += 1) {
        const previous = screenPoints[index - 1];
        const next = screenPoints[index];
        lineSegments.push({
          x1: previous.x,
          y1: previous.y,
          x2: next.x,
          y2: next.y,
        });
      }
      if (activeMeasurementCallout.guide.labelPoint) {
        const labelPoint = projectWorldPoint(activeMeasurementCallout.guide.labelPoint);
        if (labelPoint) {
          const leaderStart = screenPoints[screenPoints.length - 1] ?? labelPoint;
          lineSegments.push({
            x1: leaderStart.x,
            y1: leaderStart.y,
            x2: labelPoint.x,
            y2: labelPoint.y,
          });
          badgePoint = labelPoint;
        }
      }
      if (!badgePoint && screenPoints.length > 0) {
        const first = screenPoints[0];
        const last = screenPoints[screenPoints.length - 1];
        badgePoint = {
          x: (first.x + last.x) * 0.5,
          y: Math.min(first.y, last.y) - 18,
        };
      }
    }

    if (!badgePoint) {
      badgePoint = fallbackMeasurementPoint();
    }

    if (!badgePoint) {
      measurementOverlay = null;
      return;
    }

    measurementOverlay = {
      badgeLeft: badgePoint.x,
      badgeTop: badgePoint.y,
      lineSegments,
      label: activeMeasurementCallout.badgeLabel,
      explanation: activeMeasurementCallout.explanation,
    };
  }

  function updateOverlayAnchor() {
    if (!overlayPartLabel || !showPartOverlay) {
      overlayVisible = false;
      overlayFallback = true;
      dimensionFrame = null;
      updateMeasurementOverlay();
      return;
    }

    overlayVisible = true;

    if (!selectedPartId) {
      overlayFallback = true;
      dimensionFrame = null;
      overlayLeft = 24;
      overlayTop = 24;
      updateMeasurementOverlay();
      return;
    }

    if (!camera || !renderer || !viewerHost) {
      overlayFallback = true;
      dimensionFrame = null;
      updateMeasurementOverlay();
      return;
    }

    const targetMesh = runtimeMeshes.find((entry) => entry.partId === selectedPartId)?.mesh;
    if (!targetMesh) {
      overlayFallback = true;
      dimensionFrame = null;
      updateMeasurementOverlay();
      return;
    }

    const anchor = projectMeshPoint(targetMesh, 'top');
    dimensionFrame = projectMeshFrame(targetMesh);
    if (!anchor) {
      overlayFallback = true;
      updateMeasurementOverlay();
      return;
    }
    overlayLeft = anchor.x;
    overlayTop = anchor.y;
    overlayFallback = false;
    updateMeasurementOverlay();
  }

  function disposeModel() {
    disposeCropBox();
    disposeFemOverlay();
    disposeSurfaceTrimRegionOverlay();
    captureComparisonLoaded = false;
    captureDeviationOverlay = null;
    captureDeviationPointCount = 0;
    captureGuideProjected = { points: [], segments: [], planePolygons: [] };
    if (!modelRoot) {
      modelStatus = 'empty';
      runtimeMeshes = [];
      runtimeEdges = [];
      runtimeFaces = [];
      inspectedPartId = null;
      updateOverlayAnchor();
      return;
    }
    disposeRuntimeEdges(modelRoot);
    disposeRuntimeFaces(modelRoot);
    scene?.remove(modelRoot);
    disposeDetachedGroup(modelRoot);
    modelRoot = null;
    syncComponentPlacementFrame();
    alignParameterGhost();
    modelStatus = 'empty';
    runtimeMeshes = [];
    runtimeEdges = [];
    runtimeFaces = [];
    inspectedPartId = null;
    updateOverlayAnchor();
  }

  function disposeDetachedGroup(group: THREE.Object3D) {
    group.traverse((child) => {
      if (child instanceof THREE.Mesh) {
        child.geometry?.dispose?.();
        child.material?.dispose?.();
      }
      if (child instanceof THREE.Line) {
        child.geometry?.dispose?.();
        child.material?.dispose?.();
      }
      if (child instanceof THREE.Points) {
        child.geometry?.dispose?.();
        child.material?.dispose?.();
      }
    });
  }

  function handlePointerDown(event: PointerEvent) {
    if (hideModelWhileBusy) return;
    if (isInteractiveViewerControl(event.target)) return;
    if (event.button !== 0) return;
    pointerDownAt = componentPlacementPreview?.transform.dragging
      ? null
      : { x: event.clientX, y: event.clientY };
    orbitDraggedSincePointerDown = false;
  }

  function isInteractiveViewerControl(target: EventTarget | null) {
    return target instanceof HTMLElement && Boolean(target.closest('button, input, textarea, select, label, a'));
  }

  function handleControlsChange() {
    updateCameraClipPlanes();
    updateCaptureGuideOverlay();
  }

  function handleOrbitStart() {
    isOrbitDragging = true;
    if (hoveredPartId !== null || hoveredTargetId !== null) {
      hoveredPartId = null;
      hoveredTargetId = null;
      applySelectionStyles();
    }
    if (renderer) {
      renderer.domElement.style.cursor = 'grabbing';
    }
  }

  function handleOrbitEnd() {
    isOrbitDragging = false;
    if (showContextOverlay) {
      updateOverlayAnchor();
    }
    emitCameraStateChange();
    if (renderer) {
      renderer.domElement.style.cursor = hideModelWhileBusy ? 'progress' : selectionMode ? 'crosshair' : 'default';
    }
  }

  function selectionTargetFromFaceHit(object: THREE.Object3D): ContextSelectionTarget | null {
    if (!object.userData.selectionTargetId) return null;
    const targetId = object.userData.selectionTargetId as string;
    const aliasIds = Array.isArray(object.userData.selectionTargetIds)
      ? (object.userData.selectionTargetIds as string[]).filter(
          (candidate) => typeof candidate === 'string' && candidate !== targetId,
        )
      : [];
    const partId = (object.userData.partId as string | undefined) ?? null;
    const viewerNodeId = (object.userData.viewerNodeId as string | undefined) ?? null;
    return (
      resolveSelectionTargetByAnyId(targetId) ?? {
        targetId,
        aliasIds,
        kind: 'face',
        partId,
        label: partId ? `${partId} Face` : 'Face',
        editable: true,
        viewerNodeId,
        parameterKeys: [],
        primitiveIds: [],
        viewIds: [],
      }
    );
  }

  function selectionTargetFromEvent(event: PointerEvent): ContextSelectionTarget | null {
    if (hideModelWhileBusy || !renderer || !camera || !modelRoot || runtimeMeshes.length === 0) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);

    raycaster.params.Line.threshold = 6;
    const edgeHit = raycaster
      .intersectObjects(runtimeEdges.map((entry) => entry.line), true)
      .find((entry) => typeof entry.object.userData.selectionTargetId === 'string');
    if (edgeHit?.object.userData.selectionTargetId) {
      const targetId = edgeHit.object.userData.selectionTargetId as string;
      const aliasIds = Array.isArray(edgeHit.object.userData.selectionTargetIds)
        ? (edgeHit.object.userData.selectionTargetIds as string[]).filter(
            (candidate) => typeof candidate === 'string' && candidate !== targetId,
          )
        : [];
      const partId = (edgeHit.object.userData.partId as string | undefined) ?? null;
      const viewerNodeId = (edgeHit.object.userData.viewerNodeId as string | undefined) ?? null;
      return (
        resolveSelectionTargetByAnyId(targetId) ?? {
          targetId,
          aliasIds,
          kind: 'edge',
          partId,
          label: partId ? `${partId} Edge` : 'Edge',
          editable: true,
          viewerNodeId,
          parameterKeys: [],
          primitiveIds: [],
          viewIds: [],
        }
      );
    }

    const faceHit = raycaster
      .intersectObjects(runtimeFaces.map((entry) => entry.mesh), true)
      .find((entry) => typeof entry.object.userData.selectionTargetId === 'string');
    if (faceHit?.object.userData.selectionTargetId) {
      return selectionTargetFromFaceHit(faceHit.object);
    }

    let bestFace: RuntimeFace | null = null;
    let bestFaceDistance = Number.POSITIVE_INFINITY;
    for (const entry of runtimeFaces) {
      const projected = projectRuntimeFaceCenter(entry);
      if (!projected) continue;
      const distance = Math.hypot(
        projected.x - (event.clientX - rect.left),
        projected.y - (event.clientY - rect.top),
      );
      if (distance < bestFaceDistance) {
        bestFaceDistance = distance;
        bestFace = entry;
      }
    }
    const facePickRadius = Math.max(24, Math.min(88, Math.min(rect.width, rect.height) * 0.18));
    if (bestFace && bestFaceDistance <= facePickRadius) {
      return selectionTargetFromFaceHit(bestFace.mesh);
    }

    return null;
  }

  function inspectPartFromEvent(event: PointerEvent): string | null {
    if (hideModelWhileBusy || !renderer || !camera || !modelRoot || runtimeMeshes.length === 0) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    const localPoint = {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
    };
    pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);

    const meshHit = raycaster
      .intersectObjects(runtimeMeshes.map((entry) => entry.mesh), true)
      .find((entry) => typeof entry.object.userData.partId === 'string');
    const hitPartId = (meshHit?.object.userData.partId as string | undefined) ?? null;
    if (hitPartId) return hitPartId;

    let nearestPartId: string | null = null;
    let nearestDistance = Number.POSITIVE_INFINITY;
    for (const entry of runtimeMeshes) {
      if (!entry.partId) continue;
      const frame = projectMeshFrame(entry.mesh);
      if (frame) {
        const padding = Math.max(8, Math.min(32, Math.max(frame.width, frame.height) * 0.2));
        if (
          localPoint.x >= frame.left - padding &&
          localPoint.x <= frame.right + padding &&
          localPoint.y >= frame.top - padding &&
          localPoint.y <= frame.bottom + padding
        ) {
          return entry.partId;
        }
      }
      const center = projectMeshPoint(entry.mesh, 'center');
      if (!center) continue;
      const distance = Math.hypot(center.x - localPoint.x, center.y - localPoint.y);
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearestPartId = entry.partId;
      }
    }

    const pickRadius = Math.max(96, Math.min(260, Math.min(rect.width, rect.height) * 0.35));
    return nearestDistance <= pickRadius ? nearestPartId : null;
  }

  function captureSurfaceAnchorFromEvent(event: PointerEvent): CaptureSurfaceAnchorValue | null {
    if (
      !capturePickingMode
      || !captureSourceMeshContentDigest
      || !renderer
      || !camera
      || cropTransformControls?.dragging
      || (cropBoxEnabled && cropTransformControls?.axis)
    ) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const sourceMeshes = runtimeMeshes
      .map((entry) => entry.sourcePickMesh)
      .filter((mesh): mesh is THREE.Mesh<THREE.BufferGeometry, THREE.MeshBasicMaterial> => Boolean(mesh));
    const hit = raycaster.intersectObjects(sourceMeshes, false)[0];
    if (!hit || !(hit.object instanceof THREE.Mesh)) return null;
    try {
      return captureSurfaceAnchorFromIntersection(
        captureSourceMeshContentDigest,
        hit.object.geometry,
        hit.object,
        hit,
      );
    } catch (error) {
      onCaptureSurfaceAnchorError?.(error instanceof Error ? error.message : String(error));
      return null;
    }
  }

  function selectionTargetFromPartId(partId: string): ContextSelectionTarget {
    const existing = selectionTargets.find((target) => target.kind === 'part' && target.partId === partId) ??
      selectionTargets.find((target) => target.partId === partId);
    if (existing) return existing;
    const manifestPart = manifestParts.find((part) => part.partId === partId);
    return {
      targetId: `part:${partId}`,
      aliasIds: [],
      kind: 'part',
      partId,
      label: manifestPart?.label?.trim() || partId,
      editable: manifestPart?.editable ?? true,
      viewerNodeId: null,
      parameterKeys: [],
      primitiveIds: [],
      viewIds: [],
    };
  }

  function handlePointerMove(event: PointerEvent) {
    if (pointerDownAt) {
      const moved = Math.hypot(event.clientX - pointerDownAt.x, event.clientY - pointerDownAt.y);
      if (moved > 4) orbitDraggedSincePointerDown = true;
    }
    if (hideModelWhileBusy) {
      if (hoveredPartId !== null) {
        hoveredPartId = null;
        hoveredTargetId = null;
        applySelectionStyles();
      }
      if (renderer) {
        renderer.domElement.style.cursor = 'progress';
      }
      return;
    }
    if (surfaceTrimActive && capturePickingMode && !isOrbitDragging) {
      const now = performance.now();
      if (now - lastCaptureHoverAt >= 50) {
        lastCaptureHoverAt = now;
        onCaptureSurfaceHover?.(captureSurfaceAnchorFromEvent(event));
      }
    }
    if (!selectionMode && !capturePickingMode) {
      if (hoveredPartId !== null || hoveredTargetId !== null) {
        hoveredPartId = null;
        hoveredTargetId = null;
        applySelectionStyles();
      }
      if (renderer) {
        renderer.domElement.style.cursor = isOrbitDragging ? 'grabbing' : 'default';
      }
      return;
    }
    if (hoveredPartId !== null || hoveredTargetId !== null) {
      hoveredPartId = null;
      hoveredTargetId = null;
      applySelectionStyles();
    }
    if (renderer) {
      renderer.domElement.style.cursor = selectionMode || capturePickingMode ? 'crosshair' : isOrbitDragging ? 'grabbing' : 'default';
    }
  }

  function handlePointerLeave() {
    if (surfaceTrimActive) onCaptureSurfaceHover?.(null);
    hoveredPartId = null;
    hoveredTargetId = null;
    applySelectionStyles();
    if (renderer) {
      renderer.domElement.style.cursor = hideModelWhileBusy ? 'progress' : selectionMode || capturePickingMode ? 'crosshair' : 'default';
    }
  }

  function handlePointerCancel() {
    pointerDownAt = null;
    orbitDraggedSincePointerDown = false;
    componentPlacementPreview?.transform.pointerUp(null);
  }

  function placeComponentOnWorkingPlane(event: PointerEvent) {
    const preview = componentPlacementPreview;
    if (!preview || componentPlacement?.disabled || componentPlacementPositionChosen || !renderer || !camera) return;
    const rect = renderer.domElement.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    pointer.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    camera.updateMatrixWorld();
    raycaster.setFromCamera(pointer, camera);
    const normal = new THREE.Vector3(0, 0, 1).applyQuaternion(preview.frame.quaternion);
    const plane = new THREE.Plane().setFromNormalAndCoplanarPoint(normal, preview.frame.position);
    const hit = raycaster.ray.intersectPlane(plane, new THREE.Vector3());
    if (!hit) return;
    const position = preview.frame.worldToLocal(hit);
    position.z = 0;
    emitComponentPlacementMove(position);
  }

  function handlePointerUp(event: PointerEvent) {
    if (hideModelWhileBusy || !renderer || !camera) return;
    if (isInteractiveViewerControl(event.target)) {
      pointerDownAt = null;
      orbitDraggedSincePointerDown = false;
      return;
    }
    if (orbitDraggedSincePointerDown) {
      pointerDownAt = null;
      orbitDraggedSincePointerDown = false;
      return;
    }
    if (componentPlacementPreview) {
      const start = pointerDownAt;
      pointerDownAt = null;
      if (event.button === 0 && start && shouldHandleViewerClick({
        hideModelWhileBusy,
        pointerDownAt: start,
        current: { x: event.clientX, y: event.clientY },
      })) {
        placeComponentOnWorkingPlane(event);
      }
      return;
    }
    if (!modelRoot || runtimeMeshes.length === 0) return;
    if (capturePickingMode) {
      if (
        !shouldHandleViewerClick({
          hideModelWhileBusy,
          pointerDownAt,
          current: { x: event.clientX, y: event.clientY },
        })
        || cropTransformControls?.dragging
        || (cropBoxEnabled && cropTransformControls?.axis)
      ) {
        pointerDownAt = null;
        return;
      }
      pointerDownAt = null;
      const anchor = captureSurfaceAnchorFromEvent(event);
      if (anchor) onCaptureSurfaceAnchor?.(anchor);
      return;
    }
    if (selectionMode) {
      if (
        !shouldHandleSelectionClick({
          hideModelWhileBusy,
          selectionMode,
          pointerDownAt,
          current: { x: event.clientX, y: event.clientY },
        })
      ) {
        pointerDownAt = null;
        return;
      }
      pointerDownAt = null;
      inspectedPartId = null;
      onSelectTarget?.(selectionTargetFromEvent(event));
      return;
    }
    if (viewerMode === 'measure') {
      pointerDownAt = null;
      return;
    }

    if (!shouldHandleViewerClick({
      hideModelWhileBusy,
      pointerDownAt,
      current: { x: event.clientX, y: event.clientY },
    })) {
      pointerDownAt = null;
      return;
    }
    pointerDownAt = null;
    inspectedPartId = inspectPartFromEvent(event);
    if (inspectedPartId) {
      onSelectTarget?.(selectionTargetFromPartId(inspectedPartId));
    }
    applySelectionStyles();
  }
</script>

<div
  bind:this={viewerHost}
  class="viewer-host"
  data-model-status={modelStatus}
  data-component-placement-ready={componentPlacementReady}
  data-parameter-ghost-ready={parameterGhostReady}
  data-window-drag-ignore
  data-capture-comparison-loaded={captureComparisonLoaded}
  data-capture-reference-visible={captureReferenceVisible}
  data-capture-generated-visible={captureGeneratedVisible}
  data-capture-reference-opacity={captureReferenceOpacity}
  data-capture-generated-opacity={captureGeneratedOpacity}
  data-capture-deviation-visible={captureDeviationVisible}
  data-capture-deviation-point-count={captureDeviationPointCount}
  data-crop-box-enabled={cropBoxEnabled}
  data-surface-trim-active={surfaceTrimActive}
  data-surface-trim-cap-preview={Boolean(surfaceTrimCapPreview?.triangles.length)}
  data-fem-overlay-visible={Boolean(femOverlay)}
  data-fem-mesh-overlay-visible={femOverlayKind === 'mesh'}
>
  {#if femOverlayKind === 'mesh' && !hideModelWhileBusy}
    <div class="fem-legend fem-legend--mesh" data-testid="fem-mesh-legend">
      <strong>TET4 MESH PREVIEW</strong>
      <small>PREVIEW ONLY · EXPORT GEOMETRY UNCHANGED</small>
    </div>
  {/if}
  {#if femLegend && !hideModelWhileBusy}
    <div class="fem-legend" data-testid="fem-result-legend">
      <strong>{femLegend.label}</strong>
      <div class="fem-legend__ramp" aria-hidden="true"></div>
      <div class="fem-legend__range">
        <span>{femLegend.minimum.toPrecision(4)} {femLegend.unit}</span>
        <span>{femLegend.maximum.toPrecision(4)} {femLegend.unit}</span>
      </div>
      <small>PREVIEW ONLY · EXPORT GEOMETRY UNCHANGED</small>
    </div>
  {/if}
  {#if femOverlayError && !hideModelWhileBusy}
    <pre class="fem-legend fem-legend--error" role="alert">{femOverlayError}</pre>
  {/if}
  {#if captureGuidePrimitives && captureGuideProjected.points.length > 0 && !hideModelWhileBusy}
    <div class="capture-guide-overlay" data-testid="capture-guide-overlay">
      <svg class="capture-guide-overlay__geometry" aria-hidden="true">
        {#each captureGuideProjected.planePolygons as polygon (polygon.key)}
          <polygon class="capture-guide-overlay__plane" points={polygon.points} />
        {/each}
        {#each captureGuideProjected.segments as segment (segment.key)}
          <line
            class="capture-guide-overlay__segment"
            data-kind={segment.kind}
            x1={segment.x1}
            y1={segment.y1}
            x2={segment.x2}
            y2={segment.y2}
          />
        {/each}
      </svg>
      {#each captureGuideProjected.points as point (point.landmarkId)}
        <button
          type="button"
          class="capture-guide-overlay__point"
          class:selected={captureSelectedLandmarkId === point.landmarkId}
          data-role={point.role}
          data-landmark-id={point.landmarkId}
          aria-label={`Select guide landmark ${point.ordinal}: ${point.label}`}
          title={`${point.ordinal}. ${point.label} · ${point.role}`}
          style={`left:${point.x}px;top:${point.y}px`}
          onclick={() => onCaptureSelectLandmark?.(point.landmarkId)}
        >{point.ordinal}</button>
      {/each}
      <div class="capture-guide-overlay__scope">{captureGuidePrimitives.evidenceScopeLabel}</div>
      {#if captureGuidePrimitives.inferredRegionLabel}
        <div class="capture-guide-overlay__inferred">{captureGuidePrimitives.inferredRegionLabel}</div>
      {/if}
    </div>
  {/if}
  {#if surfaceTrimActive && (surfaceTrimProjected.points.length > 0 || surfaceTrimProjected.segments.length > 0) && !hideModelWhileBusy}
    <div class="surface-trim-overlay" data-testid="surface-trim-overlay" data-point-count={surfaceTrimProjected.points.length}>
      <svg class="capture-guide-overlay__geometry" aria-hidden="true">
        {#each surfaceTrimProjected.segments as segment (segment.key)}
          <line
            class="surface-trim-overlay__segment"
            x1={segment.x1}
            y1={segment.y1}
            x2={segment.x2}
            y2={segment.y2}
          />
        {/each}
      </svg>
      {#each surfaceTrimProjected.points as point, index (`${index}:${point.x}:${point.y}`)}
        <button
          type="button"
          class="surface-trim-overlay__point"
          class:selected={surfaceTrimSelectedAnchorIndex === index}
          style={`left:${point.x}px;top:${point.y}px`}
          aria-label={`Select surface trim point ${index + 1}`}
          title={`Surface trim point ${index + 1}`}
          onclick={() => onSurfaceTrimPointSelect?.(index)}
        >{index + 1}</button>
      {/each}
      {#if surfaceTrimProjected.keepSeed}
        <span
          class="surface-trim-overlay__seed"
          style={`left:${surfaceTrimProjected.keepSeed.x}px;top:${surfaceTrimProjected.keepSeed.y}px`}
          aria-label="Surface trim retained region seed"
          title="Retained region seed"
        >K</span>
      {/if}
    </div>
  {/if}
  {#if capturePlaneProjected.length > 0 && !hideModelWhileBusy}
    <div class="capture-plane-overlay" data-testid="capture-plane-overlay" data-point-count={capturePlaneProjected.length}>
      <svg class="capture-guide-overlay__geometry" aria-hidden="true">
        <defs>
          <marker id="capture-plane-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
            <path d="M0,0 L8,4 L0,8 Z" class="capture-plane-overlay__arrow-head" />
          </marker>
        </defs>
        {#if capturePlaneProjected.length === 3}
          <polygon
            class="capture-guide-overlay__plane capture-plane-overlay__polygon"
            points={capturePlaneProjected.map(point => `${point.x},${point.y}`).join(' ')}
          />
        {/if}
        {#if capturePlaneNormalProjected}
          <line
            class="capture-plane-overlay__normal"
            x1={capturePlaneNormalProjected.x1}
            y1={capturePlaneNormalProjected.y1}
            x2={capturePlaneNormalProjected.x2}
            y2={capturePlaneNormalProjected.y2}
            marker-end="url(#capture-plane-arrow)"
          />
          <text
            class="capture-plane-overlay__above-label"
            x={capturePlaneNormalProjected.x2 + 8}
            y={capturePlaneNormalProjected.y2 - 6}
          >ABOVE</text>
        {/if}
      </svg>
      {#each capturePlaneProjected as point, index (`${index}:${point.x}:${point.y}`)}
        <span
          class="capture-guide-overlay__point capture-plane-overlay__point"
          style={`left:${point.x}px;top:${point.y}px`}
          aria-label={`Crop plane point ${index + 1}`}
        >{index + 1}</span>
      {/each}
    </div>
  {/if}
  {#if showContextOverlay && overlayVisible && !hideModelWhileBusy}
    <div class="viewer-overlay-layer">
      {#if dimensionFrame && overlayControls.length > 0}
        <div class="viewer-dimension-layer">
          {#if showEditableCallouts}
            <div
              class="viewer-dimension-caption"
              style={`left: ${Math.max(14, dimensionFrame.left)}px; top: ${Math.max(14, dimensionFrame.top - 28)}px;`}
            >
              {overlayPartLabel}
            </div>
          {:else}
            {#each overlayControls.slice(0, 3) as control}
              {@const tone = overlayFieldTone(control.rawField)}
              {@const isVertical = tone === 'z'}
              {#if tone !== 'angle'}
                <div
                  class="viewer-dimension-guide"
                  data-tone={tone}
                  style={
                    isVertical
                      ? `left: ${dimensionFrame.right + 18}px; top: ${dimensionFrame.top}px; height: ${Math.max(32, dimensionFrame.height)}px;`
                      : `left: ${dimensionFrame.left}px; top: ${tone === 'y' ? dimensionFrame.bottom + 18 : Math.max(10, dimensionFrame.top - 20)}px; width: ${Math.max(48, dimensionFrame.width)}px;`
                  }
                >
                  <span class="viewer-dimension-guide__label">{control.label}</span>
                  <span class="viewer-dimension-guide__value">{control.value}</span>
                </div>
              {/if}
            {/each}
          {/if}
        </div>
      {/if}

      {#if measurementOverlay}
        <svg class="viewer-measurement-layer" aria-hidden="true">
          {#each measurementOverlay.lineSegments as segment}
            <line
              class="viewer-measurement-layer__line"
              x1={segment.x1}
              y1={segment.y1}
              x2={segment.x2}
              y2={segment.y2}
            />
          {/each}
        </svg>
        <div
          class="viewer-measurement-badge"
          style={`left: ${measurementOverlay.badgeLeft}px; top: ${measurementOverlay.badgeTop}px;`}
        >
          <span class="viewer-measurement-badge__label">{measurementOverlay.label}</span>
          {#if measurementOverlay.explanation}
            <span class="viewer-measurement-badge__meta">{measurementOverlay.explanation}</span>
          {/if}
        </div>
      {/if}

      {#if showEditableCallouts}
        <div
          class="viewer-part-overlay viewer-part-overlay-callouts"
          style={`left: ${overlayLeft}px; top: ${overlayTop}px;`}
        >
          <div class="viewer-context-hub">
            <label class="viewer-context-hub__search">
              <input
                class="viewer-context-hub__search-input"
                type="text"
                value={searchQuery}
                placeholder="Filter controls..."
                oninput={(event) => onSearchQueryChange?.(getInputValue(event))}
              />
            </label>
          </div>
          {#if overlayAdvisories.length > 0}
            <div class="viewer-context-hub__note">{overlayAdvisories[0].label}</div>
          {/if}

            <div class="viewer-callout-stack">
              {#each overlayControls as control, index}
                {@const field = control.rawField}
                {@const tone = overlayFieldTone(field)}
                <label
                  class="viewer-callout"
                  data-tone={tone}
                  onmouseenter={() => setFocusedControl(control.primitiveId, field?.key ?? null)}
                  onmouseleave={clearFocusedControl}
                  onfocusin={() => setFocusedControl(control.primitiveId, field?.key ?? null)}
                  onfocusout={clearFocusedControl}
                >
                  <span class="viewer-callout__label">{control.label}</span>
                {#if field?.type === 'range'}
                  {@const range = getRangeProps(field, control.value)}
                  <div class="viewer-callout__row viewer-callout__row-range">
                    <span class="viewer-overlay-arrow viewer-overlay-arrow-left" aria-hidden="true"></span>
                    <input
                      class="viewer-overlay-range"
                      type="range"
                      min={range.min}
                      max={range.max}
                      step={range.step}
                      value={asNumber(control.value, range.min)}
                      oninput={(event) => updateOverlayParam(control.primitiveId, parseFloat(getInputValue(event)))}
                    />
                    <span class="viewer-overlay-arrow viewer-overlay-arrow-right" aria-hidden="true"></span>
                    <input
                      class="viewer-overlay-input viewer-overlay-readout"
                      type="number"
                      min={range.min}
                      max={range.max}
                      step={range.step}
                      value={asNumber(control.value, range.min)}
                      oninput={(event) => updateOverlayParam(control.primitiveId, parseFloat(getInputValue(event)))}
                    />
                  </div>
                {:else if field?.type === 'number'}
                  <div class="viewer-callout__row">
                    <input
                      class="viewer-overlay-input"
                      type="number"
                      value={asNumber(control.value, 0)}
                      oninput={(event) => updateOverlayParam(control.primitiveId, parseFloat(getInputValue(event)))}
                    />
                  </div>
                {:else if field?.type === 'select'}
                  <div class="viewer-callout__row">
                    <select
                      class="viewer-overlay-input"
                      value={getSelectValue(control.value) ?? ''}
                      onchange={(event) => updateOverlayParam(control.primitiveId, getInputValue(event))}
                    >
                      {#each field.options || [] as option}
                        <option value={option.value}>{option.label}</option>
                      {/each}
                    </select>
                  </div>
                {:else if field?.type === 'checkbox'}
                  <div class="viewer-callout__row">
                    <label class="viewer-overlay-toggle">
                      <input
                        type="checkbox"
                        checked={Boolean(control.value)}
                        onchange={(event) => updateOverlayParam(control.primitiveId, getInputChecked(event))}
                      />
                      <span>{control.value ? 'ON' : 'OFF'}</span>
                    </label>
                  </div>
                {:else if field?.type === 'image'}
                  <div class="viewer-callout__row">
                    <button
                      class="viewer-overlay-file-btn"
                      type="button"
                      onclick={() => pickOverlayImage(control.primitiveId)}
                    >
                      {control.value ? String(control.value).split(/[/\\]/).pop() : 'Select Image...'}
                    </button>
                  </div>
                {/if}
              </label>
            {/each}
          </div>
        </div>
      {:else if showPartOverlay}
        <div
          class="viewer-part-overlay"
          class:viewer-part-overlay-docked={overlayFallback}
          class:viewer-part-overlay-readonly={!overlayPartEditable}
          style={!overlayFallback ? `left: ${overlayLeft}px; top: ${overlayTop}px;` : undefined}
        >
          <label class="viewer-part-overlay__search">
            <input
              class="viewer-part-overlay__search-input"
              type="text"
              value={searchQuery}
              placeholder="Filter controls..."
              oninput={(event) => onSearchQueryChange?.(getInputValue(event))}
            />
          </label>
          {#if overlayAdvisories.length > 0}
            <div class="viewer-part-overlay__advisory">{overlayAdvisories[0].label}</div>
          {/if}

          {#if overlayControls.length > 0}
            <div class="viewer-part-overlay__controls">
              {#each overlayControls as control}
                {@const field = control.rawField}
                <label
                  class="viewer-overlay-control"
                  onmouseenter={() => setFocusedControl(control.primitiveId, field?.key ?? null)}
                  onmouseleave={clearFocusedControl}
                  onfocusin={() => setFocusedControl(control.primitiveId, field?.key ?? null)}
                  onfocusout={clearFocusedControl}
                >
                  <span class="viewer-overlay-control__label">{control.label}</span>
                  {#if field?.type === 'range'}
                    {@const range = getRangeProps(field, control.value)}
                    <div class="viewer-overlay-control__row viewer-overlay-control__row-range">
                      <span class="viewer-overlay-arrow viewer-overlay-arrow-left" aria-hidden="true"></span>
                      <input
                        class="viewer-overlay-range"
                        type="range"
                        min={range.min}
                        max={range.max}
                        step={range.step}
                        value={asNumber(control.value, range.min)}
                        oninput={(event) => updateOverlayParam(control.primitiveId, parseFloat(getInputValue(event)))}
                      />
                      <span class="viewer-overlay-arrow viewer-overlay-arrow-right" aria-hidden="true"></span>
                      <input
                        class="viewer-overlay-input viewer-overlay-readout"
                        type="number"
                        min={range.min}
                        max={range.max}
                        step={range.step}
                        value={asNumber(control.value, range.min)}
                        oninput={(event) => updateOverlayParam(control.primitiveId, parseFloat(getInputValue(event)))}
                      />
                    </div>
                  {:else if field?.type === 'number'}
                    <div class="viewer-overlay-control__row">
                      <input
                        class="viewer-overlay-input"
                        type="number"
                        value={asNumber(control.value, 0)}
                        oninput={(event) => updateOverlayParam(control.primitiveId, parseFloat(getInputValue(event)))}
                      />
                    </div>
                  {:else if field?.type === 'select'}
                    <div class="viewer-overlay-control__row">
                      <select
                        class="viewer-overlay-input"
                        value={getSelectValue(control.value) ?? ''}
                        onchange={(event) => updateOverlayParam(control.primitiveId, getInputValue(event))}
                      >
                        {#each field.options || [] as option}
                          <option value={option.value}>{option.label}</option>
                        {/each}
                      </select>
                    </div>
                  {:else if field?.type === 'checkbox'}
                    <div class="viewer-overlay-control__row">
                      <label class="viewer-overlay-toggle">
                        <input
                          type="checkbox"
                          checked={Boolean(control.value)}
                          onchange={(event) => updateOverlayParam(control.primitiveId, getInputChecked(event))}
                        />
                        <span>{control.value ? 'ON' : 'OFF'}</span>
                      </label>
                    </div>
                  {:else if field?.type === 'image'}
                    <div class="viewer-overlay-control__row">
                      <button
                        class="viewer-overlay-file-btn"
                        type="button"
                        onclick={() => pickOverlayImage(control.primitiveId)}
                      >
                        {control.value ? String(control.value).split(/[/\\]/).pop() : 'Select Image...'}
                      </button>
                    </div>
                  {/if}
                </label>
              {/each}
            </div>
          {:else}
            <div class="viewer-part-overlay__empty">
              {overlayPartEditable
                ? overlayPreviewOnly
                  ? 'Preview-ready part.'
                  : 'No bound controls on this part yet.'
                : 'Inspect-only part.'}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if hideModelWhileBusy}
    <ViewportTransmutation phase={busyPhase} text={busyText} />
  {/if}
</div>

<style>
  .viewer-host {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    transition: filter 0.5s ease-in-out;
  }
  .fem-legend { position: absolute; right: 14px; bottom: 72px; z-index: 12; width: 190px; padding: 9px; overflow: hidden; border: 1px solid var(--secondary); border-radius: 0; background: color-mix(in srgb, var(--bg-100) 92%, transparent); color: var(--text); font-family: var(--font-mono); pointer-events: none; }
  .fem-legend strong { display: block; margin-bottom: 6px; color: var(--primary); font-size: .64rem; letter-spacing: .08em; }
  .fem-legend__ramp { height: 9px; border: 1px solid var(--bg-300); background: linear-gradient(90deg, #143f8c, #14adb8 35%, #e0ba2e 65%, #c71f14); }
  .fem-legend__range { display: flex; justify-content: space-between; gap: 8px; margin-top: 4px; font-size: .56rem; }
  .fem-legend small { display: block; margin-top: 6px; color: var(--text-dim); font-size: .5rem; }
  .fem-legend--error { border-color: var(--danger); color: var(--danger); font-size: .6rem; white-space: pre-wrap; }

  .capture-guide-overlay {
    position: absolute;
    inset: 0;
    z-index: 5;
    overflow: hidden;
    pointer-events: none;
  }

  .surface-trim-overlay {
    position: absolute;
    inset: 0;
    z-index: 7;
    overflow: hidden;
    pointer-events: none;
  }

  .surface-trim-overlay__segment {
    stroke: var(--secondary);
    stroke-width: 2.5;
    vector-effect: non-scaling-stroke;
  }

  .surface-trim-overlay__point,
  .surface-trim-overlay__seed {
    position: absolute;
    display: grid;
    width: 24px;
    height: 24px;
    place-content: center;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--secondary);
    border-radius: 0;
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--secondary);
    font: 800 0.65rem/1 var(--font-mono);
    transform: translate(-50%, -50%);
  }

  .surface-trim-overlay__point { pointer-events: auto; }
  .surface-trim-overlay__point.selected {
    background: var(--secondary);
    color: var(--bg-100);
  }
  .surface-trim-overlay__seed {
    border-color: var(--primary);
    background: var(--primary);
    color: var(--bg-100);
    pointer-events: none;
  }

  .capture-plane-overlay {
    position: absolute;
    inset: 0;
    z-index: 6;
    overflow: hidden;
    pointer-events: none;
  }

  .capture-plane-overlay__point {
    display: grid;
    place-content: center;
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .capture-plane-overlay__polygon {
    fill: color-mix(in srgb, var(--primary) 22%, transparent);
    stroke: var(--primary);
  }

  .capture-plane-overlay__normal {
    stroke: var(--secondary);
    stroke-width: 2;
  }

  .capture-plane-overlay__arrow-head { fill: var(--secondary); }

  .capture-plane-overlay__above-label {
    fill: var(--secondary);
    font: 700 11px/1 var(--font-mono);
  }

  .capture-guide-overlay__geometry {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .capture-guide-overlay__plane {
    fill: color-mix(in srgb, var(--secondary) 12%, transparent);
    stroke: color-mix(in srgb, var(--secondary) 66%, transparent);
    stroke-width: 1.5;
    stroke-dasharray: 7 5;
  }

  .capture-guide-overlay__segment {
    stroke: var(--primary);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }

  .capture-guide-overlay__segment[data-kind="axis"] {
    stroke: var(--secondary);
    stroke-dasharray: 9 4;
  }

  .capture-guide-overlay__point {
    position: absolute;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--primary);
    border-radius: 0;
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--primary);
    font: 800 0.65rem/1 var(--font-mono);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .capture-guide-overlay__point.selected {
    border-color: var(--secondary);
    background: var(--secondary);
    color: var(--bg-100);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--secondary) 22%, transparent);
  }

  .capture-guide-overlay__point[data-role="ignoredDamagedRegion"] {
    border-color: var(--danger);
    color: var(--danger);
  }

  .capture-guide-overlay__scope,
  .capture-guide-overlay__inferred {
    position: absolute;
    bottom: 12px;
    padding: 5px 7px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    font: 700 0.62rem/1 var(--font-mono);
    letter-spacing: 0.08em;
  }

  .capture-guide-overlay__scope {
    left: 12px;
    color: var(--primary);
  }

  .capture-guide-overlay__inferred {
    right: 12px;
    color: var(--secondary);
  }


  .viewer-overlay-layer {
    position: absolute;
    inset: 0;
    z-index: 4;
    pointer-events: none;
    overflow: hidden;
  }

  .viewer-dimension-layer {
    position: absolute;
    inset: 0;
    pointer-events: none;
    overflow: hidden;
  }

  .viewer-measurement-layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    overflow: hidden;
  }

  .viewer-measurement-layer__line {
    stroke: color-mix(in srgb, var(--secondary) 58%, var(--green) 42%);
    stroke-width: 1.5;
    stroke-linecap: square;
    stroke-dasharray: 8 5;
    filter: drop-shadow(0 0 6px color-mix(in srgb, var(--green) 22%, transparent));
  }

  .viewer-measurement-badge {
    position: absolute;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 124px;
    max-width: min(220px, 26vw);
    padding: 7px 9px;
    border: 1px solid color-mix(in srgb, var(--secondary) 46%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--bg-100) 94%, #000 6%) 0%,
        color-mix(in srgb, var(--bg-200) 97%, #000 3%) 100%
      );
    box-shadow:
      0 8px 18px rgba(0, 0, 0, 0.38),
      inset 0 0 0 1px color-mix(in srgb, #000 34%, transparent);
    pointer-events: none;
    transform: translate(-50%, calc(-100% - 14px));
    overflow: hidden;
  }

  .viewer-measurement-badge__label {
    color: var(--secondary);
    font-size: 0.62rem;
    font-weight: 800;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .viewer-measurement-badge__meta {
    color: var(--text-dim);
    font-size: 0.58rem;
    line-height: 1.35;
    letter-spacing: 0.04em;
  }

  .viewer-dimension-guide {
    --guide-tone: color-mix(in srgb, var(--green) 78%, var(--primary) 22%);
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-width: 78px;
    padding: 0 12px;
    color: var(--text);
    font-size: 0.6rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .viewer-dimension-caption {
    position: absolute;
    padding: 3px 7px;
    border: 1px solid color-mix(in srgb, var(--secondary) 40%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 92%, #000 8%);
    color: var(--text);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 30%, transparent);
  }

  .viewer-dimension-guide::before,
  .viewer-dimension-guide::after {
    content: '';
    position: absolute;
    background: color-mix(in srgb, var(--guide-tone) 72%, var(--bg-300));
  }

  .viewer-dimension-guide__label,
  .viewer-dimension-guide__value {
    position: relative;
    z-index: 1;
    padding: 2px 6px;
    border: 1px solid color-mix(in srgb, var(--guide-tone) 40%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 92%, #000 8%);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 30%, transparent);
  }

  .viewer-dimension-guide__value {
    color: var(--green);
  }

  .viewer-dimension-guide[data-tone="x"],
  .viewer-dimension-guide[data-tone="y"] {
    height: 1px;
  }

  .viewer-dimension-guide[data-tone="x"]::before,
  .viewer-dimension-guide[data-tone="y"]::before {
    left: 0;
    right: 0;
    top: 0;
    height: 1px;
  }

  .viewer-dimension-guide[data-tone="x"]::after,
  .viewer-dimension-guide[data-tone="y"]::after {
    left: 0;
    right: 0;
    top: -5px;
    height: 11px;
    background:
      linear-gradient(90deg, color-mix(in srgb, var(--guide-tone) 72%, var(--bg-300)) 0 1px, transparent 1px calc(100% - 1px), color-mix(in srgb, var(--guide-tone) 72%, var(--bg-300)) calc(100% - 1px) 100%);
  }

  .viewer-dimension-guide[data-tone="z"] {
    width: 1px;
    flex-direction: column;
    min-width: 0;
    padding: 12px 0;
  }

  .viewer-dimension-guide[data-tone="z"]::before {
    top: 0;
    bottom: 0;
    left: 0;
    width: 1px;
  }

  .viewer-dimension-guide[data-tone="z"]::after {
    top: 0;
    bottom: 0;
    left: -5px;
    width: 11px;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--guide-tone) 72%, var(--bg-300)) 0 1px, transparent 1px calc(100% - 1px), color-mix(in srgb, var(--guide-tone) 72%, var(--bg-300)) calc(100% - 1px) 100%);
  }

  .viewer-dimension-guide[data-tone="y"] {
    --guide-tone: color-mix(in srgb, var(--secondary) 62%, var(--green) 38%);
  }

  .viewer-dimension-guide[data-tone="z"] {
    --guide-tone: color-mix(in srgb, var(--text) 44%, var(--green) 56%);
  }

  .viewer-part-overlay {
    position: absolute;
    min-width: 220px;
    max-width: min(320px, 48vw);
    padding: 10px;
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--bg-100) 92%, #000 8%) 0%,
        color-mix(in srgb, var(--bg-200) 96%, #000 4%) 100%
      );
    box-shadow:
      0 10px 24px rgba(0, 0, 0, 0.45),
      inset 0 0 0 1px color-mix(in srgb, #000 35%, transparent);
    pointer-events: auto;
    transform: translate(-50%, calc(-100% - 18px));
    overflow: hidden;
  }

  .viewer-part-overlay-callouts {
    min-width: 0;
    max-width: none;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
    pointer-events: none;
    transform: translate(-50%, 0);
    overflow: visible;
  }

  .viewer-part-overlay-callouts::after {
    display: none;
  }

  .viewer-context-hub {
    position: absolute;
    left: 0;
    top: 0;
    min-width: 240px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--bg-100) 92%, #000 8%) 0%,
        color-mix(in srgb, var(--bg-200) 96%, #000 4%) 100%
      );
    box-shadow:
      0 10px 24px rgba(0, 0, 0, 0.45),
      inset 0 0 0 1px color-mix(in srgb, #000 35%, transparent);
    transform: translate(-50%, calc(-100% - 18px));
    pointer-events: auto;
    white-space: nowrap;
  }

  .viewer-context-hub::after {
    content: '';
    position: absolute;
    left: 50%;
    bottom: -14px;
    width: 1px;
    height: 14px;
    background: color-mix(in srgb, var(--secondary) 60%, var(--bg-300));
    transform: translateX(-50%);
  }

  .viewer-context-hub__search,
  .viewer-part-overlay__search {
    display: block;
  }

  .viewer-context-hub__search-input,
  .viewer-part-overlay__search-input {
    width: 100%;
    min-height: 32px;
    padding: 7px 10px;
    border: 1px solid color-mix(in srgb, var(--primary) 40%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 94%, #000 6%);
    color: var(--text);
    font-family: 'IBM Plex Mono', monospace;
    font-size: 0.72rem;
    outline: none;
  }

  .viewer-context-hub__search-input:focus,
  .viewer-part-overlay__search-input:focus {
    border-color: color-mix(in srgb, var(--primary) 68%, var(--secondary) 32%);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--primary) 24%, transparent);
  }

  .viewer-context-hub__note,
  .viewer-part-overlay__advisory {
    margin-top: 8px;
    padding: 6px 8px;
    border: 1px solid color-mix(in srgb, var(--green) 34%, var(--bg-300));
    background: color-mix(in srgb, var(--green) 8%, var(--bg-100));
    color: var(--text-dim);
    font-size: 0.6rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    pointer-events: auto;
  }

  .viewer-callout-stack {
    position: absolute;
    left: 34px;
    top: calc(-100% + 44px);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: min(52vh, 320px);
    overflow: auto;
    padding-right: 4px;
    pointer-events: none;
  }

  .viewer-callout {
    --callout-tone: color-mix(in srgb, var(--green) 78%, var(--primary) 22%);
    position: relative;
    min-width: 210px;
    max-width: min(280px, 38vw);
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--callout-tone) 42%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--bg-100) 92%, #000 8%) 0%,
        color-mix(in srgb, var(--bg-200) 96%, #000 4%) 100%
      );
    box-shadow:
      0 10px 24px rgba(0, 0, 0, 0.42),
      inset 0 0 0 1px color-mix(in srgb, #000 35%, transparent);
    pointer-events: auto;
    overflow: visible;
  }

  .viewer-callout::before {
    content: '';
    position: absolute;
    left: -34px;
    top: 50%;
    width: 34px;
    height: 1px;
    background: color-mix(in srgb, var(--callout-tone) 58%, var(--bg-300));
    transform: translateY(-50%);
  }

  .viewer-callout::after {
    content: '';
    position: absolute;
    left: -6px;
    top: 50%;
    width: 6px;
    height: 6px;
    border-left: 1px solid color-mix(in srgb, var(--callout-tone) 58%, var(--bg-300));
    border-bottom: 1px solid color-mix(in srgb, var(--callout-tone) 58%, var(--bg-300));
    transform: translateY(-50%) rotate(45deg);
    background: var(--bg-200);
  }

  .viewer-callout[data-tone="y"] {
    --callout-tone: color-mix(in srgb, var(--secondary) 62%, var(--green) 38%);
  }

  .viewer-callout[data-tone="z"] {
    --callout-tone: color-mix(in srgb, var(--text) 44%, var(--green) 56%);
  }

  .viewer-callout[data-tone="angle"] {
    --callout-tone: color-mix(in srgb, var(--secondary) 78%, white 22%);
  }

  .viewer-callout__label {
    color: var(--text-dim);
    font-size: 0.56rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .viewer-callout__row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .viewer-callout__row-range {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
  }

  .viewer-part-overlay::after {
    content: '';
    position: absolute;
    left: 50%;
    bottom: -8px;
    width: 1px;
    height: 16px;
    background: color-mix(in srgb, var(--secondary) 60%, var(--bg-300));
    transform: translateX(-50%);
  }

  .viewer-part-overlay-docked {
    left: 22px;
    bottom: 22px;
    top: auto;
    transform: none;
  }

  .viewer-part-overlay-docked::after {
    display: none;
  }

  .viewer-part-overlay-readonly {
    border-color: color-mix(in srgb, var(--text-dim) 42%, var(--bg-300));
  }

  .viewer-part-overlay__controls {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: min(58vh, 420px);
    overflow: auto;
    padding-right: 4px;
  }

  .viewer-overlay-control {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .viewer-overlay-control__label {
    color: var(--text-dim);
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .viewer-overlay-control__row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .viewer-overlay-control__row-range {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
  }

  .viewer-overlay-arrow {
    width: 10px;
    height: 10px;
    background: var(--callout-tone, color-mix(in srgb, var(--green) 78%, var(--primary) 22%));
    filter: drop-shadow(0 0 4px color-mix(in srgb, var(--callout-tone, var(--green)) 36%, transparent));
  }

  .viewer-overlay-arrow-left {
    clip-path: polygon(100% 0, 0 50%, 100% 100%);
    -webkit-clip-path: polygon(100% 0, 0 50%, 100% 100%);
  }

  .viewer-overlay-arrow-right {
    clip-path: polygon(0 0, 100% 50%, 0 100%);
    -webkit-clip-path: polygon(0 0, 100% 50%, 0 100%);
  }

  .viewer-overlay-range {
    width: 100%;
    appearance: none;
    height: 6px;
    background:
      linear-gradient(
        90deg,
        color-mix(in srgb, var(--callout-tone, var(--green)) 42%, var(--bg-300)) 0%,
        color-mix(in srgb, var(--callout-tone, var(--green)) 18%, var(--bg-300)) 100%
      );
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 35%, transparent);
  }

  .viewer-overlay-range::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    border: 1px solid color-mix(in srgb, #fff 18%, #000 82%);
    background: var(--callout-tone, color-mix(in srgb, var(--green) 78%, var(--primary) 22%));
    box-shadow: 0 0 10px color-mix(in srgb, var(--callout-tone, var(--green)) 28%, transparent);
    cursor: pointer;
  }

  .viewer-overlay-range::-moz-range-thumb {
    width: 14px;
    height: 14px;
    border: 1px solid color-mix(in srgb, #fff 18%, #000 82%);
    background: var(--callout-tone, color-mix(in srgb, var(--green) 78%, var(--primary) 22%));
    box-shadow: 0 0 10px color-mix(in srgb, var(--callout-tone, var(--green)) 28%, transparent);
    cursor: pointer;
  }

  .viewer-overlay-readout,
  .viewer-overlay-input {
    padding: 4px 6px;
    border: 1px solid color-mix(in srgb, var(--secondary) 36%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 90%, #000 10%);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.7rem;
  }

  .viewer-overlay-readout {
    min-width: 46px;
    text-align: right;
    color: var(--callout-tone, color-mix(in srgb, var(--green) 78%, var(--primary) 22%));
  }

  .viewer-overlay-input {
    width: 100%;
  }

  .viewer-overlay-file-btn {
    width: 100%;
    min-height: 34px;
    padding: 4px 6px;
    border: 1px solid color-mix(in srgb, var(--secondary) 36%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 90%, #000 10%);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.7rem;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .viewer-overlay-file-btn:hover,
  .viewer-overlay-file-btn:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--primary) 68%, var(--secondary) 32%);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--primary) 24%, transparent);
  }

  .viewer-overlay-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
    font-size: 0.68rem;
  }

  .viewer-part-overlay__empty {
    color: var(--text-dim);
    font-size: 0.68rem;
  }

  @media (max-width: 900px) {
    .viewer-part-overlay-callouts {
      display: none;
    }
  }

</style>
