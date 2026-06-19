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

Use `offset` to grow or shrink a 2D outline by a fixed distance before extruding. A positive distance pushes the outline outward.

```scheme
(model
  (part gasket
    (extrude
      (profile
        :outer (offset 3 (rounded-rect 30 18 4))
        :holes (rounded-rect 30 18 4))
      4)))
```

![Rendered output for Sketch to Solid: Plate from a Profile, example 3](assets/02-sketch-extrude-03.png)

`offset 3` grows the rounded-rect into the outer boundary; the original becomes the hole. The wall is a uniform 3 mm everywhere — the classic gasket move.

`scale` stretches a profile by separate x, y, z factors. Scale a circle in one axis and it becomes an ellipse, so you reach for `scale` instead of a separate ellipse primitive.

```scheme
(model
  (part oval_plate
    (extrude (scale 1.6 1 1 (circle 10 48)) 5)))
```

![Rendered output for Sketch to Solid: Plate from a Profile, example 4](assets/02-sketch-extrude-04.png)

> **Watch for:** `extrude` only works on a _closed_ region. An open polyline or a profile whose `:holes` poke through the `:outer` edge has no well-defined inside, and the extrude fails or produces junk. Keep holes strictly inside the outer boundary, and reach for `profile` (not a raw shape) the moment material needs to be removed — the `:outer`/`:holes` split is what tells Ecky which side is solid.
