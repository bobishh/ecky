<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
  import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
  import ViewportTransmutation from './ViewportTransmutation.svelte';
  import { estimateBase64Bytes, profileLog } from './debug/profiler';
  import type { ParamValue, UiField, ViewerAsset } from './types/domain';
  import type { ImportedPreviewTransform } from './modelRuntime/importedRuntime';
  import type { MaterializedSemanticControl } from './modelRuntime/semanticControls';

  type ViewportBusyPhase = 'generating' | 'repairing' | 'rendering' | 'committing' | null;

  let {
    stlUrl = null,
    viewerAssets = [],
    selectedPartId = null,
    overlayPartLabel = null,
    overlayPartEditable = false,
    overlayPreviewOnly = false,
    overlayControls = [],
    previewTransforms = {},
    isGenerating = false,
    hideModelWhileBusy = false,
    busyPhase = null,
    busyText = null,
    onSelectPart,
    onOverlayChange,
  }: {
    stlUrl?: string | null;
    viewerAssets?: ViewerAsset[];
    selectedPartId?: string | null;
    overlayPartLabel?: string | null;
    overlayPartEditable?: boolean;
    overlayPreviewOnly?: boolean;
    overlayControls?: MaterializedSemanticControl[];
    previewTransforms?: Record<string, ImportedPreviewTransform>;
    isGenerating?: boolean;
    hideModelWhileBusy?: boolean;
    busyPhase?: ViewportBusyPhase;
    busyText?: string | null;
    onSelectPart?: (partId: string | null) => void;
    onOverlayChange?: (primitiveId: string, value: ParamValue) => Promise<void> | void;
  } = $props();

  type RuntimeMesh = {
    partId: string | null;
    baseBounds: THREE.Box3 | null;
    mesh: THREE.Mesh<THREE.BufferGeometry, THREE.MeshStandardMaterial>;
  };

  let viewerHost: HTMLDivElement;
  let scene: THREE.Scene | null = null;
  let camera: THREE.PerspectiveCamera | null = null;
  let renderer: THREE.WebGLRenderer | null = null;
  let controls: OrbitControls | null = null;
  let modelRoot: THREE.Group | null = null;
  let runtimeMeshes: RuntimeMesh[] = [];
  let animationFrameId: number | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let loadToken = 0;
  let overlayLeft = $state(24);
  let overlayTop = $state(24);
  let overlayVisible = $state(false);
  let overlayFallback = $state(true);
  let hoveredPartId = $state<string | null>(null);
  let dimensionFrame = $state<{ bottom: number; height: number; left: number; right: number; top: number; width: number } | null>(null);
  const showEditableCallouts = $derived.by(
    () => !hideModelWhileBusy && !overlayFallback && overlayPartEditable && overlayControls.length > 0,
  );

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let pointerDownAt: { x: number; y: number } | null = null;

  export function captureScreenshot(overlayCanvas: HTMLCanvasElement | null = null): string | null {
    if (!renderer || !scene || !camera) return null;
    renderer.render(scene, camera);
    const source = renderer.domElement;
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
    return dataUrl;
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

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
  }

  function updateOverlayParam(primitiveId: string, value: ParamValue) {
    if (hideModelWhileBusy) return;
    onOverlayChange?.(primitiveId, value);
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
    void loadCurrentModel();

    resizeObserver = new ResizeObserver(() => {
      onResize();
    });
    resizeObserver.observe(viewerHost);
  });

  onDestroy(() => {
    if (animationFrameId) cancelAnimationFrame(animationFrameId);
    if (renderer) {
      renderer.domElement.removeEventListener('pointerdown', handlePointerDown);
      renderer.domElement.removeEventListener('pointermove', handlePointerMove);
      renderer.domElement.removeEventListener('pointerleave', handlePointerLeave);
      renderer.domElement.removeEventListener('pointerup', handlePointerUp);
    }
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
    if (!scene) return;
    void loadCurrentModel();
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
    if (!controls) return;
    controls.autoRotate = isGenerating && !hideModelWhileBusy;
    controls.autoRotateSpeed = controls.autoRotate ? 1.8 : 0;
    if (!controls.autoRotate) {
      controls.update();
    }
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

    camera = new THREE.PerspectiveCamera(45, viewerHost.clientWidth / viewerHost.clientHeight, 0.1, 2000);
    camera.position.set(140, 120, 140);

    renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(viewerHost.clientWidth, viewerHost.clientHeight);
    viewerHost.appendChild(renderer.domElement);
    renderer.domElement.addEventListener('pointerdown', handlePointerDown);
    renderer.domElement.addEventListener('pointermove', handlePointerMove);
    renderer.domElement.addEventListener('pointerleave', handlePointerLeave);
    renderer.domElement.addEventListener('pointerup', handlePointerUp);

    controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;

    const hemi = new THREE.HemisphereLight(0xc7d8ff, 0x202020, 0.9);
    scene.add(hemi);

    const dir = new THREE.DirectionalLight(0xffffff, 0.95);
    dir.position.set(120, 180, 140);
    scene.add(dir);

    const grid = new THREE.GridHelper(250, 24, 0x24314f, 0x18203a);
    grid.position.y = 0;
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

  function onResize() {
    if (!viewerHost || !camera || !renderer) return;
    const w = viewerHost.clientWidth;
    const h = viewerHost.clientHeight;
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
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
        const geometry = await loader.loadAsync(cacheBust(asset.path));
        if (token !== loadToken) {
          geometry.dispose();
          return;
        }

        geometry.computeVertexNormals();
        geometry.computeBoundingBox();
        const material = createMaterial(asset.partId === selectedPartId);
        const mesh = new THREE.Mesh(geometry, material);
        mesh.userData.partId = asset.partId;
        mesh.userData.nodeId = asset.nodeId;
        nextRoot.add(mesh);
        nextMeshes.push({
          partId: asset.partId,
          baseBounds: geometry.boundingBox?.clone() ?? null,
          mesh,
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
      applySelectionStyles();
      updateOverlayAnchor();
    } catch (error) {
      console.error('Failed to load multipart STL assets:', error);
      disposeDetachedGroup(nextRoot);
    }
  }

  async function loadSingleStl(token: number, url: string) {
    if (!scene || !camera) return;
    const loader = new STLLoader();
    const nextRoot = new THREE.Group();
    nextRoot.rotation.x = -Math.PI / 2;

    try {
      const geometry = await loader.loadAsync(cacheBust(url));
      if (token !== loadToken) {
        geometry.dispose();
        return;
      }

      geometry.computeVertexNormals();
      geometry.computeBoundingBox();
      const material = createMaterial(false);
      const mesh = new THREE.Mesh(geometry, material);
      nextRoot.add(mesh);

      disposeModel();
      modelRoot = nextRoot;
      runtimeMeshes = [{ partId: null, baseBounds: geometry.boundingBox?.clone() ?? null, mesh }];
      applyPreviewTransforms();
      scene.add(modelRoot);
      frameModel(modelRoot);
      updateOverlayAnchor();
    } catch (error) {
      console.error('Failed to load STL:', error);
      disposeDetachedGroup(nextRoot);
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

  function createMaterial(isSelected: boolean) {
    return new THREE.MeshStandardMaterial({
      color: isSelected ? 0xe5ca88 : 0xd2bf89,
      emissive: isSelected ? 0x5b4120 : 0x000000,
      emissiveIntensity: isSelected ? 0.45 : 0,
      metalness: 0.1,
      roughness: 0.68,
    });
  }

  function applySelectionStyles() {
    for (const entry of runtimeMeshes) {
      const isSelected = !!selectedPartId && entry.partId === selectedPartId;
      const isHovered = !isSelected && !!hoveredPartId && entry.partId === hoveredPartId;
      entry.mesh.material.color.setHex(isSelected ? 0xe5ca88 : isHovered ? 0xdbcb94 : 0xd2bf89);
      entry.mesh.material.emissive.setHex(isSelected ? 0x5b4120 : isHovered ? 0x0f5146 : 0x000000);
      entry.mesh.material.emissiveIntensity = isSelected ? 0.45 : isHovered ? 0.32 : 0;
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

  function updateOverlayAnchor() {
    if (!selectedPartId || !overlayPartLabel) {
      overlayVisible = false;
      overlayFallback = true;
      dimensionFrame = null;
      return;
    }

    overlayVisible = true;

    if (!camera || !renderer || !viewerHost) {
      overlayFallback = true;
      dimensionFrame = null;
      return;
    }

    const targetMesh = runtimeMeshes.find((entry) => entry.partId === selectedPartId)?.mesh;
    if (!targetMesh) {
      overlayFallback = true;
      dimensionFrame = null;
      return;
    }

    const anchor = projectMeshPoint(targetMesh, 'top');
    dimensionFrame = projectMeshFrame(targetMesh);
    if (!anchor) {
      overlayFallback = true;
      return;
    }
    overlayLeft = anchor.x;
    overlayTop = anchor.y;
    overlayFallback = false;
  }

  function disposeModel() {
    if (!modelRoot) {
      runtimeMeshes = [];
      updateOverlayAnchor();
      return;
    }
    scene?.remove(modelRoot);
    disposeDetachedGroup(modelRoot);
    modelRoot = null;
    runtimeMeshes = [];
    updateOverlayAnchor();
  }

  function disposeDetachedGroup(group: THREE.Group) {
    group.traverse((child) => {
      if (child instanceof THREE.Mesh) {
        child.geometry?.dispose?.();
        child.material?.dispose?.();
      }
    });
  }

  function cacheBust(url: string) {
    return `${url}${url.includes('?') ? '&' : '?'}t=${Date.now()}`;
  }

  function handlePointerDown(event: PointerEvent) {
    if (hideModelWhileBusy) return;
    pointerDownAt = { x: event.clientX, y: event.clientY };
  }

  function hoveredPartFromEvent(event: PointerEvent): string | null {
    if (hideModelWhileBusy || !renderer || !camera || !modelRoot || viewerAssets.length === 0) return null;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);

    const intersections = raycaster.intersectObjects(modelRoot.children, true);
    const hit = intersections.find((entry) => typeof entry.object.userData.partId === 'string');
    if (hit?.object.userData.partId) {
      return hit.object.userData.partId as string;
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
    return bestDistance <= selectionRadius ? bestPartId : null;
  }

  function handlePointerMove(event: PointerEvent) {
    if (hideModelWhileBusy) {
      if (hoveredPartId !== null) {
        hoveredPartId = null;
        applySelectionStyles();
      }
      if (renderer) {
        renderer.domElement.style.cursor = 'progress';
      }
      return;
    }
    const nextHovered = hoveredPartFromEvent(event);
    if (nextHovered !== hoveredPartId) {
      hoveredPartId = nextHovered;
      applySelectionStyles();
    }
    if (renderer) {
      renderer.domElement.style.cursor = nextHovered ? 'pointer' : 'default';
    }
  }

  function handlePointerLeave() {
    hoveredPartId = null;
    applySelectionStyles();
    if (renderer) {
      renderer.domElement.style.cursor = hideModelWhileBusy ? 'progress' : 'default';
    }
  }

  function handlePointerUp(event: PointerEvent) {
    if (hideModelWhileBusy || !renderer || !camera || !modelRoot || viewerAssets.length === 0) return;
    if (pointerDownAt) {
      const deltaX = Math.abs(event.clientX - pointerDownAt.x);
      const deltaY = Math.abs(event.clientY - pointerDownAt.y);
      if (deltaX > 4 || deltaY > 4) {
        pointerDownAt = null;
        return;
      }
    }
    pointerDownAt = null;

    const bestPartId = hoveredPartFromEvent(event);
    onSelectPart?.(bestPartId);
  }
</script>

<div bind:this={viewerHost} class="viewer-host">
  {#if overlayVisible && !hideModelWhileBusy}
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

      {#if showEditableCallouts}
        <div
          class="viewer-part-overlay viewer-part-overlay-callouts"
          style={`left: ${overlayLeft}px; top: ${overlayTop}px;`}
        >
          <div class="viewer-part-badge">
            <span class="viewer-part-badge__status">{overlayPreviewOnly ? 'PREVIEW' : 'EDIT'}</span>
            <span class="viewer-part-badge__title">{overlayPartLabel}</span>
          </div>

            <div class="viewer-callout-stack">
              {#each overlayControls as control, index}
                {@const field = control.rawField}
                {@const tone = overlayFieldTone(field)}
                <label
                  class="viewer-callout"
                  data-tone={tone}
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
                    <span class="viewer-overlay-readout">{control.value}</span>
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
          <div class="viewer-part-overlay__header">
            <span class="viewer-part-overlay__status">
              {overlayPartEditable ? (overlayPreviewOnly ? 'PREVIEW' : 'EDIT') : 'INSPECT'}
            </span>
            <span class="viewer-part-overlay__title">{overlayPartLabel}</span>
          </div>

          {#if overlayControls.length > 0}
            <div class="viewer-part-overlay__controls">
              {#each overlayControls as control}
                {@const field = control.rawField}
                <label class="viewer-overlay-control">
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
                      <span class="viewer-overlay-readout">{control.value}</span>
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

  .viewer-part-badge {
    position: absolute;
    left: 0;
    top: 0;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
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

  .viewer-part-badge::after {
    content: '';
    position: absolute;
    left: 50%;
    bottom: -14px;
    width: 1px;
    height: 14px;
    background: color-mix(in srgb, var(--secondary) 60%, var(--bg-300));
    transform: translateX(-50%);
  }

  .viewer-part-badge__status {
    padding: 2px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 48%, var(--bg-300));
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
    font-size: 0.56rem;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .viewer-part-badge__title {
    color: var(--text);
    font-size: 0.76rem;
    font-weight: 700;
  }

  .viewer-callout-stack {
    position: absolute;
    left: 34px;
    top: calc(-100% - 12px);
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

  .viewer-part-overlay__header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .viewer-part-overlay__status {
    padding: 2px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 48%, var(--bg-300));
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
    font-size: 0.56rem;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .viewer-part-overlay-readonly .viewer-part-overlay__status {
    border-color: color-mix(in srgb, var(--text-dim) 42%, var(--bg-300));
    color: var(--text-dim);
    background: color-mix(in srgb, var(--text-dim) 10%, var(--bg-100));
  }

  .viewer-part-overlay__title {
    color: var(--text);
    font-size: 0.76rem;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .viewer-part-overlay__controls {
    display: flex;
    flex-direction: column;
    gap: 8px;
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
