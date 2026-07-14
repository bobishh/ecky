<script lang="ts">
  import { onDestroy } from 'svelte';
  import * as THREE from 'three';
  import {
    DEFAULT_GENIE_TRAITS,
    resolveModeTraits,
    seededSigned,
    seededUnit,
    type ResolvedGenieProfile,
  } from '@genome/traits';
  import { buildStoneGeometry, type StonePoint3 } from '@genome/stoneGeometry';

  // The canonical Ecky genome: default traits, idle mode. Same math as the app,
  // so this is literally the same creature — seed 1 is Ecky.
  const profile = resolveModeTraits(DEFAULT_GENIE_TRAITS, 'idle');

  let {
    size = 180,
    interactive = true,
  }: { size?: number; interactive?: boolean } = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);

  // Pointer-drag rotation state (kept on a mutable object the rAF loop reads).
  const rt = {
    userYaw: 0,
    userPitch: 0,
  };
  let dragPointerId: number | null = null;
  let dragLastX = 0;
  let dragLastY = 0;

  function clamp(value: number, min: number, max: number): number {
    return Math.max(min, Math.min(max, value));
  }

  function smoothStep(value: number): number {
    const x = clamp(value, 0, 1);
    return x * x * (3 - 2 * x);
  }

  function buildScene(
    renderer: THREE.WebGLRenderer,
    currentProfile: ResolvedGenieProfile,
  ) {
    const scene = new THREE.Scene();
    const stoneGroup = new THREE.Group();
    scene.add(stoneGroup);
    const camera = new THREE.PerspectiveCamera(22, 1, 0.1, 100);
    camera.position.set(0, 0.02, 11.0);
    const visualGroup = new THREE.Group();
    stoneGroup.add(visualGroup);

    const stone = buildStoneGeometry(currentProfile);
    const hue = stone.hue;
    const base = new THREE.Color().setHSL(hue / 360, 0.32, 0.28);
    const edgeColor = new THREE.Color().setHSL(hue / 360, 0.32, 0.54);

    const frontCount = stone.front.length;
    const positions: number[] = [];
    const colors: number[] = [];
    const toVector = (point: StonePoint3) => new THREE.Vector3(point.x, point.y, point.z);
    const front = stone.front.map(toVector);
    const rim = stone.rim.map(toVector);
    const back = stone.back.map(toVector);
    const backCenter = new THREE.Vector3(
      back.reduce((sum, point) => sum + point.x, 0) / frontCount + seededSigned(currentProfile.seed, 1486) * 0.04,
      back.reduce((sum, point) => sum + point.y, 0) / frontCount,
      -0.92,
    );
    const sideMid = front.map((point, index) => {
      const next = (index + 1) % frontCount;
      const prev = (index + frontCount - 1) % frontCount;
      const clump =
        0.02 +
        seededUnit(currentProfile.seed, 1470 + index) * 0.13 +
        (index % 4 === 0 ? 0.08 : index % 4 === 2 ? 0.05 : 0);
      const basePoint = point.clone().lerp(rim[index], 0.5 + seededUnit(currentProfile.seed, 1478 + index) * 0.16);
      const ridgeSource = rim[index].clone().lerp(rim[next], 0.22 + seededUnit(currentProfile.seed, 1480 + index) * 0.22);
      const shoulder = rim[prev].clone().lerp(rim[index], 0.62);
      return basePoint
        .lerp(ridgeSource, clump)
        .lerp(shoulder, seededUnit(currentProfile.seed, 1484 + index) * 0.08)
        .setZ(basePoint.z + clump * (0.46 + seededUnit(currentProfile.seed, 1488 + index) * 0.28));
    });
    const rearMid = rim.map((point, index) => {
      const prev = (index + frontCount - 1) % frontCount;
      const next = (index + 1) % frontCount;
      const clump =
        0.03 +
        seededUnit(currentProfile.seed, 1490 + index) * 0.16 +
        (index % 5 === 1 ? 0.1 : index % 5 === 3 ? 0.06 : 0);
      const basePoint = point.clone().lerp(back[index], 0.44 + seededUnit(currentProfile.seed, 1498 + index) * 0.16);
      const ridgeSource = point
        .clone()
        .lerp(rim[next], 0.18 + seededUnit(currentProfile.seed, 1502 + index) * 0.18)
        .lerp(back[prev], 0.18 + seededUnit(currentProfile.seed, 1506 + index) * 0.16);
      return basePoint
        .lerp(ridgeSource, clump)
        .setZ(basePoint.z - clump * (0.28 + seededUnit(currentProfile.seed, 1510 + index) * 0.24));
    });
    const deformRing = (ring: THREE.Vector3[], sourceOffset: number, depthBase: number, depthRange: number, zBias: number) =>
      ring.map((point, index) => {
        const outward = point.clone().sub(backCenter).setZ(0);
        if (outward.lengthSq() < 0.01) return point.clone();
        outward.normalize();
        const tangent = new THREE.Vector3(-outward.y, outward.x, 0);
        const majorPeak = index % 5 === sourceOffset % 5 || seededUnit(currentProfile.seed, sourceOffset + index) > 0.72;
        const minorPeak = index % 5 === (sourceOffset + 2) % 5 || seededUnit(currentProfile.seed, sourceOffset + 50 + index) > 0.56;
        const peakDepth = majorPeak
          ? depthBase + seededUnit(currentProfile.seed, sourceOffset + 100 + index) * depthRange
          : minorPeak
            ? depthBase * 0.48 + seededUnit(currentProfile.seed, sourceOffset + 100 + index) * depthRange * 0.48
            : depthBase * 0.12 + seededUnit(currentProfile.seed, sourceOffset + 100 + index) * depthRange * 0.16;
        return point
          .clone()
          .add(outward.multiplyScalar(peakDepth))
          .add(tangent.multiplyScalar(seededSigned(currentProfile.seed, sourceOffset + 150 + index) * (majorPeak ? 0.1 : 0.04)))
          .setZ(point.z + zBias * (majorPeak ? 1 : 0.45) + seededSigned(currentProfile.seed, sourceOffset + 200 + index) * Math.abs(zBias) * 0.28);
      });
    const sideShell = deformRing(sideMid, 1516, 0.16, 0.22, 0.1);
    const rimShell = deformRing(rim, 1540, 0.28, 0.34, 0.14);
    const rearShell = deformRing(rearMid, 1564, 0.3, 0.36, -0.14);
    const backShell = deformRing(back, 1588, 0.32, 0.42, -0.18);
    const backCrown = back.map((point, index) =>
      point
        .clone()
        .lerp(backCenter, 0.42 + seededUnit(currentProfile.seed, 1552 + index) * 0.14)
        .setZ(-0.72 - seededUnit(currentProfile.seed, 1560 + index) * 0.18),
    );
    const crownShell = deformRing(backCrown, 1630, 0.26, 0.36, -0.22);
    const center = toVector(stone.center);
    const pushTri = (a: THREE.Vector3, b: THREE.Vector3, c: THREE.Vector3, shade: number) => {
      const color = base.clone().multiplyScalar(shade);
      for (const point of [a, b, c]) {
        positions.push(point.x, point.y, point.z);
        colors.push(color.r, color.g, color.b);
      }
    };
    const pushQuad = (a: THREE.Vector3, b: THREE.Vector3, c: THREE.Vector3, d: THREE.Vector3, shade: number) => {
      pushTri(a, b, c, shade);
      pushTri(a, c, d, shade * 1.06);
    };
    for (let index = 0; index < frontCount; index++) {
      const next = (index + 1) % frontCount;
      pushTri(center, front[index], front[next], 1.34);
      pushQuad(front[index], sideShell[index], sideShell[next], front[next], 1.02 + seededUnit(currentProfile.seed, 1400 + index) * 0.1);
      pushQuad(sideShell[index], rimShell[index], rimShell[next], sideShell[next], 0.86 + seededUnit(currentProfile.seed, 1420 + index) * 0.12);
      pushQuad(rimShell[index], rearShell[index], rearShell[next], rimShell[next], 0.7 + seededUnit(currentProfile.seed, 1440 + index) * 0.12);
      pushQuad(rearShell[index], backShell[index], backShell[next], rearShell[next], 0.56 + seededUnit(currentProfile.seed, 1460 + index) * 0.12);
      pushQuad(backShell[index], crownShell[index], crownShell[next], backShell[next], 0.48 + seededUnit(currentProfile.seed, 1480 + index) * 0.1);
      pushTri(backCenter, crownShell[next], crownShell[index], 0.42 + seededUnit(currentProfile.seed, 1500 + index) * 0.1);
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    geometry.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
    geometry.computeVertexNormals();

    const material = new THREE.MeshStandardMaterial({
      vertexColors: true,
      flatShading: true,
      roughness: 0.78,
      metalness: 0.02,
      side: THREE.DoubleSide,
    });
    const mesh = new THREE.Mesh(geometry, material);
    visualGroup.add(mesh);
    const edgeGeometry = new THREE.EdgesGeometry(geometry, 16);
    const edgeMaterial = new THREE.LineBasicMaterial({ color: edgeColor, transparent: true, opacity: 0.42 });
    const edgeLines = new THREE.LineSegments(edgeGeometry, edgeMaterial);
    visualGroup.add(edgeLines);

    // Face grooves: eyes + mouth, same as the app.
    const grooveColor = new THREE.Color().setHSL(
      stone.face.grooveHue / 360,
      stone.face.grooveSaturation,
      stone.face.grooveLightness,
    );
    const grooveMaterial = new THREE.MeshBasicMaterial({ color: grooveColor });
    const faceGroup = new THREE.Group();
    const zFace = 1.18;
    const { eyeShape, mouthShape, eyeY, mouthY, eyeSpacing, eyeWidth, eyeSlant, mouthWidth, mouthCurve, grooveDepth, grooveHeight } = stone.face;
    const addGroove = (x: number, y: number, width: number, angle = 0) => {
      const group = new THREE.Group();
      const groove = new THREE.Mesh(new THREE.BoxGeometry(width, grooveHeight, grooveDepth), grooveMaterial);
      group.add(groove);
      group.position.set(x, y, zFace);
      group.rotation.z = angle;
      faceGroup.add(group);
      return group;
    };
    const addEye = (x: number, y: number, angle = 0) => {
      if (eyeShape === 'dot' || eyeShape === 'oval') {
        const radius = eyeShape === 'dot' ? eyeWidth * 0.22 : eyeWidth * 0.28;
        const group = new THREE.Group();
        const eye = new THREE.Mesh(new THREE.SphereGeometry(radius, 12, 8), grooveMaterial);
        group.add(eye);
        group.position.set(x, y, zFace + 0.012);
        group.scale.set(eyeShape === 'oval' ? 1.45 : 1, eyeShape === 'oval' ? 0.68 : 1, 0.28);
        group.rotation.z = angle;
        faceGroup.add(group);
        return group;
      }
      if (eyeShape === 'triangle') {
        const shape = new THREE.Shape();
        shape.moveTo(0, grooveHeight * 0.75);
        shape.lineTo(-eyeWidth * 0.42, -grooveHeight * 0.62);
        shape.lineTo(eyeWidth * 0.42, -grooveHeight * 0.62);
        shape.closePath();
        const group = new THREE.Group();
        const eye = new THREE.Mesh(new THREE.ShapeGeometry(shape), grooveMaterial);
        group.add(eye);
        group.position.set(x, y, zFace + 0.018);
        group.rotation.z = angle;
        faceGroup.add(group);
        return group;
      }
      return addGroove(x, y, eyeWidth, angle);
    };
    const leftEye = addEye(-eyeSpacing, eyeY, -eyeSlant + seededSigned(currentProfile.seed, 1510) * 0.02);
    const rightEye = addEye(eyeSpacing, eyeY, eyeSlant + seededSigned(currentProfile.seed, 1511) * 0.02);
    const leftEyeBaseRotation = leftEye.rotation.z;
    const rightEyeBaseRotation = rightEye.rotation.z;
    const addMouth = () => {
      if (mouthShape === 'triangle') {
        const shape = new THREE.Shape();
        shape.moveTo(0, -grooveHeight * 0.85);
        shape.lineTo(-mouthWidth * 0.32, grooveHeight * 0.58);
        shape.lineTo(mouthWidth * 0.32, grooveHeight * 0.58);
        shape.closePath();
        const group = new THREE.Group();
        const mouth = new THREE.Mesh(new THREE.ShapeGeometry(shape), grooveMaterial);
        group.add(mouth);
        group.position.set(0.02, mouthY + mouthCurve * 0.5, zFace + 0.018);
        group.rotation.z = mouthCurve * 0.08;
        faceGroup.add(group);
        return group;
      }
      return addGroove(0.02, mouthY + mouthCurve * 0.5, mouthWidth * 0.74, mouthCurve * 0.12);
    };
    const mouthLine = addMouth();
    visualGroup.add(faceGroup);

    const baseScaleX = 0.86;
    const baseScaleY = 0.94;
    const baseScaleZ = 0.86;
    stoneGroup.scale.set(baseScaleX, baseScaleY, baseScaleZ);
    visualGroup.updateMatrixWorld(true);
    const visualBounds = new THREE.Box3().setFromObject(visualGroup);
    const visualCenter = visualBounds.getCenter(new THREE.Vector3());
    visualGroup.position.sub(visualCenter);

    scene.add(new THREE.AmbientLight(0x9fb7a6, 1.25));
    const keyLight = new THREE.DirectionalLight(0xdce8db, 2.4);
    keyLight.position.set(-3.5, 3.2, 4.5);
    scene.add(keyLight);
    const rimLight = new THREE.DirectionalLight(0x5b9a78, 1.1);
    rimLight.position.set(3.2, -1.8, 2.2);
    scene.add(rimLight);

    return {
      scene,
      camera,
      stoneGroup,
      material,
      edgeMaterial,
      grooveMaterial,
      geometry,
      edgeGeometry,
      faceGroup,
      leftEye,
      rightEye,
      leftEyeBaseRotation,
      rightEyeBaseRotation,
      mouthLine,
      eyeY,
      mouthY,
      mouthCurve,
      baseScaleX,
      baseScaleY,
      baseScaleZ,
      edgeColor,
      grooveColor,
    };
  }

  function pulseAt(time: number, center: number, width: number): number {
    const distance = Math.abs(time - center);
    if (distance >= width) return 0;
    return smoothStep(1 - distance / width);
  }

  let renderer: THREE.WebGLRenderer | null = null;
  let frameId = 0;
  let disposables: Array<() => void> = [];

  $effect(() => {
    if (!canvas) return;
    const r = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true });
    renderer = r;
    r.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    r.setSize(size, size, false);
    r.outputColorSpace = THREE.SRGBColorSpace;
    const built = buildScene(r, profile);

    const started = performance.now();
    const animate = () => {
      const t = (performance.now() - started) * 0.001;
      built.material.color.setRGB(1, 1, 1);
      built.edgeMaterial.color.copy(built.edgeColor);
      built.grooveMaterial.color.copy(built.grooveColor);

      const motion = 0.58;
      const glanceCycle = 2.6;
      const glanceIndex = Math.floor(t / glanceCycle);
      const glanceProgress = smoothStep((t % glanceCycle) / glanceCycle);
      const previousGlance = seededSigned(profile.seed, 1700 + glanceIndex) * 0.11;
      const nextGlance = seededSigned(profile.seed, 1701 + glanceIndex) * 0.11;
      const glanceYaw = previousGlance + (nextGlance - previousGlance) * glanceProgress;
      const breathe = Math.sin(t * (1.25 + motion * 0.18));

      const yaw = -0.24 + rt.userYaw + glanceYaw * 0.42 + Math.sin(t * 0.48) * 0.016 * motion;
      const pitch = 0.03 + rt.userPitch + breathe * 0.017 * motion;
      const roll = Math.sin(t * 0.42 + 0.8) * 0.012 * motion;
      built.stoneGroup.rotation.set(pitch, yaw, roll);
      const breatheScale = 0.018;
      built.stoneGroup.scale.set(
        built.baseScaleX + breathe * breatheScale * 0.72,
        built.baseScaleY - breathe * breatheScale * 0.5,
        built.baseScaleZ,
      );
      built.stoneGroup.position.y = Math.sin(t * 1.05) * 0.018 * motion;

      const blinkEvery = 3.8 + seededUnit(profile.seed, 1710) * 1.4;
      const blinkPhase = t % blinkEvery;
      const blink = Math.max(pulseAt(blinkPhase, blinkEvery - 0.16, 0.08), pulseAt(blinkPhase, blinkEvery - 0.04, 0.05));
      const eyeOpen = 1 - blink * 0.86;
      built.leftEye.scale.y = eyeOpen;
      built.rightEye.scale.y = eyeOpen * (1 - blink * 0.08);
      built.leftEye.position.y = built.eyeY - (1 - eyeOpen) * 0.008;
      built.rightEye.position.y = built.eyeY - (1 - eyeOpen) * 0.008;
      built.leftEye.rotation.z = built.leftEyeBaseRotation;
      built.rightEye.rotation.z = built.rightEyeBaseRotation;

      r.render(built.scene, built.camera);
      frameId = requestAnimationFrame(animate);
    };
    animate();

    disposables.push(() => {
      cancelAnimationFrame(frameId);
      built.geometry.dispose();
      built.edgeGeometry.dispose();
      built.faceGroup.traverse((child) => {
        if (child instanceof THREE.Mesh) child.geometry.dispose();
      });
      built.material.dispose();
      built.edgeMaterial.dispose();
      built.grooveMaterial.dispose();
      r.dispose();
    });

    return () => disposables.forEach((d) => d());
  });

  function startDrag(event: PointerEvent) {
    if (!interactive) return;
    dragPointerId = event.pointerId;
    dragLastX = event.clientX;
    dragLastY = event.clientY;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function moveDrag(event: PointerEvent) {
    if (!interactive || dragPointerId !== event.pointerId) return;
    const dx = event.clientX - dragLastX;
    const dy = event.clientY - dragLastY;
    rt.userYaw = clamp(rt.userYaw + dx * 0.016, -2.45, 2.45);
    rt.userPitch = clamp(rt.userPitch + dy * 0.009, -0.62, 0.62);
    dragLastX = event.clientX;
    dragLastY = event.clientY;
  }

  function endDrag(event: PointerEvent) {
    if (dragPointerId !== event.pointerId) return;
    dragPointerId = null;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  onDestroy(() => {
    disposables.forEach((d) => d());
  });
</script>

<div
  class="mascot"
  style={`width:${size}px;height:${size}px;`}
  class:interactive
  role="img"
  aria-label="Ecky, the prompt-driven CAD mascot"
>
  <canvas
    bind:this={canvas}
    width={size}
    height={size}
    onpointerdown={startDrag}
    onpointermove={moveDrag}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  ></canvas>
</div>

<style>
  .mascot {
    position: relative;
    display: inline-block;
  }
  .mascot canvas {
    display: block;
    width: 100%;
    height: 100%;
    touch-action: none;
  }
  .mascot.interactive canvas {
    cursor: grab;
  }
  .mascot.interactive canvas:active {
    cursor: grabbing;
  }
</style>
