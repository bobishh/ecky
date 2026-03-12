# Ecky CAD Control Framework (Authoritative)

Version: 0.2

Purpose:
- Declare all editable inputs in a single control list.
- Bind parameters to a typed config object.
- Build geometry from config only.

Rules:
- Do not define custom control classes. Use the provided SDK only.
- Every editable input must be declared in CONTROLS.
- Geometry must read cfg values only (not raw params).
- Raw params access is allowed only when binding config via `registry.bind(params)`.
- Optional `BuildContext(..., params=params, ...)` construction is allowed, but geometry must still read `ctx.config` / `cfg`, not `ctx.params`.
- Keep control keys stable across iterations unless the user explicitly renames them.

SDK:
- from cad_sdk import number, select, toggle, ControlRegistry, BuildContext
- ControlRegistry provides defaults(), bind(params), and to_ui_spec().

Required macro structure:
1. Define CONTROLS list.
2. Create registry = ControlRegistry(CONTROLS).
3. Build cfg from registry.bind(params).
4. Use cfg for all geometry.
5. Do not read params.get(...), params[...], or params directly anywhere else.

Minimal example:

from cad_sdk import number, toggle, ControlRegistry

CONTROLS = [
    number("width", 105.0, min=60, max=180, step=1, label="Width"),
    toggle("enable_tie_ears", True, label="Tie Ears"),
]

registry = ControlRegistry(CONTROLS)
cfg = registry.bind(params)

# Use cfg["width"], cfg["enable_tie_ears"] for geometry
