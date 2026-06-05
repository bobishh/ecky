# Ecky CAD

**Describe a part in words. Get an exact, editable, manufacturable solid — not a triangle blob.**

Ecky is a desktop CAD tool where you talk to an LLM and it builds real geometry. Instead of emitting a one-shot mesh or an opaque script, the model produces **Ecky IR** — a small, readable, verifiable language that compiles to an exact B-rep solid on a native OCCT kernel. You can read what the AI wrote, edit it by hand, fork it, re-render it, and trust that the same source always builds the same part.

## Why Ecky is different

Most "AI CAD" tools hand the model a Python API or a mesh generator and hope for the best. The output is hard to read, impossible to diff, and breaks the moment you tweak it. Ecky puts a **finite intermediate language** between the LLM and the kernel:

- **Readable surface.** An `.ecky` file is parenthesized Scheme — `(model (part ...))`. Friendly to read, friendly to edit, friendly to diff in git.
- **Finite Core IR.** The surface lowers to a small, fixed vocabulary of operations: primitives, booleans, selectors, placements, repeats. The kernel never sees arbitrary code — only this closed set. That's what makes a model reproducible, verifiable, and portable.
- **Exact B-rep output.** The default backend is a native OCCT kernel: real faces and edges with stable identities you can select and tag — not a triangle soup. **build123d** and **FreeCAD** are supported as follower backends for cross-checking and import.

The payoff: the AI's output is something a human can actually own.

## What it looks like

The smallest model that renders — a ball on a base:

```scheme
(model
  (part marker
    (union
      (box 28 28 4)
      (translate 0 0 10
        (sphere 10)))))
```

`model` is the root. `part` gives the geometry a stable id. `box` is the base, `translate` lifts the `sphere` so it sits on top, and `union` fuses them into one solid. Everything else in the language is this same tree with more branches — parameters, sketches, fillets, shells, selectors, repeats, and reusable components.

New to the language? Read the [**Ecky IR Field Guide**](docs/books/ecky-ir/index.md) — it builds up real models chapter by chapter.

## Features

- **Exact modeling, not meshes.** Native OCCT B-rep kernel with selectable faces/edges; build123d and FreeCAD as interop backends.
- **Readable, diffable source.** Scheme-surface `.ecky` files that lower to a finite Core IR.
- **Multi-version history.** Every render is versioned and persisted in SQLite.
- **Visual feedback loop.** Viewport screenshots are fed back to the LLM so it can see what it built and correct course.
- **Design forking.** Branch a new thread off any existing design to mutate geometry without losing the original.
- **Manual commits.** Edit the generated IR directly and commit it as a new version.
- **Pluggable LLMs.** Works with Gemini, OpenAI-compatible providers, and local Ollama models.
- **Agent-native (MCP).** A built-in MCP server lets external agents inspect, validate, preview, and commit models through a typed protocol.

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

## For agents: the MCP authoring loop

Ecky exposes a local MCP server so AI agents can drive the modeler safely. **Never write `history.sqlite` directly** — all state changes flow through MCP. Follow `inspect → validate → preview → commit`:

1. `thread_borrow` (or `thread_create` for new work).
2. `macro_preview_render` with your `.ecky` source.
3. Verify the `artifactDigest` in the preview response.
4. `commit_preview_version` to persist the draft.
5. Record the returned `threadId`, `messageId`, `modelId`, and artifact digest in your change notes.

Smoke-test the full preview→commit path:

```bash
npm run mcp:smoke -- <thread-id> <path-to-model.ecky> [mcp-url]
```

## Development

Ecky follows a BDD dual-loop workflow — see [AGENTS.md](AGENTS.md) for the full protocol. Key commands:

```bash
npm run test:unit      # Svelte/TS unit tests
npm run test:e2e       # Playwright end-to-end tests
npm run typecheck      # svelte-check + tsc
cd src-tauri && cargo test   # Rust backend tests
```

The Rust backend owns the Tauri boundary: frontend payloads are `camelCase`, backend structs are `snake_case`, and the contract layer translates between them.
