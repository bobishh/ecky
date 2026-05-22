## Real Model Patterns: Procedural Cuts and Arrayed Frames

Before the final film adapter, three smaller real fixtures show language features that are not obvious from hand-sized teaching examples: generated cutter lists, deterministic pseudo-random layout, path frames, array helpers, and parameter-driven repeated cavities.

### Procedural perforated panel

This model uses `map` and `range` to generate cutters, `hash-signed` to jitter each cutter, `voronoi2` to vary cutter radius, and `apply union` to turn the generated list into one cutter body.

<!-- render-source: ../examples/voronoi-perforated-panel.ecky -->

![Rendered output for Real Model Patterns: Procedural Cuts and Arrayed Frames, example 1](assets/10-real-model-patterns-01.png)

The important line is the result expression:

```scheme
(result
  (difference
    panel
    (apply union
      (map
        (lambda (cell)
          (let* ((col (- cell (* 4 (floor (/ cell 4)))))
                 (row (floor (/ cell 4)))
                 (x (* (- col 1.5) 14))
                 (y (* (- row 1.0) 12))
                 (jx (+ x (* 2.4 (hash-signed col row 23))))
                 (jy (+ y (* 2.4 (hash-signed (+ col 19.19) (+ row 7.73) 54))))
                 (r (+ 2.2 (* 1.1 (voronoi2 (/ jx 14.0) (/ jy 12.0) 23)))))
            (translate jx jy 0
              (cylinder r 8 24))))
        (range 0 cell-count)))))
```

`range` decides how many cutters exist. `map` builds one cylinder per cell. `let*` is required because `jx`, `jy`, and `r` depend on earlier bindings. `apply union` converts the list of cylinders into one boolean operand for `difference`.

This is the pattern to use when the count is parametric but the result is still one printable part.

### Frame and array bracket

This fixture combines curve-driven placement with arrays. The rib is swept along a bezier path. The pad is placed at a sampled path frame. The base holes, locator posts, and fan stops use three array helpers.

<!-- render-source: ../examples/frame-array-bracket.ecky -->

![Rendered output for Real Model Patterns: Procedural Cuts and Arrayed Frames, example 2](assets/10-real-model-patterns-02.png)

The model has three distinct placement styles:

```scheme
(shape rail
  (bezier-path ((-18 0 4) (-8 7 9) (8 -7 12) (18 0 16))))
(shape rib
  (sweep (circle 1.1) rail))
(shape end-frame
  (path-frame rail :at end :up (0 0 1)))
(shape placed-pad
  (place end-frame pad :offset (0 0 -1.5) :rotate (0 0 18)))
```

`sweep` makes geometry follow the path. `path-frame` samples a pose from the path. `place` uses that pose to attach another solid.

The array helpers do the repeated work:

```scheme
(linear-array 3 14 0 0
  (translate -14 0 -2 (cylinder 2.1 10)))

(grid-array 2 3 16 10
  (translate -16 -5 4 (cylinder 1.2 8)))

(radial-array 6 60 11
  (translate 0 0 4 (cone 1.8 0.8 5)))
```

Use these when the pattern is regular. Use `map` and `range` when each instance needs custom math.

### Woodlouse hotel

This small habitat uses one cutter list for the entrances, then repeated shelves and vertical dividers. The point is not insect biology; the point is using named dimensions to keep repeated voids aligned with repeated structure.

<!-- render-source: ../examples/woodlouse-hotel.ecky -->

![Rendered output for Real Model Patterns: Procedural Cuts and Arrayed Frames, example 3](assets/10-real-model-patterns-03.png)

The entrances are generated from one parametric chamber count:

```scheme
(shape entrances
  (apply union
    (map
      (lambda (cell)
        (let* ((col (- cell (* chamber_cols (floor (/ cell chamber_cols)))))
               (row (floor (/ cell chamber_cols)))
               (x (+ (* -0.5 hotel_w) wall (* (+ col 0.5) col_gap)))
               (z (+ wall (* (+ row 0.55) floor_gap))))
          (translate x (* -0.5 hotel_d) z
            (rotate 90 0 0
              (cylinder entrance_r (+ hotel_d 6) 24)))))
      (range 0 (* chamber_cols 3)))))
```

`chamber_cols` drives both cutter count and divider spacing. `col_gap` is derived from `hotel_w` and `chamber_cols`, so openings stay centered when the model is resized.
