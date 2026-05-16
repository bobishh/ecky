import type { ResolvedGenieProfile } from './traits';
import { seededSigned, seededUnit } from './traits';

export type CornerPoint = {
  x: number;
  y: number;
};

export type CornerEdge = {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};

export type CornerGlyph = {
  seed: number;
  bodyPoints: string;
  facePoints: string;
  nodes: CornerPoint[];
  edges: CornerEdge[];
  selectedEdge: CornerEdge;
  eyeY: number;
  eyeSlant: number;
  mouthY: number;
  mouthCurve: number;
  cornerSharpness: number;
};

function round(value: number): number {
  return Math.round(value * 10) / 10;
}

function pointString(points: CornerPoint[]): string {
  return points.map((point) => `${round(point.x)},${round(point.y)}`).join(' ');
}

function edge(a: CornerPoint, b: CornerPoint): CornerEdge {
  return {
    x1: round(a.x),
    y1: round(a.y),
    x2: round(b.x),
    y2: round(b.y),
  };
}

export function buildCornerGlyph(profile: ResolvedGenieProfile): CornerGlyph {
  const seed = profile.seed;
  const sharpness = 0.58 + seededUnit(seed, 510) * 0.28;
  const width = 58 + seededSigned(seed, 511) * 5 + (profile.vertexCount - 16) * 0.45;
  const height = 72 + seededSigned(seed, 512) * 4 + profile.warpScale * 4;
  const lean = seededSigned(seed, 513) * 4 + profile.tiltScale * 40;
  const waist = 12 + seededUnit(seed, 514) * 7;
  const topCut = 12 + seededUnit(seed, 515) * 8;
  const bottomCut = 10 + seededUnit(seed, 516) * 9;
  const centerX = 75 + seededSigned(seed, 517) * 2;
  const centerY = 79 + seededSigned(seed, 518) * 2;

  const body: CornerPoint[] = [
    { x: centerX - width * 0.34 + lean, y: centerY - height * 0.42 + topCut * 0.2 },
    { x: centerX + width * 0.18 + lean * 0.5, y: centerY - height * 0.5 },
    { x: centerX + width * 0.5 + lean * 0.2, y: centerY - height * 0.16 + topCut * 0.25 },
    { x: centerX + width * 0.36 - lean * 0.4, y: centerY + height * 0.34 },
    { x: centerX - width * 0.02 - waist, y: centerY + height * 0.5 - bottomCut * 0.2 },
    { x: centerX - width * 0.48 + lean * 0.15, y: centerY + height * 0.14 },
  ];

  const face: CornerPoint[] = [
    { x: centerX - width * 0.24 + lean * 0.5, y: centerY - height * 0.18 },
    { x: centerX + width * 0.2 + lean * 0.25, y: centerY - height * 0.24 },
    { x: centerX + width * 0.31 - lean * 0.2, y: centerY + height * 0.13 },
    { x: centerX - width * 0.15 - lean * 0.1, y: centerY + height * 0.2 },
  ];

  const nodes: CornerPoint[] = [
    body[0],
    body[1],
    body[2],
    body[3],
    body[4],
    body[5],
  ].map((point, index) => ({
    x: round(point.x + seededSigned(seed, 530 + index) * 1.4),
    y: round(point.y + seededSigned(seed, 540 + index) * 1.4),
  }));

  const edges = [
    edge(nodes[0], nodes[1]),
    edge(nodes[1], nodes[2]),
    edge(nodes[2], nodes[3]),
    edge(nodes[3], nodes[4]),
    edge(nodes[4], nodes[5]),
    edge(nodes[5], nodes[0]),
    edge(nodes[1], nodes[4]),
  ];

  const selectedEdgeIndex =
    profile.palettePreset === 'error'
      ? 2
      : profile.palettePreset === 'repairing'
        ? 6
        : profile.palettePreset === 'rendering'
          ? 1
          : 3;

  return {
    seed,
    bodyPoints: pointString(body),
    facePoints: pointString(face),
    nodes,
    edges,
    selectedEdge: edges[selectedEdgeIndex],
    eyeY: round(centerY - 7 + profile.seedOffsets.eyeY * 2),
    eyeSlant: round(profile.eyeStyle === 'slant' ? 1.4 + profile.seedOffsets.eyeX : 0),
    mouthY: round(centerY + 13 + profile.seedOffsets.eyeY),
    mouthCurve: round(profile.mouthCurve),
    cornerSharpness: round(sharpness),
  };
}
