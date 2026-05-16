import assert from 'node:assert/strict';
import test from 'node:test';

import { DEFAULT_GENIE_TRAITS, resolveModeTraits } from './traits';
import { buildCornerGlyph } from './angularGeometry';

test('buildCornerGlyph is deterministic for same resolved profile', () => {
  const profile = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle');

  assert.deepEqual(buildCornerGlyph(profile), buildCornerGlyph(profile));
});

test('buildCornerGlyph keeps Ecky angular and seed-specific', () => {
  const first = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const second = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 456 }, 'idle'));

  assert.equal(first.nodes.length, 6);
  assert.equal(first.edges.length, 7);
  assert.notEqual(first.bodyPoints, second.bodyPoints);
  assert.ok(first.cornerSharpness > 0.5);
});

test('buildCornerGlyph exposes mode cues without changing identity seed', () => {
  const idle = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'idle'));
  const error = buildCornerGlyph(resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 123 }, 'error'));

  assert.equal(idle.seed, error.seed);
  assert.notEqual(idle.mouthCurve, error.mouthCurve);
  assert.notEqual(idle.selectedEdge, error.selectedEdge);
});
