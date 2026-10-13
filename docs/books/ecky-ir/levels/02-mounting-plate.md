## Level 02: Mounting Plate

**Mission:** Turn a blank into a useful plate with repeated through-holes.

**Clear condition:** Every cutter crosses the plate and the exported STL remains one component.

Our robot now wants bolts, not optimism, to hold its biscuit shelf. Think of each cutter as a cookie cutter passing completely through the dough: a cutter that stops inside leaves a blind pocket.

Use `build` when a part needs several boolean stages. Each `shape` names an intermediate result; `result` identifies the final geometry. Names keep cutters and later selectors readable.

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

> **Watch for:** make cutters cross the stock completely; coincident cutter and stock faces can leave unstable slivers. Booleans also rebuild topology, so raw face and edge indices are not durable selectors. Use geometric selectors or tags after the boolean.
