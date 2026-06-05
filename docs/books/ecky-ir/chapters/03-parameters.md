## Parameters: Make the Plate Editable

The plate in the last chapter had its size baked in — change the design and you go hunting for four scattered numbers. The moment a model is worth keeping, its dimensions want names. `params` hoists the design choices to the top of the model, where the UI can expose them as labelled sliders and the geometry reads them back by name.

```scheme
(model
  (params
    (number plate_w 70 :label "Plate width" :min 40 :max 120 :step 1)
    (number plate_h 42 :label "Plate height" :min 20 :max 80 :step 1)
    (number corner_r 5 :label "Corner radius" :min 0 :max 12 :step 0.5)
    (number thickness 4 :label "Thickness" :min 1 :max 12 :step 0.5))
  (part plate
    (extrude
      (rounded-rect plate_w plate_h corner_r)
      thickness)))
```

![Rendered output for Parameters: Make the Plate Editable, example 1](assets/03-parameters-01.png)

The geometry reads the parameter names directly. The UI reads labels, min/max, and step from the declarations.

Keep parameters physical: widths, heights, clearances, radii. Put derived math near the geometry.

```scheme
(shape hole_r (/ bore_d 2))
```

That line is better than repeating `(/ bore_d 2)` through cuts and selectors.

### Units: bare numbers already have one

Every number you have written so far carried a hidden unit. Ecky has two base units, and a bare number is already expressed in them: **lengths are millimeters, angles are degrees.** `(box 70 42 4)` is 70 mm by 42 mm by 4 mm; `(rotate 90 0 0 ...)` turns 90 degrees. You never have to write a suffix.

When you do write one, the suffix is a **conversion into that base unit** — nothing more:

| Suffix | Family | Becomes |
| --- | --- | --- |
| `mm` | length | itself (`12mm` → `12`) |
| `cm` | length | ×10 (`1cm` → `10`) |
| `in` | length | ×25.4 (`1in` → `25.4`) |
| `deg` | angle | itself (`90deg` → `90`) |
| `rad` | angle | ×(180/π) (`1.5708rad` → `90`) |

So `(box 12mm 1cm 1in)` is exactly `(box 12 10 25.4)`, and `(rotate 1.5708rad 0 0 ...)` is the same 90-degree turn as `(rotate 90 0 0 ...)`. Suffixes exist so you can author in the unit a spec is written in and let Ecky normalize.

**Some numbers stay unitless on purpose.** Counts (`(repeat 5 ...)`), ratios, segment counts on a cylinder (`(cylinder 6 12 96)` — that `96` is facets, not millimeters), and indices are pure numbers. A suffix on them is meaningless; leave them bare.

**One honest caveat: Ecky does not police dimensions.** The suffix only scales a number into its base unit; it does not tag the value as "a length" or "an angle." Put `45deg` where a width is expected and you get a 45 mm width, no warning — the `deg` is just stripped to its base, which for the box slot is read as millimeters. Units are a convenience for _writing_ correct numbers, not a type system that catches mixing them up. That discipline is yours: author lengths in `mm`/`cm`/`in`, angles in `deg`/`rad`, and keep counts and ratios bare.
