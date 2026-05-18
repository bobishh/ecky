# Design: Ecky CLI Surface

## Overview

Create one new Rust binary `src-tauri/src/bin/ecky.rs` that dispatches to
existing library surfaces:

- compile/check: `crate::ecky_scheme::compile_to_core_program`
- lower: existing `ecky_ir::lower_to_build123d` / `ecky_ir::lower_to_freecad`
- render: existing render/service/backend functions

CLI owns:

- argument parsing
- parameter parsing/merging
- output-path validation
- stdout/stderr formatting
- exit codes

CAD backends remain owned by existing runtime modules.

## Command Surface

### `check`

Purpose:

- compile source to typed Core IR
- fail fast with raw compiler diagnostics

Behavior:

- reads one `.ecky` input file
- optional `--json` emits machine-readable summary
- success prints short summary: parts, params, backend-compatible note if known

### `lower`

Purpose:

- emit backend source without rendering

Flags:

- `--backend build123d|freecad`
- `--out <path>` optional, stdout if absent

Behavior:

- reuses existing lowerers
- preserves raw lowering errors

### `render`

Purpose:

- render `.ecky` to artifact files

Flags:

- `--backend build123d|freecad|direct-occt`
- `--stl <path>` optional but required unless `--bundle-dir` used
- `--step <path>` optional
- `--bundle-dir <dir>` optional for runtime artifacts/report copy
- `--param key=value` repeatable
- `--params <json-file>` optional

Behavior:

- parse/merge params
- select backend
- render through existing runtime path
- copy resulting preview STL and optional STEP to requested output paths
- optionally copy manifest/report/artifact bundle outputs into bundle dir

## Parameter Parsing

Accepted sources:

- repeated `--param key=value`
- JSON file via `--params`

Merge order:

1. defaults from model/runtime
2. JSON file
3. repeated `--param`

Value inference:

- `true` / `false` -> boolean
- numeric literal -> number
- everything else -> string

Failure cases:

- malformed `key=value`
- unreadable params file
- invalid JSON root
- unsupported JSON value type

## Backend Routing

### build123d

- route through existing build123d render path
- supports STL always, STEP when available from artifact bundle

### freecad

- route through existing FreeCAD render path
- raw runner errors must pass through untouched

### direct-occt

- route through existing direct-OCCT runtime/export path
- preserve native raw errors

## Output Model

CLI writes only explicitly requested artifacts.

Rules:

- parent dirs auto-created
- existing files overwritten
- if requested artifact missing after render, command fails

Optional `--json` for `render` may print:

- backend used
- preview STL path
- STEP path if produced
- manifest path if produced
- content hash if available

## Exit Codes

- `0` success
- `2` usage/argument error
- `3` source compile/check error
- `4` lowering error
- `5` render/backend/runtime error
- `6` output copy/write error

## Worker Split

### W1 parser + check/lower

Write scope:

- `src-tauri/src/bin/ecky.rs`
- CLI-focused tests if colocated

### W2 render orchestration

Write scope:

- `src-tauri/src/bin/ecky.rs`
- `src-tauri/src/services/render.rs` only if adapter seam needed

### W3 docs/help/tests

Write scope:

- `README.md`
- `openspec/changes/ecky-cli-surface/tasks.md`
- CLI integration tests / smoke docs

Workers must not revert each other. Main thread integrates.
