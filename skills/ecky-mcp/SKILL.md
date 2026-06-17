---
name: ecky-mcp
description: Author and edit Ecky CAD models over the Ecky MCP server. Use when connected to an Ecky workspace to inspect, preview, verify, and commit .ecky models.
---

# Ecky MCP authoring

You are driving an Ecky CAD workspace through its MCP server. Ecky models are
written in **Ecky IR** — a small Scheme surface (`(model (part ...))`) that lowers
to a finite Core IR and renders on an exact OCCT B-rep kernel. You author the
surface; the kernel only ever sees the lowered Core IR.

The complete tool list, with arguments, is in [`reference/tools.md`](reference/tools.md)
— it is generated from the live server, so trust it over memory.

## The authoring loop: inspect → validate → preview → commit

Always move in this order. Do not write history directly; every state change
flows through MCP.

1. **Inspect.** Call `workspace_overview` first — it resolves the default
   editable target, lists recent threads, and reports any conflicting lease.
   Borrow a thread (`thread_borrow`) or create one (`thread_create`) before
   editing.
2. **Validate.** Check the source compiles and the Core IR is well-formed before
   rendering.
3. **Preview.** Render the draft with `macro_preview_render` (your `.ecky`
   source). Confirm the returned `artifactDigest` before going further.
4. **Commit.** Persist the previewed draft with `commit_preview_version`. Record
   the returned `threadId`, `messageId`, and `modelId`.

## Rules

- **Prefer AST patches over full rewrites.** When an `ecky_ast_*` patch can
  express the change, use it instead of replacing the whole macro. Smaller diffs
  preserve stable node ids and selector bindings.
- **Verify red-to-green.** Treat each authored `verify` clause as an outer test:
  write it from the requirement, expect the first `verify_generated_model` run to
  go red, then fix the geometry or parameters and re-render until it goes green.
  Never weaken or delete a clause to force a pass.
- **Commit only green.** Call `verify_generated_model` on the preview/render
  draft before commit. If verification is capped red, do not commit — report it
  honestly.
- **Never promise STEP unless artifact truth proves it.** Read the
  `artifactBundle` (`hasStepExport`, `stepExportPath`) rather than assuming.
- **No junk threads.** Do not create throwaway `TMP` threads for debugging; fork
  or inspect an existing target, and clean up any noise you create.

## The language

For Ecky IR syntax and patterns — primitives, booleans, parameters, selectors,
fillets/shells, repetition, components, and verification — read the **Ecky IR
Field Guide** (`docs/books/ecky-ir/`), which builds up real models chapter by
chapter. Also available over MCP as the `ecky://guides/ecky-source` resource.
