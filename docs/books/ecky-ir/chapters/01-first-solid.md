## First Solid: Ball on a Base

Every model is a tree, and the fastest way to feel that is to grow the smallest one that renders. One `model`, one `part`, one primitive — three nested forms and you have a solid on screen. Everything later in this book is this same tree with more branches.

```scheme
(model
  (part marker
    (sphere 10)))
```

![Rendered output for First Solid: Ball on a Base, example 1](assets/01-first-solid-01.png)

`model` is the root. `part` gives the geometry a stable id. `sphere` produces the solid.

Add another primitive with `union` when two solids should become one part.

```scheme
(model
  (part marker
    (union
      (box 28 28 4)
      (translate 0 0 10
        (sphere 10)))))
```

![Rendered output for First Solid: Ball on a Base, example 2](assets/01-first-solid-02.png)

`box` makes the base. `translate` moves the ball up so it sits on the base instead of overlapping the center.

Use this pattern for first tests: primitive first, then one transform, then one boolean.

> **Watch for:** every primitive is born centered on the origin, so two solids written at the same spot interpenetrate instead of stacking. The `translate` above is not decoration — delete it and the ball swallows the base. When a union looks fused-but-wrong, the first question is always "did I move the second solid before combining it?"
