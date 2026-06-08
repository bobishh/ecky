# Tasks: Convenience Ops & Language Standard Library

Each native op follows the per-op recipe in `design.md` (surface → Core IR →
3 lowerings → tests) and is not done until its **parity test** (native vs
build123d bbox+volume) is green.

## 1. Phase 1a — Convenience primitives

- [x] 1.1 `torus` (major/minor radius) — DONE. Core IR primitive + 3 lowerings;
  planner + both lowering unit tests GREEN; native-vs-build123d bbox+volume parity
  VERIFIED on a real render (torus 10 3): both backends bbox x/y [-13,13] z [-3,3],
  volume 1776.529 (= 2π²·R·r²) — exact.
  Sites: `ecky_core_ir/mod.rs` (CorePrimitive::Torus), `signatures.rs` (TORUS_DIMS +
  verify arm + slots + name + op list), `compiler.rs` (dispatch + emit-back),
  `cad.rs`, `build123d_lowering.rs` (name + `Torus(major,minor)` arm),
  `freecad_lowering.rs` (name + `_ecky_torus` helper via `Part.makeTorus`),
  `direct_occt.rs` (OcctOp::Torus + op map + name), `direct_occt_executor.rs`
  (TorusArgs/torus_args/emit_torus_operation + include), `direct_occt_runner.rs`
  (gate + token + support test), `direct_occt_runner.cpp` (TorusArgs/torus_args/
  make_torus + `BRepPrimAPI_MakeTorus` + dispatch; rebuilt + synced to target/{debug,
  release}/runtime/occt/bin). Also Torus arm in the three op-name matches in
  `runtime.rs`, `model.rs`, `services/render.rs`.
  Tests: `plans_torus_primitive_for_direct_occt`,
  `lower_to_build123d_torus_emits_torus_with_major_minor`,
  `freecad_lowering_emits_torus_helper_with_major_minor`.
  SCOPE (mapped): true analytic primitive (NOT a `revolve` desugar — three
  backend `revolve` impls risk parity drift). Touch sites, modelled on `cone`:
  - `ecky_core_ir/mod.rs`: add `CorePrimitive::Torus`.
  - `signatures.rs`: verify arm (~526), `DimensionSlots` (~1512), name (~1769).
  - `compiler.rs`: dispatch `"torus" =>` (~8537), emit-back (~9030). (value kind
    defaults to Solid — no `infer_value_kind` change.)
  - `cad.rs`: add `"torus"` export.
  - `build123d_lowering.rs`: name map (~782) + dispatch arm → `Torus(major, minor)`
    (build123d `Torus(major_radius, minor_radius)`).
  - `freecad_lowering.rs`: name map (~124) + dispatch + `_ecky_torus` helper
    (`Part.makeTorus(major, minor)`).
  - `direct_occt.rs`: `OcctOp::Torus` (~62), op map (~2518), name (~2610).
  - `direct_occt_executor.rs`: emit sites (~724, 755, 3301, 3500).
  - `direct_occt_runner.rs`: runner command builder (~351, 648, 937, 1712, 2124).
  - `direct_occt_runner.cpp`: `#include <BRepPrimAPI_MakeTorus.hxx>`, `TorusArgs`,
    `torus_args`, `make_torus` (BRepPrimAPI_MakeTorus(major, minor)), `if op ==
    "torus"` dispatch; then `scripts/build_direct_occt_runner.sh` + sync runtimes.
  - Tests: planner op test + lowering test + native-vs-build123d bbox parity.
- [x] 1.2 `ellipse` (rx, ry) 2D profile — DONE. Sketch primitive + 3 lowerings;
  planner + both lowering unit tests GREEN; native-vs-build123d parity VERIFIED on
  extruded ellipse both `(ellipse 10 4)` and swapped `(ellipse 4 10)`: bbox + volume
  628.319 (= π·rx·ry·h) exact, incl. major-axis swap when ry>rx. Sites mirror torus
  plus value-kind Sketch (`infer_value_kind`). build123d `Ellipse(x,y)`; freecad
  `_ecky_ellipse` via `Part.Ellipse(center, major, minor)` + 90° rotate when ry>rx;
  native `make_ellipse_face` via `gp_Elips` with axis pick (`gp_Elips.hxx`). Tests:
  `plans_ellipse_profile_for_direct_occt`,
  `lower_to_build123d_ellipse_emits_ellipse_with_x_y_radius`,
  `freecad_lowering_emits_ellipse_helper_with_x_y_radius`.
- [x] 1.3 `regular-polygon` (sides, circumradius, optional `:rotation`) — DONE.
  Custom op; one shared `regular_polygon_vertices` (in `ecky_core_ir`) drives all
  three backends so geometry is identical by construction. Parity verified
  native vs build123d (20 facets, bbox x[-10,10] y[-8.66,8.66] z[0,5] — exact).
  Tests: `plans_regular_polygon_as_polygon_for_direct_occt`,
  `lower_to_build123d_regular_polygon_emits_helper_with_sides_radius_rotation`.
- [x] 1.4 Slot full set — all 4 variants DONE.
  Key finding: obround ≠ rounded-rect — native rounded-rect dies at r=W/2
  (degenerate zero-length straight segments → `BRep_API: command not done`) and
  build123d `RectangleRounded` forbids r=W/2. So slot needs its own obround builder
  (2 straight + 2 semicircle arcs, no short segments). Implemented as ONE canonical
  primitive `CorePrimitive::Slot(length, width)` (the shared obround); the named
  variants are thin front-ends:
  - `slot-overall L W` → Slot primitive directly. build123d `SlotOverall`, freecad
    `_ecky_slot` (line+arc wire), native `make_slot_face` / executor inline wire.
  - `slot-center-to-center sep W` → Custom op, expands to Slot(sep+W, W). build123d
    `SlotCenterToCenter`, freecad `_ecky_slot_c2c`.
  - `slot-center-point cx cy px py W` → Custom op, expands to Slot wrapped in
    rotate+translate (length 2·d+W, angle atan2). build123d `SlotCenterPoint`,
    freecad `_ecky_slot_center_point`.
  Parity VERIFIED native==build123d: overall(40,10) & c2c(30,10) both
  bbox[-20,20]×[-5,5]×[0,5] vol 1892.699; center-point along-X / rotated-90° /
  offset all exact. Tests: `plans_slot_overall_primitive_for_direct_occt`,
  `plans_slot_center_to_center_as_slot_for_direct_occt`,
  `plans_slot_center_point_as_transformed_slot_for_direct_occt`,
  `lower_to_build123d_slot_overall_and_center_to_center_emit_slot_calls`,
  `lower_to_build123d_slot_center_point_emits_slot_center_point`,
  `freecad_lowering_emits_slot_helpers`,
  `freecad_lowering_emits_slot_center_point_helper`.
  - `slot-arc radius start end width` → primitive `CorePrimitive::SlotArc` (curved
    annular obround = Minkowski sum of a circular-arc centerline with a disk of r=W/2).
    build123d `SlotArc(CenterArc((0,0), radius, start, end-start), width)`; freecad
    `_ecky_slot_arc` + native `make_slot_arc_face` / executor inline build the same
    4-arc wire (outer arc Ro=R+r, end-cap semicircle, inner arc Ri=R-r, end-cap),
    angles in degrees. Parity VERIFIED native==build123d on quarter `(slot-arc 20 0 90
    10)` bbox[-5,25]² vol 1963.495 and wide >180° `(slot-arc 30 30 200 8)`
    bbox[-34,29.981]×[-14.261,34] vol 3049.439 — exact. Tests:
    `plans_slot_arc_primitive_for_direct_occt`,
    `lower_to_build123d_slot_arc_emits_slot_arc_with_center_arc`,
    `freecad_lowering_emits_slot_arc_helper`.
- [x] 1.5 `trapezoid` (bottom, top, height, optional skew) — DONE. Custom op (like
  regular-polygon): one shared `trapezoid_vertices` (in `ecky_core_ir`) drives all
  three backends → geometry identical by construction; expands to a `polygon`, so no
  new OCCT primitive / no cpp change. Sites: `ecky_core_ir/mod.rs`
  (`trapezoid_vertices`), `cad.rs` allowlist, `compiler.rs` `infer_value_kind`→Sketch,
  `direct_occt.rs` (`expand_trapezoid_node` → polygon), `direct_occt_normalize.rs`
  (custom-op allowlist), `build123d_lowering.rs` + `freecad_lowering.rs` (dispatch +
  `_ecky_trapezoid` helper). Parity VERIFIED on `(trapezoid 20 10 8 :skew 3)` extrude
  5: bbox [-10,10]×[-4,4]×[0,5], vol 600.0 native == build123d exact. Tests:
  `plans_trapezoid_as_polygon_for_direct_occt`,
  `lower_to_build123d_trapezoid_emits_helper_with_bottom_top_height_skew`,
  `freecad_lowering_emits_trapezoid_helper_with_bottom_top_height_skew`.
- [x] 1.6 `wedge` (dx, dy, dz + taper params) — DONE. Surface form chosen to map 1:1
  onto OCCT `BRepPrimAPI_MakeWedge` and build123d `Wedge` (verified by introspecting
  the build123d signature): `(wedge dx dy dz xmin zmin xmax zmax)` + optional
  `:align` (default center). True analytic primitive across Core IR + 3 backends.
  Native/freecad are origin-built, so alignment uses the origin-offset helper
  (`align_offset` / `_ecky_axis_offset`), not the centered one — caught a bbox-offset
  drift in review and fixed. Parity VERIFIED on `(wedge 20 10 20 5 5 15 15)`: bbox
  [-10,10]×[-5,5]×[-10,10], vol 2333.333 native == build123d exact. Sites mirror torus
  (enum/signatures/compiler/cad/3 op-name arms/build123d/freecad `_ecky_wedge` via
  `Part.makeWedge`/direct_occt enum+map+name/executor `emit_wedge_operation` +
  `BRepPrimAPI_MakeWedge.hxx`/runner gate+token+support/cpp `make_wedge`+dispatch).
  Tests: `plans_wedge_primitive_for_direct_occt`,
  `lower_to_build123d_wedge_emits_wedge_with_seven_dims`,
  `freecad_lowering_emits_wedge_helper_with_seven_dims`.
- [x] 1.7 Docs: DONE. Added "Convenience Shapes" section to the ecky-ir book in both
  source copies — `docs/books/ecky-ir/chapters/02a-convenience-shapes.md` (+ index.md
  entry) and the assembled `public/docs/ecky-ir.md` (after "Sketch to Solid"). One
  compiling example each for torus, ellipse, regular-polygon, trapezoid, wedge, and
  all four slots; every snippet verified via `ecky lower --backend build123d`. Book
  rebuilds clean (`npm run build:book` → HTML + EPUB); book test green.

## 2. Phase 1b — Convenience feature ops

> ARCHITECTURE NOTE (learned while scoping 2.4): a Scheme-prelude desugar (defining
> `thread` in `cad.rs` `source()` to return `(union (cylinder…) (helical-ridge…))`)
> does NOT work. The primary lowering path is `lower_to_build123d` → `parse_model`,
> a direct AST parse that does NOT run the Steel prelude (Steel eval via
> `compile_to_core_program` is only the parse-failure fallback). `parse_model` accepts
> unknown heads as custom nodes, then the Value-path lowerer rejects them. So any new
> composite op (thread/rib/groove) must be implemented as a dual-pipeline custom op
> like `helical-ridge`: (a) CoreNode expansion in `direct_occt.rs` + normalize
> allowlist for native, AND (b) Value/IrExpr handling in `build123d_lowering.rs` and
> `freecad_lowering.rs` (the `parse_model` path). There is no single shared desugar
> layer across the three backends. Probe results: build123d has native `draft(faces,
> neutral_plane, angle)` (good for 2.1) but `fillet` takes a single radius only
> (variable-radius 2.3 has parity risk) and NO thread classes (2.4 must compose).

- [x] 2.1 `draft` — DONE (MVP: all vertical side faces). `(draft angle solid
  [:neutral-z z])` tapers every vertical face about the `z = neutral-z` plane (default
  0), pulling +Z. Parity-target native↔build123d (both OCCT); FreeCAD has no Part
  draft API → clear backend error. Native: `emit_draft_operation` (executor) iterates
  faces, filters vertical via `BRepGProp_Face` normal, `BRepOffsetAPI_DraftAngle.Add`.
  build123d: `_ecky_draft` filters vertical faces + `draft(faces, Plane(z), angle)`.
  Parity VERIFIED native==build123d on `(draft 10 (box 20 20 20))` vol 5510.408,
  bbox [-10,10]×[-10,10]×[0,20] exact. Tests: `plans_draft_as_draft_op_for_direct_occt`,
  `lower_to_build123d_draft_emits_draft_helper`,
  `freecad_lowering_rejects_draft_with_clear_error`. FOLLOW-UP: `:faces` selector
  targeting (reuse shell's face-selector C++ emission) so specific faces can be
  drafted instead of all vertical ones; native cpp-runner `draft` op (currently
  native goes through the executor path, which renders draft fine).
- [x] 2.2 `rib` and `groove` — DONE. Dual-pipeline custom ops desugaring to existing
  parity-clean ops: `(rib solid profile path)` → `union(solid, sweep(profile, path))`;
  `(groove solid profile path)` → `difference(solid, sweep(profile, path))`. Native:
  `expand_rib_groove_node` builds the boolean(solid, sweep) CoreNode tree + normalize
  allowlist. build123d: `sweep(_ecky_face(profile), path) ± solid` (BinOp). freecad:
  `_ecky_sweep` + `_ecky_fuse_many`/`_ecky_cut_many`. Parity VERIFIED native==build123d:
  protruding rib `(rib (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))` vol 8282.743
  (= box + π·9·10 protruding tube) exact; groove identical structure. Tests:
  `plans_rib_and_groove_as_sweep_booleans_for_direct_occt`,
  `lower_to_build123d_rib_and_groove_emit_sweep_booleans`,
  `freecad_lowering_emits_rib_and_groove`. NOTE: geometry follows existing `sweep`
  semantics (the section sweeps at the profile's location along the path direction).
- [x] 2.3 Variable-radius `fillet` — DONE as a **tapered** fillet (`:to-radius`),
  backward compatible. Parity-target correction (per review): variable fillet is an
  OCCT capability, not a build123d one — so parity is **native↔freecad** (both OCCT),
  not native↔build123d. Verified FreeCAD `makeFillet` accepts `(r, edges)` and
  `(r1, r2, edges)` (taper r1→r2 along the edge) but NOT per-edge-different radii in
  one builder; so the achievable, parity-clean variable form is the **taper**.
  `(fillet r1 :to-radius r2 [:edges …] solid)` → native OCCT `MakeFillet.Add(r1, r2,
  edge)` (executor inline + cpp runner `fillet_shape`), freecad
  `_ecky_fillet(..., to_radius=…)` → `makeFillet(r1, r2, edges)`. build123d cannot
  taper (single-radius `fillet()`), so it raises a clear caveat error directing to
  native/freecad; uniform fillet unchanged on all three. `chamfer :to-radius` is
  rejected (fillet-only). Parity VERIFIED native==freecad on `(fillet 3 :to-radius 1
  (box 30 30 30))` vol 26668.908 exact; uniform fillet regression intact (vol
  7804.696). Tests: `lower_to_build123d_tapered_fillet_is_rejected_with_clear_error`,
  `freecad_lowering_emits_tapered_fillet`, plus updated selector-aware fillet/chamfer
  tests (24 fillet/chamfer tests green). NOTE: established the **native↔freecad parity
  harness** (freecadcmd → STEP → build123d `import_step` measure) — reusable for any
  OCCT-only feature build123d lacks (e.g. 2.1 draft's freecad side).
- [x] 2.4 `thread` (parametric core) — DONE. Dual-pipeline custom op (NOT a Scheme
  prelude desugar — see ARCHITECTURE NOTE). `(thread :radius r :pitch p :length len
  :depth d [:base-width][:crest-width][:female #t][:clearance c][:lefthand #t])`.
  Composes the existing parity-clean `helical-ridge` + a core `cylinder`: male =
  `union(cylinder(r,len), ridge)`, female = bare ridge cutter (`helical-ridge :female`).
  base/crest default to 0.75·p / 0.25·p. Native: `expand_thread_node` in
  `direct_occt.rs` reuses `expand_helical_ridge_node` + normalize allowlist.
  build123d/freecad: `_ecky_thread` helper calling `_ecky_helical_ridge` + core
  cylinder + fuse. Parity VERIFIED native==build123d: male `(thread :radius 8 :pitch 2
  :length 16 :depth 1)` vol 3503.07 exact; female cutter vol 691.88 exact. Tests:
  `plans_thread_as_union_of_cylinder_and_ridge_for_direct_occt`,
  `plans_female_thread_as_ridge_cutter_for_direct_occt`,
  `lower_to_build123d_thread_emits_thread_helper`, `freecad_lowering_emits_thread_helper`.
- [x] 2.4b `thread :iso "M4"` — DONE. Shared `iso_metric_thread_core(designation)` in
  `ecky_core_ir` (table M2–M24, coarse pitch; external depth H1=0.6134·P, core
  radius=D/2−depth) decodes into the same parametric core in all three backends, so
  `:iso` is pure sugar over 2.4. Unknown designations fail with a clear diagnostic.
  Parity native==build123d on `(thread :iso "M6" :length 18)` within tolerance
  (vol 373.17 vs 373.22, <0.02% — same swept-helix kernel tolerance as helical-ridge).
  Tests: `iso_metric_thread_core_decodes_known_and_rejects_unknown`,
  `lower_to_build123d_thread_iso_decodes_designation`.
- [x] 2.5 Docs: DONE. thread ("Threads" in `02a-convenience-shapes.md`), tapered-fillet
  + draft ("Tapered fillets" / "Draft" in `05-round-shell-select.md`), rib/groove
  ("Ribs and grooves" in `06-paths-and-surfaces.md`) — all in both book copies, with
  backend caveats; examples compile; book rebuilds clean. Below: per-feature detail.
  "Threads" subsection in
  `02a-convenience-shapes.md` + `public/docs/ecky-ir.md` (parametric, `:iso` sugar,
  female cutter via `difference`, lefthand, unknown-designation error). "Tapered
  fillets" subsection in `05-round-shell-select.md` + `public/docs/ecky-ir.md`
  (`:to-radius` + build123d backend caveat). Examples compile (build123d for thread,
  freecad for the taper which build123d can't do); book rebuilds clean. `draft`
  examples pending on 2.1.

## 3. Phase 2 — Language standard library

> RE-SCOPED: stdlib storage/extract/search/import infra ALREADY EXISTS
> (component-unification T5). Components are HAND-AUTHORED from a curated
> "gentleman's set" — a few genuinely parametric families — using the FreeCAD
> library only as a dimensional REFERENCE. Evidence killed the transpile-to-source
> plan: a 97-part library survey found 0% with Array/expression/spreadsheet
> parametricity and 45% PartDesign, so a deterministic transpile yields
> dead-number geometry needing manual re-parametrization anyway, and the library
> is family-redundant (1034 fastener files = size-variants of ~5 families). A
> separate LLM-based transpile is a user-facing convenience for importing foreign
> CAD, NOT the stdlib's source of truth — see
> `openspec/changes/cad-transpile-engine`.

- [x] 3.1 stdlib storage + manifest format — DONE-by-existing-infra
  (`component_package_runtime.rs`: `component-library/<package_id>/<version>/`
  with `ecky-package.json` + `ecky-header.json`). No new format needed; do NOT
  change it (shared contract with Phase 3).
- [x] 3.2 search/get over the library — DONE-by-existing-infra
  (`component_search` header-only, `component_get` full source).
- [ ] 3.3 Ship fasteners — hand-authored parametric families (not per-size files):
  `hex-bolt` (d/length/pitch, uses `thread`), `socket-head-cap-screw`, `hex-nut`,
  `washer`, `threaded-rod`. Library = dimensional reference only.
- [ ] 3.4 Ship mechanical: `ball-bearing` (608/623/624 family), `gt2-pulley`
  (teeth/bore), `standoff`, `heat-set-insert-pocket` (FDM-critical).
- [ ] 3.5 Ship mountings where `repeat-union` earns its keep: `corner-bracket`,
  `l-bracket`, `hole-plate` (parametric `cols×rows` grid).
- [x] 3.6 version pinning — DONE-by-existing-infra (storage is version-keyed;
  `resolve_installed_component_source(version)`, version-keyed install/list).
- [ ] 3.7 Tests: every shipped stdlib component compiles + passes its own
  `verify` (single-solid + manifold at minimum) on the default backend.

## 4. Phase 3 — Component import

- [ ] 4.1 Shared import path: list stdlib + user-library components and insert a
  chosen one into the active model as copy-inlined instantiated source.
- [ ] 4.2 MCP `component_import` tool over the shared path (agent entry point).
- [ ] 4.3 Workbench import panel over the same shared path (human entry point):
  search → click → insert; no agent required. Theme-consistent (Tactical
  Midnight), behaves identically to the MCP tool.
- [ ] 4.4 Guard: import is copy-inline only — no implicit registry reference;
  inserted source is self-contained.
- [ ] 4.5 Tests: import → compile → render round-trip via both the tool and the
  panel path for a representative stdlib part.

## 5. Cross-cutting

- [ ] 5.1 Parity harness: a reusable test helper that renders a model on native
  and build123d and asserts bbox + volume agree within tolerance (generalize the
  `helical-ridge` clip regression).
- [ ] 5.2 `verify_core_program` coverage for every new op's arity/type errors.
