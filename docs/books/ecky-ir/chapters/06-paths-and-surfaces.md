## Paths and Surfaces: Revolve and Sweep

Use `revolve` when a 2D profile turns around an axis.

```scheme
(model
  (part knob
    (revolve
      (make-face
        (path
          (12 0 0)
          (18 0 0)
          (18 18 0)
          (10 24 0)
          (12 0 0)))
      360)))
```

![Rendered output for Paths and Surfaces: Revolve and Sweep, example 1](assets/06-paths-and-surfaces-01.png)

`path` creates the outline. `make-face` turns the closed outline into a face. `revolve` spins it into a solid.

Use `sweep` when a profile follows a path.

```scheme
(model
  (part handle
    (sweep
      (circle 2.2 32)
      (bezier-path
        ((-24 0 0) (-10 18 6) (10 18 6) (24 0 0))))))
```

![Rendered output for Paths and Surfaces: Revolve and Sweep, example 2](assets/06-paths-and-surfaces-02.png)

The circle is the cross-section. The bezier path is the centerline. Sweep keeps those responsibilities separate.

Use `loft` when one profile needs to become another profile across height or distance.

```scheme
(model
  (part nozzle
    (loft 24
      (circle 14 32)
      (circle 5 32))))
```

![Rendered output for Paths and Surfaces: Revolve and Sweep, example 3](assets/06-paths-and-surfaces-03.png)

The first profile is the base, the last is the cap, and `loft` skins a smooth wall between them. The leading number is the total height; profiles stack evenly along it, so the wide circle sits at the bottom and the narrow one at the top.

### Ribs and grooves

`rib` and `groove` are the two-step "sweep a profile, then combine" move rolled into one op. Both take a solid, a profile, and a path: `rib` sweeps the profile along the path and fuses the result onto the solid (a reinforcing rib); `groove` sweeps it and cuts it away (a channel).

```scheme
(model
  (part p
    (rib
      (box 20 20 20)
      (circle 3)
      (path (0 0 0) (0 0 30)))))
```

![Rendered output for Paths and Surfaces: Revolve and Sweep, example 4](assets/06-paths-and-surfaces-04.png)

Swap `rib` for `groove` to subtract the same swept run instead of adding it. They lower to `sweep` + `union`/`difference`, so they render on every backend.
