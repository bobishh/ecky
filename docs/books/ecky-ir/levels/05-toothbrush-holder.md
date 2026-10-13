## Level 05: Perforated Toothbrush Holder

**Mission:** Build a shelled product, prove one custom cutter, then repeat it across curved walls.

**Clear condition:** All four checkpoints compile and the final boolean is one body minus one generated cutter group.

Imagine a tiny umbrella stand for a hedgehog who keeps bringing wet toothpicks indoors. Build its hollow body first, then one drain and one wall cutter. Only repeat a cutter after it demonstrably crosses the wall.

Small examples teach syntax. This project teaches control: preserve one useful
body while shelling it, closing it, proving a custom cutter, then generating
dozens of cutters without turning the source into copied geometry.

Every stage is a complete `.ecky` file. Run a checkpoint before reading the
next one with `src-tauri/target/debug/ecky check <checkpoint.ecky>`.

The manifest at
`docs/books/ecky-ir/examples/toothbrush-holder/manifest.json` keeps the stages
ordered and machine-checkable.

### Stage 1: subtract one named cavity

The outside is a union of a centered cylinder and a rear box. The cavity uses
the same construction with dimensions derived from `wall_thickness`.

```scheme
(model
  (params
    (number width 36.0 :min 20.0 :max 60.0 :label "Width")
    (number depth 25.0 :min 15.0 :max 50.0 :label "Depth")
    (number wall_thickness 2.6 :min 1.5 :max 5.0 :label "Wall thickness")
    (number height 85.0 :min 40.0 :max 150.0 :label "Height"))
  (part toothbrush_holder
    (let* ((radius (/ width 2.0))
           (inner_width (- width (* 2.0 wall_thickness)))
           (inner_depth (- depth wall_thickness))
           (inner_radius (- radius wall_thickness))
           (outer
             (union
               (box width depth height :align '(center max center))
               (cylinder radius height :align '(center center center))))
           (cavity
             (union
               (box inner_width inner_depth (+ height 10.0)
                 :align '(center max center))
               (cylinder inner_radius (+ height 10.0)
                 :align '(center center center)))))
      (difference outer cavity))))
```

Canonical checkpoint:
`docs/books/ecky-ir/examples/toothbrush-holder/01-shell.ecky`.

The cavity is taller than the body. That deliberate overrun avoids coincident
top and bottom faces. `wall_thickness` owns every fit-critical offset; there is
no second anonymous wall value waiting to drift.

### Stage 2: close the base, then reopen a drain

Stage 1 is a tube. Stage 2 adds a bottom blank at `(- (/ height 2.0))`, cuts a
drain through it, then unions the drained bottom with the shell.

```text
(bottom_blank
  (translate 0 0 (- (/ height 2.0))
    (union
      (box width depth wall_thickness :align '(center max min))
      (cylinder radius wall_thickness :align '(center center min)))))
(drain
  (translate 0 0 (- (/ height 2.0) 1.0)
    (cylinder drain_radius (+ wall_thickness 2.0)
      :align '(center center min))))
(bottom (difference bottom_blank drain))
```

Canonical checkpoint:
`docs/books/ecky-ir/examples/toothbrush-holder/02-drained-base.ecky`.

The drain begins 1 mm below the bottom and ends 1 mm above it. Cutters should
cross material completely; exact tangency is not a useful manufacturing
constraint.

### Stage 3: prove one custom cutter

The wall pattern starts as a closed `polygon`, becomes a solid with `extrude`,
then receives a frame with `location` and `place`. Only one cutter is
subtracted.

```text
(spade_profile
  (polygon
    ((0.0 4.7) (-3.05 1.5) (-3.25 -0.8) (-2.15 -1.95)
     (-0.55 -3.35) (-1.15 -5.35) (1.15 -5.35)
     (0.55 -3.35) (2.15 -1.95) (3.25 -0.8) (3.05 1.5))))
(spade
  (place
    (location (plane :origin '(0.0 0.0 0.0))
      :rotate '(-90.0 0.0 180.0))
    (extrude spade_profile cutter_depth)))
(front_cutter (translate 0.0 front_cutter_y 0.0 spade))
```

Canonical checkpoint:
`docs/books/ecky-ir/examples/toothbrush-holder/03-single-cutter.ecky`.

Do not start with the full pattern. A wrong profile, axis, or cutter depth
multiplied fifty times only makes the failure slower to understand.

### Stage 4: generate cutters, subtract once

The production model uses nested `repeat-union` forms for staggered front,
left, and right rows. Row and column indices feed local `let*` bindings for
angle, side offset, and height. Those groups become `spade_holes`.

The final intent stays small:

```text
(spade_holes (union front_spades left_spades right_spades))
(final (difference (union shell bottom) spade_holes))
```

The planner flattens the generated tool union into one n-ary `difference`, so
independent cutters can be prepared in parallel before OCCT performs the
boolean. Source still describes one semantic cut; runtime chooses the execution
schedule.

Canonical complete model:
`src-tauri/tests/fixtures/cad/perf/toothbrush_holder_versions.ecky`.

This is the reusable project shape:

**1. Expose physical controls.** Parameters describe user decisions.

**2. Derive dimensions once.** Put dependent math in `let*`.

**3. Name body and cutter stages.** Final expressions should read as intent.

**4. Prove one repeated unit.** Debug one profile, axis, and overrun.

**5. Generate the remaining units.** Use `repeat` or arrays.

**6. Keep the final boolean boring.** One body, one cutter group, one cut.

**Watch for:** a repeated cutter that only touches a wall may disappear or
split topology unpredictably. Give cutter depth explicit overrun and keep safe
top/bottom margins as named bindings.
