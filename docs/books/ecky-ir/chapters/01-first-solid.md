## First Solid: Ball on a Base

Start with the smallest complete `.ecky` program: one `model`, one `part`, one primitive.

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
