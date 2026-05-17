import assert from 'node:assert/strict';
import test from 'node:test';

import { DEFAULT_GENIE_TRAITS, resolveModeTraits } from './traits';
import { buildCornerGlyph } from './angularGeometry';

function parsePointString(points: string): Array<{ x: number; y: number }> {
  return points.split(' ').map((pair) => {
    const [x, y] = pair.split(',').map(Number);
    return { x, y };
  });
}

function totalContourDelta(first: string, second: string): number {
  const firstPoints = parsePointString(first);
  const secondPoints = parsePointString(second);
  const count = Math.min(firstPoints.length, secondPoints.length);
  const delta = firstPoints.slice(0, count).reduce((sum, point, index) => {
    const next = secondPoints[index];
    return sum + Math.hypot(point.x - next.x, point.y - next.y);
  }, 0);
  return delta + Math.abs(firstPoints.length - secondPoints.length) * 18;
}

function pointCount(points: string): number {
  return parsePointString(points).length;
}

function minNodeGap(first: Array<{ x: number; y: number }>, second: Array<{ x: number; y: number }>): number {
  return first.reduce((min, point, index) => {
    const next = second[index];
    return Math.min(min, Math.hypot(point.x - next.x, point.y - next.y));
  }, Number.POSITIVE_INFINITY);
}

function minAdjacentGap(points: Array<{ x: number; y: number }>): number {
  return points.reduce((min, point, index) => {
    const next = points[(index + 1) % points.length];
    return Math.min(min, Math.hypot(point.x - next.x, point.y - next.y));
  }, Number.POSITIVE_INFINITY);
}

function minPairGap(points: Array<{ x: number; y: number }>): number {
  let min = Number.POSITIVE_INFINITY;
  for (let i = 0; i < points.length; i++) {
    for (let j = i + 1; j < points.length; j++) {
      min = Math.min(min, Math.hypot(points[i].x - points[j].x, points[i].y - points[j].y));
    }
  }
  return min;
}

function isConvex(points: Array<{ x: number; y: number }>): boolean {
  const signs = points.map((point, index) => {
    const next = points[(index + 1) % points.length];
    const after = points[(index + 2) % points.length];
    return (next.x - point.x) * (after.y - next.y) - (next.y - point.y) * (after.x - next.x);
  });
  return signs.every((sign) => sign > 0) || signs.every((sign) => sign < 0);
}

function bounds(points: Array<{ x: number; y: number }>) {
  const xs = points.map((point) => point.x);
  const ys = points.map((point) => point.y);
  return {
    minX: Math.min(...xs),
    maxX: Math.max(...xs),
    minY: Math.min(...ys),
    maxY: Math.max(...ys),
  };
}

test('buildCornerGlyph is deterministic for same resolved profile', () => {
  const profile = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle');

  assert.deepEqual(buildCornerGlyph(profile), buildCornerGlyph(profile));
});

test('buildCornerGlyph keeps Ecky angular and seed-specific', () => {
  const first = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const second = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 456 }, 'idle'));

  assert.equal(first.nodes.length, pointCount(first.bodyPoints));
  assert.ok(first.edges.length >= pointCount(first.bodyPoints) * 5);
  assert.equal(first.cellPolygons.length, pointCount(first.bodyPoints) * 2 + 1);
  assert.equal(first.cellShades.length, first.cellPolygons.length);
  assert.equal(first.dualEdges.length, pointCount(first.bodyPoints));
  assert.ok(pointCount(first.bodyPoints) >= 10);
  assert.notEqual(first.bodyPoints, second.bodyPoints);
  assert.ok(first.cornerSharpness > 0.5);
});

test('buildCornerGlyph weights stone facets around a large central face cell', () => {
  const glyph = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const centralArea = glyph.cellAreas[0];
  const outerAreas = glyph.cellAreas.slice(1);

  assert.ok(centralArea > Math.max(...outerAreas) * 1.45);
  assert.ok(glyph.facePoints === glyph.cellPolygons[0]);
  assert.ok(minNodeGap(glyph.outlineNodes, glyph.nodes) > 4.5);
  assert.ok(glyph.cellShades[0] > Math.max(...glyph.cellShades.slice(1)));
});

test('buildCornerGlyph constrains the outer frame around the face', () => {
  for (const seed of [1, 7, 123, 456, 9999, 271828]) {
    const glyph = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed }, 'idle'));
    const frame = bounds(glyph.outlineNodes);
    const face = bounds(parsePointString(glyph.facePoints));
    const width = frame.maxX - frame.minX;
    const height = frame.maxY - frame.minY;

    assert.ok(width >= 62 && width <= 104);
    assert.ok(height >= 78 && height <= 118);
    assert.ok(height / width <= 1.55);
    assert.ok(face.minX - frame.minX >= 11);
    assert.ok(frame.maxX - face.maxX >= 11);
    assert.ok(face.minY - frame.minY >= 9.5);
    assert.ok(frame.maxY - face.maxY >= 13);
    assert.ok(minAdjacentGap(glyph.outlineNodes) > 14);
    assert.ok(minPairGap([...glyph.outlineNodes, ...glyph.nodes]) > 4.5);
    assert.ok(isConvex(glyph.outlineNodes));
  }
});

test('buildCornerGlyph changes the outer contour from geometry traits', () => {
  const compact = buildCornerGlyph(
    resolveModeTraits(
      {
        ...DEFAULT_GENIE_TRAITS,
        seed: 123,
        vertexCount: 10,
        radiusBase: 25,
        stretchY: 0.9,
        asymmetry: 0.88,
        jitterScale: 0.7,
        warpScale: 0.35,
      },
      'idle',
    ),
  );
  const expanded = buildCornerGlyph(
    resolveModeTraits(
      {
        ...DEFAULT_GENIE_TRAITS,
        seed: 123,
        vertexCount: 24,
        radiusBase: 34,
        stretchY: 1.06,
        asymmetry: 1.14,
        jitterScale: 1.45,
        warpScale: 1.25,
      },
      'idle',
    ),
  );
  const idle = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const repairing = buildCornerGlyph(
    resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'repairing'),
  );

  assert.notEqual(pointCount(compact.bodyPoints), pointCount(expanded.bodyPoints));
  assert.ok(totalContourDelta(compact.bodyPoints, expanded.bodyPoints) > 70);
  assert.ok(totalContourDelta(idle.bodyPoints, repairing.bodyPoints) > 28);
});

test('buildCornerGlyph exposes mode cues without changing identity seed', () => {
  const idle = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const error = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'error'));

  assert.equal(idle.seed, error.seed);
  assert.notEqual(idle.mouthPath, error.mouthPath);
  assert.notEqual(idle.selectedEdge, error.selectedEdge);
});
