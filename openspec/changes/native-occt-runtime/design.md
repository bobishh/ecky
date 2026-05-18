# Design: Native OCCT Runtime

## Architecture

Target pipeline:

```text
Ecky source
  -> Core IR
  -> Core normalization
  -> OcctPlan
  -> native OCCT runner
  -> STEP + STL + topology.json
  -> runtime bundle + manifest
```

Rust owns:

- parsing and typechecking.
- Core normalization.
- `OcctPlan` construction.
- SDK capability reporting.
- runner process orchestration.
- bundle/manifest/cache/error ownership.
- durable/public topology IDs.

Native OCCT owns:

- BREP shape creation.
- booleans/transforms/fillets/chamfers/shells.
- STEP/STL writes.
- observed topology enumeration.

## Runtime SDK

Current state ties runtime discovery to build123d/OCP:

- runtime root is resolved through `BUILD123D_RUNTIME_DIR`,
  `runtime/build123d`, or `.dist/build123d-runtime`.
- OCP package and `.dylibs` are searched under Python site-packages.
- macOS install-name logic uses `/DLC/OCP/.dylibs`.

Target SDK contract:

```text
runtime/occt/
  manifest.json
  include/opencascade/*.hxx
  lib/<platform OCCT libraries>
  licenses/
```

`manifest.json` should define:

- `schemaVersion`
- `platform`
- `arch`
- `occtVersion`
- `abiTag`
- `includeDir`
- `libDir`
- `requiredHeaders`
- `requiredLibraries`
- `libraryHashes`

Probe order:

1. `ECKY_OCCT_ROOT`
2. bundled `runtime/occt`
3. existing runtime discovery during burn-in

Platform matrix:

- macOS: `.dylib`, rpath/install-name handling.
- Linux: `.so`, rpath handling.
- Windows: `.dll` runtime path and import libs where needed.

## Runner ABI

Phase 1 may keep generated C++ source while standalone SDK probing lands.

Phase 2 introduces a precompiled runner:

```text
direct-occt-runner --plan plan.json --out <bundle-dir>
```

Input:

- `plan.json` matching resolved runner ABI shape.
- output paths.

Output:

- `model.step`
- `preview.stl`
- `topology.json`
- process exit code
- stdout/stderr raw diagnostics

The runner must not parse Ecky source.

## Core Normalization

Add a Rust module before planning:

```text
CoreProgram + DesignParams -> NormalizedCoreProgram
```

Normalization rules:

- resolve scalar `if`.
- expand finite `range`.
- expand finite `map`.
- expand `apply` into explicit calls.
- expand `repeat`, `repeat-union`, `repeat-compound`, `repeat-pick`.
- preserve existing `sampled-radial-loft` expansion behavior.
- reject `xor`, unfilled `hole`, unresolved custom ops, and unsupported imports
  with direct, operation-specific errors.

No dynamic `if/range/map/apply` may reach native planning.

## SVG Profile

SVG is vector profile geometry, not pixel height extrusion.

Target:

```text
(extrude
  (svg-profile "logo.svg" :width 24 :height 12 :fit contain)
  1.2)
```

Meaning:

- parse SVG in Rust.
- resolve viewBox, transforms, units.
- extract visible closed vector contours.
- classify one outer loop and zero or more hole loops.
- fit into requested model-space dimensions.
- convert to direct OCCT profile geometry.
- extrude along Z by requested height.

Use `usvg` for parsing/simplification unless implementation proof shows a smaller
parser is enough.

Rejected first-slice cases:

- raster-only SVG.
- open paths without stroke conversion.
- ambiguous multi-outer-loop SVG.
- self-intersecting or near-zero-area loops.
- unresolved text nodes.

## Topology

Native runner returns observed topology facts only:

- face index, center, normal, area.
- edge index, start/end points.
- part key/label.

Rust manifest layer derives:

- canonical IDs.
- stable IDs.
- durable IDs.
- aliases.

Selectors:

- exact target ID selectors remain supported.
- broad selectors require deterministic filtering proof before becoming editable
  user workflows.

## Integration

Default render path changes only after proof gates pass.

During burn-in:

- current render paths remain available.
- direct OCCT status must show blocked/no-step/ready accurately.
- raw native error detail must reach UI.
- no dependency removal tasks are allowed.
