# Tasks: Parametric Thread Primitive

Builds on the landed radial-thread fix (`1a58364`: Frenet + RightCorner sweep).
Author tests first (BDD red→green) per task; verify native ↔ build123d parity.

## 1. Fix the `thread` coincident-face / hollow bug (highest priority)

- [x] 1.1 (test) external `thread` with coarse/deep params (e.g. d12 pitch3
  depth1.2) produces ONE solid with the expected solid-core volume — not a
  hollow spiral. Guard against regression: assert `volume` ≈ core+ridge, and
  `connected-component-count = 1`. DONE 2026-07-06: fix (1.2) landed in
  `64af169` without an automated test — only manual verification in the
  commit message. Added
  `thread_union_stays_solid_on_coarse_deep_params_regression` in
  `direct_occt_executor.rs` (radius 6/pitch 3/length 10/depth 1.2, matching
  the bug's repro shape); confirmed it actually catches the regression by
  temporarily zeroing the fix's `overlap` term (hard failure — runner
  couldn't even triangulate the degenerate shape) then restoring it (green:
  1 component, volume 1426 ≥ 0.9× bare-core 1131). STL facet-adjacency
  non-manifold counting is NOT used as a signal here — helical sweep
  tessellation produces legitimate seam T-vertices the heuristic flags on a
  valid solid; volume + component count are the real hollow-vs-solid proof.
- [x] 1.2 Build the core with an internal overlap in `expand_thread_node`
  (native) and `_ecky_thread` (build123d): `core_radius = radius + overlap`,
  ridge `:depth = depth + overlap`. No coincident core/ridge face. DONE in
  `64af169` (both native `direct_occt.rs::expand_thread_node` and
  `build123d_lowering.rs::_ecky_thread`); now proven by 1.1's test.
- [ ] 1.3 (test) geometric-sanity verify catches a hollow result (a valid but
  wrong solid): expected-volume / core-present check, not only `IsValid`.
  Distinct from 1.1: this wants the app's own `verify_core_program`/authored
  `(verify)` pipeline to catch it as a diagnostic, not an external test
  harness — still open.

## 2. Intent-derived profile (`:flank`, derivation)

- [ ] 2.1 (test) `(thread … :flank 45deg :depth d :crest c)` derives
  `base = c + 2·d·tan(45°)` and renders; changing `:flank` changes `base`.
- [ ] 2.2 Implement derivation in the op (build123d + native); explicit
  `:base-width`/`:crest-width` still override.
- [ ] 2.3 (test + diagnostic) `pitch ≤ base + clearance` emits a printability
  diagnostic (turns merge) without hard-failing.

## 3. `tapped-hole` cutter (manifold by construction)

- [ ] 3.1 (test) `(difference wall (tapped-hole :iso "M8" :length L))` yields a
  manifold body (`non-manifold-edge-count = 0`) with a through bore at minor and
  helical relief out to major.
- [ ] 3.2 Implement `tapped-hole` = `union(bore@minor, female-relief)` with the
  relief radius inset below the bore (overlap) so no coincident face.
- [ ] 3.3 (test) mating: an external `thread` and a `tapped-hole` of equal
  nominal with complementary clearance engage (bbox/fit check).

## 4. Asymmetric (buttress) profile — op enhancement

- [ ] 4.1 `helical-ridge` accepts an asymmetric profile (independent
  upper/lower flanks, or an axial crest offset).
- [ ] 4.2 `thread :profile 'buttress :load-flank … :return-flank …` maps to it.
- [ ] 4.3 (test) buttress overhang flank ≤ 45° from vertical while the load flank
  stays steep; parity native ↔ build123d.

## 5. Printability verify clauses

- [ ] 5.1 Reusable verify set for a printed thread: single-solid, manifold,
  overhang within budget, `pitch > base`. Author once, reuse.
- [ ] 5.2 (test) a thread with too-shallow flank (overhang > budget) goes red on
  the overhang clause; loosening `:flank` goes green.

## 6. Placement + boolean (reuse, document)

- [ ] 6.1 Confirm `place`/`location` positions a `thread`/`tapped-hole` on an
  arbitrary axis; document the "thread into a wall" pattern.
- [ ] 6.2 Cone/tapered support (pipe/NPT): thread on a conical core.

## 7. Actualize consumers

- [ ] 7.1 Migrate the helicoid (`Film scanning adapter - Ecky helicoid top
  cover`) to the intent primitive: replace the two hardcoded `crest = base*0.58`
  helical-ridges with `:flank`, expose the flank as a model param.
- [ ] 7.2 Point `language-convenience-stdlib` fasteners (3.3) at `thread` +
  `tapped-hole`.

## Notes / gotchas captured this session

- Param retention: `macro_preview_render` keeps the target's current param
  VALUES; a new source's defaults do NOT override them. A coarse-pitch thread
  rendered with a retained fine pitch (1.25) makes `base > pitch` → turns merge →
  2 solids / non-manifold. Pass params explicitly or reset on a new design.
- A valid single solid is not proof of correctness — a hollow spiral passed
  `IsValid` + `single-solid`. Always also check volume / core presence.
- Native render goes runner-first; the runner needs the `:frenet` keyword
  (landed) and the cpp rebuilt into the runtime the app actually resolves.
