# Build123d Best Practices & Common Pitfalls

This guide is derived from VibeCAD best practices and common mistakes observed during modeling in Ecky. Use these patterns to ensure successful, manifold, and printable 3D models.

## 1. Algebra vs. Builder Mode

**Algebra Mode (Simple, Direct)**
Best for quick primitives and simple boolean operations.
```python
from build123d import Sphere, Box
result = Sphere(radius=20) - Box(15, 15, 40)
```

**Builder Mode (Context Managers)**
Best for complex parts, sketches, and filtering. **This is the preferred mode for Ecky macros.**
```python
from build123d import *
with BuildPart() as part:
    with BuildSketch() as profile:
        Rectangle(30, 4)
    extrude(amount=100)
```

## 2. Object Attributes (`.part`, `.sketch`, `.line`)

**CRITICAL:** `BuildPart`, `BuildSketch`, and `BuildLine` are builders. Operations like `sweep`, `offset`, `revolve`, or `extrude` often expect the underlying geometric object.
- Use `builder.part` for the resulting solid.
- Use `builder.sketch` for the resulting 2D face.
- Use `builder.line` for the resulting wire/path.

```python
# Correct usage:
sweep(sections=my_sketch.sketch, path=my_line.line)
offset(amount=-2, openings=my_part.faces().sort_by(Axis.Z)[-1])
```

## 3. Avoiding Boolean Failures

**Error:** "Boolean operation unable to clean".
**Cause:** Perfectly coincident surfaces (e.g., a handle exactly touching a cylinder surface).
**Fix:** Ensure a small overlap (e.g., 0.1mm) or a small gap. When adding a part, move its start point slightly *into* the other part.

```python
# Overlap pattern:
with BuildPart(mode=Mode.ADD):
    # Start slightly inside the main body
    Plane(origin=(radius - 0.1, 0, height * 0.5))
    ...
```

## 4. Plane and Positioning

- **`Plane.offset(amount)`**: Shifts the plane along its normal. Only takes a scalar `amount`.
  - `Plane.XY.offset(10)` -> Shifts 10mm in +Z.
- **`Rot(rx, ry, rz)`**: Rotation in degrees.
- **`Pos(x, y, z)`**: Translation.

```python
# Correct transform:
my_shape = Pos(10, 0, 0) * Rot(0, 0, 45) * Box(5, 5, 5)
```

## 5. Selection and Filtering

- **Faces:** `part.faces().sort_by(Axis.Z)[-1]` (Top face).
- **Edges:** `part.edges().filter_by(Axis.X)` (Edges parallel to X).
- **Vertices:** Use `.position_at(0)` or `.position_at(1)` on an edge to get endpoints.

```python
# Correct endpoint access:
edge = my_line.line.edges()[0]
start_pt = edge.position_at(0)
end_pt = edge.position_at(1)
```

## 6. Common Enums

- **`Kind.ARC`**: Rounded corners for `offset`.
- **`Kind.INTERSECTION`**: Sharp corners for `offset`.
- **`Mode.ADD`, `Mode.SUBTRACT`, `Mode.INTERSECT`**: For combining parts/sketches.

## 7. Shelling (Hollow Parts)

Use `offset` with `openings` to hollow a part.
```python
top_face = my_part.faces().sort_by(Axis.Z)[-1]
offset(amount=-wall_thickness, openings=top_face)
```

## 8. Critical Imports

**NEVER** use `from build123d.exporters import ...`. All common functions are in the main module.
```python
from build123d import * # Usually best for Ecky macros
```

## 9. Units

Default units are **millimeters**.
- `10 * CM` = 100mm
- `1 * IN` = 25.4mm
