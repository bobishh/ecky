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
  cellPolygons: string[];
  cellAreas: number[];
  dualEdges: CornerEdge[];
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

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

function interpolate(a: CornerPoint, b: CornerPoint, t: number): CornerPoint {
  return {
    x: lerp(a.x, b.x, t),
    y: lerp(a.y, b.y, t),
  };
}

function centroid(points: CornerPoint[]): CornerPoint {
  const sum = points.reduce(
    (acc, point) => ({ x: acc.x + point.x, y: acc.y + point.y }),
    { x: 0, y: 0 },
  );
  return {
    x: sum.x / points.length,
    y: sum.y / points.length,
  };
}

function jitter(point: CornerPoint, seed: number, channel: number, amount: number): CornerPoint {
  return {
    x: round(point.x + seededSigned(seed, channel) * amount),
    y: round(point.y + seededSigned(seed, channel + 70) * amount),
  };
}

function uniqueEdges(edges: CornerEdge[]): CornerEdge[] {
  const seen = new Set<string>();
  const out: CornerEdge[] = [];
  for (const current of edges) {
    const forward = `${current.x1},${current.y1}:${current.x2},${current.y2}`;
    const reverse = `${current.x2},${current.y2}:${current.x1},${current.y1}`;
    if (seen.has(forward) || seen.has(reverse)) continue;
    seen.add(forward);
    out.push(current);
  }
  return out;
}

function polygonArea(points: CornerPoint[]): number {
  let area = 0;
  for (let i = 0; i < points.length; i++) {
    const current = points[i];
    const next = points[(i + 1) % points.length];
    area += current.x * next.y - next.x * current.y;
  }
  return Math.abs(area) * 0.5;
}

function clampPoint(point: CornerPoint): CornerPoint {
  return {
    x: round(Math.max(14, Math.min(136, point.x))),
    y: round(Math.max(18, Math.min(132, point.y))),
  };
}

export function buildCornerGlyph(profile: ResolvedGenieProfile): CornerGlyph {
  const seed = profile.seed;
  const sharpness = 0.58 + seededUnit(seed, 510) * 0.28;
  const radiusScale = profile.radiusBase / 30;
  const vertexScale = (profile.vertexCount - 16) / 8;
  const warpScale = profile.warpScale - 1;
  const jitterScale = profile.jitterScale;
  const width =
    (70 + seededSigned(seed, 511) * 8 + vertexScale * 10) *
    radiusScale *
    (1 + warpScale * 0.08);
  const height =
    (86 + seededSigned(seed, 512) * 7 + vertexScale * 8) *
    profile.stretchY *
    radiusScale *
    (1 + warpScale * 0.1);
  const lean = seededSigned(seed, 513) * 5 + profile.tiltScale * 78 + (profile.asymmetry - 1) * 26;
  const waist = 12 + seededUnit(seed, 514) * 7;
  const topCut = 12 + seededUnit(seed, 515) * 8;
  const bottomCut = 10 + seededUnit(seed, 516) * 9;
  const centerX = 75 + seededSigned(seed, 517) * 2;
  const centerY = 79 + seededSigned(seed, 518) * 2;
  const modeScale =
    profile.palettePreset === 'sleeping'
      ? 0.82
      : profile.palettePreset === 'thinking'
        ? 0.94
        : profile.palettePreset === 'repairing'
          ? 1.08
          : profile.palettePreset === 'rendering'
            ? 1.12
            : profile.palettePreset === 'error'
              ? 1.1
              : 1;
  const modeSkew =
    profile.palettePreset === 'repairing'
      ? 9
      : profile.palettePreset === 'error'
        ? -8
        : profile.palettePreset === 'rendering'
          ? 5
          : 0;
  const notchScale =
    1 +
    Math.abs(profile.asymmetry - 1) * 1.6 +
    Math.max(0, profile.jitterScale - 1) * 0.35 +
    Math.max(0, profile.warpScale - 1) * 0.5;
  const contourPoint = (
    index: number,
    xWeight: number,
    yWeight: number,
    leanWeight: number,
  ): CornerPoint => {
    const side = xWeight >= 0 ? 1 : -1;
    const sideScale =
      side > 0
        ? 1 + (profile.asymmetry - 1) * 1.35
        : 1 - (profile.asymmetry - 1) * 0.85;
    const radialNoise = 1 + seededSigned(seed, 600 + index) * 0.13 * jitterScale + warpScale * 0.05;
    const xNoise = seededSigned(seed, 640 + index) * 3.2 * jitterScale;
    const yNoise = seededSigned(seed, 660 + index) * 2.8 * jitterScale;
    return {
      x: round(centerX + width * xWeight * sideScale * radialNoise + lean * leanWeight + xNoise),
      y: round(centerY + height * yWeight * radialNoise + yNoise),
    };
  };

  const body: CornerPoint[] = [
    contourPoint(0, -0.38, -0.44 + topCut * 0.0018, 1),
    contourPoint(1, 0.18 + topCut * 0.002, -0.53, 0.5),
    contourPoint(2, 0.52, -0.16 + topCut * 0.002, 0.2),
    contourPoint(3, 0.38, 0.36, -0.4),
    contourPoint(4, -0.08 - waist / width, 0.53 - bottomCut * 0.0018, 0),
    contourPoint(5, -0.52, 0.16, 0.15),
  ];
  const outlinePoint = (
    index: number,
    xWeight: number,
    yWeight: number,
    leanWeight: number,
    notchWeight: number,
  ): CornerPoint => {
    const notch =
      (seededSigned(seed, 720 + index) * 12.5 +
        (index % 2 === 0 ? 10.5 : -7.5) * notchWeight +
        modeSkew * (xWeight > 0 ? 0.45 : -0.28)) *
      notchScale;
    const side = xWeight >= 0 ? 1 : -1;
    const rightAsym = side > 0 ? profile.asymmetry : 2 - profile.asymmetry;
    return clampPoint({
      x:
        centerX +
        width * xWeight * modeScale * rightAsym +
        lean * leanWeight +
        notch * side +
        modeSkew * (0.5 + yWeight),
      y:
        centerY +
        height * yWeight * modeScale +
        seededSigned(seed, 760 + index) * 6.4 * jitterScale +
        notch * 0.3,
    });
  };
  const outline: CornerPoint[] = [
    outlinePoint(0, -0.4, -0.45, 1.05, 1.1),
    outlinePoint(1, -0.08, -0.6, 0.7, -0.7),
    outlinePoint(2, 0.25, -0.54, 0.42, 0.8),
    outlinePoint(3, 0.55, -0.18, 0.18, -0.4),
    outlinePoint(4, 0.48, 0.24, -0.32, 0.9),
    outlinePoint(5, 0.22, 0.52, -0.08, -0.8),
    outlinePoint(6, -0.18 - waist / width, 0.56, 0.02, 0.7),
    outlinePoint(7, -0.56, 0.17, 0.18, -0.5),
  ];

  const face: CornerPoint[] = [
    { x: centerX - width * 0.32 + lean * 0.5, y: centerY - height * 0.24 },
    { x: centerX + width * 0.28 + lean * 0.25, y: centerY - height * 0.28 },
    { x: centerX + width * 0.36 - lean * 0.2, y: centerY + height * 0.2 },
    { x: centerX - width * 0.24 - lean * 0.1, y: centerY + height * 0.26 },
  ];

  const outerNodes = body.map((point, index) => jitter(point, seed, 530 + index, 1.4));
  const faceCenter = jitter(centroid(face), seed, 560, 1.0);
  const faceTopLeft = { x: round(face[0].x), y: round(face[0].y) };
  const faceTopRight = { x: round(face[1].x), y: round(face[1].y) };
  const faceBottomRight = { x: round(face[2].x), y: round(face[2].y) };
  const faceBottomLeft = { x: round(face[3].x), y: round(face[3].y) };
  const topFacet = jitter(interpolate(body[0], body[2], 0.5), seed, 570, 1.5);
  const rightFacet = jitter(interpolate(body[2], body[3], 0.54), seed, 572, 1.5);
  const bottomFacet = jitter(interpolate(body[3], body[5], 0.46), seed, 574, 1.5);
  const leftFacet = jitter(interpolate(body[5], body[0], 0.55), seed, 576, 1.5);

  const nodes: CornerPoint[] = [
    ...outerNodes,
    faceCenter,
    faceTopLeft,
    faceTopRight,
    faceBottomRight,
    faceBottomLeft,
    topFacet,
    rightFacet,
    bottomFacet,
    leftFacet,
  ];

  const edgePairs: Array<[number, number]> = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 4],
    [4, 5],
    [5, 0],
    [0, 11],
    [11, 1],
    [1, 10],
    [10, 2],
    [2, 12],
    [12, 3],
    [3, 13],
    [13, 4],
    [4, 14],
    [14, 5],
    [5, 11],
    [11, 7],
    [11, 10],
    [12, 8],
    [12, 9],
    [13, 9],
    [13, 10],
    [14, 10],
    [14, 7],
    [7, 8],
    [8, 9],
    [9, 10],
    [10, 7],
  ];
  const edges = uniqueEdges(edgePairs.map(([a, b]) => edge(nodes[a], nodes[b])));

  const cellPoints: CornerPoint[][] = [
    [nodes[7], nodes[8], nodes[9], nodes[10]],
    [nodes[0], nodes[1], nodes[11], nodes[7], nodes[10], nodes[14], nodes[5]],
    [nodes[1], nodes[2], nodes[12], nodes[8], nodes[7], nodes[11]],
    [nodes[2], nodes[3], nodes[13], nodes[9], nodes[8], nodes[12]],
    [nodes[3], nodes[4], nodes[14], nodes[10], nodes[9], nodes[13]],
    [nodes[4], nodes[5], nodes[14]],
    [nodes[0], nodes[11], nodes[5]],
    [nodes[12], nodes[13], nodes[9], nodes[8]],
  ];
  const cellPolygons = cellPoints.map(pointString);
  const cellAreas = cellPoints.map((points) => round(polygonArea(points)));
  const cellCenters = cellPoints.map((points, index) => jitter(centroid(points), seed, 650 + index, 0.9));
  const dualEdgePairs: Array<[number, number]> = [
    [0, 1],
    [1, 2],
    [2, 4],
    [2, 6],
    [3, 4],
    [4, 7],
    [5, 6],
    [5, 7],
    [6, 7],
  ];
  const dualEdges = dualEdgePairs.map(([a, b]) => edge(cellCenters[a], cellCenters[b]));

  const selectedEdgeIndex =
    profile.palettePreset === 'error'
      ? 2
      : profile.palettePreset === 'repairing'
        ? 18
        : profile.palettePreset === 'rendering'
          ? 15
          : 3;

  return {
    seed,
    bodyPoints: pointString(outline),
    facePoints: pointString(face),
    nodes,
    edges,
    cellPolygons,
    cellAreas,
    dualEdges,
    selectedEdge: edges[selectedEdgeIndex],
    eyeY: round(centerY - 7 + profile.seedOffsets.eyeY * 2),
    eyeSlant: round(profile.eyeStyle === 'slant' ? 1.4 + profile.seedOffsets.eyeX : 0),
    mouthY: round(centerY + 13 + profile.seedOffsets.eyeY),
    mouthCurve: round(profile.mouthCurve),
    cornerSharpness: round(sharpness),
  };
}
