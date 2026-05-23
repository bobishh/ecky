# Design: Architecture Decomposition

## Principle

Mechanical, behavior-preserving. Each task is a refactor whose proof is "the
same tests, still green, plus byte-stable generated artifacts." Nothing here
touches the IR pipeline's internals; it only reorganizes the edges that wrap it.

## T1: contracts.rs -> contracts/ module

Current: one 8330-LOC file, 245 types + 118 fn/impl, 29 importers,
`models.rs` re-exports it wholesale, `bindings.rs`/`export_contracts` generate
`contracts.ts` from its `#[derive(Type)]` types.

Target layout (path-stable):

```text
contracts/
  mod.rs        // pub use of every submodule -> `crate::contracts::Foo` unchanged
  error.rs      // AppError, AppErrorCode, AppResult  (the one true error type)
  render.rs     // ArtifactBundle, ModelManifest, render/preview structs
  verify.rs     // StructuralVerificationResult, AuthoredVerifyCheck, ...
  component.rs  // component package / port / feature-graph types
  mcp.rs        // McpConfig, session/target refs, tool request/response structs
  config.rs     // Config, Engine, VoiceConfig, ...
  geometry.rs   // GeometryBackend, MacroDialect, SourceLanguage, core enums
```

Rules:
- `mod.rs` re-exports everything so no importer or `contracts.ts` path moves.
- Pure data stays; `impl` blocks that are trait impls (Serialize/Type/Display)
  stay with their type; free `pub fn` business logic moves to the owning
  service module (e.g. a `fn that builds a manifest` belongs in `services`).
- `models.rs` stays as the historical facade (`pub use crate::contracts::*`)
  until a later cleanup; not load-bearing here.

Proof: `npm run generate:contracts` produces a byte-identical `contracts.ts`
(diff exits 0); `cargo build` and full test suite green.

## T2: mcp/handlers.rs -> mcp/handlers/<domain>.rs + dispatch registry

Current: 19941 LOC, 70 `handle_*` fns, ~8.4k of in-file tests; `server.rs` has
one ~145-arm `match params.name.as_str()`.

Target:

```text
mcp/handlers/
  mod.rs            // pub use; shared AgentContext, helpers
  verify.rs         // verify_generated_model, structural summary
  component.rs      // component_extract/search/get
  project_folder.rs // export/status/apply + watcher
  macro_buffer.rs   // buffer get/replace/preview
  target.rs         // target_macro/detail/meta get
  session.rs        // login/logout/resume/borrow
  render.rs         // macro_preview_render, commit_preview_version
  thread.rs         // thread create/list/fork/messages
  ...               // one file per cohesive tool group
```

Dispatch becomes a table, not a match:

```text
type ToolHandler = fn(&HttpServerState, &str, CallToolParams) -> BoxFuture<...>;
static TOOLS: &[(&str, ToolHandler)] = &[ ("verify_generated_model", verify::...), ... ];
```

Adding a tool = one table row + one domain fn. Tests move next to their
handler (`handlers/verify.rs` `#[cfg(test)] mod tests`), which also kills the
single-file test-contamination surface.

Proof: identical tool list (`tool_definitions` test), full mcp suite green,
no behavior diff.

## T3: backend capability table (decision + visibility)

Today the three backends are implicit peers in `services/render.rs`. Make the
contract explicit:

```text
struct BackendCapability { backend, ops: Set<CoreOperation>, role }
```

- Native OCCT (`EckyRust`): role = primary, must cover the full op set.
- build123d / freecad: role = export/interop, declared op subset.
- A completeness test enumerates every `CoreOperation` and asserts native
  covers it (or is explicitly marked unsupported), and that the text backends'
  declared subsets match reality.

This does not remove anything. It turns "some ops need three implementations"
from a silent tax into a visible, tested contract, and records the direction
(native primary) so a later change can demote the text backends safely.

## Boundary + UI hygiene (small)

- `DocsSite.svelte`: replace the direct `invoke` with a `tauri/client` wrapper
  (one function), matching every other component.
- `ParamPanel.svelte`: extract the New Params map — `macroAstMap` projection
  wiring, camera state/handlers, minimap, source-pane mount — into
  `MacroAstMap.svelte`, props in / events out. ParamPanel keeps the tab shell
  and the legacy params/views/litho/raw modes. Existing e2e selectors
  (`.macro-ast-map-shell`, `.macro-ast-node-*`, `macro-source-pane`) preserved.

## Compatibility gates

- G-LOCKS: component-unification key/emit/core/render locks re-run green.
- G-CONTRACTS: `contracts.ts` byte-identical after T1 (CI `check:contracts`).
- G-TOOLS: MCP `tool_definitions` list unchanged after T2.
- G-GREEN: full `cargo test` + `npm run test:unit` + the touched e2e green.

## Risks

- Re-export drift hiding a moved type: caught by `check:contracts` + cargo.
- Async fn-pointer registry ergonomics in Rust (BoxFuture): if it fights the
  borrow checker, fall back to a macro that expands match arms but keeps the
  per-domain handler split — the split is the win, the table is the polish.
- ParamPanel extraction breaking camera/focus effects: the e2e map specs
  (19 passing) are the safety net; extract behind them.
