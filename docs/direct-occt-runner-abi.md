# Direct OCCT Runner ABI

## Scope

Define a precompiled native runner contract for executing Rust-derived `OcctPlan`
without generating or compiling per-render C++ source.

The contract applies only to
`src-tauri/src/ecky_cad_host/direct_occt.rs`-derived plans and current runtime
artifacts:
`model.step`, `preview.stl`, `topology.json`.

## 1) Plan input (`plan.json`)

### 1.1 Top-level fields

Top-level object fields:

- `schemaVersion` (number, required). Current target: `1`.
- `planId` (string, required). Stable per normalized render input.
- `parts` (array, required). List of part plans.
- `meta` (object, optional). Tooling/runtime metadata.

### 1.2 Version policy

- `schemaVersion` must be a positive integer.
- `1` is the initial native-runner schema.
- Unknown versions fail fast before OpenCascade work.

### 1.3 Canonical shape

```text
{
  "schemaVersion": 1,
  "planId": "sha256:...",
  "parts": [...],
  "meta": {
    "createdBy": "ecky-rust-direct-occt",
    "createdAt": "2026-06-04T00:00:00Z"
  }
}
```

### 1.4 `parts` entries

Each part entry:

- `key` (string, required)
- `label` (string, required)
- `root` (number, required): output slot id
- `commands` (array, required): ordered command stream

### 1.5 `commands` entries

Each command entry:

- `output` (number, required): output slot id
- `op` (string, required): canonical ABI vocabulary from executor emission:  
  `box`, `sphere`, `cylinder`, `cone`, `circle`, `rectangle`, `rounded-rect`,
  `rounded-polygon`, `polygon`, `profile`, `make-face`, `extrude`, `revolve`,
  `loft`, `sweep`, `twist`, `taper`, `offset`, `path`, `bezier-path`,
  `bspline`, `plane`, `location`, `path-frame`, `place`, `clip-box`,
  `linear-array`, `radial-array`, `grid-array`, `arc-array`, `union`,
  `difference`, `intersection`, `fillet`, `chamfer`, `shell`, `translate`,
  `rotate`, `scale`, `mirror`, `compound`
- `args` (array, required): typed arguments
- `keywords` (array, required): name/value pairs

ABI vocabulary does not imply native runner implementation. Current Rust
runner-first dispatch accepts only the proven keyword-free subset listed in
section 5.1. Plans outside that subset must use generated-source fallback until
runner parity tests exist.

### 1.6 argument/tag encoding

- `arg` entries:
  - `kind`: one of `number|boolean|text|symbol|point2|point3|list|ref`
  - `value`: typed payload for that kind
- `ref` value: slot id number
- `point2`: `[x, y]`
- `point3`: `[x, y, z]`
- `list`: array of `arg` values
- `symbol`: symbol string (`start`, `end`, `xy`, `yz`, `xz`, `min`, `center`, `max`)

All args reaching runner ABI are already runtime-resolved. Native runner does
not accept deferred parameter references.

- `keyword` entries:
  - `name` (string, required)
  - `kind`: `arg` or `selector`
  - `value`: same encoded arg when `kind=arg`
  - `payload` when `kind=selector`, with fields below

### 1.7 selector payload

`selector.payload` supported discriminators:

- `{ "type": "targetIds", "kind": "edge|face", "targetIds": [string...] }`
- `{ "type": "clauses", "kind": "face", "clauses": [...] }`

Clause encodings for selectors:

- face clause: `{ "type": "planar" }`
- face clause: `{ "type": "normal", "axis": "x|y|z" }`
- face clause: `{ "type": "area", "rank": "min|max" }`
- face clause: `{ "type": "boundary", "axis": "x|y|z", "bound": "min|max" }`

### 1.8 broad selector filtering

Supported broad selector clauses are discovery filters, not durable edit
handles.

- `targetIds` are preferred on edit replay.
- currently only face `clauses` on `shell` are routed through runner-first.
- edge clauses for `fillet` / `chamfer` remain unsupported in runner-first and
  generated-source exact path.
- supported face `clauses` may be used only before an exact target set exists.
- runner resolves supported face `clauses` against the immediate input shape
  topology for that command.
- zero matches fail with `validation_error` / `selector_no_match`.
- ambiguous matches fail with `validation_error` / `selector_ambiguous` when the
  operation requires one exact target.
- successful supported broad selection writes exact generated `targetId` values
  into `topology.json` for later replay.
- UI/backend must surface raw selector resolution detail, including source slot,
  selector kind, clause payload, match count, and matched ids.

## 2) CLI contract

Required invocation:

```text
direct-occt-runner --plan plan.json --out <bundle-dir>
```

Optional:

- `--version` prints semantic version and exits 0.
- `--help` prints usage and exits 0.
- `--log-level <error|warn|info|debug>` controls runner-internal logs.
- `--timeout-ms N` optional hard timeout guard.
- Rust host may bypass runner-first mode with
  `ECKY_DIRECT_OCCT_RUNNER_DISABLED=1`; generated C++ export stays fallback.

Paths:

- `plan` points to a UTF-8 JSON file.
- `--out` path is created if missing.
- Runner writes outputs inside `--out`.

## 3) Runner outputs

On successful execution:

- creates `model.step`
- creates `preview.stl`
- creates `topology.json`
- exits `0`
- returns stdout + stderr as plain process text (normal logs optional)

`topology.json` shape:

- `parts`: array
- each part:
  - `partId`: string
  - `label`: string
  - `edges`: array of edges
  - `faces`: array of faces
- each edge:
  - `targetId`, `edgeIndex`, `label`, `start`, `end`
- each face:
  - `targetId`, `faceIndex`, `label`, `center`, `normal`, `area`

If failure prevents full artifact generation, outputs may be partial.

## 4) Error classes and exit behavior

Use a fixed class map for orchestration:

- `parse_error` → bad CLI args, unreadable `plan.json`, malformed JSON.
- `schema_error` → unknown `schemaVersion`, missing required field, incompatible op/arg kind.
- `validation_error` → planner incompatibility, unsupported operation for native execution.
- `io_error` → path/permissions/write/read failure.
- `runtime_error` → OpenCascade execution failure, write failures, missing outputs.
- `timeout_error` → exceeded runtime guard.
- `internal_error` → unhandled panic or unexpected bug.

Exit codes:

- `0` success
- `1` argument/usage error (`parse_error`)
- `2` schema mismatch (`schema_error`)
- `3` validation (`validation_error`)
- `4` IO (`io_error`)
- `5` native runtime (`runtime_error`)
- `6` timeout (`timeout_error`)
- `10` internal (`internal_error`)

Error body:

- one canonical JSON line on `stderr` with:
  - `class` (`enum above`)
  - `code` (`machine string`)
  - `message` (`short human text`)
  - `details` (string; raw body from OCCT/OS where available)
- plus raw tool/runtime text on `stderr`.
- Rust shall preserve raw stderr verbatim in surfaced backend detail.

## 5) Compatibility with Rust `OcctPlan`

Current Rust plan model maps as follows:

- `OcctPartPlan.key` → part `key`
- `OcctPartPlan.label` → part `label`
- `OcctSlot` (`u64`) → `root`, `output`, `args` refs, selector sources
- `OcctCommand.op` (`OcctOp`) → `op` string
- resolved `OcctArg` variant → `args` encoding (`number`, `ref`, `list`, etc.)
- `OcctKeyword`/`OcctKeywordValue` → `keywords` (and selector `payload` when present)

Non-goals:

- `OcctOp` support is opt-in; unsupported op must fail with `schema_error`/`validation_error`.
- No plan should carry source code or include dynamic Core IR nodes.

### 5.1 Current runner-first host gate

The host currently invokes `direct-occt-runner` only when every command matches
the proven runner subset below:

- `box`
- `sphere`
- `cylinder`
- `cone`
- `circle`
- `rectangle`
- `rounded-rect`
- `rounded-polygon`
- `polygon`
- `profile` (keyword-free single outer ref, or `:outer` / `:holes` arg keywords)
- `make-face`
- `extrude`
- `revolve`
- `loft`
- `sweep`
- `twist`
- `taper`
- `offset`
- `path`
- `bezier-path`
- `bspline`
- `plane`
- `location`
- `path-frame`
- `place`
- `clip-box` (`:x`, `:y`, `:z` numeric arg keywords)
- `fillet` (all edges keyword-free, or exact `:edges` target ids)
- `chamfer` (all edges keyword-free, or exact `:edges` target ids)
- `shell` (exact `:faces` target ids, or face-clause selectors using
  `boundary` / `planar` / `normal` / `area`)
- `linear-array`
- `radial-array`
- `grid-array`
- `arc-array`
- `union`
- `difference`
- `intersection`
- `translate`
- `rotate`
- `scale`
- `mirror`
- `compound`

Broad selector-clause plans and all other Direct OCCT operations remain on the
generated-source fallback path. This includes clause-driven `fillet` and
`chamfer`, plus any unsupported keyword-bearing form outside the subset above.

## 6) Migration path from generated C++ source

Phase 1: compatibility mode

- runner-first host dispatch is the default when `direct-occt-runner`
  exists and the plan fits the current runner-first host gate.
- existing generated C++ path remains fallback when runner is missing, explicitly
  disabled, or the plan contains runner-unsupported ops/keywords.
- add `plan.json` emission from `OcctPlan` in Rust.
- add runner invocation behind feature/flag; runner path and process output are optional.

Phase 2: shadow mode

- emit `plan.json`, run both C++ and runner for a fixed fixture set.
- compare:
  - file existence (`model.step`, `preview.stl`, `topology.json`)
  - deterministic topological counts
  - checksum deltas within allowed bounds
- block mode switch until parity is stable.
- expand the host gate only after an operation has runner implementation plus
  generated-source artifact parity proof.

Phase 3: runner-only mode

- remove per-render C++ generation only after full Direct OCCT operation parity,
  including keyword and selector plans.
- keep `topology.json` schema and `ArtifactBundle` parsing unchanged.
- keep generated C++ path as emergency fallback until product-wide proof gates pass.

Phase 4: decommission

- generated-source code path is removed only after:
  - proof gates pass,
  - runner success on release hardware set,
  - `cd src-tauri && cargo check` passes,
  - existing e2e/render tests updated to assert runner-backed artifacts.
