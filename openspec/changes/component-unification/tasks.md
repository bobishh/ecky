# Tasks: Component Unification

## Worker Rules

- Use cheap implementation workers for T0-T6.
- Each worker edits only its assigned write scope.
- Workers must not change `CoreProgram` / `ecky_core_ir` public structs.
- Workers must not change stable-node-key derivation for `model`/`part`
  spellings (T0's lock test must stay green).
- Workers must not remove or rewrite existing fixtures.
- Workers follow AGENTS.md: TDD, no commits/staging, no direct DB writes.
- Workers must list changed files and tests added.
- Workers in this environment cannot run `cargo`; tests are written to compile
  on host. Main thread reviews diffs; T8 runs all gates on host.
- Main thread reviews before integration.

## 0. T0 - Compatibility Lock (lands first)

Write scope:

- new tests in `src-tauri/src/ecky_scheme/compiler.rs` test module
- new tests in `src-tauri/src/debug_ast.rs` test module
- fixture list under `model-runtime/examples/` (read-only use)

Tasks:

- [x] 0.1 Add fixture-lock test: parse every `model-runtime/examples/*.ecky`,
  snapshot stable node keys for all parts, assert exact equality against a
  committed snapshot file.
- [x] 0.2 Add emit-spelling lock test: every fixture roundtrips parse->emit
  with `model`/`part`/`feature` spellings preserved byte-for-byte at the
  clause-head level.
- [x] 0.3 Add CoreProgram digest lock test: compile every fixture and snapshot
  a structural digest; assert equality against committed snapshot.

## 1. T1 - Component AST Node and Alias Parsing

Write scope:

- `src-tauri/src/ecky_scheme/compiler.rs`
- targeted tests in same file

Tasks:

- [x] 1.1 Add internal component node representation
  `{ role, spelling, signature, body }` in the compiler's clause model.
- [x] 1.2 Parse `(model ...)` into a `root`-role component (spelling=`model`);
  behavior and outputs identical to today.
- [x] 1.3 Parse `(part ...)` / `(feature ...)` into `output`-role components
  (spelling preserved); CorePart output identical to today.
- [x] 1.4 Emit writes original spelling back for all three forms.
- [x] 1.5 Prove T0 lock tests stay green with the new internal representation.

## 2. T2 - define-component and Instantiation

Write scope:

- `src-tauri/src/ecky_scheme/compiler.rs`
- `src-tauri/src/ecky_scheme/cad.rs` (runtime-path lowering only)
- targeted tests

Tasks:

- [x] 2.1 Parse `(define-component name (sig...) body)` reusing the
  `(params ...)` entry grammar for signature entries.
- [x] 2.2 Reject free variables in component bodies that are not signature
  bindings, with an error naming the variable and the component.
- [x] 2.3 Implement instantiation `(name :key value ...)` with defaults,
  keyword overrides, unknown-keyword error, missing-required error.
- [x] 2.4 Inline-expand instantiations before CorePart construction with
  fresh node ids; record call-site node as source anchor (match `repeat`
  expansion discipline).
- [x] 2.5 Support the same semantics on the Steel runtime path
  (`define-component` lowers to define+lambda with keyword args).
- [x] 2.6 Add parity test: same component source compiles to identical
  CoreProgram via expanded-AST path and runtime path.
- [x] 2.7 Add nesting test: component instantiating another component.
- [x] 2.8 Add recursion guard: self/mutually-recursive instantiation fails
  with a deterministic depth/cycle error, not a hang. (Cycles detected at
  expansion; depth cap 32 enforced for diamond-shaped deep nesting.)

## 3. T3 - Verify Clause Travel

Write scope:

- `src-tauri/src/ecky_scheme/compiler.rs`
- verify evaluation tests where existing verify tests live
- targeted tests

Tasks:

- [x] 3.1 Allow `(verify ...)` clauses inside `define-component` bodies.
- [x] 3.2 Expand verify clauses once per instantiation with tag namespaced by
  instantiating part key (`partkey/tag`).
- [x] 3.3 Prove expanded verify clauses evaluate through existing structural
  verification with correct per-instance tags.
- [x] 3.4 Prove top-level verify behavior is unchanged (existing tests green).

## 4. T4 - Component Extraction

Write scope:

- new `src-tauri/src/component_extract.rs`
- `src-tauri/src/lib.rs` (module registration only)
- targeted tests

Tasks:

- [x] 4.1 Implement subtree lift: resolve part by part key, produce
  `define-component` source for its body.
- [x] 4.2 Free-variable analysis using compiler binding resolution: referenced
  model params become signature entries (metadata preserved); scalar outer
  `let*` bindings become plain defaults; non-scalar free bindings are
  reported as extraction blockers.
- [x] 4.3 Produce header JSON: name, params, tags, provenance
  (threadId/messageId/sourceDigest), referenced named-constraint keys.
- [x] 4.4 Prove extracted source recompiles standalone when wrapped in a
  minimal model and instantiated.
- [x] 4.5 Deterministic errors for unknown part key and blocked extraction.

## 5. T5 - Library Storage and MCP Tools

Write scope:

- `src-tauri/src/component_package_runtime.rs`
- `src-tauri/src/mcp/server.rs` (tool registration + handlers)
- `src-tauri/src/mcp/handlers.rs`
- targeted tests

Tasks:

- [x] 5.1 Store components as `component.ecky` + `ecky-header.json` per
  directory under the component-library dir.
- [x] 5.2 Add MCP `component_extract` tool wrapping T4 and optionally saving
  to the library.
- [x] 5.3 Add MCP `component_search` tool: header-only scan, compact results
  (name, one-liner, param keys, tags); never returns bodies.
- [x] 5.4 Add MCP `component_get` tool: returns full copy-inline source for
  one component by name.
- [x] 5.5 All writes flow through MCP commands (no direct DB/file writes from
  agents); respect existing lease/identity plumbing.

## 6. T6 - Docs and Agent Brief

Write scope:

- `dist/docs/ecky-ir.md` source inputs (book build inputs only)
- agent brief / authoring card text in `src-tauri/src/mcp/`
- docs file additions

Tasks:

- [x] 6.1 Document component authoring (define-component, instantiation,
  defaults/overrides, closedness rule) in the ecky-ir book inputs.
- [x] 6.2 Extend the authoring card: new repeated structures should prefer
  components; pasted components carry their verify clauses.
- [x] 6.3 Document extraction/search/get MCP workflow for agents.

## 7. T7 - Integration Review and Gates (host)

Write scope:

- integration edits only after T0-T6 review
- openspec task checkboxes

Tasks:

- [ ] 7.1 Review worker outputs; merge only non-conflicting patches.
- [ ] 7.2 Run `cd src-tauri && cargo check` (host).
- [ ] 7.3 Run compiler/debug_ast/component tests (host).
- [ ] 7.4 Run G-KEY / G-EMIT / G-CORE / G-RENDER gates (host).
- [ ] 7.5 Update OpenSpec task checkboxes.

## Proof Gates

- [ ] G-KEY Stable node keys byte-identical for all existing fixtures.
- [ ] G-EMIT All fixtures roundtrip with original spellings.
- [ ] G-CORE No `ecky_core_ir` public struct changes.
- [ ] G-RENDER Existing fixture artifact digests unchanged. (Lock digests
  recorded pre-change in T0 snapshots; recompile equality proven in 0.3.)
- [ ] G-COMP A model using a nested, parameterized component renders through
  the native Direct OCCT path with correct per-instance verify tags.
  (Compile-level proof via parity + verify tests; live render smoke on host.)
