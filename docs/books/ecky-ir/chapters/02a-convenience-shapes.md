## Convenience Shapes: Stop Hand-Building Common Outlines

`box`, `sphere`, and `extrude` cover a lot, but some outlines come up so often that drawing them by hand wastes time and invites mistakes. Ecky ships them as named shapes. Each one is a true analytic primitive (or expands to one), so it renders identically on every backend — no faceted approximations.

A **torus** is a ring: major radius to the tube centre, minor radius of the tube.

```scheme
(model
  (part ring
    (torus 20 5)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 1](assets/02a-convenience-shapes-01.png)

An **ellipse** is a 2D profile — give it the x and y radii, then `extrude` it like any sketch. When the y radius is larger, the long axis simply swings to y; you do not rotate anything yourself.

```scheme
(model
  (part oval
    (extrude (ellipse 18 10) 4)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 2](assets/02a-convenience-shapes-02.png)

A **regular-polygon** takes a side count and a circumradius (optionally `:rotation`).

```scheme
(model
  (part hex
    (extrude (regular-polygon 6 12) 5)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 3](assets/02a-convenience-shapes-03.png)

A **trapezoid** takes the bottom width, top width, and height; add `:skew` to slide the top sideways.

```scheme
(model
  (part wedge_plate
    (extrude (trapezoid 40 24 18 :skew 4) 5)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 4](assets/02a-convenience-shapes-04.png)

A **wedge** is the 3D ramp: a `dx × dy × dz` box whose top face shrinks to the rectangle `xmin..xmax` by `zmin..zmax`.

```scheme
(model
  (part ramp
    (wedge 40 20 30 10 5 30 25)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 5](assets/02a-convenience-shapes-05.png)

### Slots

A slot is an obround — a rectangle capped by two semicircles. Four front-ends describe the same shape from whatever you happen to know.

`slot-overall` takes the tip-to-tip length and the width.

```scheme
(model
  (part track
    (extrude (slot-overall 50 12) 4)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 6](assets/02a-convenience-shapes-06.png)

`slot-center-to-center` takes the distance between the two end-arc centres and the width.

```scheme
(model
  (part track_c2c
    (extrude (slot-center-to-center 38 12) 4)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 7](assets/02a-convenience-shapes-07.png)

`slot-center-point` takes the slot centre `(cx cy)`, the centre of one end arc `(px py)`, and the width — handy when you already know where the holes go. It orients itself along the line between the two points.

```scheme
(model
  (part track_cp
    (extrude (slot-center-point 0 0 30 0 12) 4)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 8](assets/02a-convenience-shapes-08.png)

`slot-arc` curves the slot along a circular arc: centreline radius, start and end angle (degrees), and width.

```scheme
(model
  (part curved_track
    (extrude (slot-arc 30 0 120 10) 4)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 9](assets/02a-convenience-shapes-09.png)

> **Watch for:** the slot, ellipse, regular-polygon, and trapezoid examples here are 2D profiles — they need an `extrude` (or `revolve`) to become a solid. `torus` and `wedge` are already solids, so they stand alone.

### Threads

`thread` builds a screw thread by sweeping a ridge along a helix around a core cylinder — you do not hand-build the helix. Give it a radius, pitch, length, and depth.

```scheme
(model
  (part screw
    (thread :radius 6 :pitch 1.5 :length 18 :depth 0.9)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 10](assets/02a-convenience-shapes-10.png)

For standard hardware, `:iso "M…"` decodes an ISO metric coarse-pitch designation into the radius, pitch, and depth for you — pass only the length.

```scheme
(model
  (part bolt
    (thread :iso "M8" :length 20)))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 11](assets/02a-convenience-shapes-11.png)

`:female #t` makes the matching cutter instead of a solid screw. Subtract it from a bore to tap a hole; `:clearance` widens the envelope so the parts actually mate.

```scheme
(model
  (part nut
    (difference
      (cylinder 10 8)
      (thread :iso "M8" :length 8 :female #t :clearance 0.2))))
```

![Rendered output for Convenience Shapes: Stop Hand-Building Common Outlines, example 12](assets/02a-convenience-shapes-12.png)

`:lefthand #t` reverses the helix. Unknown ISO designations (e.g. `"M7"`) fail with a clear error rather than guessing.
