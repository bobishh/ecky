# Delta for mcp-skill

## ADDED Requirements

### Requirement: Repo-owned skill with a generated tool catalog

The system SHALL provide an Ecky MCP skill under `skills/ecky-mcp/` whose tool
catalog is generated from the server's own tool definitions, so it cannot drift
from the running server.

#### Scenario: Catalog regenerates from the live definitions

- GIVEN the MCP server's `tool_definitions_with_ast_enabled(true)` catalog
- WHEN `npm run generate:skill` runs
- THEN `skills/ecky-mcp/reference/tools.md` is written
- AND it lists every tool with its name, description, and argument names.

#### Scenario: Drift is caught

- GIVEN a committed `skills/ecky-mcp/reference/tools.md`
- WHEN the tool catalog changes but the file is not regenerated
- THEN `npm run check:skill` exits non-zero.

### Requirement: Skill carries the agent authoring discipline

The skill's `SKILL.md` SHALL state the MCP authoring loop and the agent-facing
rules that do not belong in the README or human-contributor docs.

#### Scenario: SKILL.md states the loop

- GIVEN `skills/ecky-mcp/SKILL.md`
- WHEN it is read
- THEN it documents `inspect → validate → preview → commit`
- AND it states the AST-patch-over-rewrite preference and verify red-to-green
- AND it points to the Ecky IR Field Guide for the language.

### Requirement: Export zips the repo-owned skill

`export_ecky_mcp_skill_zip` SHALL resolve the repo `skills/ecky-mcp` directory
when present.

#### Scenario: Export uses the repo skill

- GIVEN the repo contains `skills/ecky-mcp/SKILL.md`
- WHEN `export_ecky_mcp_skill_zip` runs without `ECKY_MCP_SKILL_DIR` set
- THEN it zips the repo `skills/ecky-mcp` directory
- AND the existing `ECKY_MCP_SKILL_DIR` override and Codex-home fallbacks still apply.
