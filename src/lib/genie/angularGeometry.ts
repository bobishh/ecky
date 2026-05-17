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
  outlineNodes: CornerPoint[];
  outlineEdges: CornerEdge[];
  facePoints: string;
  nodes: CornerPoint[];
  edges: CornerEdge[];
  cellPolygons: string[];
  cellShades: number[];
  cellAreas: number[];
  dualEdges: CornerEdge[];
  selectedEdge: CornerEdge;
  leftEye: CornerEdge;
  rightEye: CornerEdge;
  mouthPath: string;
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

function edgeKey(a: number, b: number): string {
  return a < b ? `${a}:${b}` : `${b}:${a}`;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function distance(a: CornerPoint, b: CornerPoint): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function triangleCentroid(points: CornerPoint[]): CornerPoint {
  const sum = points.reduce((acc, point) => ({ x: acc.x + point.x, y: acc.y + point.y }), { x: 0, y: 0 });
  return {
    x: sum.x / points.length,
    y: sum.y / points.length,
  };
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

function bounds(points: CornerPoint[]): { minX: number; maxX: number; minY: number; maxY: number } {
  return points.reduce(
    (acc, point) => ({
      minX: Math.min(acc.minX, point.x),
      maxX: Math.max(acc.maxX, point.x),
      minY: Math.min(acc.minY, point.y),
      maxY: Math.max(acc.maxY, point.y),
    }),
    { minX: Infinity, maxX: -Infinity, minY: Infinity, maxY: -Infinity },
  );
}

function pointInPolygon(point: CornerPoint, polygon: CornerPoint[]): boolean {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const current = polygon[i];
    const previous = polygon[j];
    const crosses =
      current.y > point.y !== previous.y > point.y &&
      point.x < ((previous.x - current.x) * (point.y - current.y)) / (previous.y - current.y) + current.x;
    if (crosses) inside = !inside;
  }
  return inside;
}

function convexify(points: CornerPoint[], center: CornerPoint): CornerPoint[] {
  const sorted = [...points].sort(
    (a, b) => Math.atan2(a.y - center.y, a.x - center.x) - Math.atan2(b.y - center.y, b.x - center.x),
  );
  const hull: CornerPoint[] = [];
  for (const point of sorted) {
    while (hull.length >= 2) {
      const a = hull[hull.length - 2];
      const b = hull[hull.length - 1];
      const cross = (b.x - a.x) * (point.y - b.y) - (b.y - a.y) * (point.x - b.x);
      if (cross > 0) break;
      hull.pop();
    }
    hull.push(point);
  }
  const lowerLength = hull.length;
  for (let i = sorted.length - 2; i >= 0; i--) {
    const point = sorted[i];
    while (hull.length > lowerLength) {
      const a = hull[hull.length - 2];
      const b = hull[hull.length - 1];
      const cross = (b.x - a.x) * (point.y - b.y) - (b.y - a.y) * (point.x - b.x);
      if (cross > 0) break;
      hull.pop();
    }
    hull.push(point);
  }
  hull.pop();
  return hull.sort(
    (a, b) => Math.atan2(a.y - center.y, a.x - center.x) - Math.atan2(b.y - center.y, b.x - center.x),
  );
}

function uniqueEdges(edges: CornerEdge[]): CornerEdge[] {
  const seen = new Set<string>();
  const out: CornerEdge[] = [];
  for (const current of edges) {
    const key = `${Math.min(current.x1, current.x2)},${Math.min(current.y1, current.y2)}:${Math.max(
      current.x1,
      current.x2,
    )},${Math.max(current.y1, current.y2)}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(current);
  }
  return out;
}

function clampPoint(point: CornerPoint): CornerPoint {
  return {
    x: round(Math.max(14, Math.min(136, point.x))),
    y: round(Math.max(18, Math.min(132, point.y))),
  };
}

type Triangle = [number, number, number];

type CornerPoint3 = CornerPoint & {
  z: number;
};

function circumcenter(points: CornerPoint[], triangle: Triangle): CornerPoint {
  const a = points[triangle[0]];
  const b = points[triangle[1]];
  const c = points[triangle[2]];
  const d = 2 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
  if (Math.abs(d) < 0.00001) return triangleCentroid([a, b, c]);
  const a2 = a.x * a.x + a.y * a.y;
  const b2 = b.x * b.x + b.y * b.y;
  const c2 = c.x * c.x + c.y * c.y;
  return {
    x: (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
    y: (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
  };
}

function circumcircleContains(points: CornerPoint[], triangle: Triangle, point: CornerPoint): boolean {
  const center = circumcenter(points, triangle);
  const radius = distance(center, points[triangle[0]]);
  return distance(center, point) <= radius + 0.001;
}

function delaunay(points: CornerPoint[]): Triangle[] {
  const frame = points.reduce(
    (acc, point) => ({
      minX: Math.min(acc.minX, point.x),
      maxX: Math.max(acc.maxX, point.x),
      minY: Math.min(acc.minY, point.y),
      maxY: Math.max(acc.maxY, point.y),
    }),
    { minX: Infinity, maxX: -Infinity, minY: Infinity, maxY: -Infinity },
  );
  const span = Math.max(frame.maxX - frame.minX, frame.maxY - frame.minY) * 8;
  const midX = (frame.minX + frame.maxX) * 0.5;
  const midY = (frame.minY + frame.maxY) * 0.5;
  const work = [
    ...points,
    { x: midX - span, y: midY - span },
    { x: midX, y: midY + span },
    { x: midX + span, y: midY - span },
  ];
  const superA = points.length;
  const superB = points.length + 1;
  const superC = points.length + 2;
  let triangles: Triangle[] = [[superA, superB, superC]];

  for (let pointIndex = 0; pointIndex < points.length; pointIndex++) {
    const point = work[pointIndex];
    const bad = triangles.filter((triangle) => circumcircleContains(work, triangle, point));
    const badKeys = new Set(bad.map((triangle) => triangle.join(':')));
    const edgeCounts = new Map<string, [number, number, number]>();

    for (const triangle of bad) {
      for (const [a, b] of [
        [triangle[0], triangle[1]],
        [triangle[1], triangle[2]],
        [triangle[2], triangle[0]],
      ] as Array<[number, number]>) {
        const key = edgeKey(a, b);
        const current = edgeCounts.get(key);
        edgeCounts.set(key, [a, b, (current?.[2] ?? 0) + 1]);
      }
    }

    triangles = triangles.filter((triangle) => !badKeys.has(triangle.join(':')));
    for (const [a, b, count] of edgeCounts.values()) {
      if (count !== 1) continue;
      triangles.push([a, b, pointIndex]);
    }
  }

  return triangles.filter((triangle) => triangle.every((index) => index < points.length));
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
  const modeVertexDelta =
    profile.palettePreset === 'sleeping'
      ? 0
      : profile.palettePreset === 'thinking'
        ? 1
        : profile.palettePreset === 'repairing'
          ? 2
          : profile.palettePreset === 'rendering'
            ? 3
            : profile.palettePreset === 'error'
              ? 1
              : 0;
  const outlineCount = Math.max(
    10,
    Math.min(12, Math.round(10 + (profile.vertexCount - 10) * 0.12 + modeVertexDelta * 0.35)),
  );
  const frameWidth = clamp(width * modeScale * 1.26, 82, 108);
  const frameHeight = clamp(height * modeScale * 1.05, 84, 108);
  const halfWidth = frameWidth * 0.5;
  const halfHeight = frameHeight * 0.5;
  const faceCount = outlineCount;
  const faceRadiusX = halfWidth * (0.56 + seededSigned(seed, 914) * 0.012);
  const faceRadiusY = halfHeight * (0.52 + seededSigned(seed, 916) * 0.012);
  const depth = 44 + seededUnit(seed, 917) * 8;
  const yaw = -0.42 + seededSigned(seed, 919) * 0.08 + modeSkew * 0.006;
  const pitch = 0.28 + seededSigned(seed, 921) * 0.06;
  const cosYaw = Math.cos(yaw);
  const sinYaw = Math.sin(yaw);
  const cosPitch = Math.cos(pitch);
  const sinPitch = Math.sin(pitch);
  const lightAngle = -Math.PI * 0.72 + seededSigned(seed, 918) * 0.16;
  const project = (point: CornerPoint3): CornerPoint => {
    const rotatedX = point.x * cosYaw + point.z * sinYaw;
    const rotatedZ = -point.x * sinYaw + point.z * cosYaw;
    const rotatedY = point.y * cosPitch - rotatedZ * sinPitch;
    return clampPoint({
      x: centerX + rotatedX + lean * 0.06,
      y: centerY + rotatedY + 1.5,
    });
  };
  const stoneSurface = (x: number, y: number, lift = 0): CornerPoint3 => {
    const u = clamp(x / halfWidth, -0.98, 0.98);
    const v = clamp(y / halfHeight, -0.98, 0.98);
    const z = depth * Math.sqrt(Math.max(0.03, 1 - u * u - v * v)) + lift;
    return { x, y, z };
  };
  const projectedOutline: CornerPoint[] = Array.from({ length: outlineCount }, (_, index) => {
    const t = index / outlineCount;
    const angle =
      -Math.PI / 2 +
      t * Math.PI * 2 +
      seededSigned(seed, 720 + index) * 0.012 +
      Math.sin(t * Math.PI * 2 + profile.seedOffsets.chord) * 0.008 * notchScale;
    const side = Math.cos(angle) >= 0 ? 1 : -1;
    const sideAsym = side > 0 ? 1 + (profile.asymmetry - 1) * 0.32 : 1 - (profile.asymmetry - 1) * 0.24;
    const radiusPulse =
      1 +
      seededSigned(seed, 760 + index) * 0.015 * jitterScale +
      Math.sin(index * 1.9 + seed * 0.001) * 0.012 * notchScale;
    const sin = Math.sin(angle);
    const cos = Math.cos(angle);
    const shoulder = Math.abs(cos) > 0.62 && sin < -0.05 ? 1.12 : 1;
    const topFacet = sin < -0.72 ? 0.9 + seededUnit(seed, 880) * 0.04 : 1;
    const bottomFacet = sin > 0.7 ? 1.08 + seededUnit(seed, 882) * 0.035 : 1;

    return project(
      stoneSurface(
        cos * halfWidth * sideAsym * radiusPulse * shoulder + modeSkew * (0.24 + sin * 0.22),
        sin * halfHeight * radiusPulse * topFacet * bottomFacet +
          seededSigned(seed, 900 + index) * 1.2 * jitterScale,
        -8,
      ),
    );
  });
  const outline = convexify(projectedOutline, { x: centerX, y: centerY });
  const outlineEdges = outline.map((point, index) => edge(point, outline[(index + 1) % outline.length]));

  const projectedFace: CornerPoint[] = Array.from({ length: faceCount }, (_, index) => {
    const t = index / faceCount;
    const angle = -Math.PI / 2 + t * Math.PI * 2 + seededSigned(seed, 930 + index) * 0.025;
    const pulse = 1 + seededSigned(seed, 960 + index) * 0.035 * jitterScale;
    return project(stoneSurface(Math.cos(angle) * faceRadiusX * pulse, 4.3 + Math.sin(angle) * faceRadiusY * pulse, 8));
  });
  const faceBounds = bounds(projectedFace);
  const faceShiftX = centerX - (faceBounds.minX + faceBounds.maxX) * 0.5;
  const face: CornerPoint[] = projectedFace.map((point) =>
    clampPoint({
      x: point.x + faceShiftX,
      y: point.y,
    }),
  );

  const nodes: CornerPoint[] = outline.map((point, index) => {
    const dx = point.x - centerX;
    const dy = point.y - centerY;
    const tangentX = -dy;
    const tangentY = dx;
    const tangentLength = Math.max(1, Math.hypot(tangentX, tangentY));
    const ringScale = 0.82 + seededSigned(seed, 1000 + index) * 0.01;
    const tangentOffset = seededSigned(seed, 1040 + index) * 0.65;
    return {
      x: round(centerX + dx * ringScale + (tangentX / tangentLength) * tangentOffset),
      y: round(centerY + dy * ringScale + (tangentY / tangentLength) * tangentOffset),
    };
  });

  const faceEdges = face.map((point, index) => edge(point, face[(index + 1) % face.length]));
  const facetPoints: CornerPoint[][] = outline.flatMap((point, index) => {
    const next = (index + 1) % outline.length;
    const faceIndex = Math.round((index / outline.length) * face.length) % face.length;
    const nextFaceIndex = Math.round((next / outline.length) * face.length) % face.length;
    const outer = [point, outline[next], nodes[next], nodes[index]];
    const inner = [nodes[index], nodes[next], face[nextFaceIndex], face[faceIndex]];
    return [outer, inner];
  });
  const triangleEdges = facetPoints.flatMap((points) =>
    points.map((point, index) => edge(point, points[(index + 1) % points.length])),
  );
  const edges = uniqueEdges([...triangleEdges, ...faceEdges]);

  const cellPoints: CornerPoint[][] = [
    face,
    ...facetPoints,
  ];
  const cellPolygons = cellPoints.map(pointString);
  const cellAreas = cellPoints.map((points) => round(polygonArea(points)));
  const light = { x: -0.42, y: -0.58, z: 0.7 };
  const lightLength = Math.hypot(light.x, light.y, light.z);
  const cellShades = [
    0.96,
    ...facetPoints.map((points, index) => {
      const centroid = triangleCentroid(points);
      const unrotatedX = (centroid.x - centerX) * cosYaw - (depth * 0.45) * sinYaw;
      const unrotatedY = centroid.y - centerY;
      const u = clamp(unrotatedX / halfWidth, -0.98, 0.98);
      const v = clamp(unrotatedY / halfHeight, -0.98, 0.98);
      const z = Math.sqrt(Math.max(0.04, 1 - u * u - v * v));
      const normal = { x: u / halfWidth, y: v / halfHeight, z: z / 26 };
      const normalLength = Math.hypot(normal.x, normal.y, normal.z);
      const lambert =
        (normal.x * light.x + normal.y * light.y + normal.z * light.z) / (normalLength * lightLength);
      const angle = Math.atan2(centroid.y - centerY, centroid.x - centerX);
      const rim = Math.max(0, Math.cos(angle - lightAngle)) * 0.1;
      const angularShade = 0.5 + Math.max(0, lambert) * 0.42 + rim + seededSigned(seed, 980 + index) * 0.035;
      return round(clamp(angularShade, 0.46, 0.92));
    }),
  ];
  const dualEdges = nodes.map((point, index) => edge(point, face[index % face.length]));

  const selectedEdgeIndexRaw =
    profile.palettePreset === 'error'
      ? 2
      : profile.palettePreset === 'repairing'
        ? 18
        : profile.palettePreset === 'rendering'
          ? 15
          : 3;
  const selectedEdge = edges[selectedEdgeIndexRaw % edges.length];
  const visibleFaceBounds = bounds(face);
  const faceWidth = visibleFaceBounds.maxX - visibleFaceBounds.minX;
  const faceHeight = visibleFaceBounds.maxY - visibleFaceBounds.minY;
  const faceCenterX = (visibleFaceBounds.minX + visibleFaceBounds.maxX) * 0.5;
  const eyeY = round(visibleFaceBounds.minY + faceHeight * 0.38 + profile.seedOffsets.eyeY * 0.8);
  const eyeSlant = profile.eyeStyle === 'slant' ? 1.2 + profile.seedOffsets.eyeX * 0.5 : 0;
  const eyeHalf = clamp(faceWidth * 0.13, 4.8, 7.4);
  const eyeOffset = clamp(faceWidth * 0.22 + profile.eyeSpacing * 0.12, 9.5, 14.5);
  const mouthY = round(visibleFaceBounds.minY + faceHeight * 0.67 + profile.seedOffsets.mouth * 0.8);
  const mouthHalf = clamp(faceWidth * 0.29, 12, 18);
  const mouthCurve = clamp(profile.mouthCurve * 1.45, 1.2, 4.2);
  const leftEye = edge(
    { x: faceCenterX - eyeOffset - eyeHalf, y: eyeY + eyeSlant },
    { x: faceCenterX - eyeOffset + eyeHalf, y: eyeY - eyeSlant },
  );
  const rightEye = edge(
    { x: faceCenterX + eyeOffset - eyeHalf, y: eyeY - eyeSlant },
    { x: faceCenterX + eyeOffset + eyeHalf, y: eyeY + eyeSlant },
  );
  const mouthPath = `M ${round(faceCenterX - mouthHalf)} ${mouthY} Q ${round(faceCenterX)} ${round(
    mouthY + mouthCurve,
  )} ${round(faceCenterX + mouthHalf)} ${mouthY}`;

  return {
    seed,
    bodyPoints: pointString(outline),
    outlineNodes: outline,
    outlineEdges,
    facePoints: pointString(face),
    nodes,
    edges,
    cellPolygons,
    cellShades,
    cellAreas,
    dualEdges,
    selectedEdge,
    leftEye,
    rightEye,
    mouthPath,
    cornerSharpness: round(sharpness),
  };
}
