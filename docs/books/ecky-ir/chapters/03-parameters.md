## Parameters: Make the Plate Editable

Once a model works with constants, move design choices into `params`.

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

### Units and suffixed literals

For physical authoring, generation should emit suffixed literals like mm/cm/in/deg/rad.

Examples:

- `12mm`
- `2.54cm`
- `0.25in`
- `45deg`
- `1.5708rad`

Emit suffixed literals for lengths and angles. Use bare numbers only for counts, ratios, and unitless math.
