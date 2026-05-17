<script lang="ts">
  import { onDestroy } from 'svelte';
  import * as THREE from 'three';
  import {
    DEFAULT_GENIE_TRAITS,
    resolveModeTraits,
    seededSigned,
    seededUnit,
    type GenieMode,
    type ResolvedGenieProfile,
  } from './genie/traits';
  import { buildStoneGeometry, type StonePoint3 } from './genie/stoneGeometry';
  import type { GenieTraits } from './types/domain';

  type StoneRenderState = {
    mode: GenieMode;
    hasSpeech: boolean;
    motionScale: number;
    wakeStartedAt: number;
	    pokeState: 'calm' | 'poked' | 'angry';
	    pokeStartedAt: number;
	    userYaw: number;
	    userPitch: number;
	  };

  let {
    mode = 'idle',
    bubble = '',
    compact = false,
    badge = null,
    contextLabel = null,
    question = '',
    onDismiss = null,
    actions = null,
    traits = {},
    intensity = 1.0,
    wakeUp = 0,
    agentConnected = true,
    safeRightInset = 360,
    fitToCanvas = false,
  }: {
    mode?: GenieMode;
    bubble?: string;
    compact?: boolean;
    badge?: string | null;
    contextLabel?: string | null;
    question?: string;
    onDismiss?: (() => void) | null;
    actions?: Array<{ label: string; onclick: () => void }> | null;
    traits?: Partial<GenieTraits> | null;
    intensity?: number;
    wakeUp?: number;
    agentConnected?: boolean;
    safeRightInset?: number;
    fitToCanvas?: boolean;
  } = $props();

  let copyFeedback = $state('');
  let copyFeedbackTimer: number | null = null;
  let wakePulse = $state(0);
  let stoneCanvas = $state<HTMLCanvasElement | null>(null);
  let pokeState = $state<'calm' | 'poked' | 'angry'>('calm');
	  let pokeCount = 0;
	  let lastPokeAt = 0;
	  let pokeResetTimer: number | null = null;
	  let angryTimer: number | null = null;
	  let dragPointerId: number | null = null;
	  let dragStartX = 0;
	  let dragStartY = 0;
	  let dragged = false;
	  let dragRevision = $state(0);
	  let repelX = $state(0);
	  let repelY = $state(0);

  const MAX_BUBBLE_LEN = 1200;
  const effectiveMode = $derived(mode === 'sleeping' ? 'sleeping' : agentConnected ? mode : 'sleeping');
  const profile = $derived.by(() =>
    resolveModeTraits(traits ?? DEFAULT_GENIE_TRAITS, effectiveMode),
  );
  const geometryProfile = $derived.by(() =>
    resolveModeTraits(traits ?? DEFAULT_GENIE_TRAITS, 'idle'),
  );
  const motionScale = $derived(Math.min(2.6, Math.max(0.6, intensity)));
  const stoneRuntime: StoneRenderState = {
    mode: 'idle',
    hasSpeech: false,
    motionScale: 1,
    wakeStartedAt: 0,
	    pokeState: 'calm',
	    pokeStartedAt: 0,
	    userYaw: 0,
	    userPitch: 0,
	  };

  $effect(() => {
    if (wakeUp !== wakePulse) {
      stoneRuntime.wakeStartedAt = performance.now();
    }
    wakePulse = wakeUp;
  });

  $effect(() => {
    stoneRuntime.mode = effectiveMode;
    stoneRuntime.hasSpeech = Boolean(cleanBubble);
    stoneRuntime.motionScale = motionScale;
    stoneRuntime.pokeState = pokeState;
  });

  const cleanBubble = $derived.by(() => {
    const text = `${bubble ?? ''}`.replace(/\s+/g, ' ').trim();
    if (!text) return '';
    return text.length > MAX_BUBBLE_LEN ? `${text.slice(0, MAX_BUBBLE_LEN - 1)}…` : text;
  });
  const cleanQuestion = $derived.by(() => `${question ?? ''}`.replace(/\s+/g, ' ').trim());

  function clamp(value: number, min: number, max: number): number {
    return Math.max(min, Math.min(max, value));
  }

  async function copyBubbleText() {
    if (!cleanBubble) return;
    try {
      await navigator.clipboard.writeText(cleanBubble);
      copyFeedback = 'COPIED';
    } catch {
      copyFeedback = 'COPY FAILED';
    }
    if (copyFeedbackTimer) clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = window.setTimeout(() => {
      copyFeedback = '';
    }, 1400);
  }

  function pokeGenie(event: MouseEvent) {
	    event.preventDefault();
	    if (dragged) {
	      dragged = false;
	      return;
	    }
	    const now = performance.now();
    if (now - lastPokeAt < 140) return;
    lastPokeAt = now;
    stoneRuntime.pokeStartedAt = now;
    pokeCount += 1;
    if (pokeResetTimer) clearTimeout(pokeResetTimer);
    pokeResetTimer = window.setTimeout(() => {
      pokeCount = 0;
      if (pokeState !== 'angry') pokeState = 'calm';
    }, 1400);

	    if (pokeCount >= 5) {
	      pokeState = 'angry';
	      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
	      repelX = clamp(rect.left + rect.width / 2 - event.clientX, -1, 1) * 14;
	      repelY = clamp(rect.top + rect.height / 2 - event.clientY, -1, 1) * 10;
	      if (angryTimer) clearTimeout(angryTimer);
	      angryTimer = window.setTimeout(() => {
	        pokeCount = 0;
	        pokeState = 'calm';
	        repelX = 0;
	        repelY = 0;
	      }, 2600);
      return;
    }

    if (pokeState !== 'angry') {
      pokeState = 'poked';
      window.setTimeout(() => {
        if (pokeState === 'poked') pokeState = 'calm';
      }, 360);
    }
	  }

  function startStoneDrag(event: PointerEvent) {
    event.preventDefault();
    dragPointerId = event.pointerId;
    dragStartX = event.clientX;
    dragStartY = event.clientY;
    dragged = false;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function moveStoneDrag(event: PointerEvent) {
    if (dragPointerId !== event.pointerId) return;
    const dx = event.clientX - dragStartX;
    const dy = event.clientY - dragStartY;
    if (Math.hypot(dx, dy) < 4 && !dragged) return;
    dragged = true;
    stoneRuntime.userYaw = clamp(stoneRuntime.userYaw + dx * 0.016, -2.45, 2.45);
    stoneRuntime.userPitch = clamp(stoneRuntime.userPitch + dy * 0.009, -0.62, 0.62);
    dragStartX = event.clientX;
    dragStartY = event.clientY;
  }

  function endStoneDrag(event: PointerEvent) {
    if (dragPointerId !== event.pointerId) return;
    dragPointerId = null;
    if (dragged) dragRevision++;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  function smoothStep(value: number): number {
    const x = clamp(value, 0, 1);
    return x * x * (3 - 2 * x);
  }

  function pulseAt(time: number, center: number, width: number): number {
    const distance = Math.abs(time - center);
    if (distance >= width) return 0;
    return smoothStep(1 - distance / width);
  }

  function renderStone(
    canvas: HTMLCanvasElement,
    currentProfile: ResolvedGenieProfile,
    renderState: StoneRenderState,
  ): () => void {
    const renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true, preserveDrawingBuffer: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(150, 150, false);
    renderer.outputColorSpace = THREE.SRGBColorSpace;

    const scene = new THREE.Scene();
    const stoneGroup = new THREE.Group();
    scene.add(stoneGroup);
    const camera = new THREE.PerspectiveCamera(22, 1, 0.1, 100);
    camera.position.set(0, 0.02, fitToCanvas ? 11.4 : 10.1);
    const visualGroup = new THREE.Group();
    stoneGroup.add(visualGroup);

    const stone = buildStoneGeometry(currentProfile);
    const angry = renderState.pokeState === 'angry' || currentProfile.palettePreset === 'error';
    const hue = angry ? 6 : stone.hue;
    const sleepy = renderState.mode === 'sleeping';
    const busy = ['thinking', 'speaking', 'rendering', 'repairing'].includes(renderState.mode);
    const base = new THREE.Color().setHSL(
      hue / 360,
      angry ? 0.56 : sleepy ? 0.12 : busy ? 0.42 : 0.32,
      angry ? 0.32 : sleepy ? 0.2 : busy ? 0.34 : 0.28,
    );
    const edgeColor = new THREE.Color().setHSL(
      hue / 360,
      angry ? 0.62 : sleepy ? 0.14 : 0.32,
      angry ? 0.54 : sleepy ? 0.38 : busy ? 0.64 : 0.54,
    );

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

    const grooveColor = new THREE.Color().setHSL(
      stone.face.grooveHue / 360,
      sleepy ? Math.max(0.28, stone.face.grooveSaturation * 0.58) : stone.face.grooveSaturation,
      sleepy ? Math.max(0.58, stone.face.grooveLightness * 0.78) : stone.face.grooveLightness,
    );
	    const grooveMaterial = new THREE.MeshBasicMaterial({
	      color: grooveColor,
	    });
	    const mouthGlowMaterial = new THREE.MeshBasicMaterial({
	      color: grooveColor,
	      transparent: true,
	      opacity: sleepy ? 0.2 : 0.44,
	      depthWrite: false,
	      blending: THREE.AdditiveBlending,
	    });
	    const faceGroup = new THREE.Group();
	    const zFace = 1.18;
	    const { eyeShape, mouthShape, eyeY, mouthY, eyeSpacing, eyeWidth, eyeSlant, mouthWidth, mouthCurve, grooveDepth, grooveHeight } = stone.face;
	    const addGroove = (x: number, y: number, width: number, angle = 0, glow = false) => {
	      const group = new THREE.Group();
	      const groove = new THREE.Mesh(new THREE.BoxGeometry(width, grooveHeight, grooveDepth), grooveMaterial);
	      if (glow) {
	        const mouthGlow = new THREE.Mesh(
	          new THREE.BoxGeometry(width, grooveHeight, grooveDepth * 0.72),
	          mouthGlowMaterial,
	        );
	        mouthGlow.position.z = 0.01;
	        group.add(mouthGlow);
	      }
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
	        const glow = new THREE.Mesh(new THREE.ShapeGeometry(shape), mouthGlowMaterial);
	        const mouth = new THREE.Mesh(new THREE.ShapeGeometry(shape), grooveMaterial);
	        glow.position.z = 0.01;
	        group.add(glow);
	        group.add(mouth);
	        group.position.set(0.02, mouthY + mouthCurve * 0.5, zFace + 0.018);
	        group.rotation.z = mouthCurve * 0.08;
	        faceGroup.add(group);
	        return group;
	      }
	      return addGroove(0.02, mouthY + mouthCurve * 0.5, mouthWidth * 0.74, mouthCurve * 0.12, true);
	    };
	    const mouthLine = addMouth();
    visualGroup.add(faceGroup);
    const baseScaleX = fitToCanvas ? 0.82 : 0.9;
    const baseScaleY = fitToCanvas ? 0.9 : 0.98;
    const baseScaleZ = fitToCanvas ? 0.82 : 0.9;
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

    let frameId = 0;
    const started = performance.now();
    const animate = () => {
      const t = (performance.now() - started) * 0.001;
      const runtimeAngry = renderState.pokeState === 'angry' || renderState.mode === 'error';
	      if (runtimeAngry) {
        material.color.setHSL(6 / 360, 0.45, 0.9);
        edgeMaterial.color.setHSL(6 / 360, 0.62, 0.54);
        grooveMaterial.color.setHSL(54 / 360, 0.9, 0.84);
        mouthGlowMaterial.color.setHSL(54 / 360, 0.9, 0.84);
      } else {
        material.color.setRGB(1, 1, 1);
        edgeMaterial.color.copy(edgeColor);
        grooveMaterial.color.copy(grooveColor);
        mouthGlowMaterial.color.copy(grooveColor);
      }
      const modeSpeed = renderState.mode === 'speaking' || renderState.hasSpeech ? 0.92 : renderState.mode === 'sleeping' ? 0.22 : 0.58;
      const motion = renderState.motionScale * modeSpeed;
      const glanceCycle = 2.6;
      const glanceIndex = Math.floor(t / glanceCycle);
      const glanceProgress = smoothStep((t % glanceCycle) / glanceCycle);
      const previousGlance = seededSigned(currentProfile.seed, 1700 + glanceIndex) * 0.11;
      const nextGlance = seededSigned(currentProfile.seed, 1701 + glanceIndex) * 0.11;
      const glanceYaw = previousGlance + (nextGlance - previousGlance) * glanceProgress;
      const breathe = Math.sin(t * (1.25 + motion * 0.18));
      const now = performance.now();
      const wakeBurst = renderState.wakeStartedAt > 0 ? Math.max(0, 1 - ((now - renderState.wakeStartedAt) / 1000) * 1.8) : 0;
      const pokeBurst = renderState.pokeStartedAt > 0 ? Math.max(0, 1 - ((now - renderState.pokeStartedAt) / 1000) * 5.2) : 0;
      const alertNod = busy ? Math.sin(t * 2.2) * 0.018 : 0;
      const speakingNod = renderState.hasSpeech ? Math.sin(t * 5.8) * 0.012 : 0;
	      const sleepYawDrift = sleepy ? Math.sin(t * 0.16 + 0.5) * 0.018 : 0;
	      const sleepHeadDrop = sleepy ? 0.11 : 0;
	      const sleepSideTilt = sleepy ? 0.095 + seededSigned(currentProfile.seed, 1734) * 0.035 : 0;
	      const yaw = -0.24 + renderState.userYaw + (sleepy ? sleepYawDrift : glanceYaw * 0.42) + Math.sin(t * 0.48) * 0.016 * motion;
	      const pitch = 0.03 + sleepHeadDrop + renderState.userPitch + breathe * (sleepy ? 0.024 : 0.017) * motion + alertNod + speakingNod + wakeBurst * 0.06 + pokeBurst * 0.045;
      const roll = (sleepy ? sleepSideTilt : Math.sin(t * 0.42 + 0.8) * 0.012 * motion) + pokeBurst * seededSigned(currentProfile.seed, 1730) * 0.028;
      const angryShake = runtimeAngry ? Math.sin(t * 18) * 0.026 : 0;
      stoneGroup.rotation.set(pitch, yaw + angryShake + wakeBurst * Math.sin(t * 8) * 0.04, roll);
      const breatheScale = sleepy ? 0.016 : 0.018;
      stoneGroup.scale.set(
        baseScaleX + breathe * breatheScale * 0.72 + wakeBurst * 0.026 + pokeBurst * 0.018,
        baseScaleY - breathe * breatheScale * 0.5 - wakeBurst * 0.012 - pokeBurst * 0.01,
        baseScaleZ + wakeBurst * 0.016,
      );
	      stoneGroup.position.y = Math.sin(t * (sleepy ? 0.52 : 1.05)) * (sleepy ? 0.016 : 0.018) * motion - wakeBurst * 0.04;

      const blinkEvery = sleepy ? 2.8 : 3.8 + seededUnit(currentProfile.seed, 1710) * 1.4;
      const blinkPhase = t % blinkEvery;
      const blink = Math.max(pulseAt(blinkPhase, blinkEvery - 0.16, 0.08), pulseAt(blinkPhase, blinkEvery - 0.04, 0.05));
      const eyeOpen = sleepy ? 0.14 + Math.sin(t * 0.58) * 0.025 : 1 - blink * 0.86;
      const angryBrow = runtimeAngry ? 1 : 0;
      leftEye.scale.y = eyeOpen;
      rightEye.scale.y = eyeOpen * (1 - blink * 0.08);
      leftEye.position.y = eyeY - (1 - eyeOpen) * 0.008;
      rightEye.position.y = eyeY - (1 - eyeOpen) * 0.008;
      leftEye.rotation.z = leftEyeBaseRotation - angryBrow * 0.28;
      rightEye.rotation.z = rightEyeBaseRotation + angryBrow * 0.28;

      const talking = renderState.hasSpeech || renderState.mode === 'speaking';
      const talkA = talking
        ? 0.5 + 0.5 * Math.sin(t * 10.5 + seededUnit(currentProfile.seed, 1720) * Math.PI)
        : 0;
      const talkB = talking ? 0.5 + 0.5 * Math.sin(t * 15.5 + 0.8) : 0;
	      const talkOpen = talking ? 0.08 + Math.max(talkA, talkB) * 0.42 : renderState.mode === 'sleeping' ? 0 : 0.02;
	      mouthLine.position.y = mouthY + mouthCurve * 0.5 - talkOpen * 0.018;
	      mouthLine.scale.x = 1 + talkOpen * 0.06;
	      mouthLine.scale.y = 1 + talkOpen * 1.8;
      renderer.render(scene, camera);
      frameId = requestAnimationFrame(animate);
    };
    animate();

    return () => {
      cancelAnimationFrame(frameId);
	      geometry.dispose();
	      edgeGeometry.dispose();
      faceGroup.traverse((child) => {
	        if (child instanceof THREE.Mesh) child.geometry.dispose();
	      });
      material.dispose();
      edgeMaterial.dispose();
      grooveMaterial.dispose();
      mouthGlowMaterial.dispose();
      renderer.dispose();
    };
  }

  $effect(() => {
    if (!stoneCanvas) return;
    return renderStone(stoneCanvas, geometryProfile, stoneRuntime);
  });

  onDestroy(() => {
    if (copyFeedbackTimer) clearTimeout(copyFeedbackTimer);
    if (pokeResetTimer) clearTimeout(pokeResetTimer);
    if (angryTimer) clearTimeout(angryTimer);
  });
</script>

<div
  class="genie-shell"
  data-agent-connected={agentConnected ? 'true' : 'false'}
  style={`--genie-safe-right: ${Math.max(0, safeRightInset)}px;`}
>
  <button
    class="genie-stone-button"
    type="button"
	    aria-label="Poke Ecky"
	    data-seed={geometryProfile.seed}
	    data-poke-state={pokeState}
	    data-drag-revision={dragRevision}
	    style={`--repel-x: ${repelX}px; --repel-y: ${repelY}px;`}
	    onpointerdown={startStoneDrag}
	    onpointermove={moveStoneDrag}
	    onpointerup={endStoneDrag}
	    onpointercancel={endStoneDrag}
	    onclick={pokeGenie}
	  >
	    <canvas
	      class="genie-stone-canvas"
	      data-mode={effectiveMode}
	      bind:this={stoneCanvas}
      width="150"
      height="150"
	      aria-hidden="true"
	    ></canvas>
	  </button>
  {#if cleanBubble}
    <div class="genie-bubble" class:genie-bubble--compact={compact} data-bubble-layout={compact ? 'compact' : 'full'}>
      <button class="bubble-copy" type="button" onclick={copyBubbleText} aria-label="Copy advisor response">
        {copyFeedback || 'COPY'}
      </button>
      <button class="bubble-close" type="button" onclick={() => onDismiss?.()} aria-label="Dismiss advisor bubble"></button>
      <div class="bubble-header">
        {#if compact}
          <div class="bubble-meta">
            {#if badge}
              <span class="bubble-badge">{badge}</span>
            {/if}
            {#if contextLabel}
              <span class="bubble-context">{contextLabel}</span>
            {/if}
          </div>
        {:else}
          <div class="bubble-speaker"><strong>ECKY EINACS:</strong></div>
        {/if}
      </div>
      {#if !compact && cleanQuestion}
        <div class="bubble-question-block">
          <div class="bubble-question-label">YOU ASKED</div>
          <div class="bubble-question">"{cleanQuestion}"</div>
        </div>
      {/if}
      <div class="bubble-text">{cleanBubble}</div>
      {#if actions?.length}
        <div class="bubble-actions">
          {#each actions as action}
            <button class="bubble-action-btn" type="button" onclick={action.onclick}>{action.label}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .genie-shell {
    position: relative;
    width: 150px;
    height: 150px;
    pointer-events: none;
  }

  .genie-stone-button {
    position: absolute;
    inset: 0;
    width: 150px;
    height: 150px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
	    overflow: hidden;
	    pointer-events: auto;
	    z-index: 1;
	    transform: translate(var(--repel-x, 0), var(--repel-y, 0));
	    transition: transform 140ms steps(2, end);
	  }

  .genie-stone-button:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--secondary) 74%, var(--text));
    outline-offset: -6px;
  }

	  .genie-stone-canvas {
	    width: 150px;
	    height: 150px;
	    display: block;
	    pointer-events: none;
	  }

	  .genie-bubble {
    position: absolute;
    left: 126px;
    top: 6px;
    width: min(380px, max(248px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    max-width: min(380px, max(248px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    min-height: 74px;
    max-height: min(34vh, 240px);
    padding: 12px 72px 12px 14px;
    border: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.74rem;
    line-height: 1.42;
    text-transform: none;
    letter-spacing: 0.01em;
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent), var(--shadow);
    backdrop-filter: blur(9px);
    pointer-events: auto;
    -webkit-user-select: text !important;
    user-select: text !important;
    overflow-y: auto;
  }

  .genie-bubble--compact {
    width: min(340px, max(236px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    max-width: min(340px, max(236px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    min-height: 66px;
    max-height: min(24vh, 176px);
    padding: 10px 64px 10px 12px;
    font-size: 0.72rem;
    line-height: 1.38;
  }

  .genie-bubble::before {
    content: '';
    position: absolute;
    left: -12px;
    top: 26px;
    width: 12px;
    height: 20px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-top: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-bottom: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
  }

  .genie-bubble::after {
    content: '';
    position: absolute;
    left: -18px;
    top: 31px;
    width: 6px;
    height: 10px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-top: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-bottom: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
  }

  .genie-bubble::selection,
  .genie-bubble *::selection {
    background: color-mix(in srgb, var(--primary) 52%, transparent);
    color: var(--text);
  }

  .bubble-copy,
  .bubble-close {
    position: absolute;
    top: 8px;
    height: 18px;
    border: 2px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, transparent);
    cursor: pointer;
    padding: 0;
    font-family: var(--font-mono);
    line-height: 1;
  }

  .bubble-copy {
    right: 34px;
    min-width: 38px;
    height: 18px;
    padding: 0 5px;
    color: var(--text-dim);
    font-size: 0.54rem;
    letter-spacing: 0.06em;
  }

  .bubble-copy:hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .bubble-close {
    right: 10px;
    width: 18px;
  }

  .bubble-close::before,
  .bubble-close::after {
    content: '';
    position: absolute;
    left: 3px;
    top: 7px;
    width: 10px;
    height: 2px;
    background: var(--text-dim);
  }

  .bubble-close::before {
    transform: rotate(45deg);
  }

  .bubble-close::after {
    transform: rotate(-45deg);
  }

  .bubble-close:hover {
    border-color: var(--secondary);
  }

  .bubble-close:hover::before,
  .bubble-close:hover::after {
    background: var(--secondary);
  }

  .bubble-text {
    white-space: pre-wrap;
    word-break: break-word;
    text-wrap: pretty;
    -webkit-user-select: text !important;
    user-select: text !important;
    max-width: 100%;
  }

  .bubble-header {
    display: flex;
    align-items: flex-start;
    min-height: 16px;
    margin-bottom: 6px;
    min-width: 0;
  }

  .bubble-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .bubble-badge,
  .bubble-context {
    min-width: 0;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    padding: 2px 6px;
    font-size: 0.56rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .bubble-badge {
    color: var(--secondary);
    border-color: color-mix(in srgb, var(--secondary) 54%, var(--bg-300));
  }

  .bubble-context {
    color: var(--text-dim);
    max-width: 132px;
  }

  .bubble-question-block {
    margin-bottom: 10px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--bg-300) 85%, transparent);
    background: color-mix(in srgb, var(--bg) 54%, transparent);
    max-height: 18vh;
    overflow-y: auto;
  }

  .bubble-question-label {
    margin-bottom: 4px;
    color: var(--text-dim);
    font-size: 0.62rem;
    letter-spacing: 0.06em;
  }

  .bubble-question {
    color: var(--text-dim);
    font-size: 0.74rem;
    line-height: 1.45;
    -webkit-user-select: text !important;
    user-select: text !important;
  }

  .bubble-speaker {
    color: var(--secondary);
    letter-spacing: 0.06em;
    font-size: 0.64rem;
  }

  .bubble-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 14px;
    padding-top: 10px;
    border-top: 1px solid color-mix(in srgb, var(--bg-300) 70%, transparent);
  }

  .bubble-action-btn {
    padding: 5px 14px;
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    font-weight: bold;
    letter-spacing: 0.06em;
    cursor: pointer;
  }

  .bubble-action-btn:hover {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-300));
  }

  @media (max-width: 960px) {
    .genie-bubble {
      left: 14px;
      top: 126px;
      width: min(calc(100vw - 28px), 320px);
      max-width: min(calc(100vw - 28px), 320px);
      min-height: 72px;
      max-height: min(32vh, 220px);
      font-size: 0.72rem;
      line-height: 1.4;
    }

    .genie-bubble--compact {
      min-height: 64px;
      max-height: min(24vh, 160px);
    }
  }
</style>
