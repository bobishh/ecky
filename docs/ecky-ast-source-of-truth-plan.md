# Ecky AST Source Of Truth Plan

## Goal

Move AI-facing authoring from fat source text buffers to bounded structural access over Ecky Lisp/Core IR.

Target spine:

```text
.ecky source
  -> parsed/expanded Core IR
  -> stable node paths + subtree digests
  -> structural AI patches
  -> regenerated canonical source or direct render
  -> backend code/render artifacts
```

Human still sees readable `.ecky`. Backend still receives normal build123d/freecad/direct runtime input. AI should stop receiving full duplicated source unless explicitly needed.

## Current Truth

| Area | Status | Evidence | Gap |
| --- | --- | --- | --- |
| `.ecky` Lisp source | Done | `SourceLanguage::EckyIrV0`, `MacroDialect::EckyIrV0` | Field names still say `macroCode` in many contracts. |
| Typed Core IR | Done | `CoreProgram`, `CorePart`, `CoreNode`, `CoreNodeKind` | Not persisted as edit surface. |
| Backend lowering | Done | `lower_to_build123d`, `lower_to_freecad`, direct runtime render path | Lowering compiles transiently from source each time. |
| Agent source reads | Done for v1 | `target_macro_get` and `macro_buffer_get` return bounded line windows | Tool names still say `macro` in many contracts. |
| Agent edits | Done for text mode | digest-checked line replace/patch tools | Hidden while AST authoring flag is enabled. |
| AST paths/digests | Partial v1 | `ecky_ast_get` returns structural paths and subtree digests | Paths are structural-position based. Many compiled nodes still lack direct editable Lisp source locations. |
| Structural patches | Broader v1 | `ecky_ast_replace_and_render` edits direct source-addressable nodes by path + expected digest and renders | Arbitrary Core-only paths, macro-expanded nodes without direct Lisp source locations, and comment/format-preserving canonical rewrites remain unsupported. |

## Execution Ledger

Status meanings: `Done`, `Doing`, `Next`, `Blocked`, `Parked`.

| Status | Owner | Slice | Acceptance Evidence |
| --- | --- | --- | --- |
| Done | Main | Write this tracking doc | `docs/ecky-ast-source-of-truth-plan.md`. |
| Done | Dalton subagent | Audit MCP payload bloat | Found `macro_buffer_get` full-source duplication and response tests. |
| Done | Dewey subagent | Audit Core AST path/digest surface | Found Core node shape, compiler path, NodeId/span limits. |
| Done | Main | Add `mcp.eckyAstAuthoring` feature toggle | Off exposes text buffer tools. On exposes AST tools and hides buffer tools. |
| Done | Main | Stop `macro_buffer_get` full source duplication | Response omits `macroCode`, returns bounded window + digest + line count. |
| Done | Main | Add `ecky_ast_get` read-only tool | For Ecky source, returns bounded Core tree window with node path/digest/type/span. |
| Done | Main | Add `ecky_ast_replace_and_render` replace path | Replaces direct source-addressable nodes by path + source/node digest guards, then renders draft. |
| Done | Dewey subagent | Add Settings MCP AST toggle | Playwright persists `mcp.eckyAstAuthoring=true` through `save_config`. |
| Done | Euclid subagent | Compact AST edit response | Response has `newSourceDigest`, `editedPath`, `operation`, `lineCount`; no heavy source/runtime payloads. |
| Done | Nietzsche subagent | Audit CST path/rename expansion | File:line implementation map for build/let/params/rename. |
| Done | Main | Correct MCP AST support matrix | Document unsupported structural operations instead of implying CST parity exists. |
| Done | Main | Split preview render from commit | `*_preview_render` stores a session draft and updates viewport; `commit_preview_version` writes one history card. |
| Done | Main + subagents | Expand source-addressable AST edits | `cargo test given_ -- --nocapture`: params replace/insert/rename, part insert/delete/rename, build shape insert/delete/rename, let binding insert/delete/rename, call arg/keyword sibling edits. |
| Next | Main | Rename wording from macro-first to source-first where safe | Tool descriptions say editable source, not full macro. |
| Next | Main | Expose source-only Lisp paths in AST get | Some source-addressable paths can now edit by explicit path, but `ecky_ast_get` still primarily emits Core-visible nodes. |
| Next | Main | Macro-expanded path policy | Keep rejecting paths that cannot resolve to original Lisp source; add clearer error text and tests for expansion-only nodes. |
| Next | Main | Persist source/AST identity | Store canonical source digest plus optional Core tree digest per design version. |

## Phase 1: Feature Toggle And MCP Source Diet

Feature flag:

```json
{
  "mcp": {
    "eckyAstAuthoring": true
  }
}
```

Tool visibility:

- `false`: expose text buffer tools: `macro_buffer_get`, `macro_buffer_replace_range`, `macro_buffer_apply_patch`, `macro_buffer_preview_render`, `macro_buffer_replace_and_preview`
- `true`: expose `ecky_ast_get` and `ecky_ast_replace_and_render`; hide text buffer tools from `tools/list`
- hidden text buffer calls reject while AST authoring is enabled

Problem:

- `target_macro_get` is already windowed.
- `macro_buffer_get` opens edit state but returns only a bounded line window.
- Tool names and guide copy keep teaching agents to think in `macroCode`.

Work:

- Add `startLine` / `endLine` to `macro_buffer_get`.
- Store full source in server-side session buffer only.
- Return:
  - `digest`
  - `lineCount`
  - `windowStartLine`
  - `windowEndLine`
  - `truncated`
  - `lines`
  - `sourceLanguage`
  - `macroDialect`
  - `geometryBackend`
  - `authoringContext`
  - `artifactDigest`
- Omit `macroCode`.
- Keep `macro_buffer_replace_range`, `macro_buffer_apply_patch`, and `macro_buffer_preview_render` as text-mode authoring while the flag is off.
- While AST authoring is enabled, buffer tools are not advertised and should not be used for Ecky agents.

Tests:

- `macro_buffer_get` JSON has no `macroCode`.
- default response returns first 200 lines at most.
- explicit `startLine` / `endLine` returns requested window.
- edit response remains digest-checked.

## Phase 2: AST Read Surface

Tool sketch:

```json
{
  "name": "ecky_ast_get",
  "input": {
    "threadId": "...",
    "messageId": "...",
    "path": "/parts/body/root",
    "depth": 2,
    "maxNodes": 120
  }
}
```

Response sketch:

```json
{
  "sourceDigest": "sha256:...",
  "coreDigest": "sha256:...",
  "path": "/parts/body/root",
  "truncated": true,
  "nodes": [
    {
      "path": "/parts/body/root",
      "digest": "sha256:...",
      "kind": "Call",
      "valueKind": "Solid",
      "op": "box",
      "span": { "start": 42, "end": 58 },
      "children": ["args/0", "args/1", "args/2"]
    }
  ]
}
```

Rules:

- Read-only first.
- Ecky-only first.
- Compile through existing Core compiler.
- Use deterministic path from structural position, not ephemeral `NodeId`.
- Include `NodeId` only as debug metadata until stability proven.
- Include model params as pseudo AST nodes under `/params/{key}`.

## Phase 3: Structural Patch Surface

Tool sketch:

```json
{
  "name": "ecky_ast_replace_and_render",
  "input": {
    "threadId": "...",
    "messageId": "...",
    "sourceDigest": "sha256:...",
    "path": "/parts/body/root/args/0",
    "expectedNodeDigest": "sha256:...",
    "replacementSource": "(box 50 20 10)"
  }
}
```

First version rewrites a direct source-addressable node, then compiles and renders. It uses Core `SourceSpan` only when the span is valid for the original Lisp source. It must reject unsupported paths instead of guessing.

Supported operations on `ecky_ast_replace_and_render`:

- `replace`: replace source-addressable node values or whole declarations by source digest + expected node digest
- `insertBefore` / `insertAfter`: splice source-addressable sibling forms for call args, params, parts, build shape clauses, and let binding pairs
- `delete`: delete source-addressable sibling forms for call args, keyword pairs, params, parts, build shape clauses, and let binding pairs; part root deletion remains rejected
- `rename`: rename build bindings, let bindings, parts, and params with compile validation

Still unsupported:

- arbitrary Core-only paths when the raw Lisp resolver cannot map them back to source
- macro-expanded nodes without direct Lisp source location
- comment/format preservation if future canonical rewrite replaces source slicing
- broad shadowing analysis beyond current let/param rename rules

Later versions can add CST-backed path resolution, scoped rename, sibling splices, and pretty-print from Core/source AST.

Acceptance:

- stale digest rejects
- wrong node digest rejects
- invalid replacement rejects before render
- unsupported macro-expanded/Core-only paths reject before render
- successful patch returns artifact digest, structural verification, and new source digest
- no full source returned by default

Observed failure:

- 2026-05-19: MCP HTTP Codex session against thread `eb267b48-a796-4159-a425-84ceee3b9f73` and base message `fb924167-bb92-4b8b-ba55-fded3bc476be` failed validation with `Unknown symbol groo0_depth`.
- Store evidence: `history.sqlite` message `1f845714-8d8c-4b36-a9f0-d8df7a5a77e0` has `status=error`, `output=NULL`, `artifact_bundle=NULL`, and `model_manifest=NULL`.
- Prior successful source `generated-b123d-e4d6d7e9c121/source.ecky` has `groove_cut_depth` at the lid `let*` binding and references it in groove cutter boxes.
- Interpretation: nested AST replace touched or generated the wrong source token. Treat nested `let*`/binding paths as unreliable until CST-backed spans and scoped replacements exist.

## Risks

- Source spans may be missing after macro expansion.
- `NodeId` may not be stable across equivalent edits.
- Pretty-printing full Core IR back to human `.ecky` may lose comments/format.
- Line edits must stay available as escape hatch until structural patches cover real workflows.

## Current Decision

Do not replace Lisp. Ecky Lisp remains human source. Tree-calculus idea only informs bounded reflective tree protocol: small reads, stable paths, subtree digests, structural patches.
