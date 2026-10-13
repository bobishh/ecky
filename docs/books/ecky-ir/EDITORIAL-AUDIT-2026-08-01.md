# Ecky learning corpus — editorial audit

Historical review. The current illustrated book, source ownership and verified
deployment instructions are described in [PUBLISHING.md](PUBLISHING.md).
Do not use the old chapter layout below as the current build graph.

Date: 2026-08-01

Scope:

- six interactive campaign missions under `docs/books/ecky-ir/missions/`
- six projected level chapters under `docs/books/ecky-ir/levels/`
- matching canonical lessons in `ecky-ir-corpus.md`
- 19 language-reference articles projected to `public/docs/ecky-ir.md`

Method: reader-value review plus claim checks against language surface, compiler,
runtime tests, and referenced `.ecky` sources. No browser, build, render, or deploy.

## Executive verdict

- Campaign count: 6.
- Mission verdict: all 6 need rewrite. Concepts mostly worth keeping.
- Blocking factual error: Level 06 claims a normal translated part is preview-only
  and excluded from export. It is exported in the translated position.
- Structural problem: mission completion checks tokens, not mission behavior.
- Reference verdict: 12 articles need rewrite; 5 should move/delete; 1 internal-IR
  article should delete; `Constraint Dojo` should delete.
- Reference contains wrong conversion semantics, wrong helper signatures, unsupported
  planned syntax, and an incomplete operation index.
- Existing `REVISION-PLAN.md` completion marks do not prove current content quality.

## Campaign-wide findings

### P0

1. Level 06 teaches false export behavior.

   - `levels/06-film-adapter.md:5,156-180`
   - `ecky-ir-corpus.md:1196-1220`
   - `examples/ecky-integrated-film-adapter-open-helicoid-v9.ecky:490-492`

   Carrier offset uses normal `translate` inside a `part` result. Export geometry
   moves. Preview-only placement requires top-level `(view ... (offset-part ...))`.
   Compiler contract: `src-tauri/crates/ecky-render/src/scheme/compiler.rs:2574,2722-2750`.
   Preview-view compiler proof:
   `src-tauri/crates/ecky-render/src/scheme/compiler.rs` test
   `compiles_preview_view_offsets_via_runtime_path`.

### P1

1. Every mission gate is gameable.

   `src/lib/docs/eckyMissionEvaluator.ts:97-110` checks source tokens/forms. It does
   not prove compilation, geometry, named dependency, connectivity, boolean result,
   multipart structure, or export behavior. `nativeProofNote` admits this; visible
   completion and solution prose still overclaim.

2. Canonical ownership is split.

   Mission JSON is canonical for interactive workbench. Corpus is canonical for
   campaign Markdown. Level Markdown duplicates corpus sections. Same lesson has
   multiple editable representations and drift risk.

3. `supportsOpenInCode` is dead content.

   `missions/level-01-corner-bracket.json:133-136` and
   `missions/level-02-mounting-plate.json:131-134` declare it. Button visibility
   depends on callback presence in `src/lib/EckyMissionWorkbench.svelte:395-398`.

## Mission 01 — Corner Bracket

Verdict: **rewrite**. Keep first union/overlap lesson.

### P1

- `missions/level-01-corner-bracket.json:63-103`: arbitrary disconnected `union`
  plus two `box` forms can pass. Gate does not prove nesting, overlap, valid part,
  or compilation.
- `missions/level-01-corner-bracket.json:5-9,112-119`,
  `levels/01-corner-bracket.md:3-10,23-55`, `ecky-ir-corpus.md:7-10,23-55`,
  `examples/corner-bracket.ecky:4-7`: geometry is a coplanar plan-view L plate,
  not a horizontal foot plus vertical flange. Both boxes use default `x/y=center,
  z=min`; no rotation makes a flange upright. Defaults:
  `src-tauri/src/ecky_cad_host/direct_occt_executor.rs:2234-2252`.

### P2

- Origin prose is imprecise. Boxes center only in X/Y and start at Z=0.
- Worked cylinder starts at Z=0; it does not fully pass through stock.
- Transfer asks for named reinforcement before naming/staging syntax is taught.
- Clear condition claims compile/preview while interactive gate proves neither.

## Mission 02 — Mounting Plate

Verdict: **rewrite**. Keep boolean-cut progression.

### P1

- `missions/level-02-mounting-plate.json:63-101`: gate does not prove two placed,
  overshooting cutters or `difference blank hole_left hole_right`.
- `missions/level-02-mounting-plate.json:10-31`: worked example says
  `(+ sleeve_h 1)` crosses stock completely. Cutter and stock both start at Z=0;
  bottom faces remain coincident. Correct pattern needs negative Z overshoot.
  Defaults: `src-tauri/src/ecky_cad_host/direct_occt_executor.rs:2265-2276`.

### P2

- Mission says repeated holes but authors two individually named cutters. Rename
  lesson or use actual repetition.
- Clear condition claims exported one-component STL; gate cannot establish it.

## Mission 03 — Dovetail Fit

Verdict: **rewrite**. Keep named-clearance concept.

### P1

- `missions/level-03-dovetail-fit.json:64-102`: unrelated
  `(* 2 fit_clearance)` can pass. Gate does not prove channel dimensions depend on
  clearance.
- `levels/03-dovetail-fit.md:12-16`, `ecky-ir-corpus.md:640-644`: prose says defect
  requires editing two offsets. Starter repeats one unnamed value twice. Actual
  failure is duplicated unnamed clearance.
- `missions/level-03-dovetail-fit.json:107`: hint says clearance drives both sides.
  Intended relation keeps male rail nominal and drives both female-channel dimensions.

### P2

- “Real/proven/tested film-adapter profile” overclaims a simplified derivative.
- “Recessed pocket” points at a separate rounded-rectangle fit, not dovetail channels.

## Mission 04 — Procedural Workshop

Verdict: **rewrite**. Keep procedural-list concept; narrow scope or expand exercise.

### P1

- `missions/level-04-procedural-workshop.json:62-108`: disconnected `map`, `range`,
  `apply union`, and `difference` tokens pass.
- `missions/level-04-procedural-workshop.json:117`: solution exposes `cell-count`
  but hard-codes four columns and stock dimensions. Counts above 16 run outside stock.
- `levels/04-procedural-workshop.md:3-7`, `ecky-ir-corpus.md:803-838`: framing promises
  path frames, arrays, valid solids, and export. Exercise teaches mapped cylinders
  only and never compiles/exports.

### P2

- Transfer asks for `grid-array` comparison without teaching its signature.
- Claimed `repeat-union` debugging trade-off is unsupported opinion.

## Mission 05 — Perforated Toothbrush Holder

Verdict: **rewrite**. Use real Stage 4 project source or rename to flat-wall drill.

### P1

- `missions/level-05-toothbrush-holder.json:5,7-9,56-124`,
  `levels/05-toothbrush-holder.md:1-16,106-125`,
  `ecky-ir-corpus.md:905-916,1006-1025`: mission promises shelled holder and
  checkpoint continuation. Practice uses isolated `box 60 8 40` with five holes.
  No shell, drain, curved wall, named margins, or checkpoint continuity.
- `missions/level-05-toothbrush-holder.json:61-98,114`: unrelated `repeat-union`,
  `difference`, and `extrude` forms pass. Gate does not prove cutter body, group,
  spacing, or final subtraction.

### P2

- “One cutter group” conflicts with transfer and full model using three groups.
- “All four checkpoints compile” lacks recorded proof.
- Planner/parallel-OCCT prose is implementation speculation, not reader value.

## Mission 06 — Film Adapter

Verdict: **rewrite; blocked by P0**. Current mission regresses from Level 03.

### P1

- `missions/level-06-film-adapter.json:5,7-9,58-116`: objective promises multipart
  production fit plus preview/export separation. Practice edits two literals inside
  one compound. No separate parts, top-level `view`, or mechanism handoff.
- `missions/level-06-film-adapter.json:63-99,114`: disconnected clearance expression
  can pass; no mating relation is established.
- `examples/ecky-integrated-film-adapter-open-helicoid-v9.ecky:26-27,145-170,
  215-245`: declared join controls do not create claimed two-piece interface.
  `join_profile`, channels, and rails use 0.01 dummy geometry. “Production-scale
  multipart mechanism” hides unfinished joins.

### P2

- Worked example detours to shaft/sleeve after dovetail already taught.
- “Physical, not arbitrary” conflicts with unnamed fit-critical literals.
- “Verification records fit requirements” is false: example has no `verify` clause.

## Language reference

### P1

1. Operation Index incomplete.

   `public/docs/ecky-ir.md:5` omits 17 shipped operations including `thread`,
   `tapped-hole`, `rib`, `groove`, `torus`, `ellipse`, `regular-polygon`,
   `trapezoid`, `wedge`, four slot forms, `mesh`, `polyhedron`, `heightfield`,
   and `hull`. Registry: `src-tauri/src/ecky_language_surface.rs:76,153`.
   Contract test proves subset validity, not completeness:
   `src-tauri/tests/agent_language_source_contract.rs:67`.

2. Unsupported planned syntax appears as accepted reference.

   `public/docs/ecky-ir.md:194,212` presents reserved `assembly` and `export`
   sketches. Accepted model clauses are `params`, `part`, and `meta`:
   `src-tauri/src/ecky_language_surface.rs:4`.

3. Angle conversion semantics are inverted.

   `public/docs/ecky-ir.md:676` says `deg` converts degrees to radians and `rad`
   converts radians to degrees. Registry says `deg` converts radians to degrees;
   `rad` converts degrees to radians:
   `src-tauri/src/ecky_language_surface.rs:746,757`.

4. Helper signatures drift registry.

   `public/docs/ecky-ir.md:622,628,765,770,792,797` misdocuments `enumerate`,
   `flat-map`, `concat-map`, `wave-loop`, `superellipse-point`,
   `logistic-bifurcation-points`, `henon-points`, `lorenz-points`, and
   `rossler-points`. Registry: `src-tauri/src/ecky_language_surface.rs:22,57,919`.

5. `wall-pattern` is incomplete and non-normative.

   `public/docs/ecky-ir.md:1373` lists four “observed” modes. Registry defines 14
   modes and required `:seed`: `src-tauri/src/ecky_language_surface.rs:155,160,1109`.

6. Tutorials and placeholder occupy reference navigation.

   `public/docs/ecky-ir.md:1549,1574,1592`: move three tutorials to campaign.
   `public/docs/ecky-ir.md:1618`: delete `Constraint Dojo`; migrate named-fit facts
   into Params/Verify. UI test created preservation pressure:
   `e2e/docs-site.spec.ts:33`.

### P2

- Delete tutorial-style “read in this order” from Language Overview.
- Split package/MCP workflow from Components language grammar.
- Delete or reduce Value Kinds and IR Nodes; compiler internals lack lookup value.
- Move Cookbook to campaign.
- Rewrite selector grammar as normative syntax plus explicit backend matrix.
- Sidebar parser treats every `##` as peer article:
  `src/lib/docs/eckyIrGuide.ts:183`. Add manifest/categories or restrict article headings.

## Reference disposition

| Article | Action |
| --- | --- |
| Operation Index | Rewrite |
| Language Overview | Rewrite |
| Forms and Structure | Rewrite; remove planned syntax |
| Components | Rewrite; split package internals |
| Verify Clauses | Rewrite; exact grammar/metrics |
| Params and Controls | Rewrite; absorb named-fit material |
| Core Helper Library | Rewrite from registry |
| Value Kinds and IR Nodes | Delete |
| Primitive Signatures | Rewrite; add shipped primitives |
| Boolean and Transform Signatures | Rewrite from registry |
| Surface and Path Signatures | Rewrite from registry |
| Array and Frame Signatures | Rewrite from registry |
| Special / Custom Operations | Rewrite exact contracts |
| Selector Strings and Named Keywords | Rewrite |
| Cookbook | Move to campaign |
| Tutorial: Loop to Profile | Move to campaign |
| Tutorial: Path to Solid | Move to campaign |
| Tutorial: Repeat Logic | Move to campaign |
| Constraint Dojo | Delete |

## Repair order

1. Remove false Level 06 export claim. Stop publication until corrected.
2. Correct wrong reference semantics/signatures and remove unsupported syntax.
3. Delete `Constraint Dojo`; move tutorials/Cookbook out of reference.
4. Replace token-only mission pass language with honest “source-shape check,” or
   implement semantic/runtime mission validation before claiming completion.
5. Rewrite missions 01–06 against one objective each.
6. Generate operation/signature reference from registry with completeness test.
7. Choose one canonical lesson source; generate corpus/levels/projections from it.
8. Run one final cross-chapter editor pass for progression, terminology, and tone.
