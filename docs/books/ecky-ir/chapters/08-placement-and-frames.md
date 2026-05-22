## Placement and Frames: Put Geometry Where It Belongs

Simple transforms are enough for many models.

```scheme
(translate 20 0 0 (box 10 10 10))
(rotate 0 0 45 (box 10 10 10))
(mirror :normal (1 0 0) (box 10 10 10))
```

Use frames when placement should be named and reused.

```scheme
(model
  (part angled_pin
    (build
      (shape pin_pose
        (plane
          :origin (20 0 4)
          :normal (0 1 1)
          :x (1 0 0)))
      (shape pin
        (cylinder 3 24))
      (result
        (place pin_pose pin)))))
```

![Rendered output for Placement and Frames: Put Geometry Where It Belongs, example 1](assets/08-placement-and-frames-01.png)

`plane` describes a local coordinate system. `place` moves geometry into it.

For path-driven models, `path-frame` can sample a location and tangent along a path. Use it when attachments must follow a curve instead of a fixed world axis.
