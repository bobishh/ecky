# Proposal: Native ↔ build123d Differential Parity Harness

## Intent

Stop proving native-backend correctness with hand-picked point assertions
(bbox here, non-manifold count there). Every regression so far (lid swallowed
by glyph fuse, hairline cracks, empty-overlay fuse poisoning) would have been
caught immediately by one check: **render the same macro through build123d and
require the native output to match the reference within tolerance — in
geometry and in time.**

## Problem (evidence)

- The woodlouse-hotel eval asserted bbox + manifoldness with fixture params;
  the user's real render (different params, empty SVG overlays) lost the lid
  and produced 4020 non-manifold edges while the eval stayed green.
- Point assertions encode what we already know can break. A differential
  reference encodes what "correct" means, parameter set by parameter set.
- Time has no baseline either: "fast enough" drifted until real artwork blew
  past the 60 s MCP timeout.

## Variables

- **Goal:** any macro that renders through build123d must render natively with
  matching geometry and comparable time, across parameter sets (including
  empty/default image params).
- **Reference:** `build123d::render_model` (bundled runtime) on the identical
  `.ecky` source + params → reference preview STL + reference wall time.
- **Geometry metrics compared:** mesh volume (divergence theorem), surface
  area, per-axis bbox, shell/component count, native non-manifold edge count.
- **Tolerances:** volume ±2 %, area ±5 % (tessellation-sensitive), bbox
  ±0.5 mm per axis, component count exact, native non-manifold = 0.
  Rationale: both meshers approximate the same B-rep; chord error is ≤0.04 mm
  native and comparable in build123d, so 2 %/0.5 mm is generous headroom while
  still catching any lost/inverted/duplicated solid instantly.
- **Time budget:** native wall time ≤ max(10 s, 3 × build123d wall time) per
  fixture ("three sigma"-style envelope around the reference baseline; the
  floor absorbs CI noise on trivially fast references).
- **Fixture corpus:** woodlouse hotel with (a) artwork SVG params set,
  (b) image params empty (`""` — the default-UI state that regressed), plus
  glyph-text and artwork-soup minimal macros.
- **Ownership:** test harness in `direct_occt_executor.rs` tests; no
  production-code surface.
- **Runtime constraints:** live tests skip when the bundled build123d runtime
  or the native runner is unavailable (same guards as existing live tests).

## Decision

Add `assert_native_matches_build123d_reference(macro, params, label)`:
render both backends, compare STL metrics within the tolerances above, and
enforce the time envelope. Wire the fixture corpus through it. Existing point
assertions stay (they are cheaper and more localized), but parity claims are
only made by differential tests.

## Rejected Paths

- **Exact mesh diff / Hausdorff distance.** Overkill; meshers legitimately
  differ facet-by-facet. Integral metrics catch every real regression seen.
- **Golden STL files in repo.** Reference must track build123d behavior and
  parameter sets, not a frozen snapshot.
- **Statistical timing over N runs.** One run with a 3× multiplier + floor is
  stable enough for CI; repeated runs triple suite time.

## Scope

- STL metrics helper (ASCII + binary STL).
- Differential assertion helper + fixture corpus tests.
- Fix whatever the harness catches on the woodlouse empty-params case.

## Out of Scope

- FreeCAD as a second reference.
- Per-face topology diffing / selector parity (separate concern).

## Proof Plan

- Red: woodlouse with empty image params fails the differential test against
  build123d (lid missing natively today).
- Green: after the fix, all corpus fixtures pass geometry + time envelopes on
  both native tiers.
