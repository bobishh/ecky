## Cut and Join: Mounting Plate

A solid is rarely the end state — you drill it, pocket it, add a boss. Once a part is more than one boolean deep, nesting it all inline becomes unreadable and impossible to point at. `build` is the fix: name each intermediate solid, then combine the names in a final `result`. The geometry is the same; the difference is that every step now has a handle you can reference, cut against, or select later.

```scheme
(model
  (params
    (number plate_w 80)
    (number plate_h 48)
    (number thickness 5)
    (number hole_r 4))
  (part mount
    (build
      (shape blank
        (extrude (rounded-rect plate_w plate_h 4) thickness))
      (shape hole_left
        (translate -24 0 -0.5
          (cylinder hole_r (+ thickness 1))))
      (shape hole_right
        (translate 24 0 -0.5
          (cylinder hole_r (+ thickness 1))))
      (result
        (difference blank hole_left hole_right)))))
```

![Rendered output for Cut and Join: Mounting Plate, example 1](assets/04-cut-and-join-01.png)

`build` names each step. `difference` subtracts cutters from the blank. The cutters are slightly taller than the plate so the cut passes fully through.

Add material with `union` or `fuse`.

```scheme
(result
  (union
    (difference blank hole_left hole_right)
    (translate 0 0 thickness
      (cylinder 12 8))))
```

The result is still one part, but the intent stays readable.

> **Watch for:** two gotchas bite here. First, a cutter that is exactly as tall as the stock leaves a paper-thin film of material at the cut floor (a "coplanar face") — make cutters _overshoot_, which is why the holes above are `(+ thickness 1)` tall and start at `-0.5`. Second, booleans rebuild topology: every face and edge is renumbered afterward, so a selector that pointed at "the top face" before a `difference` may point somewhere else after it. That is exactly the problem the next chapter's `tag-face` and selector strings exist to solve.
