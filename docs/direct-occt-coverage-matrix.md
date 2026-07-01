# Direct OCCT Coverage Matrix

Status terms:

- `direct`: planned and executed by Direct OCCT.
- `runner-supported`: accepted by current precompiled runner gate.
- `runner-fallback`: Direct OCCT generated-source path remains required until
  runner parity expands.
- `normalized-direct`: rewritten by Rust normalizer into direct operations.
- `mesh-only`: intentionally handled by Rust mesh path, not BREP.
- `explicit-exact`: available only when caller explicitly selects build123d or
  FreeCAD exact backend.
- `unsupported`: deterministic Direct OCCT rejection.
- `gap`: missing proof or unclear behavior.

## Core Operation Coverage

| Group | Operation | Status | Evidence | Notes |
| --- | --- | --- | --- | --- |
| primitive | `box` | direct | planner + live box export tests | BREP solid |
| primitive | `sphere` | direct | solid ops live test | BREP solid |
| primitive | `cylinder` | direct | solid ops live test | BREP solid |
| primitive | `cone` | direct | cone planner/live coverage | BREP solid |
| primitive | `circle` | direct | extrude/revolve/sweep tests | sketch profile |
| primitive | `rectangle` | direct | extruded sketch tests | sketch profile |
| primitive | `rounded-rect` | direct | rounded rectangle planner test | sketch profile |
| primitive | `rounded-polygon` | direct | rounded polygon planner test | sketch profile |
| primitive | `polygon` | direct | profile/SVG/path tests | sketch profile |
| primitive | `profile` | direct | profile holes live test | outer loop plus holes |
| primitive | `make-face` | direct | make-face planner test | face creation |
| primitive | `svg` | normalized-direct | SVG profile planner + live export tests | vector paths only |
| primitive | `text` | mesh-only | normalizer rejection test | no BREP text path yet |
| primitive | `stl` | mesh-only | normalizer rejection test | mesh import, not BREP |
| boolean | `union` | direct | solid ops/live boolean tests | BREP boolean |
| boolean | `difference` | direct | solid ops/live boolean tests | BREP boolean |
| boolean | `intersection` | direct | solid ops/live boolean tests | BREP boolean |
| boolean | `xor` | unsupported | planner/normalizer rejection tests | explicit unsupported |
| transform | `translate` | direct | transform live test | BREP transform |
| transform | `rotate` | direct | transform live test | BREP transform |
| transform | `scale` | direct | transform live test | BREP transform |
| transform | `mirror` | direct | mirror live/planner tests | axis-limited |
| surface | `extrude` | direct | extrude/profile/SVG tests | BREP surface/solid |
| surface | `revolve` | direct | revolve live test | BREP solid |
| surface | `loft` | direct | loft live test | BREP solid |
| surface | `sweep` | direct | sweep/bezier sweep live tests | BREP solid |
| surface | `shell` | direct | shell live tests | BREP shell |
| surface | `offset` | direct | offset sketch live test | sketch/shape offset |
| surface | `offset-rounded` | direct | mirror/taper/offset-rounded live test | emitted as offset |
| surface | `fillet` | direct | fillet/chamfer live test | target-id selectors supported |
| surface | `chamfer` | direct | fillet/chamfer live test | target-id selectors supported |
| surface | `taper` | direct | taper live test | BREP transform-like op |
| surface | `twist` | direct | twist live test | BREP generated op |
| surface | `draft` | direct | draft planner test; runner live + build123d differential tests | side-wall face draft, runner-supported |
| path | `polyline` | direct | path frame/sweep tests | emitted as path |
| path | `bezier-path` | direct | bezier sweep live test | cubic-control validation |
| path | `bspline` | direct | bspline profile live test | closed/open profile usage |
| array | `linear-array` | direct | array ops live test | BREP copies |
| array | `radial-array` | direct | array ops live test | BREP copies |
| array | `grid-array` | direct | array ops live test | BREP copies |
| array | `arc-array` | direct | array ops live test | BREP copies |
| array | `repeat` | normalized-direct | normalizer tests | finite expansion |
| array | `repeat-union` | normalized-direct | normalizer tests | expands to union |
| array | `repeat-compound` | normalized-direct | normalizer tests | expands to group/compound |
| array | `repeat-pick` | normalized-direct | normalizer tests | finite selection |
| frame | `plane` | direct | plane/location/clip-box live test | frame primitive |
| frame | `location` | direct | plane/location/clip-box live test | frame placement |
| frame | `path-frame` | direct | path-frame/place live test | path placement |
| frame | `place` | direct | path-frame/place live test | placement op |
| frame | `clip-box` | direct | plane/location/clip-box live test | clipped BREP |
| meta | `group` | direct | multi-part/compound tests | emitted as compound |
| meta | `comment` | unsupported | normalizer/planner unsupported branch | rejected by operation name |
| meta | `annotate` | unsupported | normalizer/planner unsupported branch | rejected by operation name |
| custom | `sampled-radial-loft` | normalized-direct | sampled-radial-loft live + differential parity tests | portable: native, build123d, FreeCAD (not mesh) |
| custom | `hull` | direct | hull capsule live tests (runner + shim tiers) | native-only convex hull; build123d/FreeCAD reject |
| custom | `helical-ridge` | normalized-direct | native helical-ridge render tests; build123d/freecad lowering tests | planner-expanded into helix sweep + boolean forms |
| custom | `hole` | unsupported | typed-hole rejection test | must be filled before planning |
| custom | `wall-pattern` | mesh-only | mesh path tests | Rust mesh-only operation |
| custom | `pattern` | mesh-only | source classifier | legacy mesh alias |
| custom | scalar eval ops | normalized-direct | normalizer scalar tests | only when fully evaluable |
| custom | other custom ops | unsupported | normalizer rejection test | deterministic diagnostic |

## Open Gaps

- FreeCAD/build123d exact-only operations still outside Direct OCCT path:
  `text`, `import-stl`, `xor`.
- Typed `hole` placeholders still must be filled before Direct OCCT planning.
- Precompiled runner is proven for the covered subset below, but generated C++
  fallback remains required for Direct OCCT forms that are not yet admitted by
  the runner gate.

## Current Runner Subset

Runner-first dispatch is enabled only when each command matches the current
proven runner subset:

| Operation | Runner status | Notes |
| --- | --- | --- |
| `box` | runner-supported | solid primitive |
| `sphere` | runner-supported | solid primitive |
| `cylinder` | runner-supported | solid primitive |
| `cone` | runner-supported | solid primitive |
| `circle` | runner-supported | sketch primitive |
| `rectangle` | runner-supported | sketch primitive |
| `rounded-rect` | runner-supported | sketch primitive |
| `rounded-polygon` | runner-supported | sketch primitive |
| `polygon` | runner-supported | sketch primitive |
| `profile` | runner-supported | positional outer profile or `:outer` / `:holes` arg keywords |
| `make-face` | runner-supported | face creation |
| `extrude` | runner-supported | keyword-free profile/face extrude |
| `revolve` | runner-supported | keyword-free profile revolve |
| `loft` | runner-supported | keyword-free profile loft |
| `sweep` | runner-supported | keyword-free profile/path sweep |
| `twist` | runner-supported | keyword-free profile twist |
| `taper` | runner-supported | keyword-free profile taper |
| `offset` | runner-supported | keyword-free sketch offset |
| `path` | runner-supported | polyline path |
| `bezier-path` | runner-supported | cubic Bezier path |
| `bspline` | runner-supported | sketch profile |
| `plane` | runner-supported | keyword-free frame primitive |
| `location` | runner-supported | keyword-free frame placement |
| `path-frame` | runner-supported | keyword-free path placement |
| `place` | runner-supported | keyword-free frame placement |
| `clip-box` | runner-supported | `:x`, `:y`, `:z` numeric arg keywords |
| `fillet` | runner-supported | all edges keyword-free, `:edges "all"`, exact `:edges` target ids, and coarse edge clauses |
| `chamfer` | runner-supported | all edges keyword-free, `:edges "all"`, exact `:edges` target ids, and coarse edge clauses |
| `shell` | runner-supported | keywordless default shell, exact `:faces` target ids, and face clauses using `boundary` / `planar` / `normal` / `area` |
| `linear-array` | runner-supported | transform array |
| `radial-array` | runner-supported | transform array |
| `grid-array` | runner-supported | transform array |
| `arc-array` | runner-supported | transform array |
| `union` | runner-supported | BREP boolean |
| `difference` | runner-supported | BREP boolean |
| `intersection` | runner-supported | BREP boolean |
| `translate` | runner-supported | transform |
| `rotate` | runner-supported | transform |
| `scale` | runner-supported | transform |
| `mirror` | runner-supported | transform |
| `compound` | runner-supported | grouping output |
| `draft` | runner-supported | keyword-free or `:neutral-z`/`:neutral_z` numeric keyword; native `draft_shape` added 2026-07-06, no longer generated-source-only |
| `hull` | runner-supported | variadic shape refs; incremental 3-D convex hull added 2026-07-09 |

Every other Direct OCCT op is currently runner-fallback, not unsupported by
Direct OCCT itself. Generated C++ fallback remains the active execution path for
forms that still miss runner admission/proof.

## Parity Against Exact Lowerings

This section answers a narrower question than “can Direct OCCT plan it?”:
whether the current `EckyRust -> runner-first` path already covers forms that
the build123d / FreeCAD exact lowerings can render.

| Form in exact lowerings | build123d | FreeCAD | Direct OCCT planner | Runner-first status | Notes |
| --- | --- | --- | --- | --- | --- |
| primitives `box/sphere/cylinder/cone/circle/rectangle/polygon` | yes | yes | yes | covered | direct BREP |
| `rounded-rect`, `rounded-polygon` | yes | yes | yes | covered | direct sketch/BREP |
| `profile` with `:outer` / `:holes` | yes | yes | yes | covered | runner keyword support |
| `svg` profile extrusion | yes | yes | yes | covered | normalized to profile loops, then runner/direct |
| `union/difference/intersection` | yes | yes | yes | covered | direct booleans |
| `extrude/revolve/loft/sweep/taper/twist/offset` | yes | yes | yes | covered | direct BREP forms |
| `offset-rounded` | yes | yes | yes | covered | normalized/emitted as direct offset path |
| `sampled-radial-loft` | yes | yes | yes | covered | planner-expanded into loft sections; differential parity proven vs build123d |
| `hull` | no | no | yes | covered (native-only) | direct-OCCT-required op; exact lowerings reject with a diagnostic |
| arrays `linear/radial/grid/arc` | yes | yes | yes | covered | direct transform arrays |
| `repeat`, `repeat-union`, `repeat-pick`, `repeat-compound` | yes | yes | yes | covered | normalized into finite direct forms before runner/direct execution |
| frames `plane/location/path-frame/place/clip-box` | yes | yes | yes | covered | runner-first proven |
| `fillet` / `chamfer` all-edges | yes | yes | yes | covered | keyword-free and `:edges "all"` |
| `fillet` / `chamfer` exact edge target ids | yes | yes | yes | covered | runner-first parity proven |
| `fillet` / `chamfer` coarse edge clauses | yes | yes | yes | covered | runner-first parity proven |
| `shell` exact face target ids | yes | yes | yes | covered | runner-first parity proven |
| `shell` face clauses (`top`, `planar+normal-z+area-max`, etc.) | yes | yes | yes | covered | runner-first parity proven |
| keywordless `shell` | yes | yes | yes | covered | runner-first parity proven |
| `text` | limited/no | yes | no | not covered | FreeCAD exact-only today |
| `import-stl` | yes | yes | no | not covered | mesh/import path, not Direct OCCT BREP |
| `xor` | yes | yes | no | not covered | Direct OCCT rejects `xor` |
| `helical-ridge` | yes | yes | yes | covered | planner-expanded helix sweep; native render test proven |
| typed `hole` placeholders | rejected until filled | rejected until filled | rejected until filled | not applicable | authoring placeholder, not runtime op |

### Practical reading

- `build123d` and `FreeCAD` still cover a bigger surface than current
  runner-first only because of exact-only extras like `text`, `import-stl`,
  and `xor`. Conversely `hull` is native-only: Direct OCCT renders it and the
  exact lowerings reject it.
- For the shared BREP subset, `EckyRust -> Direct OCCT -> precompiled runner`
  now covers the common primitives, booleans, transforms, arrays, frames,
  profile/SVG workflows, and the supported selector-driven `fillet` /
  `chamfer` / `shell` flows.
- Generated C++ fallback still exists inside the Direct OCCT path. Removing
  build123d/FreeCAD dependencies only makes sense after the remaining exact-only
  surface is either implemented or explicitly out of scope.

## Removal Blockers From This Matrix

- Build123d fallback is not part of the EckyRust direct path. Remaining
  exact-only surface for full dependency removal: `text`, `import-stl`, `xor`.
- Generated C++ compile cannot be removed until `plan.json` runner covers the
  complete Direct OCCT op set, including keyword/selector-driven exact ops.
- UI/MCP cannot claim full Direct OCCT support until broad selector filtering
  and exact op closure are deterministic.
