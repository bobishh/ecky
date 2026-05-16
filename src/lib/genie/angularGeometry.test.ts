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
  return firstPoints.reduce((sum, point, index) => {
    const next = secondPoints[index];
    return sum + Math.hypot(point.x - next.x, point.y - next.y);
  }, 0);
}

function pointCount(points: string): number {
  return parsePointString(points).length;
}

test('buildCornerGlyph is deterministic for same resolved profile', () => {
  const profile = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle');

  assert.deepEqual(buildCornerGlyph(profile), buildCornerGlyph(profile));
});

test('buildCornerGlyph keeps Ecky angular and seed-specific', () => {
  const first = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const second = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 456 }, 'idle'));

  assert.equal(first.nodes.length, 15);
  assert.ok(first.edges.length >= 24);
  assert.equal(first.cellPolygons.length, 8);
  assert.ok(first.dualEdges.length >= 8);
  assert.ok(pointCount(first.bodyPoints) >= 8);
  assert.notEqual(first.bodyPoints, second.bodyPoints);
  assert.ok(first.cornerSharpness > 0.5);
});

test('buildCornerGlyph weights Voronoi cells around a large central face cell', () => {
  const glyph = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const centralArea = glyph.cellAreas[0];
  const outerAreas = glyph.cellAreas.slice(1);

  assert.ok(centralArea > Math.max(...outerAreas) * 1.45);
  assert.ok(glyph.facePoints === glyph.cellPolygons[0]);
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

  assert.ok(totalContourDelta(compact.bodyPoints, expanded.bodyPoints) > 70);
  assert.ok(totalContourDelta(idle.bodyPoints, repairing.bodyPoints) > 28);
});

test('buildCornerGlyph exposes mode cues without changing identity seed', () => {
  const idle = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const error = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'error'));

  assert.equal(idle.seed, error.seed);
  assert.notEqual(idle.mouthCurve, error.mouthCurve);
  assert.notEqual(idle.selectedEdge, error.selectedEdge);
});
