# Proposal: Component Unification

## Intent

Unify `model` / `part` / reusable library pieces into one recursive authored
entity: `component`. A component is a named, parameterized, closed s-expression
with a knob signature, a geometry body composed of other component
instantiations, optional named interface constraints, and optional `verify`
clauses that travel with it.

`model` and `part` remain valid surface spellings forever. They parse to
component nodes with fixed roles (`root` and `output` respectively), so every
existing `.ecky` model keeps working with zero source edits, zero stable-node-key
churn, and zero emit diff noise.

Core IR does not change. Components expand at compile time into the existing
`CoreProgram { parameters, parts, constraints }` shape, the same way the
normalizer already expands `repeat`/`map`/`apply`. The planner, Direct OCCT,
topology, runtime bundles, and MCP artifact contracts never learn the word
"component".

## Scope

- Add `define-component` and component instantiation with keyword overrides to
  the authored surface (both compile paths: expanded AST and Steel runtime).
- Parse `model` as the root component and `part` as an anonymous zero-arg
  component instance with the `output` role. Aliases only; no behavior change.
- Lexical parameter scoping for components: declared signature with defaults
  and optional `:min`/`:max`/`:step`/`:label` metadata; keyword overrides at
  the call site; deterministic errors for unknown or missing arguments.
- Compile-time inline expansion of component instantiations into today's
  CoreProgram, with per-instance provenance (call-site node retained as the
  source anchor for everything the expansion produced).
- Stable node key byte-compatibility for existing `model`/`part` spellings,
  proven against existing fixtures.
- Emit/roundtrip spelling preservation: source written with `model`/`part`
  re-emits as `model`/`part`; source written with `define-component` re-emits
  as `define-component`.
- `verify` clauses authored inside a component definition expand with each
  instantiation (tag-namespaced per instance key).
- Component extraction: MCP tool that lifts an existing part subtree into a
  component definition via free-variable analysis (referenced params/bindings
  become the signature), returning copy-inline `.ecky` source plus a header
  (name, params, tags, provenance threadId/messageId/sourceDigest).
- Component library: store extracted components as plain `.ecky` snippets plus
  header JSON in the existing component-library directory; MCP search tool
  returning compact headers only.

## Out of Scope

- Reference-instance reuse (`(use "lib/name" ...)` resolving against a stored
  library at render time), versioning, pinning, dependency resolution.
  First slice is copy-inline only.
- Hierarchical instance-path selectors (addressing "fillet on edge of
  instance 2" through a component boundary). Expanded nodes get distinct ids
  exactly as `repeat` expansion does today; richer addressing is follow-up.
- Any UI work (params view, parts gallery, library browser).
- User-authored `define-syntax` policy changes.
- Core IR schema changes.
- Deprecating or rewriting any existing model source.

## Approach

One umbrella OpenSpec change with independent worker tasks:

- T1: component AST node + alias parsing (model/part -> component roles).
- T2: `define-component` + instantiation with lexical params, both compile
  paths.
- T3: stable-node-key and emit-spelling compatibility proof.
- T4: verify clause travel through expansion.
- T5: component extraction (free-variable analysis) + header contract.
- T6: component library storage + MCP search/extract tools.
- T7: docs/book/agent-brief coverage.
- T8: integration review and host-run gates.

## Expected Outcome

Authors (human or LLM) can lift any part into a named, parameterized component,
search the library by compact header, paste a component into any model as plain
source, override its knobs at the call site, and get its verify checks for
free — while every existing model, thread, selector, and AST patch target keeps
working unchanged.
