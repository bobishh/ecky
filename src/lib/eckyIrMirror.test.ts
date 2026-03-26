import assert from 'node:assert/strict';
import test from 'node:test';

import { fromMirrorIr, isEckyIrSource, toMirrorIr } from './eckyIrMirror';

test('detects canonical and mirror ecky ir sources', () => {
  assert.equal(isEckyIrSource('(model (part body (cylinder 10 20)))'), true);
  assert.equal(isEckyIrSource('(scene (piece body (cylinder 10 20)))'), true);
  assert.equal(isEckyIrSource('import FreeCAD'), false);
});

test('round-trips canonical ir through mirror aliases', () => {
  const canonical = `(model
  (params
    (number width 120 :label "Width" :min 20 :max 300)
    (toggle vents #t))
  (part body
    (wall-pattern
      (:mode ribs :depth 1.2 :uFreq 12 :softness 0.1)
      (shell 2
        (extrude
          (difference
            (offset-rounded 2 (circle 10 24))
            (circle 10 24))
          8))))
  (part accent
    (xor
      (arc-array 4 24 -45 45
        (mirror x 0
          (loft 20
            (circle 10 24)
            (scale (smoothstep 0 1 0.5) (smoothstep 0 1 0.5) 1 (circle 10 24)))))
      (translate 0 0 2 (cylinder 20 80 48)))))`;

  const mirrored = toMirrorIr(canonical);
  assert.match(mirrored, /\(scene/);
  assert.match(mirrored, /\(controls/);
  assert.match(mirrored, /\(num width 120/);
  assert.match(mirrored, /\(surface/);
  assert.match(mirrored, /\(hollow 2/);
  assert.match(mirrored, /\(inflate-round 2/);
  assert.match(mirrored, /:style ribs/);
  assert.match(mirrored, /:amount 1\.2/);
  assert.match(mirrored, /:u 12/);
  assert.match(mirrored, /:soft 0\.1/);
  assert.match(mirrored, /\(exclusive/);
  assert.match(mirrored, /\(arc 4 24 -45 45/);
  assert.match(mirrored, /\(flip x 0/);
  assert.match(mirrored, /\(blend 20/);
  assert.match(mirrored, /\(softstep 0 1 0\.5/);
  assert.match(mirrored, /\(move 0 0 2/);
  assert.match(mirrored, /:name "Width"/);
  assert.equal(fromMirrorIr(mirrored), canonical);
});

test('round-trips new hole-aware and organic aliases', () => {
  const canonical = `(model
  (part body
    (profile
      (:outer (rounded-polygon ((0 10) (10 0) (0 -10) (-10 0)) 2 8))
      (:holes (bspline ((0 5) (5 0) (0 -5) (-5 0)) #t 12)))))`;

  const mirrored = toMirrorIr(canonical);
  assert.match(mirrored, /\(piece body/);
  assert.match(mirrored, /\(outline/);
  assert.match(mirrored, /:rim \(shape-round/);
  assert.match(mirrored, /:cuts \(curve/);
  assert.equal(fromMirrorIr(mirrored), canonical);
});

test('does not rewrite tokens inside string literals', () => {
  const mirrored = toMirrorIr(
    '(model (params (select mode "difference" :options (("Union" "union")))) (part body (box 1 1 1)))',
  );
  assert.match(mirrored, /"difference"/);
  assert.match(mirrored, /"union"/);
});
