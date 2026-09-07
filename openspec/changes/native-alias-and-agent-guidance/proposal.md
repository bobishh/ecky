# Native shape aliases and agent guidance

## Problem

The iPhone case session compiles successfully but native normalization folds a
shape alias into text. The native type checker then rejects a `union` operand.
Agent repairs obscure this defect with unrelated source edits. MCP language
resources also prepend the API-only instruction that the agent has no tools.

## Scope

- Preserve geometry references during scalar folding in the native normalizer.
- Keep compiler and native type validation strict; prove both valid aliases and
  invalid shape operands at their existing boundaries.
- Keep one shared language body/catalogue with separate API and MCP operating
  instructions. MCP guidance distinguishes constraint checks from render proof.

No new language forms, geometry rewrites, or lifecycle changes.
