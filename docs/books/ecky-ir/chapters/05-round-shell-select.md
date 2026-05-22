## Round, Chamfer, Shell: Select Edges and Faces

Edge operations happen after the main solid exists.

```scheme
(model
  (part soft_block
    (fillet 2
      :edges "top"
      (box 60 36 16))))
```

![Rendered output for Round, Chamfer, Shell: Select Edges and Faces, example 1](assets/05-round-shell-select-01.png)

`:edges "top"` selects top boundary edges. Use `chamfer` when the edge should become flat instead of rounded.

```scheme
(model
  (part beveled_block
    (chamfer 1.5
      :edges "bottom"
      (box 60 36 16))))
```

![Rendered output for Round, Chamfer, Shell: Select Edges and Faces, example 2](assets/05-round-shell-select-02.png)

Use `shell` to hollow a solid by removing selected faces.

```scheme
(model
  (part open_tray
    (shell 2
      :faces "top"
      (box 70 44 22))))
```

![Rendered output for Round, Chamfer, Shell: Select Edges and Faces, example 3](assets/05-round-shell-select-03.png)

Selectors should describe a physical feature: top, bottom, planar normal, or a stable target id. Avoid anonymous offsets for fit-critical faces.

Tag any fit-critical selector. The tag records intended topology in the manifest, so param changes can rebind the same seat, lip, or opening instead of chasing backend face indexes.

```scheme
(model
  (tag-face tray_opening :faces "top" tray)
  (part tray
    (shell 2
      :faces (tag tray_opening)
      (box 70 44 22))))
```

When a `build` introduces helper solids, use `:created-by <shape>` to keep clause selectors scoped to topology from that intermediate shape only.

```scheme
(model
  (part body
    (build
      (shape blank (box 70 44 22))
      (shape pocket (translate 0 0 10 (box 30 18 12)))
      (shape tray (difference blank pocket))
      (result
        (shell 2
          :faces "planar+normal-z+area-max"
          :created-by pocket
          tray)))))
```

Here `:created-by pocket` limits face candidates to the cavity created from `pocket`, not every planar top-facing face on `tray`.
