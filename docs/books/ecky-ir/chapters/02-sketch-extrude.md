## Sketch to Solid: Plate from a Profile

Primitives get you a ball or a box, but real parts rarely start as a primitive — they start as a shape someone drew. Most useful CAD begins as a 2D outline that you then give thickness. In Ecky that move is `extrude`: hand it a closed profile, hand it a height, get a solid.

```scheme
(model
  (part plate
    (extrude
      (rounded-rect 70 42 5)
      4)))
```

![Rendered output for Sketch to Solid: Plate from a Profile, example 1](assets/02-sketch-extrude-01.png)

`rounded-rect` is the closed 2D profile. `extrude` gives it thickness.

Use `profile` when the shape has holes.

```scheme
(model
  (part washer_plate
    (extrude
      (profile
        :outer (rounded-rect 70 42 5)
        :holes (circle 9 64))
      4)))
```

![Rendered output for Sketch to Solid: Plate from a Profile, example 2](assets/02-sketch-extrude-02.png)

The outer profile defines material. The hole profile removes material during the extrusion.

This is the core move: draw a closed 2D region, then give it height.

> **Watch for:** `extrude` only works on a _closed_ region. An open polyline or a profile whose `:holes` poke through the `:outer` edge has no well-defined inside, and the extrude fails or produces junk. Keep holes strictly inside the outer boundary, and reach for `profile` (not a raw shape) the moment material needs to be removed — the `:outer`/`:holes` split is what tells Ecky which side is solid.
