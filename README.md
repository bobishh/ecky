# Ecky CAD

Ecky is an experimental desktop application for building parametric 3D parts from code, with optional help from an LLM. It brings a source editor, a 3D preview, parameter controls, conversation history, and model versions into one workspace.

Models are stored as `.ecky` source: a small modeling language with Lisp-style syntax. You can write it yourself, ask a model to generate it, or let an external agent edit it through MCP. The native geometry backend uses Open CASCADE Technology (OCCT); supported models can be exported as STEP or STL.

The project is at **0.0.1** and under active development. Expect to build from source, encounter bugs, and see changes to the language and APIs.

[Modeling tutorial](public/tutorials/ecky-campaign.md) · [Language reference](public/docs/ecky-ir.md) · [Website and examples](https://ecky-cad.com/)

## How it started

I bought a 3D printer, and making parts gradually turned into building the software to model them. The first version of Ecky asked an LLM to write FreeCAD Python macros, ran them, and displayed the result. It was a way to try a description, see what came out, and make another attempt.

That experiment kept acquiring tools around it: parameter controls, editable source, screenshots for feedback, version history, and design forks. Eventually, the model description became a project of its own. Ecky gained a Lisp-style language, a compiler, explicit geometry operations, and checks written alongside the model. The rendering path evolved too, through work with FreeCAD and Build123d to direct OCCT execution.

So a printer purchase ended up involving a desktop application, a small programming language, and a CAD runtime. Each of those now brings its own problems to work on. That is where the project stands: an ongoing personal experiment whose scope has grown well beyond its starting point. The everyday loop remains simple — describe or write a part, inspect it, change it, and try again.

## Working with a model

A design lives in a thread containing its conversation and model history. You can:

- Write or edit `.ecky` source and rebuild the geometry.
- Expose dimensions as controls and adjust them in the workbench.
- Ask a configured LLM to create or revise a model. API adapters support Gemini, OpenAI-compatible endpoints, and Ollama.
- Connect an external agent through the local MCP server to inspect source, make edits, render, and run checks.
- Inspect the preview, compare versions, fork a design, and export available geometry.

Source and history are stored locally. Using a remote provider sends it the context needed for the request, which can include model source, conversation, and viewport images. Manual authoring does not require an LLM provider.

For a concrete example, the repository contains a [bicycle bottle holder](sites/landing/src/models/bicycle-bottle-holder.ecky) and a separate [frame mount rail](sites/landing/src/models/bottle-holder-frame-mount-rail.ecky). Their source includes dimensions for the bottle, frame, walls, and dovetail connection. These are modeling examples; the files alone do not establish physical fit.

## The model source

Save this as `model.ecky`:

```scheme
(model
  (params
    (number radius 10 :label "Radius" :min 1 :max 40 :step 1))

  (part body
    (sphere radius)))
```

This defines a sphere and exposes its radius as a parameter. More involved models use profiles, extrusions, boolean operations, placements, repeated geometry, and reusable components. Models can also declare `verify` checks for requirements that the application can measure.

The source compiles to an intermediate representation before geometry is built. `.ecky` has a defined modeling vocabulary; it does not expose the entire OCCT API or run arbitrary Python. See the [language reference](public/docs/ecky-ir.md) for supported forms and the [tutorial](public/tutorials/ecky-campaign.md) for worked models.

## Running from source

The desktop application uses Tauri, Rust, Svelte, and Three.js. Native runtime preparation has macOS and Linux branches; the current runner linker is oriented toward macOS libraries, so Linux setup may require further work.

You need:

- Node.js/npm, Rust/Cargo, and the platform build dependencies for Tauri 2.
- C and C++ compilers, CMake, and Python 3 with `venv` and `pip`.
- An installed OCCT SDK, including headers and libraries. The preparation script discovers Homebrew's `opencascade`, or accepts `ECKY_OCCT_SOURCE_ROOT` pointing to a directory containing `include/opencascade` and `lib`.
- The shell utilities used by the preparation scripts, including `curl`, `rsync`, and `shasum`.

From the repository root:

```bash
npm install
bash scripts/prepare_manifold_runtime.sh
npm run runtimes:prepare
npm run tauri dev
```

The Manifold preparation script downloads and builds the mesh library required by the native runner. `runtimes:prepare` copies the installed OCCT SDK into the local runtime bundle, builds the runner, and prepares the Python speech client. It does not install OCCT itself. See the [OCCT](scripts/prepare_occt_runtime.sh), [Manifold](scripts/prepare_manifold_runtime.sh), and [speech](scripts/prepare_speech_runtime.sh) scripts for paths and overrides.

Open settings to configure a provider if you want AI assistance. FreeCAD is needed only when using its backend; `freecadcmd` must be available on `PATH`.

`npm run dev` starts the Vite frontend and Node server. Use `npm run tauri dev` for the desktop application.

Settings are stored in the platform app-config directory as `config.edn`. The older `config.json` format is accepted for import only.

### Command line

The repository also includes an `ecky` CLI for checking and rendering source. After preparing the runtimes, build it from the repository root:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --features cli --bin ecky
```

Using `model.ecky` from the example above:

```bash
src-tauri/target/debug/ecky check model.ecky
src-tauri/target/debug/ecky render --backend native model.ecky \
  --param radius=12 --stl model.stl --json
```

`check` compiles the source without rendering. `render` builds the requested artifact. The CLI also supports lowering to FreeCAD source and rendering through FreeCAD; its [entry point](src-tauri/src/bin/ecky.rs) contains the command syntax.

## Limitations

Generated source can be wrong, supported operations can fail on particular geometry, and changing a dimension can break a model. A successful render means the backend produced an artifact. A passing `verify` result covers the checks that were declared, not every property of the design.

STEP availability depends on the geometry path; mesh-based results do not necessarily have a STEP export. Examples and previews do not prove that a part fits, carries a load, or prints successfully. Those still need measurements and physical testing.

## Documentation and development

- [Modeling tutorial](public/tutorials/ecky-campaign.md) — worked examples, starting with a bracket.
- [Language reference](public/docs/ecky-ir.md) — syntax and modeling operations.
- [MCP tool reference](skills/ecky-mcp/reference/tools.md) — external-agent integration.
- [VS Code extension](editors/vscode/README.md) and [Emacs mode](editors/emacs/README.md) — editor support.
- [Agent and contributor protocol](AGENTS.md) — ownership boundaries, coding conventions, and verification rules.
- [OpenSpec changes](openspec/changes/) — implementation plans and their status; unfinished proposals are not a list of available features.

Common checks:

```bash
npm run test:unit
npm run test:component
npm run test:e2e
npm run typecheck
cargo test --manifest-path src-tauri/Cargo.toml
```

Geometry tests need their corresponding native runtimes or FreeCAD installation. `npm run test:rust:clean` runs Rust tests in a temporary Cargo target and removes it afterward.

Documentation content lives in files under `public/tutorials/`, `public/docs/`, and `docs/`. `npm run build:book` builds the book and its generated chapter files; `npm run build:docs-site` builds the static documentation routes.
