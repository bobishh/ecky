# Tasks: Architecture Decomposition

## Worker Rules

- Behavior-preserving only. No Core IR schema change; no stable-node-key,
  emit-spelling, or render-digest change (component-unification locks).
- Public Rust paths stay stable via re-exports; `contracts.ts` stays
  byte-identical (`npm run check:contracts`).
- `cd src-tauri && cargo check` before any success claim; no commits/staging.
- One task = one mechanical move; run the relevant suite after each.

## 1. T1 - Split contracts.rs (S/M)

- [x] 1.1 Create `contracts/` module; move `AppError`/`AppErrorCode`/`AppResult`
  into `contracts/error.rs`; `mod.rs` re-exports so `crate::contracts::AppError`
  resolves unchanged.
- [x] 1.2 Move render/manifest types -> `contracts/render.rs`; verify types ->
  `contracts/verify.rs`; component types -> `contracts/component.rs`; mcp
  request/response -> `contracts/mcp.rs`; config -> `contracts/config.rs`;
  core enums (GeometryBackend/MacroDialect/SourceLanguage) -> `contracts/geometry.rs`.
- [ ] 1.3 Relocate free `pub fn` business logic off the types into the owning
  service module; keep trait impls (Serialize/Type/Display) with their type.
- [ ] 1.4 G-CONTRACTS: `npm run generate:contracts` -> `contracts.ts` diff
  exits 0; `cargo build` green; full lib suite green.

## 2. T2 - Split mcp/handlers.rs + dispatch registry (M)

- [ ] 2.1 Create `mcp/handlers/` with `mod.rs` (shared AgentContext + helpers);
  move handlers into per-domain files (verify, component, project_folder,
  macro_buffer, target, session, render, thread, ...).
- [ ] 2.2 Move each handler's `#[cfg(test)]` tests next to it (kills the
  single-file test-contamination surface).
- [ ] 2.3 Replace the `server.rs` name-match with a `&str -> handler` registry
  table; adding a tool = one row + one fn. Fall back to a match-expanding
  macro if async fn-pointers fight the borrow checker (keep the file split).
- [ ] 2.4 G-TOOLS: `tool_definitions` list test unchanged; full mcp suite green.

## 3. T3 - Backend capability table (M)

- [x] 3.1 Define `BackendCapability { backend, ops, role }`; native OCCT =
  primary (full op set), build123d/freecad = export/interop (declared subset).
- [x] 3.2 Completeness test: enumerate every `CoreOperation`; assert native
  covers each (or explicit unsupported marker) and text-backend subsets match
  reality.
- [x] 3.3 Document the demotion direction (native primary) in the change so a
  later removal change has a recorded decision. No removal here.

## 4. Hygiene (S)

- [x] 4.1 `DocsSite.svelte`: route its direct `invoke` through a `tauri/client`
  wrapper.
- [x] 4.2 Extract `MacroAstMap.svelte` from `ParamPanel.svelte` (projection
  wiring, camera, minimap, source-pane mount); ParamPanel keeps the tab shell.
  Preserve e2e selectors; map specs (19) stay green. ParamPanel < ~3k LOC.

## Proof Gates

- [ ] G-LOCKS component-unification key/emit/core/render locks green.
- [ ] G-CONTRACTS `contracts.ts` byte-identical after T1.
- [ ] G-TOOLS MCP tool_definitions unchanged after T2.
- [ ] G-GREEN full `cargo test` + `npm run test:unit` + touched e2e green.
- [ ] G-SIZE no single resulting file > 3k LOC for the split modules.

## Suggested order

T1 first (biggest compile-time + navigation win, lowest risk: pure type moves
behind re-exports) -> 4.1 (trivial) -> T2 -> 4.2 (behind e2e net) -> T3.
