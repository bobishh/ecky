# Tasks: Native ↔ build123d Differential Parity

## 1. Harness
- [x] 1.1 STL metrics helper: volume, area, bbox, component count for ASCII
      and binary STL.
- [x] 1.2 `assert_native_matches_reference(macro, params, label)`:
      render both backends, compare metrics (vol ±2 %, area ±5 %, bbox
      ±0.5 mm, components exact, native non-manifold = 0), enforce
      native ≤ max(10 s, 3 × reference time).

## 2. Corpus (red → green)
- [x] 2.1 Woodlouse hotel, artwork params set (reference: FreeCAD — build123d
      lowering cannot express geometry `if` / 4-arg `svg` yet; see follow-up).
- [x] 2.2 Woodlouse hotel, image params empty ("" — the regressed UI default;
      reference: FreeCAD).
- [x] 2.3 Glyph text minimal macro (red kept as #[ignore] with reason:
      build123d centers text, native/FreeCAD left-baseline — lowering fix
      follow-up).
- [x] 2.4 Artwork wire-soup minimal macro (red kept as #[ignore] with reason:
      soup region resolution diverges from ocpsvg for self-intersecting wires —
      BOPAlgo_Tools::WiresToFaces follow-up).

## 3. Fix + proof
- [x] 3.1 Fix the defect the corpus exposed: near-coincident bezier contour
      endpoints emitted micro closing segments (degenerate edges → swallowed
      fuses, thousands of non-manifold edges). Closure now snaps within 1e-6;
      zero-extent segments dropped. Also: native default font stack reordered
      to regular-weight-first (build123d/FreeCAD parity).
- [x] 3.2 Woodlouse corpus green on runner tier and shim tier
      (ECKY_DIRECT_OCCT_RUNNER_DISABLED=1).
- [x] 3.3 `openspec validate native-build123d-differential-parity`.
