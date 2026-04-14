# Build123d Common Pitfalls & Best Practices

This guide highlights common mistakes when using the `build123d` library within Ecky and provides the correct patterns to follow.

## 1. Boolean Operations & Clean Solids

**Pitfall:** "Boolean operation unable to clean" error.
**Cause:** This usually happens when you have perfectly coincident surfaces (e.g., trying to join a handle that exactly touches the body surface).
**Fix:** Ensure a small overlap (e.g., 0.1mm) or a small gap. When adding a part, move it slightly *into* the other part.

```python
# Avoid:
with BuildPart(mode=Mode.ADD):
    # Plane exactly on the surface
    Plane(origin=(radius, 0, 0))
    ...

# Prefer:
with BuildPart(mode=Mode.ADD):
    # Move slightly inside
    Plane(origin=(radius - 0.1, 0, 0))
    ...
```

## 2. Object Attributes (`.part`, `.sketch`, `.line`)

**Pitfall:** `RuntimeError: sweep doesn't accept BuildSketch` or `TypeError`.
**Cause:** `BuildPart`, `BuildSketch`, and `BuildLine` are context managers. Operations like `sweep`, `offset`, or `revolve` often expect the underlying geometric object, not the builder itself.
**Fix:** Use `.part`, `.sketch`, or `.line` attributes.

```python
# Avoid:
sweep(sections=my_sketch_builder, path=my_line_builder)

# Prefer:
sweep(sections=my_sketch_builder.sketch, path=my_line_builder.line)
```

## 3. Plane Offset

**Pitfall:** `TypeError: Plane.offset() got an unexpected keyword argument 'direction'`.
**Cause:** `Plane.offset(amount)` only takes a single scalar `amount` representing a shift along its normal.
**Fix:** Use `Plane.offset(10)` or create a new plane with a translated origin.

```python
# Avoid:
Plane.XY.offset(10, direction=Axis.Z)

# Prefer:
Plane.XY.offset(10)
```

## 4. Enum Constants (`Kind`, `GeomType`, etc.)

**Pitfall:** `AttributeError: type object 'Kind' has no attribute 'ROUND'`.
**Cause:** Guessed enum names that don't exist.
**Fix:** Use the correct `Kind` constants:
- `Kind.ARC`: Rounded corners (use for `offset`).
- `Kind.INTERSECTION`: Sharp corners.

## 5. Spline & Curve Endpoints

**Pitfall:** `AttributeError: 'Spline' object has no attribute 'end'`.
**Cause:** `Spline` (and other curves) are not simple segments with `.start` or `.end`.
**Fix:** Access the underlying edges or use the coordinates you used to create it.

```python
# Accessing vertex:
my_spline.edges()[0].position_at(0) # Start
my_spline.edges()[-1].position_at(1) # End
```

## 6. BuildPart vs Mode.ADD

**Pitfall:** Confusing `with BuildPart() as p:` with `with BuildPart(mode=Mode.ADD):`.
**Cause:** Nesting `BuildPart` without a mode creates a *new* local context that doesn't automatically join the parent.
**Fix:** Use `mode=Mode.ADD` (or `SUBTRACT`, `INTERSECT`) for nested builders if you want them to modify the current part.

```python
with BuildPart() as main:
    ...
    with BuildPart(mode=Mode.ADD):
        # This will be fused to 'main'
        ...
```
