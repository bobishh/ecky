# Tasks: Semantic AST Authoring

BDD dual-loop per AGENTS.md: every slice starts from a failing integration
test (Rust compiler/manifest tests; parity harness for helpers), then unit
red-green. Slices are ordered so each lands independently; nothing below
changes geometry of existing models (digest tests guard this throughout).

## 0. Baseline guards

- [x] 0.1 Digest guard test: full existing fixture corpus compiles and
  exports byte-identical before/after each slice (run per slice, not once).
  DONE as `digest_guard_all_fixtures_compile` in
  `src-tauri/tests/semantic_ast_baseline_guards.rs` — compile-structure
  baseline (no OCCT render), 24 fixtures green.
- [x] 0.2 Emit-back round-trip test over the fixture corpus as a harness
  the new forms plug into. DONE as `emit_back_round_trip_all_fixtures`
  (same file): parse → emit-back → re-parse, span-insensitive semantic
  compare, idempotence fallback. 16 fixtures stable; 8 allowlisted in
  `EMIT_BACK_ALLOWLIST` (Steel parser can't re-parse `#t`/`#f` keyword-arg
  literals in 7, `##` hygienic ids in voronoi_perforated_panel).

## 1. Metadata carrier (foundation for roles/groups)

- [ ] 1.1 Red: parsing a binding with `:label`/`:units` keywords currently
  fails or drops them — integration test asserting metadata reaches the
  manifest on the node id.
- [ ] 1.2 Keyword annotation channel in binding parsing (between name and
  value expr); metadata map on `CoreNode`.
- [ ] 1.3 `annotate` wrapper form for bare expressions.
- [ ] 1.4 Stop dropping top-level `meta` clause (compiler expanded-model
  clause loop); represent as model-level metadata.
- [ ] 1.5 Manifest serialization of node metadata; digest-unchanged test;
  lowering-output-clean test (no metadata in any backend script).
- [ ] 1.6 Emit-back preserves annotations verbatim.

## 2. Construction groups

- [ ] 2.1 Red: grouped part fixture — projection must yield group nodes
  with names, spans, child bindings.
- [ ] 2.2 Parse group form inside part binding sequence; nestable;
  `CoreNodeKind::Group` with name + metadata.
- [ ] 2.3 Sequential scope across groups (test: dimension defined in first
  group referenced in last).
- [ ] 2.4 Geometry-flattening in evaluation/lowerings; digest test flat vs
  grouped model identical.
- [ ] 2.5 Stable ids + patch addressing for bindings inside nested groups
  (`ecky_ast_*` path test).
- [ ] 2.6 Block-tree projection consumes group nodes; comment-heuristic
  titling becomes fallback only.

## 3. Explicit roles

- [ ] 3.1 Red: declared `:role cutter` visible on AST node before boolean
  analysis.
- [ ] 3.2 Closed role set validation; unknown role = compile error with
  span + allowed set.
- [ ] 3.3 Precedence: declared > usage-inferred > name-inferred; node
  records provenance (declared vs inferred).
- [ ] 3.4 Usage-contradiction diagnostics (declared cutter never
  subtracted; declared dimension bound to shape) with spans.
- [ ] 3.5 Projection/manifest expose declared roles; `ast-visual-blocks`
  inference short-circuits per node.

## 4. Inspectable derived dimensions

- [ ] 4.1 Red: manifest query for a derived binding returns formula
  structure + evaluated value + reference edges (node ids).
- [ ] 4.2 Preserve formula AST through compilation (no eager replacement
  by evaluated scalar in the projection path).
- [ ] 4.3 Reference-edge extraction for identifiers in scalar formulas.
- [ ] 4.4 Runtime-only values marked honestly (no fabricated number).

## 5. Semantic cut helpers (one at a time, per-op recipe)

Per helper: red parity/integration test → desugar impl → AST-intent
preservation test → emit-back test → native-vs-build123d parity green.

- [ ] 5.1 `pocket-cavity` (open-face pocket cutter).
- [ ] 5.2 `through-slot` (named face, over-cut both walls; coincident-face
  regression test from the thread/ridge bug class).
- [ ] 5.3 `panel-hole` (rear/any panel circular through-hole).
- [ ] 5.4 `rim-cut` (front-opening rim relief).
- [ ] 5.5 `centered-port-cut` (centered on named face; feeds check 6.x).
- [ ] 5.6 `side-button-cut`.
- [ ] 5.7 Desugar origin tracking: produced nodes record source helper node
  id; patch ops target the helper form, not the expansion.
- [ ] 5.8 All-backends test: model using every helper lowers with zero new
  backend ops.

## 6. Semantic checks

Per check kind: known-good fixture passes + known-bad fixture fails (both
required for done).

- [ ] 6.1 Check form parsing in `verify`; anchor resolution to node ids.
- [ ] 6.2 `cutter-intersects` (intersection volume probe).
- [ ] 6.3 `cutter-reaches` (cutter ∩ cavity region non-empty).
- [ ] 6.4 `min-wall` (distance probe between resulting faces).
- [ ] 6.5 `contains-point`.
- [ ] 6.6 `centered-on-face`.
- [ ] 6.7 Results carry anchor node id in block-view attachment format.
- [ ] 6.8 Verify-pipeline integration: red semantic check blocks commit,
  enters retry feedback, honest at cap (generation-loop test).

## 7. Fixture + guidance

- [ ] 7.1 Re-author phone-case fixture with groups, roles, metadata,
  helpers, checks; digest-identical to flat version; both kept as fixtures.
- [ ] 7.2 Update LLM generation guidance / MCP agent docs to prefer
  semantic forms (mirror of `repeat`/`instance` mandate).
- [ ] 7.3 Full suite green (`cargo check`, `cargo test`, corpus round-trip,
  parity harness).
