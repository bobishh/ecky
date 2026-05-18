# Proposal: Ecky CLI Surface

## Intent

Add first-party CLI entrypoint for Ecky source so users can compile, lower, and
render `.ecky` models without opening desktop UI.

Current repo already has pieces:

- `.ecky -> build123d.py` lowerer
- `.ecky -> FreeCAD macro` lowerer
- backend render paths that accept parameters and emit STL/STEP/runtime bundle

But no stable user-facing CLI ties them together. That blocks docs/tutorial flow,
automation, CI use, and static docs examples that users can run verbatim.

## Scope

- Add `ecky` CLI binary in `src-tauri/src/bin/`.
- Support `check`, `lower`, and `render` commands for `.ecky` source.
- Support parameter overrides from `--param key=value` and JSON file input.
- Support backend selection for `build123d`, `freecad`, and `direct-occt`.
- Emit raw actionable diagnostics and non-zero exit codes on failure.
- Document CLI usage so docs site and tutorials can reference stable commands.

## Out of Scope

- Interactive REPL or watch mode.
- Remote build farm execution.
- Full project/thread/session management through CLI.
- Desktop UI replacement.
- Packaging as global installer in this change.

## Approach

Build thin CLI wrappers over existing Rust lower/render services instead of
duplicating CAD logic.

Commands:

- `ecky check model.ecky`
- `ecky lower --backend build123d model.ecky -o out.py`
- `ecky lower --backend freecad model.ecky -o out.FCMacro`
- `ecky render --backend build123d model.ecky --stl out.stl --step out.step`
- `ecky render --backend freecad model.ecky --params params.json --stl out.stl`
- `ecky render --backend direct-occt model.ecky --param width=42`

## Why now

- Docs/tutorial site needs copy-paste runnable commands.
- CI and local automation need non-UI render path.
- Existing repo already has most primitives; missing layer is orchestration.
