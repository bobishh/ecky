<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onDestroy, onMount, untrack } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
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
    ViewportCameraState,
  } from './types/domain';
  import {
    resolveViewerNodeTarget,
    shouldDisplayViewportControlList,
    type MeasurementControlFocus,
    type ResolvedMeasurementCallout,
    type ContextSelectionTarget,
  } from './modelRuntime/contextualEditing';
  import type { ImportedPreviewTransform } from './modelRuntime/importedRuntime';
  import type { MaterializedSemanticControl } from './modelRuntime/semanticControls';
  import { cycleTopologyMode, topologyModeLabel, type TopologyMode } from './viewerDisplayMode';
  import { resolveViewerTone, type ViewerTone } from './viewerLook';
  import { resolveViewerAssetUrl } from './viewerAssetUrl';

  type ViewportBusyPhase = 'generating' | 'repairing' | 'rendering' | 'committing' | null;

  let {
    modelKey = null,
    stlUrl = null,
    viewerAssets = [],
    manifestParts = [],
    edgeTargets = [],
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
    isGenerating = false,
    hideModelWhileBusy = false,
    busyPhase = null,
    busyText = null,
    topologyMode = 'mesh',
    persistedCameraState = null,
    onSearchQueryChange,
    onSelectTarget,
    onOverlayChange,
    onControlFocusChange,
    onCameraStateChange,
    onModelLoaded,
    onModelLoadError,
  }: {
    modelKey?: string | null;
    stlUrl?: string | null;
    viewerAssets?: ViewerAsset[];
    manifestParts?: PartBinding[];
    edgeTargets?: ViewerEdgeTarget[];
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
    isGenerating?: boolean;
    hideModelWhileBusy?: boolean;
    busyPhase?: ViewportBusyPhase;
    busyText?: string | null;
    topologyMode?: TopologyMode;
    persistedCameraState?: ViewportCameraState | null;
    onSearchQueryChange?: (query: string) => void;
    onSelectTarget?: (target: ContextSelectionTarget | null) => void;
    onOverlayChange?: (primitiveId: string, value: ParamValue) => Promise<void> | void;
    onControlFocusChange?: (focus: MeasurementControlFocus | null) => void;
    onCameraStateChange?: (camera: ViewportCameraState) => void;
    onModelLoaded?: () => void;
    onModelLoadError?: (message: string) => void;
  } = $props();

  type RuntimeMesh = {
    partId: string | null;
    baseBounds: THREE.Box3 | null;
    outline: THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial> | null;
    mesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>;
    topology: THREE.LineSegments<THREE.WireframeGeometry, THREE.LineBasicMaterial> | null;
    tone: ViewerTone;
  };

  type RuntimeEdge = {
    targetId: string;
    durableTargetId?: string | null;
    canonicalTargetId?: string | null;
    aliasIds: string[];
    partId: string;
    line: THREE.Line<THREE.BufferGeometry, THREE.LineBasicMaterial>;
  };

  let viewerHost: HTMLDivElement;
  let scene: THREE.Scene | null = null;
  let camera: THREE.PerspectiveCamera | null = null;
  let renderer: THREE.WebGLRenderer | null = null;
  let controls: OrbitControls | null = null;
  let modelRoot: THREE.Group | null = null;
  let runtimeMeshes: RuntimeMesh[] = [];
  let runtimeEdges: RuntimeEdge[] = [];
  let animationFrameId: number | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let loadToken = 0;
  let overlayLeft = $state(24);
  let overlayTop = $state(24);
  let overlayVisible = $state(false);
  let overlayFallback = $state(true);
  let hoveredPartId = $state<string | null>(null);
  let hoveredTargetId = $state<string | null>(null);
  let dimensionFrame = $state<{ bottom: number; height: number; left: number; right: number; top: number; width: number } | null>(null);
  let measurementOverlay = $state<{
    badgeLeft: number;
    badgeTop: number;
    lineSegments: Array<{ x1: number; y1: number; x2: number; y2: number }>;
    label: string;
    explanation: string | null;
  } | null>(null);
  const viewerAssetSignature = $derived.by(() =>
    viewerAssets.map((asset) => `${asset.partId}:${asset.nodeId}:${asset.path}`).join('|'),
  );
  const manifestPartSignature = $derived.by(() =>
    manifestParts.map((part) => `${part.partId}:${part.label}:${part.kind}:${part.semanticRole ?? ''}`).join('|'),
  );
  const modelLoadSignature = $derived.by(
    () => `${modelKey ?? ''}::${stlUrl ?? ''}::${viewerAssetSignature}::${manifestPartSignature}`,
  );
  const showEditableCallouts = $derived.by(
    () => !hideModelWhileBusy && !overlayFallback && overlayPartEditable && overlayControls.length > 0,
  );
  const showViewportControlList = $derived.by(
    () => shouldDisplayViewportControlList(selectedTarget),
  );

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let pointerDownAt: { x: number; y: number } | null = null;

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
    updateOverlayAnchor();
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

  export function getCameraState(): ViewportCameraState | null {
    return currentCameraState();
  }

  export function setCameraState(nextState: ViewportCameraState | null = null) {
    applyCameraState(nextState);
  }

  export function captureScreenshotDetails(
    overlayCanvas: HTMLCanvasElement | null = null,
  ): { dataUrl: string; width: number; height: number; camera: ViewportCameraState } | null {
    if (!renderer || !scene || !camera) return null;
    renderer.render(scene, camera);
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
      renderer.render(scene, camera);
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
          filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
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
    if (renderer) {
      renderer.domElement.removeEventListener('pointerdown', handlePointerDown);
      renderer.domElement.removeEventListener('pointermove', handlePointerMove);
      renderer.domElement.removeEventListener('pointerleave', handlePointerLeave);
      renderer.domElement.removeEventListener('pointerup', handlePointerUp);
    }
    controls?.removeEventListener?.('change', emitCameraStateChange);
    disposeModel();
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
    if (!modelRoot) return;
    attachEdgeTargets(modelRoot);
    applyPreviewTransforms();
    applySelectionStyles();
    updateOverlayAnchor();
  });

  $effect(() => {
    applySelectionStyles();
    updateOverlayAnchor();
  });

  $effect(() => {
    applyPreviewTransforms();
    updateOverlayAnchor();
  });

  $effect(() => {
    void outlineEnabled;
    void topologyMode;
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

  function setupViewer() {
    if (renderer) return;
    scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0b0f1a);

    const { width, height } = hostSize();
    camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 2000);
    camera.position.set(140, 120, 140);

    renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.08;
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height);
    viewerHost.appendChild(renderer.domElement);
    renderer.domElement.addEventListener('pointerdown', handlePointerDown);
    renderer.domElement.addEventListener('pointermove', handlePointerMove);
    renderer.domElement.addEventListener('pointerleave', handlePointerLeave);
    renderer.domElement.addEventListener('pointerup', handlePointerUp);

    controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.autoRotate = false;
    controls.autoRotateSpeed = 0;
    controls.addEventListener('change', emitCameraStateChange);

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

    animate();
  }

  function animate() {
    if (!controls || !renderer || !scene || !camera) return;
    animationFrameId = requestAnimationFrame(animate);
    controls.update();
    updateOverlayAnchor();
    renderer.render(scene, camera);
  }

  function hostSize() {
    return {
      width: Math.max(1, viewerHost?.clientWidth ?? 1),
      height: Math.max(1, viewerHost?.clientHeight ?? 1),
    };
  }

  function onResize() {
    if (!viewerHost || !camera || !renderer) return;
    const { width, height } = hostSize();
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height);
    updateOverlayAnchor();
  }

  async function loadCurrentModel() {
    if (!scene || !camera) return;
    const token = ++loadToken;

    if (viewerAssets.length > 0) {
      await loadMultipartAssets(token, viewerAssets);
      return;
    }

    if (stlUrl) {
      await loadSingleStl(token, stlUrl);
      return;
    }

    disposeModel();
  }

  async function loadMultipartAssets(token: number, assets: ViewerAsset[]) {
    if (!scene || !camera) return;
    const loader = new STLLoader();
    const nextRoot = new THREE.Group();
    nextRoot.rotation.x = -Math.PI / 2;
    const nextMeshes: RuntimeMesh[] = [];

    try {
      for (const asset of assets) {
        const geometry = await loadStlGeometry(loader, asset.path);
        if (token !== loadToken) {
          geometry.dispose();
          return;
        }

        geometry.computeVertexNormals();
        geometry.computeBoundingBox();
        const tone = resolveViewerTone(asset.partId, manifestParts);
        const material = createMaterial(tone, asset.partId === selectedPartId);
        const mesh = new THREE.Mesh(geometry, material);
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
        nextRoot.add(mesh);
        nextMeshes.push({
          partId: asset.partId,
          baseBounds: geometry.boundingBox?.clone() ?? null,
          outline,
          mesh,
          topology,
          tone,
        });
      }

      if (token !== loadToken) {
        disposeDetachedGroup(nextRoot);
        return;
      }

      disposeModel();
      modelRoot = nextRoot;
      runtimeMeshes = nextMeshes;
      applyPreviewTransforms();
      scene.add(modelRoot);
      frameModel(modelRoot);
      applyCameraState(persistedCameraState);
      applySelectionStyles();
      updateOverlayAnchor();
      emitCameraStateChange();
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

      geometry.computeVertexNormals();
      geometry.computeBoundingBox();
      const tone = resolveViewerTone(null, manifestParts);
      const material = createMaterial(tone, false);
      const mesh = new THREE.Mesh(geometry, material);
      const outline = createOutline(geometry, tone, false);
      const topology = createTopologyOverlay(geometry, tone);
      if (outline) {
        mesh.add(outline);
      }
      if (topology) {
        mesh.add(topology);
      }
      nextRoot.add(mesh);

      disposeModel();
      modelRoot = nextRoot;
      runtimeMeshes = [{ partId: null, baseBounds: geometry.boundingBox?.clone() ?? null, outline, mesh, topology, tone }];
      applyPreviewTransforms();
      scene.add(modelRoot);
      frameModel(modelRoot);
      applyCameraState(persistedCameraState);
      updateOverlayAnchor();
      emitCameraStateChange();
      await notifyModelLoaded(token);
    } catch (error) {
      console.error('Failed to load STL:', error);
      disposeDetachedGroup(nextRoot);
      notifyModelLoadError(token, 'Failed to load STL', error);
    }
  }

  function frameModel(object: THREE.Object3D) {
    if (!scene || !camera || !controls) return;
    object.updateMatrixWorld(true);
    const box = new THREE.Box3().setFromObject(object);
    if (box.isEmpty()) return;

    const center = new THREE.Vector3();
    box.getCenter(center);
    object.position.x -= center.x;
    object.position.z -= center.z;
    object.position.y -= box.min.y;
    object.updateMatrixWorld(true);

    const reframed = new THREE.Box3().setFromObject(object);
    const size = new THREE.Vector3();
    reframed.getSize(size);
    const maxDim = Math.max(size.x, size.y, size.z, 1);

    camera.position.set(maxDim * 1.3, maxDim * 1.1, maxDim * 1.3);
    controls.target.set(0, maxDim * 0.35, 0);
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
    return new THREE.LineSegments(
      outlineGeometry,
      new THREE.LineBasicMaterial({
        color: isSelected ? 0xe5ca88 : tone.edge,
        transparent: true,
        opacity: isSelected ? 0.95 : 0.26,
      }),
    );
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
      opacity: isSelected ? 1 : isHovered ? 0.95 : 0.46,
    });
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

  function attachEdgeTargets(root: THREE.Group) {
    disposeRuntimeEdges(root);
    if (edgeTargets.length === 0) return;

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

  function applySelectionStyles() {
    const measurementPartIds = new Set(activeMeasurementCallout?.partIds || []);
    const measurementTargetIds = new Set(activeMeasurementCallout?.targetIds || []);

    for (const entry of runtimeMeshes) {
      const isSelected = !!selectedPartId && entry.partId === selectedPartId;
      const isHovered = !isSelected && !!hoveredPartId && entry.partId === hoveredPartId;
      const isMeasured =
        !isSelected && !isHovered && !!entry.partId && measurementPartIds.has(entry.partId);
      entry.mesh.material.color.setHex(
        isSelected ? 0xe5ca88 : isHovered ? entry.tone.hoverColor : isMeasured ? entry.tone.measuredColor : entry.tone.color,
      );
      entry.mesh.material.emissive.setHex(
        isSelected ? entry.tone.emissive : isHovered ? entry.tone.hoverEmissive : isMeasured ? entry.tone.measuredEmissive : 0x000000,
      );
      entry.mesh.material.emissiveIntensity = isSelected ? 0.38 : isHovered ? 0.24 : isMeasured ? 0.18 : 0;
      if (entry.outline) {
        entry.outline.visible = outlineEnabled || topologyMode === 'feature';
        entry.outline.material.color.setHex(
          isSelected || (topologyMode === 'feature' && isHovered) ? 0xe5ca88 : entry.tone.edge,
        );
        entry.outline.material.opacity = !entry.outline.visible
          ? 0
          : isSelected
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
        entry.topology.visible = topologyMode === 'mesh' && isHovered;
        entry.topology.material.opacity = topologyMode === 'mesh' && isHovered ? 0.28 : 0;
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
      entry.line.material.color.setHex(
        isSelected ? 0xe5ca88 : isHovered ? 0x78c0a8 : isMeasured ? 0x9ad8c5 : 0x405371,
      );
      entry.line.material.opacity = isSelected ? 1 : isHovered ? 0.95 : isMeasured ? 0.88 : 0.46;
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

      const { scale, anchor } = preview;
      entry.mesh.scale.set(scale.x, scale.y, scale.z);
      entry.mesh.position.set(
        (1 - scale.x) * anchor.x,
        (1 - scale.y) * anchor.y,
        (1 - scale.z) * anchor.z,
      );
    }

    for (const entry of runtimeEdges) {
      const preview = previewTransforms[entry.partId];
      if (!preview) {
        entry.line.scale.set(1, 1, 1);
        entry.line.position.set(0, 0, 0);
        continue;
      }

      const { scale, anchor } = preview;
      entry.line.scale.set(scale.x, scale.y, scale.z);
      entry.line.position.set(
        (1 - scale.x) * anchor.x,
        (1 - scale.y) * anchor.y,
        (1 - scale.z) * anchor.z,
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
    if (!overlayPartLabel) {
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
    if (!modelRoot) {
      runtimeMeshes = [];
      runtimeEdges = [];
      updateOverlayAnchor();
      return;
    }
    disposeRuntimeEdges(modelRoot);
    scene?.remove(modelRoot);
    disposeDetachedGroup(modelRoot);
    modelRoot = null;
    runtimeMeshes = [];
    runtimeEdges = [];
    updateOverlayAnchor();
  }

  function disposeDetachedGroup(group: THREE.Group) {
    group.traverse((child) => {
      if (child instanceof THREE.Mesh) {
        child.geometry?.dispose?.();
        child.material?.dispose?.();
      }
      if (child instanceof THREE.Line) {
        child.geometry?.dispose?.();
        child.material?.dispose?.();
      }
    });
  }

  function handlePointerDown(event: PointerEvent) {
    if (hideModelWhileBusy) return;
    pointerDownAt = { x: event.clientX, y: event.clientY };
  }

  function fallbackPartTarget(partId: string | null, viewerNodeId: string | null): ContextSelectionTarget | null {
    if (!partId) return null;
    return (
      resolveViewerNodeTarget(selectionTargets, viewerNodeId, partId) ?? {
        targetId: `part:${partId}`,
        aliasIds: [],
        kind: 'part',
        partId,
        label: partId,
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

    const intersections = raycaster.intersectObjects(runtimeMeshes.map((entry) => entry.mesh), true);
    const hit = intersections.find((entry) => typeof entry.object.userData.partId === 'string');
    if (hit?.object.userData.partId) {
      const partId = hit.object.userData.partId as string;
      const viewerNodeId =
        typeof hit.object.userData.nodeId === 'string' ? (hit.object.userData.nodeId as string) : null;
      return (
        resolveViewerNodeTarget(selectionTargets, viewerNodeId, partId) ??
        fallbackPartTarget(partId, viewerNodeId)
      );
    }

    let bestPartId: string | null = null;
    let bestDistance = Number.POSITIVE_INFINITY;

    for (const entry of runtimeMeshes) {
      if (!entry.partId) continue;
      const projected = projectMeshPoint(entry.mesh, 'center');
      if (!projected) continue;
      const distance = Math.hypot(
        projected.x - (event.clientX - rect.left),
        projected.y - (event.clientY - rect.top),
      );
      if (distance < bestDistance) {
        bestDistance = distance;
        bestPartId = entry.partId;
      }
    }

    const selectionRadius = Math.max(96, Math.min(rect.width, rect.height) * 0.4);
    return bestDistance <= selectionRadius ? fallbackPartTarget(bestPartId, null) : null;
  }

  function handlePointerMove(event: PointerEvent) {
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
    const nextHovered = selectionTargetFromEvent(event);
    const nextHoveredPartId = nextHovered?.partId ?? null;
    const nextHoveredTargetId = nextHovered?.targetId ?? null;
    if (nextHoveredPartId !== hoveredPartId || nextHoveredTargetId !== hoveredTargetId) {
      hoveredPartId = nextHoveredPartId;
      hoveredTargetId = nextHoveredTargetId;
      applySelectionStyles();
    }
    if (renderer) {
      renderer.domElement.style.cursor = nextHoveredTargetId ? 'pointer' : 'default';
    }
  }

  function handlePointerLeave() {
    hoveredPartId = null;
    hoveredTargetId = null;
    applySelectionStyles();
    if (renderer) {
      renderer.domElement.style.cursor = hideModelWhileBusy ? 'progress' : 'default';
    }
  }

  function handlePointerUp(event: PointerEvent) {
    if (hideModelWhileBusy || !renderer || !camera || !modelRoot || runtimeMeshes.length === 0) return;
    if (pointerDownAt) {
      const deltaX = Math.abs(event.clientX - pointerDownAt.x);
      const deltaY = Math.abs(event.clientY - pointerDownAt.y);
      if (deltaX > 4 || deltaY > 4) {
        pointerDownAt = null;
        return;
      }
    }
    pointerDownAt = null;

    onSelectTarget?.(selectionTargetFromEvent(event));
  }
</script>

<div bind:this={viewerHost} class="viewer-host">
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
      {:else}
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

          {#if showViewportControlList && overlayControls.length > 0}
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
          {:else if showViewportControlList}
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
