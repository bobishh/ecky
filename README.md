# Ecky CAD

Prompt-driven CAD: describe a part in words, an LLM writes it in a small modeling language, and it renders as an exact B-rep solid you can read, edit, and version.

> Early and pre-release (v0.0.1). Expect rough edges and breaking changes.

## How it works

The LLM doesn't emit a mesh or a script. It writes `.ecky`, which compiles through three layers:

- **Surface** — parenthesized Scheme: `(model (part ...))`.
- **Core IR** — the fixed set of operations the surface lowers to: primitives, booleans, selectors, placements, repeats. The kernel only sees this set.
- **Backend** — native OCCT by default (B-rep, selectable faces and edges). build123d and FreeCAD are follower backends for cross-check and import.

## What it looks like

A parametric enclosure: named dimensions, a hollow body with a vent bored through it, a filleted top edge, and a `verify` clause that pins the lid clearance to a requirement.

```scheme
(model
  (params
    (number body_w 80 :label "Body width"  :min 40 :max 120 :step 1)
    (number body_d 50 :label "Body depth"  :min 30 :max 90  :step 1)
    (number body_h 20 :label "Body height" :min 10 :max 40  :step 1)
    (number wall    2 :label "Wall"         :min 1 :max 5    :step 0.5)
    (number vent_r  3 :label "Vent radius"  :min 1 :max 6    :step 0.5))

  ; the lid must keep at least 0.3 mm clearance above the body
  (verify
    (tag lid_clearance body.lid_gap)
    (metric gap (clearance min-distance body lid))
    (expect gap (>= 0.3)))

  (part body
    (build
      (shape hollow (shell wall :faces "top" (box body_w body_d body_h)))
      (shape vent   (translate 0 0 -0.5 (cylinder vent_r (+ body_h 1))))
      (result
        (fillet 1.5 :edges "top"
          (difference hollow vent)))))

  (part lid
    (translate 0 0 (+ body_h 0.4)
      (box (- body_w (* wall 2)) (- body_d (* wall 2)) 3))))
```

`params` hoists dimensions to labelled sliders. `build` names each intermediate solid; `difference` bores the vent through the shelled body; `fillet` rounds the top edges. `verify` states an invariant — minimum lid clearance — that `verify_generated_model` checks red-to-green, independent of whatever geometry currently renders.

Smaller is fine too — `(model (part p (sphere 10)))` renders on its own. The [Ecky IR Field Guide](docs/books/ecky-ir/index.md) builds up from there, chapter by chapter.

## Features

- Version history, persisted in SQLite.
- Viewport screenshots fed back to the LLM between iterations.
- Fork a design into a new thread.
- Edit the generated IR by hand and commit it.
- LLMs: Gemini, OpenAI-compatible, Ollama.
- MCP server for external agents (inspect / validate / preview / commit).

## Getting started

### Prerequisites

- **Node.js** and **Rust** — for the Tauri/Svelte app.
- **Python 3.10+** — used by the interop backends and runtime tooling.
- **FreeCAD** (optional) — only needed for the FreeCAD interop backend; `freecadcmd` must be on your `PATH`.

The native OCCT kernel and build123d/speech runtimes are built locally by the prepare scripts below — you don't need a system FreeCAD for the default backend.

### Install and run

```bash
# 1. Clone, then install JS dependencies
npm install

# 2. Build the native CAD runtimes (OCCT kernel, build123d, speech)
npm run runtimes:prepare

# 3. Launch the desktop app in dev mode
npm run tauri dev
```

Then open the in-app settings (⚙️) to pick an LLM provider and add your API key. Start typing what you want to build.

> Want just the web frontend without the desktop shell? `npm run dev` runs the Vite frontend and the Node server side by side.

## Two ways to drive it

**API mode.** Ecky calls an LLM provider directly with your key and generates `.ecky` from your prompt. Pick the provider and model in settings (⚙️). Gemini, any OpenAI-compatible endpoint, and local Ollama are supported.

**Agent mode (MCP).** Point an external coding agent at Ecky's built-in MCP server and it authors models with its own tools, going `inspect → validate → preview → commit` (`workspace_overview` to look around, `macro_preview_render` to preview an `.ecky` source, `commit_preview_version` to persist). Ecky can export a ready-made MCP skill bundle for the agent to load.

Smoke-test the MCP preview→commit path:

```bash
npm run mcp:smoke -- <thread-id> <path-to-model.ecky> [mcp-url]
```

## Development

Full conventions are in [AGENTS.md](AGENTS.md). The short version:

- **Test-first (BDD dual-loop).** A change starts from a failing integration test, driven inward through unit red-green-refactor. Run the relevant suite after each step.
- **Conventional Commits.** `type(scope): description`; release-please reads them to compute versions and the changelog.
- **Tauri boundary.** Frontend payloads are `camelCase`, Rust structs `snake_case`; the contract layer translates. Regenerate the TS bindings with `npm run generate:contracts`.

```bash
npm run test:unit      # Svelte/TS unit tests
npm run test:e2e       # Playwright end-to-end tests
npm run typecheck      # svelte-check + tsc
cd src-tauri && cargo test   # Rust backend tests
```
