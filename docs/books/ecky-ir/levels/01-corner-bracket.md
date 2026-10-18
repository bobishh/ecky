## Level 01: Corner Bracket

**Mission:** Build one connected corner bracket from a horizontal foot and a vertical flange.

**Clear condition:** Preview shows one connected L-bracket where the foot and flange overlap, and the source compiles.

These are extra engineering exercises. For the illustrated beginner book,
start with [Worm Farm](https://ecky-cad.com/docs/chapters/03-rake/): build a rake,
house, garden, snap-together tractor and articulated worm. Return here to apply those ideas to
practical parts. This bracket is the shelf support for a robot's emergency
biscuit stash. Its two shapes must overlap; robots cannot eat an invisible join.

A renderable file needs a `model` root, a named `part`, and geometry. The first
useful part is a **corner bracket**: one horizontal foot and one vertical flange
that cross at a corner and fuse into a single printable L-shape. Starting with
two primitives keeps each transform and boolean obvious.

Begin with one solid so the root, the part id, and the primitive are clear.

```scheme
(model
  (part bracket
    (box 40 40 6)))
```

`model` is the root. `part` gives the geometry a stable id (`bracket`). `box`
produces one solid centered at the origin. Nothing is joined yet.

Make the bracket from two solids that share the corner. A long thin foot and a
long thin flange, each crossing the origin, overlap in a small square region.
`union` fuses that overlap into one connected part.

```scheme
(model
  (part bracket
    (union
      (box 40 8 6)
      (box 8 40 6))))
```

The foot is `40 x 8 x 6`; the flange is `8 x 40 x 6`. Both start at the origin,
so they overlap in an `8 x 8 x 6` corner. That deliberate overlap is what makes
the union produce one connected L-bracket instead of two loose pieces. `union`
merges overlapping solids; it does not glue solids that merely touch at a face.

When the foot and flange must sit at different heights, move one with
`translate` so the overlap is preserved, not destroyed.

```scheme
(model
  (part bracket
    (union
      (box 40 8 6)
      (translate 0 0 3
        (box 8 40 6)))))
```

`translate 0 0 3` lifts the flange so it still crosses the foot by half its
thickness. The overlap region shrinks but never disappears, so the union stays
one body. Move geometry to control placement; keep enough overlap so the
boolean has connected material to merge.

Use this pattern for the first real part: name the part, place primitives so
they overlap, then join them with one boolean.

> **Watch for:** primitives start at the origin. Two solids that do not overlap
> produce a union of two disconnected bodies, not one part. If a union has the
> right members but reads as two pieces, inspect placement and overlap before
> changing the boolean.
