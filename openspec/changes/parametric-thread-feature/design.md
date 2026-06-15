# Design: Parametric Thread Primitive

## 1. Layering

```
intent (d, pitch|turns, flank, depth, profile, clearance)
  └─ derive  base/crest/radii (closed-form)            ← pure, in the op
       └─ helical-ridge (swept trapezoid, Frenet/RightCorner)   ← landed 1a58364
            └─ compose: external = union(core, ridge)
                        tapped-hole = difference-cutter(bore ∪ relief)
                 └─ place (axis) + boolean into a body
                      └─ verify: single-solid, manifold, overhang ≤ flank, pitch > base
```

## 2. Op surface (`thread` / `helical-ridge` / `tapped-hole`)

### 2.1 `thread` gains intent keywords (derive base/crest)
Today `thread` takes `:radius :pitch :length :depth :base-width :crest-width`.
Add **intent** keywords that derive the profile so callers stop hand-computing:

- `:flank <angle>` (symmetric) → `base = crest + 2·depth·tan(flank)`.
- `:profile 'sym | 'buttress`; buttress takes `:load-flank` / `:return-flank`.
- `:crest <width>` optional (default a small fraction of pitch).
- Explicit `:base-width`/`:crest-width` still allowed (override derivation).

### 2.2 Fix the coincident-face / hollow bug
`thread` (external) must build the core with a small **overlap** so the
`union(core, ridge)` never shares a coincident cylinder face:

```
core_radius = minor + overlap          (overlap ~0.2–0.3 mm)
ridge :radius = minor, :depth = depth + overlap
```

This is the same overlap the helicoid already does by hand; move it inside the
op so `thread` is robust for all params (no more hollow spirals).

### 2.3 `tapped-hole` (female cutter, manifold by construction)
A new helper op producing the **negative** to `difference` from a body:

```
minor = d/2 − depth
bore   = cylinder(minor, length)                 (the drilled hole)
relief = helical-ridge female, radius < bore      (overlaps the bore → no
         coincident face), crest reaches major
cutter = union(bore, relief)                       (one solid)
```

Subtracting `cutter` carves a manifold internal thread (minor bore + helical
relief out to major). The radius-inset of the relief vs the bore is what keeps
the result manifold (observed: equal radius → 209 non-manifold edges).

### 2.4 `buttress` asymmetric profile (op enhancement)
`helical-ridge` currently sweeps a **symmetric** trapezoid (base and crest both
centred on the path). For buttress, the profile needs **independent upper/lower
flanks** — e.g. a `:lower-flank`/`:upper-flank` or a skewed trapezoid where the
crest is offset axially. This is the only genuinely new geometry in the change.

## 3. Derivation (closed-form, in the op)

```
flank from horizontal α ;  depth d_r ;  crest c
base b = c + 2·d_r·tan(α)
guard:  pitch > b + clearance      → else emit a printability diagnostic
major = D/2 ; minor = D/2 − d_r
helix pitch = pitch  (or carrier_h / turns when turns is given)
```

No iteration. A diagnostic (not a hard error) fires when `pitch ≤ base +
clearance` so the caller knows the turns will merge.

## 4. Placement + application (reuse)
- Axis via existing `location` / `place` (`plane :origin … :rotate …`).
- External thread → `union` (boss) or stands alone (bolt).
- `tapped-hole` → `difference` from any wall/nut/socket body.
- Mating: an external `thread` and a `tapped-hole` of equal nominal with
  complementary `clearance` engage.

## 5. Printability as verify
Authored `(verify …)` clauses make print-readiness measurable:

- `stl connected-component-count = 1` (solid, not exploded/hollow);
- `stl non-manifold-edge-count = 0` (watertight export);
- `stl overhang-face-count` / overhang ratio within budget for the orientation;
- a derived guard surfaced as a clause: `pitch > base` (turns do not merge).

Note the observed gap: a *valid solid* can still be the *wrong* solid (a hollow
spiral passed `IsValid` + single-solid). So verification must include a
**geometric sanity** check (expected volume / core present), not only topology.

## 6. Backends
- build123d: `_ecky_helical_ridge` helper (Frenet via `is_frenet=True`, landed).
  Add asymmetric profile + the overlap in the `thread` helper.
- native OCCT runner: `sweep_shape` Frenet + RightCorner (landed); add the
  overlap in `expand_thread_node`, the `tapped-hole` expansion, and asymmetric
  profile in `expand_helical_ridge_node`.
- Parity: native ↔ build123d on bbox + volume + manifold per the existing
  harness.

## 7. Consumers (actualize)
- **Helicoid** (`Film scanning adapter - Ecky helicoid top cover`): replace the
  two hardcoded `crest = base*0.58` helical-ridges with the intent primitive
  (`:flank`), so its thread is tunable for print overhang.
- **stdlib fasteners** (`language-convenience-stdlib` 3.3): hex-bolt /
  threaded-rod / nut use `thread` + `tapped-hole` instead of bespoke geometry.

## 8. Alternatives considered
- **Constraint solver**: rejected — relationships are closed-form; a solver is
  only justified for bidirectional "fix any subset" which threads do not need.
- **Keep low-level params, document the math**: rejected — the helicoid proves it
  rots into hardcoded magic numbers (`0.58`) duplicated across parts.
- **Wrap a helix on arbitrary surfaces**: deferred — axis + placement covers the
  real "thread into a wall" case; freeform is research.
