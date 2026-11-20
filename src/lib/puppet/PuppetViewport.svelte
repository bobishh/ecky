<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import * as THREE from 'three';
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
  import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import type { PuppetPart } from '../tauri/contracts';

  let { path, refreshToken = 0, selected, playing, picking, onpick, onerror, onready }: {
    path: string; refreshToken?: number; selected: PuppetPart | null; playing: boolean; picking: boolean;
    onpick: (point: [number, number, number]) => void; onerror: (message: string) => void; onready: (ready: boolean) => void;
  } = $props();
  let container: HTMLDivElement;
  let ready = $state(false);
  let renderer: THREE.WebGLRenderer, camera: THREE.PerspectiveCamera, scene: THREE.Scene, controls: OrbitControls;
  let root: THREE.Group | null = null, mixer: THREE.AnimationMixer | null = null;
  let marker: THREE.Mesh, axes: THREE.AxesHelper;
  let generation = 0;
  let activeMixer: THREE.AnimationMixer | null = null;
  let status = $state('No preview');
  let disposed = false;
  const previewSource = $derived(path);
  const previewRefresh = $derived(refreshToken);

  function disposeObject(object: THREE.Object3D) {
    const materials = new Set<THREE.Material>();
    const textures = new Set<THREE.Texture>();
    object.traverse(node => {
      if (node instanceof THREE.Mesh) {
        node.geometry.dispose();
        for (const material of Array.isArray(node.material) ? node.material : [node.material]) materials.add(material);
      }
    });
    for (const material of materials) {
      for (const value of Object.values(material)) if (value instanceof THREE.Texture) textures.add(value);
      material.dispose();
    }
    for (const texture of textures) { (texture.image as ImageBitmap)?.close?.(); texture.dispose(); }
  }
  function neutralPose() {
    mixer?.stopAllAction();
    root?.updateMatrixWorld(true);
  }
  $effect(() => {
    if (!ready || !previewSource) return;
    const source = previewSource;
    // A retry must reload the same content-addressed file after a load failure.
    void previewRefresh;
    const token = ++generation;
    status = 'Loading preview';
    untrack(() => {
      onready(false);
      new GLTFLoader().load(convertFileSrc(source), gltf => {
        if (disposed || token !== generation) { disposeObject(gltf.scene); return; }
        if (root) { scene.remove(root); disposeObject(root); }
        root = gltf.scene; scene.add(root);
        mixer = new THREE.AnimationMixer(root);
        for (const clip of gltf.animations) mixer.clipAction(clip);
        const box = new THREE.Box3().setFromObject(root);
        const size = Math.max(box.getSize(new THREE.Vector3()).length(), 0.01);
        const center = box.getCenter(new THREE.Vector3());
        camera.near = Math.max(size / 10000, 0.00001); camera.far = Math.max(size*100, 10); camera.updateProjectionMatrix();
        camera.position.copy(center).add(new THREE.Vector3(0.8, 0.6, 1).multiplyScalar(size));
        controls.target.copy(center); controls.update();
        marker.scale.setScalar(size * 0.012); axes.scale.setScalar(size * 0.15);
        root.userData.clips = gltf.animations;
        status = 'Preview ready'; onready(true);
      }, undefined, error => {
        if (disposed || token !== generation) return;
        status = 'Preview unavailable';
        onerror(`Could not load puppet preview: ${error instanceof Error ? error.message : String(error)}`);
      });
    });
    return () => { generation++; };
  });
  onMount(() => {
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true });
      renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
      scene = new THREE.Scene(); scene.background = new THREE.Color('#10151d');
      camera = new THREE.PerspectiveCamera(42, 1, 0.0001, 100);
      controls = new OrbitControls(camera, renderer.domElement); controls.enableDamping = true;
      scene.add(new THREE.HemisphereLight(0xffffff, 0x34303a, 2));
      const light = new THREE.DirectionalLight(0xffffff, 3); light.position.set(2, 4, 3); scene.add(light);
      marker = new THREE.Mesh(new THREE.SphereGeometry(1, 12, 8), new THREE.MeshBasicMaterial({ color: 0xe3aa69, depthTest: false }));
      marker.renderOrder = 100; scene.add(marker);
      axes = new THREE.AxesHelper(1); scene.add(axes);
      container.appendChild(renderer.domElement);
      const resize = new ResizeObserver(() => {
        const { width, height } = container.getBoundingClientRect();
        if (width && height) { renderer.setSize(width, height); camera.aspect = width/height; camera.updateProjectionMatrix(); }
      }); resize.observe(container);
      const clock = new THREE.Clock(); let wasPlaying = false;
      renderer.setAnimationLoop(() => {
        const delta = Math.min(clock.getDelta(), 0.1);
        if (root && mixer) {
          if (playing && (!wasPlaying || activeMixer !== mixer)) for (const clip of root.userData.clips ?? []) mixer.clipAction(clip).reset().play();
          if (!playing && wasPlaying) neutralPose();
          if (playing) mixer.update(delta);
        }
        wasPlaying = playing; activeMixer = mixer;
        const joint = selected ? root?.getObjectByName(`joint_${selected.id}`) : null;
        marker.visible = axes.visible = Boolean(joint);
        if (joint) { joint.getWorldPosition(marker.position); axes.position.copy(marker.position); joint.getWorldQuaternion(axes.quaternion); }
        controls.update(); renderer.render(scene, camera);
      });
      let down = new THREE.Vector2();
      const pointerDown = (event: PointerEvent) => { down.set(event.clientX, event.clientY); };
      const pointerUp = (event: PointerEvent) => {
        if (!picking || !selected || !root || down.distanceTo(new THREE.Vector2(event.clientX,event.clientY)) > 4) return;
        const rect = renderer.domElement.getBoundingClientRect();
        const ray = new THREE.Raycaster(); ray.setFromCamera(new THREE.Vector2((event.clientX-rect.left)/rect.width*2-1, -(event.clientY-rect.top)/rect.height*2+1), camera);
        const joint = root.getObjectByName(`joint_${selected.id}`);
        if (!joint) return;
        // Only the selected part's mesh, excluding descendants attached to it.
        const visual = joint.children.find(child => child instanceof THREE.Mesh || !child.name.startsWith('joint_'));
        if (!visual) return;
        const hit = ray.intersectObject(visual, true)[0];
        if (hit) {
          const point = joint.worldToLocal(hit.point).multiplyScalar(1000/selected.scale).add(new THREE.Vector3(...selected.pivotMm));
          onpick(point.toArray());
        }
      };
      renderer.domElement.addEventListener('pointerdown', pointerDown);
      renderer.domElement.addEventListener('pointerup', pointerUp);
      ready = true;
      return () => {
        disposed = true; generation++; resize.disconnect(); renderer.setAnimationLoop(null);
        renderer.domElement.removeEventListener('pointerdown', pointerDown); renderer.domElement.removeEventListener('pointerup', pointerUp);
        controls.dispose(); if (root) disposeObject(root); marker.geometry.dispose(); (marker.material as THREE.Material).dispose(); axes.dispose(); renderer.dispose(); renderer.domElement.remove();
      };
    } catch (error) { onerror(`3D preview unavailable: ${error instanceof Error ? error.message : String(error)}`); }
  });
</script>
<div class="viewport" class:picking bind:this={container} aria-label="Puppet preview" data-testid="puppet-viewport" data-status={status}>
  <span>{picking ? 'Click the joint point on the selected part' : status}</span>
</div>
<style>
  .viewport { position: relative; min-width: 0; min-height: 240px; height: 100%; overflow: hidden; background: #10151d; }
  .viewport span { position: absolute; top: 10px; left: 10px; color: var(--text-dim); font-size: 11px; pointer-events: none; z-index: 1; }
  .picking { cursor: crosshair; }
  .viewport :global(canvas) { display: block; }
</style>
