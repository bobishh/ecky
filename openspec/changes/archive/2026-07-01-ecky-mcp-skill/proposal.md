# Proposal: Repo-Owned, Generated Ecky MCP Skill

## Intent

Make the Ecky MCP skill a first-class, repo-owned, partly-generated artifact
instead of a hand-maintained file living in the user's Codex home. An external
agent loading the skill should see the current MCP tool catalog, the Ecky
authoring discipline, and a pointer to the language reference — with no drift
from the running server.

## Findings (grounded in code)

- `export_ecky_mcp_skill_zip` (`src-tauri/src/commands/config.rs`) does not build
  a skill — it zips one that already exists under `CODEX_HOME/skills/ecky-mcp` or
  `~/.codex/skills/ecky-mcp` and requires a `SKILL.md`. The skill content is
  hand-maintained outside the repo.
- The authoritative tool catalog already exists in code:
  `tool_definitions_with_ast_enabled(bool) -> Vec<Value>`
  (`src-tauri/src/mcp/server.rs`) is what the server returns for `tools/list`.
  It is pure (takes a bool), so it can be rendered without a running server.
- The authoring discipline is also already in code as `workflow_guide_text(&AppState)`
  (the bootstrap prompt). The stable parts of it — `inspect → validate → preview
  → commit`, prefer AST patches over rewrites, verify red-to-green — are the
  agent-facing rules that do **not** belong in README or human-contributor docs.
- The repo already has the right patterns to copy: `export_contracts` (a Rust bin
  that writes a generated artifact to disk) and `check:contracts` (generate +
  `git diff --exit-code` as a CI drift gate).

## Scope

- Add a repo-owned skill at `skills/ecky-mcp/`:
  - `SKILL.md` — hand-authored, stable: how to connect, the authoring loop
    (`inspect → validate → preview → commit`), AST-patch preference, verify
    red-to-green, and a pointer to the Ecky IR Field Guide for the language.
  - `reference/tools.md` — **generated** from the live tool catalog.
- Add a Rust bin `export_mcp_skill` that renders `reference/tools.md` from
  `tool_definitions_with_ast_enabled(true)` (mirrors `export_contracts`).
- Add npm scripts `generate:skill` and `check:skill` (drift gate, mirrors
  `check:contracts`).
- Rewire `resolve_ecky_mcp_skill_dir` to prefer the repo `skills/ecky-mcp`
  directory, so `export_ecky_mcp_skill_zip` zips the repo skill.
- Put the markdown rendering in a testable pure function (`mcp::skill`).

## Out of Scope

- Auto-generating the discipline prose from `workflow_guide_text` (needs runtime
  `AppState`); the stable discipline stays hand-authored in `SKILL.md`.
- Embedding the full Field Guide into the skill; link to it instead.
- Changing any MCP tool behavior, names, or schemas.

## Success Criteria

- `npm run generate:skill` writes `skills/ecky-mcp/reference/tools.md` from the
  catalog; `npm run check:skill` fails if the committed file drifts.
- `reference/tools.md` lists every tool from `tool_definitions_with_ast_enabled`
  with name, description, and argument names.
- `export_ecky_mcp_skill_zip` resolves and zips the repo `skills/ecky-mcp`.
- `mcp::skill::render_tools_markdown` is unit-tested; `cargo test` and
  `npm run test:unit` stay green.
