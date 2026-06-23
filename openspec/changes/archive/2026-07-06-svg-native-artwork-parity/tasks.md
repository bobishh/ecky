# Tasks: SVG Native Artwork Parity

## 1. Tolerant wire-soup extraction (pure Rust, no OCCT dependency)
- [x] 1.1 Add `extract_svg_wire_soup(svg_text) -> Vec<SvgWireSoup>` in `ecky_cad_host/svg_profile.rs` reusing usvg path extraction but WITHOUT `self_intersects`, `reject_multi_outer_first_slice`, `reject_open_contour` guards (tolerant `extract_contours`).
- [x] 1.2 Capture `fill-rule` (nonzero/evenodd) per `<path>` from usvg into `SvgWireSoup::fill_rule`.
- [x] 1.3 Keep `parse_svg_profile` (clean fast path) unchanged; tolerant path is additive.
- [x] 1.4 Unit tests: self-intersecting lineart, evenodd multi-subpath, compound icon extract wire soups without rejection.

## 2. Native region resolution via OCCT (requires Direct OCCT bindings)
- [x] 2.1 Audit Direct OCCT Rust bindings for `BRepBuilderAPI_MakeFace` (planar), `ShapeFix_Face`, `BRepFeat::IsInside` availability. (Region resolution lives in the C++ `direct-occt-runner`, not Rust FFI; it already had `BRepBuilderAPI_MakeFace` + `BRepClass_FaceClassifier`; added `ShapeFix_Face`.)
- [x] 2.2 Implement `make_faces_from_wire_soup` in `native/direct_occt_runner.cpp` mirroring `ocpsvg.ocp.faces_from_wire_soup`: MakeFace(planar/OnlyPlane) per wire -> ShapeFix_Face/FixOrientation -> nesting by containment + parity-of-depth (BRepClass_FaceClassifier).
- [x] 2.3 Soft-fail unfaceable wires (skip), matching ocpsvg `except InvalidWiresForFace`.
- [x] 2.4 Wire tolerant path + resolver into Direct OCCT `(svg ...)` op handling: `expand_svg_node`/normalize fall back to `extract_svg_wire_soup_profile` -> `profile :outer :fill-rule` -> runner soup resolver.
- [x] 2.5 Integration test: `live_precompiled_runner_resolves_svg_wire_soup_artwork_when_available` renders a compound artwork SVG via the real Direct OCCT runner and asserts multi-face topology. (bananas/carrot are gated behind local Downloads access; synthetic compound covers the same path.)

## 3. Capability + spec surface
- [x] 3.1 Update `backend_capabilities.rs`: native `svg` message now describes the fast-path + OCCT wire-soup artwork resolution (stays raw-op `ExplicitlyUnsupported` because svg is always rewritten to `profile` before native execution).
- [x] 3.2 Validate change (`openspec validate svg-native-artwork-parity` → valid).

> Point-list comprehension feature (separate WIP from commit 39cb3ea, done alongside):
> henon/lorenz/organic point generators, `map`/`append`/`reverse` point-list
> materialization, pair-list kind error, and selector-payload error propagation —
> 7 of 8 `build123d_lowering_tests` greened. Remaining: the legacy-vs-core
> entrypoint parity test (`i` vs Steel-hygiene `i2` naming) and 7 pre-existing
> scheme-compiler WIP failures (destructuring/selector/repetition) — both out of
> the SVG scope and not regressed by this change.

## 4. Regression
- [x] 4.1 Existing `svg_profile` tests stay green (12/12).
- [x] 4.2 Clean single-outer-loop profile fast path produces identical loops/face (unchanged `parse_svg_profile` path; artwork only on its failure).

## 6. Boolean robustness for glyph/artwork profiles (build123d parity)
- [x] 6.1 Profile faces re-orient wires via ShapeFix_Face regardless of input
      winding (font glyphs with counters arrived pre-reversed and produced
      inverted solids that silently swallowed fuse operands).
- [x] 6.2 `ensure_face_normal_up` (ocpsvg parity) on every profile/soup face in
      both tiers so extrusions never emit inside-out prisms.
- [x] 6.3 Soup nesting classifies containment with `BRepFeat::IsInside` (whole
      face, ocpsvg `BRepFeat.IsInside_s`) instead of one sample point —
      partially overlapping artwork regions are separate outers, not holes.
      TKFeat added to REQUIRED_OCCT_LIBS + bundled runtime.
- [x] 6.4 `extrude` of multi-face compounds fuses per-face prisms into one
      valid solid (overlapping artwork regions poisoned downstream booleans).
- [x] 6.5 Red test `live_native_fuse_keeps_box_when_fused_with_holed_glyph_text`
      + eval bbox asserts (lid canopy must survive fuse with text/SVG overlays).

## 5. Shim executor parity + completion eval
- [x] 5.1 Shim executor tier (`direct_occt_executor.rs`) accepts `profile :fill-rule`
      and resolves wire soup via emitted `emit_wire_soup_profile_face` C++
      (ShapeFix_Face + containment-parity nesting, mirroring the runner) — the
      app's fallback tier no longer rejects artwork profiles.
- [x] 5.2 Completion eval: `live_woodlouse_hotel_macro_exports_end_to_end_when_runtime_ready`
      exports the two-part woodlouse hotel macro
      (`tests/fixtures/cad/surface/woodlouse_hotel.ecky` + artwork SVG fixtures)
      through the production runner-first path; verified green on both the
      native runner tier and the shim tier (`ECKY_DIRECT_OCCT_RUNNER_DISABLED=1`).
