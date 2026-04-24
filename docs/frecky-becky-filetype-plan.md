# Ecky Source Semantics Plan

**Date:** 2026-04-20  
**Status update:** 2026-04-23

Decision locked:

- canonical authored extension stays `.ecky`
- backend metadata stays authoritative
- `.becky` / `.frecky` stay shelved, not active rollout items

## Canonical public semantics

- One authored language: `ecky`
- One canonical authored source extension: `.ecky`
- Backend metadata is authoritative:
  - `geometryBackend: "build123d"`
  - `geometryBackend: "freecad"`
  - `geometryBackend: "mesh"` stays runtime/internal, not agent-facing by default

`.becky` and `.frecky` are no longer public primary source file types. Treat them as retired experiment names, not active user-facing contract.

## Landed

- public wire naming cleaned toward `ecky`
- workspace guides now teach one language plus backend-specific guides
- authored Ecky guidance now says `.ecky` is canonical and backend metadata decides lowering
- MCP summaries/rules now point agents to `sourceLanguage` + `geometryBackend`, not extension variants

## Remaining cleanup

- scrub old experimental `.becky` / `.frecky` wording from stale docs/screenshots/examples
- keep raw Python thread support internal and explicit:
  - `build123d` Python threads
  - FreeCAD Python/FCMacro threads
- do not reintroduce parallel “Ecky dialect” stories in prompts or UI copy

## Routing rules

For Ecky source:

1. `sourceLanguage = "ecky"` means authored source must be current lispy `.ecky`
2. `geometryBackend` chooses lowering target
3. guides split by concern:
   - `ecky://guides/ecky-source` -> language
   - `ecky://guides/build123d` -> Ecky on build123d
   - `ecky://guides/freecad` -> Ecky on FreeCAD

For raw Python sources:

- they remain supported as separate source languages
- they are not part of Ecky language semantics
- they should not leak into agent guidance for Ecky threads unless the current thread actually uses them

## Non-goals

- no separate Becky/Frecky programming languages
- no agent-facing `eckyRust` vocabulary
- no extension-driven source-language switching for Ecky threads
