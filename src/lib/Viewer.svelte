<script>
  import { onMount, onDestroy } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
  import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
  import { estimateBase64Bytes, profileLog } from './debug/profiler';

  let { stlUrl = null, isGenerating = false } = $props();

  let viewerHost;
  let scene, camera, renderer, controls, mesh;
  let animationFrameId;

  export function captureScreenshot(overlayCanvas = null) {
    if (!renderer) return null;
    renderer.render(scene, camera); // force render to ensure buffer has frame
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
        offscreen.height
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

  let resizeObserver;

  function disposeMesh() {
    if (!mesh) return;
    scene?.remove(mesh);
    mesh.geometry?.dispose?.();
    mesh.material?.dispose?.();
    mesh = null;
  }

  onMount(() => {
    setupViewer();
    if (stlUrl) loadStl(stlUrl);

    // Use ResizeObserver to catch container size changes (like resizer drags)
    resizeObserver = new ResizeObserver(() => {
      onResize();
    });
    resizeObserver.observe(viewerHost);
  });

  onDestroy(() => {
    if (animationFrameId) cancelAnimationFrame(animationFrameId);
    disposeMesh();
    controls?.dispose?.();
    if (renderer) {
      renderer.renderLists?.dispose?.();
      renderer.dispose();
      renderer.forceContextLoss?.();
      const canvas = renderer.domElement;
      if (canvas && canvas.parentNode) {
        canvas.parentNode.removeChild(canvas);
      }
    }
    renderer = null;
    controls = null;
    camera = null;
    scene = null;
    if (resizeObserver) resizeObserver.disconnect();
  });

  // Re-load STL when stlUrl changes
  $effect(() => {
    if (stlUrl && scene) {
      loadStl(stlUrl);
    } else if (!stlUrl && mesh && scene) {
      scene.remove(mesh);
      mesh.geometry.dispose();
      mesh.material.dispose();
      mesh = null;
    }
  });

  $effect(() => {
    if (!controls) return;
    controls.autoRotate = isGenerating;
    controls.autoRotateSpeed = isGenerating ? 1.8 : 0;
    if (!isGenerating) {
      controls.update();
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
    animationFrameId = requestAnimationFrame(animate);
    controls.update();
    renderer.render(scene, camera);
  }

  function onResize() {
    if (!viewerHost || !camera || !renderer) return;
    const w = viewerHost.clientWidth;
    const h = viewerHost.clientHeight;
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
  }

  async function loadStl(url) {
    const loader = new STLLoader();
    const bust = `${url}${url.includes('?') ? '&' : '?'}t=${Date.now()}`;

    try {
      const geometry = await loader.loadAsync(bust);
      geometry.computeVertexNormals();

      if (mesh) {
        scene.remove(mesh);
        mesh.geometry.dispose();
        mesh.material.dispose();
      }

      const material = new THREE.MeshStandardMaterial({
        color: 0xd2bf89,
        metalness: 0.1,
        roughness: 0.7,
      });

      mesh = new THREE.Mesh(geometry, material);
      mesh.rotation.x = -Math.PI / 2;
      scene.add(mesh);

      geometry.computeBoundingBox();
      const box = geometry.boundingBox;
      const center = new THREE.Vector3();
      box.getCenter(center);
      mesh.position.set(-center.x, -box.min.y, -center.z);

      const size = new THREE.Vector3();
      box.getSize(size);
      const maxDim = Math.max(size.x, size.y, size.z, 1);

      camera.position.set(maxDim * 1.3, maxDim * 1.1, maxDim * 1.3);
      controls.target.set(0, maxDim * 0.35, 0);
      controls.update();
    } catch (e) {
      console.error('Failed to load STL:', e);
    }
  }
</script>

<div bind:this={viewerHost} class="viewer-host" class:is-blur={isGenerating}></div>

<style>
  .viewer-host {
    width: 100%;
    height: 100%;
    overflow: hidden;
    transition: filter 0.5s ease-in-out;
  }

  .is-blur {
    filter: blur(8px) contrast(1.1) brightness(0.9);
  }
</style>
