# Tasks: SVG/Text Native Exact-Curve Parity

## 1. Segment extraction (pure Rust)
- [x] 1.1 `svg_profile.rs`: contours carry exact segment lists
      (`PathSegment::Line | Cubic`) alongside flattened check-points; fit
      transform applied to control points.
- [x] 1.2 `text_profile.rs`: glyph outlines carry exact segments; TTF quads
      elevated to cubics losslessly.
- [x] 1.3 Unit tests: curved contour segments survive fit/transform; quad
      elevation endpoints/derivatives match.

## 2. Plan emission
- [x] 2.1 Red: plan-emission test — curved SVG contour emits `bezier-path`
      (3n+1 point3), straight contour still emits `polygon`.
- [x] 2.2 `direct_occt_normalize.rs`: svg clean path + wire-soup path emit
      `bezier-path` nodes for curved contours.
- [x] 2.3 `direct_occt.rs`: text glyph loops emit `bezier-path` nodes for
      curved contours.
- [x] 2.4 Existing svg/text plan tests stay green (straight-edge fixtures
      byte-identical).

## 3. Proof
- [x] 3.1 Woodlouse-hotel live eval stays green (manifold, bbox) on both
      tiers.
- [x] 3.2 Perf guard: dense curved artwork live render within wall-clock
      bound.
- [x] 3.3 STEP export of artwork face contains Bézier/BSpline entities.
- [x] 3.4 `openspec validate svg-native-exact-curves`.
