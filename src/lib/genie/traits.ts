import type { GenieEyeStyle, GenieTraits } from '../types/domain';

export type GenieMode =
  | 'idle'
  | 'sleeping'
  | 'waking'
  | 'thinking'
  | 'light'
  | 'rendering'
  | 'repairing'
  | 'speaking'
  | 'error';

type PalettePreset =
  | 'base'
  | 'sleeping'
  | 'waking'
  | 'thinking'
  | 'light'
  | 'repairing'
  | 'rendering'
  | 'speaking'
  | 'error';

type MouthStyle = 'curve' | 'line';

export type SeedOffsets = {
  orbit: number;
  hover: number;
  pulse: number;
  jitter: number;
  warp: number;
  blink: number;
  mouth: number;
  eyeX: number;
  eyeY: number;
  asym: number;
  chord: number;
};

export type ResolvedGenieProfile = {
  version: number;
  seed: number;
  palettePreset: PalettePreset;
  colorHue: number;
  glowHueShift: number;
  vertexCount: number;
  radiusBase: number;
  stretchY: number;
  asymmetry: number;
  chordSkip: number;
  jitterScale: number;
  pulseScale: number;
  hoverScale: number;
  warpScale: number;
  eyeStyle: GenieEyeStyle;
  eyeSpacing: number;
  eyeSize: number;
  mouthCurve: number;
  mouthStyle: MouthStyle;
  mouthOpenBase: number;
  mouthOpenAmplitude: number;
  chordAlpha: number;
  spokeStride: number | null;
  nodeRadius: number;
  centerOrbitAmplitude: number;
  centerOrbitFrequency: number;
  tiltScale: number;
  lineWidth: number;
  seedOffsets: SeedOffsets;
};

const TAU = Math.PI * 2;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function normalizeHue(value: number): number {
  return ((value % 360) + 360) % 360;
}

function mixSeed(seed: number, channel: number): number {
  let value = (seed ^ Math.imul(channel + 1, 0x9e3779b1)) >>> 0;
  value ^= value >>> 16;
  value = Math.imul(value, 0x7feb352d) >>> 0;
  value ^= value >>> 15;
  value = Math.imul(value, 0x846ca68b) >>> 0;
  value ^= value >>> 16;
  return value >>> 0;
}

export function seededUnit(seed: number, channel: number): number {
  return mixSeed(seed, channel) / 0xffffffff;
}

export function seededSigned(seed: number, channel: number): number {
  return seededUnit(seed, channel) * 2 - 1;
}

export const DEFAULT_GENIE_TRAITS: GenieTraits = {
  version: 2,
  seed: 1,
  colorHue: 144,
  vertexCount: 12,
  radiusBase: 30,
  stretchY: 0.96,
  asymmetry: 1,
  chordSkip: 4,
  jitterScale: 1,
  pulseScale: 1,
  hoverScale: 1,
  warpScale: 1,
  glowHueShift: 0,
  eyeStyle: 'dot',
  eyeSpacing: 19,
  eyeSize: 2.7,
  mouthCurve: 1.6,
  thinkingBias: 0.6,
  repairBias: 0.6,
  renderBias: 0.6,
  expressiveness: 0.6,
};

const U64_MASK = 0xffff_ffff_ffff_ffffn;
const FNV_OFFSET_BASIS = 0xcbf2_9ce4_8422_2325n;
const FNV_PRIME = 0x0000_0100_0000_01b3n;

class GeneRng {
  state: bigint;

  constructor(seed: number) {
    this.state = (BigInt(seed >>> 0) ^ 0x9e37_79b9_7f4a_7c15n) & U64_MASK;
  }

  nextU64(): bigint {
    this.state = (this.state + 0x9e37_79b9_7f4a_7c15n) & U64_MASK;
    let z = this.state;
    z = ((z ^ (z >> 30n)) * 0xbf58_476d_1ce4_e5b9n) & U64_MASK;
    z = ((z ^ (z >> 27n)) * 0x94d0_49bb_1331_11ebn) & U64_MASK;
    return (z ^ (z >> 31n)) & U64_MASK;
  }

  unit(): number {
    return Number(this.nextU64() >> 11n) / 2 ** 53;
  }

  rangeF64(min: number, max: number): number {
    return min + (max - min) * this.unit();
  }

  rangeU8(min: number, maxInclusive: number): number {
    return min + Number(this.nextU64() % BigInt(maxInclusive - min + 1));
  }

  eyeStyle(): GenieEyeStyle {
    switch (this.rangeU8(0, 2)) {
      case 0:
        return 'dot';
      case 1:
        return 'bar';
      default:
        return 'slant';
    }
  }
}

export function deriveGenieSeed(identity: string): number {
  let hash = FNV_OFFSET_BASIS;
  const encoded = new TextEncoder().encode(identity);
  for (const byte of encoded) {
    hash ^= BigInt(byte);
    hash = (hash * FNV_PRIME) & U64_MASK;
  }
  const seed = Number(((hash >> 32n) ^ hash) & 0xffff_ffffn) >>> 0;
  return seed === 0 ? 1 : seed;
}

export function buildGenieTraitsFromSeed(seed: number): GenieTraits {
  const normalizedSeed = seed === 0 ? 1 : seed >>> 0;
  const rng = new GeneRng(normalizedSeed);
  return {
    version: 2,
    seed: normalizedSeed,
    colorHue: rng.rangeF64(0, 360),
    vertexCount: rng.rangeU8(10, 24),
    radiusBase: rng.rangeF64(25, 34),
    stretchY: rng.rangeF64(0.9, 1.06),
    asymmetry: rng.rangeF64(0.88, 1.14),
    chordSkip: rng.rangeU8(2, 6),
    jitterScale: rng.rangeF64(0.7, 1.45),
    pulseScale: rng.rangeF64(0.7, 1.35),
    hoverScale: rng.rangeF64(0.8, 1.6),
    warpScale: rng.rangeF64(0.35, 1.25),
    glowHueShift: rng.rangeF64(-32, 32),
    eyeStyle: rng.eyeStyle(),
    eyeSpacing: rng.rangeF64(15, 22.5),
    eyeSize: rng.rangeF64(2, 3.6),
    mouthCurve: rng.rangeF64(0.6, 2.6),
    thinkingBias: rng.rangeF64(0.2, 1),
    repairBias: rng.rangeF64(0.2, 1),
    renderBias: rng.rangeF64(0.2, 1),
    expressiveness: rng.rangeF64(0.35, 1),
  };
}

export function buildAgentGenieTraits(agentIdentity: string | null | undefined): GenieTraits {
  const identity = agentIdentity?.trim().toLowerCase() || 'ecky';
  return buildGenieTraitsFromSeed(deriveGenieSeed(`agent:${identity}`));
}

export function normalizeGenieTraits(
  input: Partial<GenieTraits> | null | undefined,
): GenieTraits {
  const traits = { ...DEFAULT_GENIE_TRAITS, ...(input ?? {}) };
  return {
    version: traits.version || DEFAULT_GENIE_TRAITS.version,
    seed: traits.seed || DEFAULT_GENIE_TRAITS.seed,
    colorHue: normalizeHue(traits.colorHue),
    vertexCount: Math.round(clamp(traits.vertexCount, 10, 24)),
    radiusBase: clamp(traits.radiusBase, 25, 34),
    stretchY: clamp(traits.stretchY, 0.9, 1.06),
    asymmetry: clamp(traits.asymmetry, 0.88, 1.14),
    chordSkip: Math.round(clamp(traits.chordSkip, 2, 6)),
    jitterScale: clamp(traits.jitterScale, 0.7, 1.45),
    pulseScale: clamp(traits.pulseScale, 0.7, 1.35),
    hoverScale: clamp(traits.hoverScale, 0.8, 1.6),
    warpScale: clamp(traits.warpScale, 0.35, 1.25),
    glowHueShift: clamp(traits.glowHueShift, -32, 32),
    eyeStyle: traits.eyeStyle,
    eyeSpacing: clamp(traits.eyeSpacing, 15, 22.5),
    eyeSize: clamp(traits.eyeSize, 2, 3.6),
    mouthCurve: clamp(traits.mouthCurve, 0.6, 2.6),
    thinkingBias: clamp(traits.thinkingBias, 0.2, 1),
    repairBias: clamp(traits.repairBias, 0.2, 1),
    renderBias: clamp(traits.renderBias, 0.2, 1),
    expressiveness: clamp(traits.expressiveness, 0.35, 1),
  };
}

function buildSeedOffsets(seed: number): SeedOffsets {
  return {
    orbit: seededUnit(seed, 1) * TAU,
    hover: seededUnit(seed, 2) * TAU,
    pulse: seededUnit(seed, 3) * TAU,
    jitter: seededUnit(seed, 4) * TAU,
    warp: seededUnit(seed, 5) * TAU,
    blink: seededUnit(seed, 6) * TAU,
    mouth: seededUnit(seed, 7) * TAU,
    eyeX: seededSigned(seed, 8),
    eyeY: seededSigned(seed, 9),
    asym: seededSigned(seed, 10),
    chord: seededUnit(seed, 11) * TAU,
  };
}

export function resolveModeTraits(
  baseInput: Partial<GenieTraits> | null | undefined,
  mode: GenieMode,
): ResolvedGenieProfile {
  const base = normalizeGenieTraits(baseInput);
  const thinkingEnergy = 0.85 + base.thinkingBias * 0.55;
  const repairEnergy = 0.90 + base.repairBias * 0.65;
  const renderEnergy = 0.95 + base.renderBias * 0.70;
  const expressive = base.expressiveness;

  const resolved: ResolvedGenieProfile = {
    version: base.version,
    seed: base.seed,
    palettePreset: 'base',
    colorHue: base.colorHue,
    glowHueShift: base.glowHueShift,
    vertexCount: base.vertexCount,
    radiusBase: base.radiusBase,
    stretchY: base.stretchY,
    asymmetry: base.asymmetry,
    chordSkip: base.chordSkip,
    jitterScale: base.jitterScale,
    pulseScale: base.pulseScale,
    hoverScale: base.hoverScale,
    warpScale: base.warpScale,
    eyeStyle: base.eyeStyle,
    eyeSpacing: base.eyeSpacing,
    eyeSize: base.eyeSize,
    mouthCurve: base.mouthCurve,
    mouthStyle: 'curve',
    mouthOpenBase: 1.15 + base.mouthCurve * 0.45,
    mouthOpenAmplitude: 0.18 + expressive * 0.14,
    chordAlpha: 0.16,
    spokeStride: null,
    nodeRadius: 2.3,
    centerOrbitAmplitude: 0.85 + (base.hoverScale - 1) * 1.4,
    centerOrbitFrequency: 1.35 + seededUnit(base.seed, 12) * 0.55,
    tiltScale: 0.012 + (base.asymmetry - 1) * 0.08,
    lineWidth: 1.3,
    seedOffsets: buildSeedOffsets(base.seed),
  };

  switch (mode) {
    case 'sleeping':
      resolved.palettePreset = 'sleeping';
      resolved.jitterScale *= 0.4;
      resolved.pulseScale *= 0.6;
      resolved.hoverScale *= 0.5;
      resolved.warpScale *= 0.3;
      resolved.eyeStyle = 'bar';
      resolved.mouthStyle = 'line';
      resolved.mouthOpenBase = 0.4;
      resolved.mouthOpenAmplitude = 0.05;
      resolved.chordAlpha = 0.08;
      resolved.nodeRadius = 1.8;
      resolved.centerOrbitAmplitude *= 0.4;
      resolved.centerOrbitFrequency *= 0.5;
      break;
    case 'thinking':
      resolved.palettePreset = 'thinking';
      resolved.glowHueShift += 10;
      resolved.vertexCount = Math.round(clamp(base.vertexCount + 4 + base.thinkingBias * 4, 12, 28));
      resolved.radiusBase = base.radiusBase * 0.93;
      resolved.stretchY = clamp(base.stretchY - 0.02, 0.9, 1.04);
      resolved.chordSkip = Math.round(clamp(base.chordSkip - 1, 2, 5));
      resolved.jitterScale = base.jitterScale * thinkingEnergy;
      resolved.pulseScale = base.pulseScale * (1.05 + base.thinkingBias * 0.35);
      resolved.hoverScale = base.hoverScale * (0.95 + base.thinkingBias * 0.18);
      resolved.warpScale = base.warpScale * (0.9 + base.thinkingBias * 0.22);
      resolved.eyeStyle = 'bar';
      resolved.eyeSize = base.eyeSize * 0.92;
      resolved.mouthStyle = 'line';
      resolved.mouthOpenBase = 0.95 + base.mouthCurve * 0.15;
      resolved.mouthOpenAmplitude = 0.08 + expressive * 0.06;
      resolved.chordAlpha = 0.22;
      resolved.spokeStride = 2;
      resolved.nodeRadius = 2.15;
      resolved.centerOrbitAmplitude *= 0.72;
      resolved.centerOrbitFrequency += 0.25;
      resolved.lineWidth = 1.4;
      break;
    case 'light':
      resolved.palettePreset = 'light';
      resolved.glowHueShift -= 18;
      resolved.vertexCount = Math.round(clamp(base.vertexCount - 2, 10, 24));
      resolved.radiusBase = base.radiusBase * 0.94;
      resolved.chordSkip = Math.round(clamp(base.chordSkip + 1, 3, 6));
      resolved.jitterScale = base.jitterScale * 0.8;
      resolved.pulseScale = base.pulseScale * 0.84;
      resolved.hoverScale = base.hoverScale * 0.76;
      resolved.warpScale = base.warpScale * 0.55;
      resolved.chordAlpha = 0.10;
      resolved.spokeStride = base.thinkingBias > 0.55 ? 4 : null;
      resolved.nodeRadius = 2.05;
      resolved.centerOrbitAmplitude *= 0.6;
      resolved.centerOrbitFrequency -= 0.08;
      break;
    case 'repairing':
      resolved.palettePreset = 'repairing';
      resolved.glowHueShift += 18;
      resolved.vertexCount = Math.round(clamp(base.vertexCount + 1 + base.repairBias * 3, 11, 28));
      resolved.asymmetry = clamp(base.asymmetry + 0.04 + base.repairBias * 0.08, 0.88, 1.14);
      resolved.chordSkip = Math.round(clamp(base.chordSkip, 2, 5));
      resolved.jitterScale = base.jitterScale * repairEnergy;
      resolved.pulseScale = base.pulseScale * (1.08 + base.repairBias * 0.26);
      resolved.hoverScale = base.hoverScale * (1.0 + base.repairBias * 0.2);
      resolved.warpScale = base.warpScale * (1.15 + base.repairBias * 0.35);
      resolved.eyeStyle = 'slant';
      resolved.eyeSize = base.eyeSize * 0.96;
      resolved.mouthStyle = 'line';
      resolved.mouthOpenBase = 1.0 + base.mouthCurve * 0.18;
      resolved.mouthOpenAmplitude = 0.14 + expressive * 0.12;
      resolved.chordAlpha = 0.18;
      resolved.spokeStride = 3;
      resolved.centerOrbitAmplitude *= 1.1;
      resolved.centerOrbitFrequency += 0.18;
      break;
    case 'rendering':
      resolved.palettePreset = 'rendering';
      resolved.glowHueShift += 32;
      resolved.vertexCount = Math.round(clamp(base.vertexCount + 2 + base.renderBias * 4, 12, 28));
      resolved.radiusBase = base.radiusBase * 1.05;
      resolved.chordSkip = Math.round(clamp(base.chordSkip - 1, 2, 5));
      resolved.jitterScale = base.jitterScale * (1.18 + base.renderBias * 0.24);
      resolved.pulseScale = base.pulseScale * renderEnergy;
      resolved.hoverScale = base.hoverScale * (1.2 + base.renderBias * 0.3);
      resolved.warpScale = base.warpScale * (1.2 + base.renderBias * 0.45);
      resolved.mouthOpenBase = 1.4 + base.mouthCurve * 0.35;
      resolved.mouthOpenAmplitude = 0.22 + expressive * 0.18;
      resolved.chordAlpha = 0.20;
      resolved.spokeStride = 3;
      resolved.nodeRadius = 2.35;
      resolved.centerOrbitAmplitude *= 1.35;
      resolved.centerOrbitFrequency += 0.35;
      resolved.lineWidth = 1.45;
      break;
    case 'speaking':
      resolved.palettePreset = 'speaking';
      resolved.eyeSize = base.eyeSize * (1.05 + expressive * 0.18);
      resolved.jitterScale = base.jitterScale * 1.02;
      resolved.pulseScale = base.pulseScale * 1.04;
      resolved.mouthOpenBase = 2.1 + base.mouthCurve * 0.55;
      resolved.mouthOpenAmplitude = 2.0 + expressive * 1.9;
      resolved.chordAlpha = 0.14;
      resolved.centerOrbitAmplitude *= 0.95;
      break;
    case 'error':
      resolved.palettePreset = 'error';
      resolved.glowHueShift += 56;
      resolved.vertexCount = Math.round(clamp(base.vertexCount + Math.max(1, Math.round(base.repairBias * 2)), 11, 28));
      resolved.asymmetry = clamp(base.asymmetry + 0.08, 0.88, 1.14);
      resolved.jitterScale = base.jitterScale * (1.35 + base.repairBias * 0.3);
      resolved.pulseScale = base.pulseScale * (1.08 + base.repairBias * 0.18);
      resolved.hoverScale = base.hoverScale * 1.1;
      resolved.warpScale = base.warpScale * (1.35 + base.repairBias * 0.4);
      resolved.eyeStyle = base.eyeStyle === 'dot' ? 'slant' : base.eyeStyle;
      resolved.mouthCurve = -Math.max(0.9, base.mouthCurve * 0.8);
      resolved.mouthOpenBase = 0.55;
      resolved.mouthOpenAmplitude = 0.05;
      resolved.chordAlpha = 0.18;
      resolved.spokeStride = null;
      resolved.centerOrbitAmplitude *= 1.05;
      resolved.centerOrbitFrequency += 0.12;
      break;
    case 'waking':
      resolved.palettePreset = 'waking';
      resolved.jitterScale *= 0.5;
      resolved.pulseScale *= 0.65;
      resolved.hoverScale *= 0.6;
      resolved.warpScale *= 0.35;
      // eye style handled per-eye in VertexGenie — one bar, one opening dot
      resolved.eyeStyle = 'dot';
      resolved.mouthStyle = 'line';
      resolved.mouthOpenBase = 0.5;
      resolved.mouthOpenAmplitude = 0.06;
      resolved.chordAlpha = 0.09;
      resolved.nodeRadius = 1.9;
      resolved.centerOrbitAmplitude *= 0.5;
      resolved.centerOrbitFrequency *= 0.65;
      break;
    case 'idle':
    default:
      break;
  }

  return resolved;
}
