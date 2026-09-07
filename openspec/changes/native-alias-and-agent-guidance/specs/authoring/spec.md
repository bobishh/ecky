## ADDED Requirements

### Requirement: Native normalization preserves shape aliases
Native scalar folding MUST preserve references to geometry bindings, including
chained aliases. It MUST continue folding known scalar values and MUST NOT
convert an unresolved shape reference into a text literal.

#### Scenario: Camera cutter alias
- GIVEN a compiled part binding a hull to `camera-opening`
- WHEN `camera-cutters` aliases it and is passed to `union`
- THEN native planning and rendering succeed with the same geometry as direct use.

#### Scenario: Actual text used as geometry
- GIVEN a text literal passed to `union`
- WHEN the source is compiled
- THEN type validation rejects it before rendering.

### Requirement: Operating instructions match the caller
MCP language resources MUST share language content and operation catalogues with
API prompts, but MUST NOT state that MCP callers have no tools. API prompts MUST
remain self-contained. MCP instructions MUST distinguish source/constraint
validation from a completed render and verification of that exact version.

#### Scenario: Tool-equipped caller reads a language guide
- GIVEN an MCP client reads a generic or backend-specific language resource
- WHEN the resource is assembled
- THEN it includes tool-aware authoring instructions and the shared language body.
