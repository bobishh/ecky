import type { ResolvedGenieProfile } from './traits';
import { seededSigned, seededUnit } from './traits';

export type StonePoint3 = {
  x: number;
  y: number;
  z: number;
};

export type StoneFaceGeometry = {
  eyeShape: 'bar' | 'dot' | 'oval' | 'triangle';
  mouthShape: 'line' | 'triangle';
  eyeY: number;
  mouthY: number;
  eyeSpacing: number;
  eyeWidth: number;
  eyeSlant: number;
  mouthWidth: number;
  mouthCurve: number;
  grooveDepth: number;
  grooveHeight: number;
  grooveHue: number;
  grooveSaturation: number;
  grooveLightness: number;
};

export type StoneSpikeGeometry = {
  x: number;
  y: number;
  z: number;
  scale: number;
  rotation: number;
};

export type StoneGeometry = {
  hue: number;
  front: StonePoint3[];
  rim: StonePoint3[];
  back: StonePoint3[];
  center: StonePoint3;
  spikes: StoneSpikeGeometry[];
  face: StoneFaceGeometry;
};

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function normalizeHue(value: number): number {
  return ((value % 360) + 360) % 360;
}

function normalizeOperationalHue(value: number): number {
  const hue = normalizeHue(value);
  const lanes = [126, 154, 184, 214, 246, 276];
  if (hue >= 90 && hue <= 290) return hue;
  return lanes[Math.floor(seededUnit(Math.round(hue * 1000) || 1, 1209) * lanes.length)] ?? 154;
}

function buildGroovePalette(profile: ResolvedGenieProfile, hue: number) {
  if (profile.palettePreset === 'error') {
    return {
      hue: 54,
      saturation: 0.9,
      lightness: 0.82,
    };
  }

  const candidates = [
    { hue: 52, saturation: 0.9, lightness: 0.82 },
    { hue: 82, saturation: 0.86, lightness: 0.74 },
    { hue: 138, saturation: 0.82, lightness: 0.72 },
    { hue: 184, saturation: 0.78, lightness: 0.78 },
    { hue: 275, saturation: 0.74, lightness: 0.84 },
    { hue: 0, saturation: 0.02, lightness: 0.92 },
  ];
  const offset =
    profile.palettePreset === 'rendering'
      ? 2
      : profile.palettePreset === 'repairing'
        ? 3
        : profile.palettePreset === 'thinking'
          ? 1
          : 0;
  const ranked = [...candidates].sort((a, b) => {
    const distanceA = Math.min(Math.abs(a.hue - hue), 360 - Math.abs(a.hue - hue));
    const distanceB = Math.min(Math.abs(b.hue - hue), 360 - Math.abs(b.hue - hue));
    return distanceB - distanceA;
  });
  const pick = ranked[(Math.floor(seededUnit(profile.seed, 1561) * ranked.length) + offset) % ranked.length];
  return {
    hue: pick.hue,
    saturation: pick.saturation,
    lightness: clamp(pick.lightness + seededSigned(profile.seed, 1562) * 0.035, 0.7, 0.94),
  };
}

export function buildStoneGeometry(profile: ResolvedGenieProfile): StoneGeometry {
  const hue = profile.palettePreset === 'error' ? 6 : normalizeOperationalHue(profile.colorHue);
  const groovePalette = buildGroovePalette(profile, hue);
  const modeVertexDelta =
    profile.palettePreset === 'rendering'
      ? 3
      : profile.palettePreset === 'repairing'
        ? 2
        : profile.palettePreset === 'thinking'
          ? 1
          : profile.palettePreset === 'error'
            ? 1
            : 0;
  const frontCount = Math.round(clamp(10 + (profile.vertexCount - 10) * 0.18 + modeVertexDelta, 10, 14));
  const radiusScale = profile.radiusBase / 30;
  const vertexScale = (profile.vertexCount - 16) / 8;
  const warpScale = profile.warpScale - 1;
  const jitterScale = profile.jitterScale;
  const asymmetry = profile.asymmetry - 1;
  const modeScale =
    profile.palettePreset === 'sleeping'
      ? 0.84
      : profile.palettePreset === 'rendering'
        ? 1.12
        : profile.palettePreset === 'repairing' || profile.palettePreset === 'error'
          ? 1.08
          : profile.palettePreset === 'thinking'
            ? 0.96
            : 1;
  const modeSkew =
    profile.palettePreset === 'repairing'
      ? 0.1
      : profile.palettePreset === 'error'
        ? -0.1
        : profile.palettePreset === 'rendering'
          ? 0.06
          : 0;
  const sideMass = Math.max(0, asymmetry);
  const faceRadiusX =
    (0.76 + vertexScale * 0.046 + seededSigned(profile.seed, 1300) * 0.026) * radiusScale * modeScale;
  const faceRadiusY =
    (0.72 + vertexScale * 0.045 + seededSigned(profile.seed, 1301) * 0.025) *
    profile.stretchY *
    radiusScale *
    modeScale;
  const rimRadiusX =
    (1.15 + vertexScale * 0.075 + seededSigned(profile.seed, 1302) * 0.055 + warpScale * 0.08) *
    radiusScale *
    modeScale;
  const rimRadiusY =
    (1.24 + vertexScale * 0.065 + seededSigned(profile.seed, 1303) * 0.05 + warpScale * 0.09) *
    profile.stretchY *
    radiusScale *
    modeScale;
  const backRadiusX =
    (0.62 + vertexScale * 0.032 + seededSigned(profile.seed, 1304) * 0.04) * radiusScale * modeScale;
  const backRadiusY =
    (0.72 + vertexScale * 0.032 + seededSigned(profile.seed, 1305) * 0.04) *
    profile.stretchY *
    radiusScale *
	    modeScale;
  const shapeFamily = Math.floor(seededUnit(profile.seed, 1310) * 4);
  const faceSquareness =
    shapeFamily === 1 || shapeFamily === 2
      ? 0.22 + seededUnit(profile.seed, 1311) * 0.2
      : 0.08 + seededUnit(profile.seed, 1311) * 0.14;
  const faceTrapezoid =
    shapeFamily === 2
      ? -0.22 - seededUnit(profile.seed, 1312) * 0.1
      : shapeFamily === 3
        ? 0.2 + seededUnit(profile.seed, 1312) * 0.12
        : seededSigned(profile.seed, 1312) * 0.08 + asymmetry * 0.05;
  const faceCornerLift =
    shapeFamily === 1 ? -0.08 : shapeFamily === 3 ? 0.08 : seededSigned(profile.seed, 1313) * 0.04;
  const rimFacetStrength = 0.05 + seededUnit(profile.seed, 1314) * 0.05 + Math.max(0, warpScale) * 0.03;
  const rimPhase = seededUnit(profile.seed, 1315) * Math.PI * 2;
  const eyeRoll = seededUnit(profile.seed, 1563);
  const mouthRoll = seededUnit(profile.seed, 1564);
  const eyeShape = eyeRoll < 0.28 ? 'dot' : eyeRoll < 0.55 ? 'oval' : eyeRoll < 0.78 ? 'bar' : 'triangle';
  const mouthShape = mouthRoll < 0.34 ? 'triangle' : 'line';
  const spikeCount = frontCount;

  const front: StonePoint3[] = [];
  const rim: StonePoint3[] = [];
  const back: StonePoint3[] = [];
  for (let index = 0; index < frontCount; index++) {
    const t = index / frontCount;
    const angle = -Math.PI / 2 + t * Math.PI * 2;
    const sin = Math.sin(angle);
    const cos = Math.cos(angle);
    const topPlane = sin < -0.66 ? 0.82 : 1;
    const side = cos >= 0 ? 1 : -1;
    const sideAsym = side > 0 ? 1 + asymmetry * 0.18 : 1 - asymmetry * 0.12;
    const antiFishMass = side > 0 ? 1 + sideMass * 0.02 : 1 + sideMass * 0.08;
    const notch =
      1 +
      Math.sin(index * profile.chordSkip + profile.seedOffsets.chord) * 0.018 * profile.warpScale +
      seededSigned(profile.seed, 1330 + index) * 0.018 * jitterScale;
    const shoulder = Math.abs(cos) > 0.55 && sin < -0.08 ? 1.03 + Math.max(0, warpScale) * 0.08 : 1;
    const bottomWeight = sin > 0.68 ? 1.02 + seededUnit(profile.seed, 1338) * 0.04 : 1;
	    const jitter = 1 + seededSigned(profile.seed, 1320 + index) * 0.02 * jitterScale;
	    const squareX = 1 + Math.abs(sin) * faceSquareness;
	    const squareY = 1 + Math.abs(cos) * faceSquareness * 0.72;
	    const trapezoidX = 1 + sin * faceTrapezoid;
	    const flatTop = sin < -0.62 ? 0.9 - faceSquareness * 0.22 : 1;
	    const faceDiagonalCut =
	      shapeFamily === 0
	        ? 1
	        : 1 - Math.max(0, Math.abs(cos) + Math.abs(sin) - 1.12) * (0.16 + faceSquareness * 0.2);
	    const rimShard =
	      1 +
	      Math.sin(angle * 3 + rimPhase) * rimFacetStrength * 0.36 +
	      Math.cos(angle * 4 - rimPhase * 0.7) * rimFacetStrength * 0.24 +
	      (index % 2 === 0 ? rimFacetStrength * 0.14 : -rimFacetStrength * 0.08);
	    const rimSquareX = 1 + Math.abs(sin) * (shapeFamily === 1 ? 0.18 : shapeFamily === 3 ? -0.04 : 0.08);
	    const rimSquareY = 1 + Math.abs(cos) * (shapeFamily === 1 ? 0.12 : shapeFamily === 2 ? 0.2 : 0.06);
	    const rimTrapezoid = 1 + sin * (shapeFamily === 2 ? -0.12 : shapeFamily === 3 ? 0.14 : seededSigned(profile.seed, 1316) * 0.06);
	    const punkScale = clamp(0.1 + seededUnit(profile.seed, 1570 + index) * 0.1 + Math.max(0, warpScale) * 0.018, 0.1, 0.22);
	    const punkPulse = 0.68 + seededUnit(profile.seed, 1580 + index) * 0.68 + (index % 3 === 0 ? 0.24 : index % 3 === 1 ? 0.08 : -0.04);
	    const punkRim = 1 + punkScale * punkPulse;
	    const punkBack = 1 + punkScale * (0.48 + seededUnit(profile.seed, 1590 + index) * 0.48);
		    front.push({
	      x: cos * faceRadiusX * sideAsym * antiFishMass * squareX * trapezoidX * faceDiagonalCut * jitter * notch + modeSkew * (0.14 + sin * 0.08),
	      y: sin * faceRadiusY * squareY * flatTop * faceDiagonalCut * jitter * notch + 0.02 + faceCornerLift * Math.abs(cos) + seededSigned(profile.seed, 1380 + index) * 0.006 * jitterScale,
	      z: 1.08,
	    });
	    rim.push({
	      x: cos * rimRadiusX * sideAsym * antiFishMass * shoulder * rimShard * punkRim * rimSquareX * rimTrapezoid * jitter * notch + modeSkew * (0.18 + sin * 0.12),
	      y: sin * rimRadiusY * topPlane * bottomWeight * rimShard * punkRim * rimSquareY * jitter * notch - 0.02,
	      z: 0.2 + punkScale * 0.34 + seededSigned(profile.seed, 1340 + index) * 0.07 * jitterScale,
	    });
	    back.push({
	      x: cos * backRadiusX * sideAsym * antiFishMass * shoulder * rimShard * punkBack * rimSquareX * rimTrapezoid * jitter * notch + modeSkew * (0.16 + sin * 0.1),
	      y: sin * backRadiusY * topPlane * bottomWeight * rimShard * punkBack * rimSquareY * jitter * notch - 0.06,
	      z: -0.5 - punkScale * 0.18 + seededSigned(profile.seed, 1360 + index) * 0.06 * jitterScale,
    });
  }

  return {
	    hue,
	    front,
	    rim,
	    back,
	    center: { x: seededSigned(profile.seed, 1390) * 0.025 + modeSkew * 0.12, y: 0.02, z: 1.1 },
    spikes: Array.from({ length: spikeCount }, (_, index) => {
      const point = rim[index];
      return {
        x: point.x,
        y: point.y,
        z: point.z,
        scale: clamp(0.1 + seededUnit(profile.seed, 1570 + index) * 0.1 + Math.max(0, warpScale) * 0.018, 0.1, 0.22),
        rotation: Math.atan2(point.y, point.x),
      };
    }),
	    face: {
	      eyeShape,
	      mouthShape,
	      eyeY: 0.15 + profile.seedOffsets.eyeY * 0.012 + seededSigned(profile.seed, 1500) * 0.018,
      mouthY: -0.3 + profile.seedOffsets.mouth * 0.012 + seededSigned(profile.seed, 1501) * 0.018,
      eyeSpacing: clamp(0.3 + profile.eyeSpacing * 0.006, 0.33, 0.43),
      eyeWidth: clamp(0.24 + profile.eyeSize * 0.036, 0.28, 0.42),
      eyeSlant: profile.eyeStyle === 'slant' ? 0.08 + profile.seedOffsets.eyeX * 0.015 : 0,
      mouthWidth: clamp(0.42 + Math.abs(profile.mouthCurve) * 0.06, 0.48, 0.64),
      mouthCurve: clamp(profile.mouthCurve * 0.024, -0.1, 0.11),
      grooveDepth: clamp(0.048 + profile.lineWidth * 0.006, 0.05, 0.068),
      grooveHeight: clamp(0.045 + profile.lineWidth * 0.006, 0.048, 0.068),
      grooveHue: groovePalette.hue,
      grooveSaturation: groovePalette.saturation,
      grooveLightness: groovePalette.lightness,
    },
  };
}
