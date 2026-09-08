# Design

The bounded agent-reference section in `docs/books/ecky-ir/ecky-ir-corpus.md`
owns prose. `sync:book-source` projects it to `public/docs/ecky-agent-reference.md`
before prompt generation. The generated
surface registry in `src-tauri/src/ecky_language_surface.rs` owns operation
signatures, examples, and backend notes. `agent_prompt.rs` adds mode-specific
operating rules, while `mcp/authoring.rs` carries the concise shared MCP card.

The catalogue uses quoted text for `clip-plane :keep`, all three ranges for
`clip-box`, and concrete control points for `bezier-path`. Focused integration
tests compile those examples and pass them through the native planner. The test
also records that the current native planner accepts a known bare `positive`
literal; guidance avoids claiming that syntax always fails and instead explains
that an unresolved local `positive` can fail name resolution. Guidance records
native Bézier's fixed 16 samples per cubic as approximation.

Evidence language distinguishes structural topology from cross-section, shape
intent, and support-free printing. `geometryBackend=mesh` is described as a
legacy native-hybrid label; artifact truth distinguishes `analyticBrep` from a
faceted mesh. Parameter preservation and exact current-version evidence remain
explicit requirements.

Committed generated prompt artifacts are derived from the canonical prose and
surface registry by the existing generator; this change does not hand-edit
those outputs.
