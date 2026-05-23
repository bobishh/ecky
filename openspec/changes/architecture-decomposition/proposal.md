# Proposal: Architecture Decomposition

## Intent

Pay down the three structural debts found in the architecture review while the
core IR pipeline (the load-bearing, well-designed part) stays untouched. All
work is mechanical splitting and dependency hygiene — no behavior change, no
Core IR schema change, every existing test stays green.

The review's finding in one line: the language→IR→geometry core is clean and
isolated; the rot is on the edges. Two god-files (`contracts.rs`,
`mcp/handlers.rs`) tax every session through compile time and navigation, and
three live geometry backends tax every new language op.

## Findings (grounded in code)

- `src-tauri/src/contracts.rs`: 8330 LOC, 245 structs/enums, 118 fn/impl,
  imported by 29 modules. The single recompilation hub; logic has leaked in
  beside the types. `models.rs` is a bare `pub use crate::contracts::*` facade.
- `src-tauri/src/mcp/handlers.rs`: 19941 LOC (~11.5k code + ~8.4k tests in one
  file), 70 handlers. Unnavigable; cross-test contamination already bit here.
  `mcp/server.rs`: 7966 LOC, ~145 dispatch arms in one match.
- Three geometry backends dispatched live in `services/render.rs`: build123d
  text-gen (`ecky_ir/build123d_lowering.rs`, 5242), freecad text-gen
  (`ecky_ir/freecad_lowering.rs`, 4946), native OCCT (`ecky_cad_host/**`,
  ~22k). Native is the runner-first winner; the text backends mean some new
  ops get authored up to three times.
- Strong properties to PRESERVE: `ecky_core_ir` imports nothing; `ecky_scheme`
  / `ecky_ir` / `ecky_cad_host` never import `mcp`/`services`/`db`/`commands`
  (clean downward dependency); frontend goes through `tauri/client` (App.svelte
  has zero direct `invoke`).

## Scope

- T1: split `contracts.rs` into a `contracts/` module by domain (render,
  verify, mcp, component, config, core-error), keep the public path stable via
  re-exports so the 29 importers and the generated `contracts.ts` do not churn;
  move type-attached logic that is not a trait impl out into the owning
  service. Regenerate `contracts.ts` and assert byte-stable.
- T2: split `mcp/handlers.rs` into `mcp/handlers/<domain>.rs` (verify,
  component, project_folder, macro_buffer, target, session, render, …) with
  tests moved next to each domain; convert the `server.rs` dispatch match into
  a registry table (`&str -> handler`) so adding a tool is one table entry.
- T3 (decision + first step, not full removal): document the backend
  demotion decision — native OCCT is primary, build123d/freecad become
  import/export-only over time — and gate it so new language ops are authored
  once. First concrete step: a single "backend capability" table that says
  which ops each backend must implement, with a completeness test, so the
  three-way tax becomes visible and bounded instead of implicit.
- Fix the one boundary leak: `DocsSite.svelte` calls `invoke` directly;
  route it through `tauri/client`.
- Extract the New Params macro-AST map (projection, camera, source pane wiring)
  out of `ParamPanel.svelte` into a dedicated `MacroAstMap.svelte`; ParamPanel
  keeps only the tab shell.

## Out of Scope

- Any Core IR schema change (G-CORE lock holds).
- Any change to stable node keys, emit spellings, or render digests
  (G-KEY/G-EMIT/G-RENDER locks hold — re-run from component-unification).
- Removing build123d or freecad lowering outright. T3 only decides direction
  and makes the tax measurable; actual demotion is a follow-up change.
- New features. This change is pure decomposition.

## Success Criteria

- No public Rust path or `contracts.ts` shape changes (re-exports preserve
  them); full `cargo test` and `npm run test:unit` stay green.
- `contracts.rs` and `mcp/handlers.rs` no longer exist as single files; the
  largest resulting file in each split is well under 3k LOC.
- Adding an MCP tool is one registry entry plus one domain handler fn.
- A backend-capability completeness test exists and passes, listing every
  CoreOperation against each backend.
- `ParamPanel.svelte` drops below ~3k LOC with the map extracted.
