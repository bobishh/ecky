# Tasks: Repo-Owned, Generated Ecky MCP Skill

## T1 — Testable renderer
- [x] Add `src-tauri/src/mcp/skill.rs` with `render_tools_markdown(tools: &[Value]) -> String`.
- [x] Unit tests written: renders name + description + arg names, marks required, skips nameless.

## T2 — Catalog access + generator bin
- [x] Expose `pub fn export_mcp_tool_catalog() -> Vec<Value>` in `mcp/server.rs` (wraps `tool_definitions_with_ast_enabled(true)`).
- [x] Rust test written: catalog non-empty and contains `health_check`, `workspace_overview`, `macro_preview_render`, `commit_preview_version`.
- [x] Add bin `src-tauri/src/bin/export_mcp_skill.rs` (mirrors `export_contracts`): render + write `skills/ecky-mcp/reference/tools.md`.

## T3 — Hand-authored SKILL.md
- [x] Write `skills/ecky-mcp/SKILL.md`: connect, `inspect → validate → preview → commit`, AST-patch preference, verify red-to-green, pointer to the Ecky IR Field Guide.

## T4 — Scripts + drift gate
- [x] `generate:skill` = `cargo run --bin export_mcp_skill`.
- [x] `check:skill` = `npm run generate:skill && git diff --exit-code -- skills/ecky-mcp/reference`.

## T5 — Rewire export
- [x] `resolve_ecky_mcp_skill_dir` prefers repo `skills/ecky-mcp` (before CODEX_HOME fallbacks).
- [x] Keep `ECKY_MCP_SKILL_DIR` override and existing fallbacks.

## T6 — Proof
- [x] `npm run generate:skill` → wrote `reference/tools.md` (74 tools).
- [x] `cargo test` green: `mcp::skill::tests` (5) + `export_mcp_tool_catalog_lists_core_tools`.
- [x] `check:skill` runs clean (regenerates + diffs `skills/ecky-mcp/reference`).
- Note: the earlier compile blocker (17 errors in the in-flight
  `authoring-error-surface` files) was resolved outside this change; the tree
  now builds. This change never touched those files.
- `tools.md` is untracked until committed — the drift gate becomes meaningful
  once it is checked in.
