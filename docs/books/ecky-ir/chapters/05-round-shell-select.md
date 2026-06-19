## Round, Chamfer, Shell: Select Edges and Faces

This is the book's first **intermediate** chapter, and it earns the label: it stacks five related ideas — `fillet`, `chamfer`, `shell`, `tag-face`, and the native-only `:created-by` — because they all answer the same question, "now that the solid exists, how do I point at the right edge or face and act on it?" Read it in passes. The finishing operations (`fillet`/`chamfer`/`shell`) come first; the selector machinery (`tag-face`, `:created-by`) is what keeps them aimed at the right topology after booleans renumber everything.

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

![Rendered output for Round, Chamfer, Shell: Select Edges and Faces, example 4](assets/05-round-shell-select-04.png)

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

> **Native-only.** `:created-by` is a provenance selector: it relies on the
> originating-slot index that the native OCCT kernel tracks for every face and
> edge. It resolves only on the native backend (Ecky's default). The build123d
> and FreeCAD interop backends have no slot-provenance index, so they reject
> `:created-by` rather than guess. If you lower a model through an interop
> backend (including `ecky check`, which uses build123d today), drop the
> `:created-by` clause and lean on the geometric predicates (`planar`,
> `normal-z`, `area-max`) or a `tag-face` instead.

### Tapered fillets

A normal `fillet` uses one radius. Add `:to-radius` and the radius varies along each selected edge — it starts at the base radius and eases to the second one. Handy for blends that need to grow or shrink along a run.

```scheme
(model
  (part p
    (fillet 4 :to-radius 1 :edges "top" (box 40 40 20))))
```

> **Backend note:** tapered fillets are an OCCT capability rendered by the native and FreeCAD backends. The build123d backend only does single-radius fillets, so it rejects `:to-radius` with a clear error rather than silently giving you a uniform fillet — render tapered fillets on native or FreeCAD.

### Draft

`draft` tilts the side walls of a solid by an angle so a molded part can release from its tool. It tapers every vertical face about a neutral plane (the level that stays the original size); pass `:neutral-z` to move that plane, otherwise it sits at `z = 0`.

```scheme
(model
  (part p
    (draft 8 (box 30 30 20))))
```

![Rendered output for Round, Chamfer, Shell: Select Edges and Faces, example 7](assets/05-round-shell-select-07.png)

> **Backend note:** draft is rendered by the native and build123d backends (both OpenCASCADE). The FreeCAD backend has no Part draft API, so it rejects `draft` with a clear error. This first cut drafts *all* vertical faces; targeting specific faces with a `:faces` selector is a planned extension.
