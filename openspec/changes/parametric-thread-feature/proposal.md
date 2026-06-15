# Proposal: Parametric Thread as a Structural Primitive

## Intent

Make threads a **first-class, intent-driven structural block** that can be
"screwed into" any body: you state the *intent* (nominal diameter, pitch, flank
angle, profile mode, clearance) and the cross-section geometry (base/crest
widths, depth, radii) is **derived**. The same primitive places via an axis and
booleans into any solid — an external bolt/boss (`union`), a tapped hole
(`difference`), or a helicoid focus thread. No hand-juggling of mutually
dependent low-level parameters.

Three surfaces of one primitive:

- **External thread** (bolt / boss / helicoid male) — `union` with a core.
- **`tapped-hole`** (female cutter) — a manifold-by-construction bore + relief
  you `difference` from any wall/nut/socket.
- **Mating pair** — an external thread and a `tapped-hole` of the same nominal
  size with complementary clearance fit together.

## Why now (the recurring pain, observed)

- The `helicoid` film-adapter hardcodes `crest = base * 0.58` in two places and
  exposes `thread_depth`/`thread_width` but **not the flank slope** — so its
  thread prints with a ~68°-from-vertical overhang ("углы не те").
- The `thread` op has a **coincident-face bug**: it builds the core cylinder at
  exactly the ridge-root radius, so on coarse/deep params the `union` fails to
  fuse and **drops the core → a hollow spiral** instead of a solid bolt. Worked
  around manually with an overlap; the op itself must be fixed.
- A `tapped-hole` cut with a female relief and a separate bore at the same radius
  leaves a coincident cylinder face → **209 non-manifold edges** (observed on a
  nut). Overlapping bore + relief fixes it — that logic belongs in the primitive.
- Printability is judged by eye. The overhang flank, the `pitch > base` guard,
  and manifold/single-solid should be **authored `verify` clauses**, not vibes.

## It is derivation, not a solver

The relationships are closed-form, so a one-pass derivation suffices — a
constraint solver would be over-engineering:

```
overhang flank α (from horizontal):   (base − crest)/2 = depth · tan(α)
  → base = crest + 2 · depth · tan(α)
guard:  pitch > base + clearance      (else turns merge / self-intersect)
major = d/2 ;  minor = d/2 − depth    (external) ;  bore at minor (tapped hole)
```

The free knobs are the *intent* (`d`, `pitch`|`turns`, `flank`, `depth`,
`profile`, `clearance`); `base`/`crest`/radii are derived. Change the flank
angle and the rest recomputes. A solver would only be needed for "fix any subset,
solve the rest", which threads do not require.

## Symmetric and asymmetric in one

A profile **mode** selects the cross-section, the angle drives it:

```scheme
(thread … :pitch p :engagement d :profile 'sym      :flank 45deg)
(thread … :pitch p :engagement d :profile 'buttress :load-flank 5deg :return-flank 45deg)
```

`sym` is expressible today (symmetric trapezoid from `helical-ridge`).
`buttress` (load flank near-vertical, overhang flank ≤ 45°) is the
print-optimal profile but needs `helical-ridge` to accept an **asymmetric**
profile (independent upper/lower flanks) — a small op enhancement.

## "Any surface" = any placement

A thread is **axis-local** (a cylinder/cone around its own axis). Applying it to
a wall is not "wrapping a helix on a freeform surface" — it is placing the
feature on an **axis** (existing `location`/`place`) and booleaning with the
body:

```scheme
(difference wall
  (place (location (plane :origin '(x y z)) :rotate '(...))
    (tapped-hole :iso "M8" :length 12 :flank 45deg :clearance 0.3)))
```

So "threads into any surface" reduces to "any placement", which already works.
Cylinder and cone (tapered/pipe) are in scope; navigating a helix over a genuine
freeform surface is deferred.

## Builds on landed work

The radial-thread fix already shipped (`1a58364`: Frenet trihedron +
RightCorner transition on the helical sweep, so the section stays radial instead
of banking). This change builds the parametric primitive on top of that solid
foundation.

## Out of scope

- A numerical constraint solver (derivation is closed-form).
- Threads wrapped on freeform (non-revolution) surfaces.
- Standards-grade thread tables beyond ISO metric coarse decode (`:iso "M…"`).
