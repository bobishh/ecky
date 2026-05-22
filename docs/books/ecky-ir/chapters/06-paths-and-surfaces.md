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
