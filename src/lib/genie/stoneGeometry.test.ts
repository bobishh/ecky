import assert from 'node:assert/strict';
import test from 'node:test';

import { DEFAULT_GENIE_TRAITS, resolveModeTraits } from './traits';
import { buildStoneGeometry, type StonePoint3 } from './stoneGeometry';

function bounds(points: StonePoint3[]) {
  const xs = points.map((point) => point.x);
  const ys = points.map((point) => point.y);
  return {
    minX: Math.min(...xs),
    maxX: Math.max(...xs),
    minY: Math.min(...ys),
    maxY: Math.max(...ys),
  };
}

function minAdjacentGap(points: StonePoint3[]): number {
  return points.reduce((min, point, index) => {
    const next = points[(index + 1) % points.length];
    return Math.min(min, Math.hypot(point.x - next.x, point.y - next.y));
  }, Number.POSITIVE_INFINITY);
}

function totalDelta(first: StonePoint3[], second: StonePoint3[]): number {
  const count = Math.min(first.length, second.length);
  const delta = first.slice(0, count).reduce((sum, point, index) => {
    const next = second[index];
    return sum + Math.hypot(point.x - next.x, point.y - next.y, point.z - next.z);
  }, 0);
  return delta + Math.abs(first.length - second.length) * 0.25;
}

function sampleAspect(points: StonePoint3[], top = -0.35, bottom = 0.35): number {
  const upper = points.filter((point) => point.y < top);
  const lower = points.filter((point) => point.y > bottom);
  const width = (sample: StonePoint3[]) => Math.max(...sample.map((point) => point.x)) - Math.min(...sample.map((point) => point.x));
  return width(upper) / width(lower);
}

test('buildStoneGeometry is deterministic for same profile', () => {
  const profile = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'thinking');

  assert.deepEqual(buildStoneGeometry(profile), buildStoneGeometry(profile));
});

test('buildStoneGeometry keeps a constrained outer frame around a larger face', () => {
  for (const seed of [1, 7, 123, 456, 9999, 271828]) {
    const geometry = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed }, 'idle'));
    const rim = bounds(geometry.rim);
    const face = bounds(geometry.front);
    const rimWidth = rim.maxX - rim.minX;
    const rimHeight = rim.maxY - rim.minY;
    const faceWidth = face.maxX - face.minX;
    const faceHeight = face.maxY - face.minY;

    assert.ok(geometry.rim.length >= 10 && geometry.rim.length <= 14);
    assert.ok(rimWidth >= 2.05 && rimWidth <= 3.35);
    assert.ok(rimHeight >= 2.0 && rimHeight <= 3.25);
    assert.ok(faceWidth / rimWidth <= 0.78);
    assert.ok(faceHeight / rimHeight <= 0.76);
    assert.ok(minAdjacentGap(geometry.rim) > 0.48);
  }
});

test('buildStoneGeometry changes contour from genetic traits without changing seed manually', () => {
  const compact = buildStoneGeometry(
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
  const expanded = buildStoneGeometry(
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

  assert.notEqual(compact.rim.length, expanded.rim.length);
  assert.ok(totalDelta(compact.rim, expanded.rim) > 5);
});

test('buildStoneGeometry varies face silhouette beyond a round oval', () => {
  const squareLike = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 16 }, 'idle'));
  const trapezoidLike = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 29 }, 'idle'));

  assert.ok(Math.abs(sampleAspect(squareLike.front) - sampleAspect(trapezoidLike.front)) > 0.16);
  assert.ok(totalDelta(squareLike.front, trapezoidLike.front) > 1.2);
});

test('buildStoneGeometry varies outer rim beyond an oval while staying constrained', () => {
  const samples = [3, 16, 29, 44, 61].map((seed) =>
    buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed }, 'idle')),
  );
  const rimAspects = samples.map((geometry) => sampleAspect(geometry.rim));
  const spread = Math.max(...rimAspects) - Math.min(...rimAspects);

  assert.ok(spread > 0.15);
  for (const geometry of samples) {
    assert.ok(minAdjacentGap(geometry.rim) > 0.42);
  }
});

test('buildStoneGeometry varies face glyphs beyond line segments', () => {
  const glyphs = [1, 2, 5, 7, 34, 55].map((seed) =>
    buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed }, 'idle')).face,
  );

  assert.ok(new Set(glyphs.map((face) => face.eyeShape)).size >= 3);
  assert.ok(new Set(glyphs.map((face) => face.mouthShape)).size >= 2);
});

test('buildStoneGeometry adds punk silhouette peaks to shell while keeping face clean', () => {
  for (const seed of [1, 7, 123, 456, 9999, 271828]) {
    const geometry = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed }, 'idle'));
    const rim = bounds(geometry.rim);
    const face = bounds(geometry.front);

    assert.ok(geometry.spikes.length >= 10 && geometry.spikes.length <= 14);
    const quadrants = new Set(geometry.spikes.map((spike) => `${spike.x >= 0 ? 'r' : 'l'}${spike.y >= 0 ? 'b' : 't'}`));
    assert.equal(quadrants.size, 4);
    assert.ok((face.maxX - face.minX) / (rim.maxX - rim.minX) <= 0.78);
    assert.ok((face.maxY - face.minY) / (rim.maxY - rim.minY) <= 0.76);
    for (const spike of geometry.spikes) {
      assert.ok(spike.scale >= 0.1 && spike.scale <= 0.22);
      assert.ok(spike.x >= rim.minX - 0.28 && spike.x <= rim.maxX + 0.28);
      assert.ok(spike.y >= rim.minY - 0.28 && spike.y <= rim.maxY + 0.28);
    }
  }
});

test('buildStoneGeometry keeps face patch flat enough to avoid a central nose', () => {
  for (const seed of [1, 7, 123, 456, 9999, 271828]) {
    const geometry = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed }, 'idle'));
    const frontZ = geometry.front.reduce((sum, point) => sum + point.z, 0) / geometry.front.length;

    assert.ok(Math.abs(geometry.center.z - frontZ) <= 0.08);
  }
});

test('buildStoneGeometry maps error state to whole-stone red hue', () => {
  const idle = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123, colorHue: 144 }, 'idle'));
  const error = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123, colorHue: 144 }, 'error'));

  assert.equal(error.hue, 6);
  assert.notEqual(idle.hue, error.hue);
});

test('buildStoneGeometry gives face grooves bright seeded contrast instead of fixed gray', () => {
  const first = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123, colorHue: 144 }, 'idle'));
  const second = buildStoneGeometry(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 456, colorHue: 144 }, 'idle'));

  assert.ok(first.face.grooveSaturation >= 0.7);
  assert.ok(first.face.grooveLightness >= 0.7);
  assert.notDeepEqual(
    [first.face.grooveHue, first.face.grooveSaturation, first.face.grooveLightness],
    [second.face.grooveHue, second.face.grooveSaturation, second.face.grooveLightness],
  );
});
