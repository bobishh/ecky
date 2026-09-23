---
id: mission-04-gillette-travel-kit
title: Make a hollow travel case
---

# Make a hollow travel case

The razor travel kit has a base, sliding lid, and small blade cover. Start with the empty base: it shows how wall and floor thickness come from one subtraction. Then inspect the features that hold the lid and contents.

## Separate the moving pieces {#separate-parts}

Use separate `part` forms for the base, lid, and blade cover. Join features that belong to the same printed piece; leave parts that must move independently separate. A lid positioned above a base is still a separate part, even when their edges touch.

## Subtract the cavity {#shell}

```scheme
(model
  (params
    (number case_length 116 :label "Case length" :min 90 :max 150 :step 1)
    (number case_width 72 :label "Case width" :min 50 :max 100 :step 1)
    (number case_height 20 :label "Case height" :min 12 :max 36 :step 1)
    (number wall 3 :label "Wall" :min 1.6 :max 5 :step 0.2)
    (number floor 1.8 :label "Floor" :min 1 :max 4 :step 0.2)
    :relations ((> case_length case_width) (> case_width wall)))
  (part shell_blank
    (let* (
      (outer (extrude (rounded-rect case_length case_width 6) case_height))
      (inner_length (- case_length (* 2 wall)))
      (inner_width (- case_width (* 2 wall)))
      (cavity (translate 0 0 floor
                (extrude (rounded-rect inner_length inner_width 3)
                         (+ (- case_height floor) 1)))))
      (difference outer cavity))))
```

[Download shell example](../projects/08-gillette-kit/shell-blank.ecky)

The outside is 116 × 72 × 20 mm. With `wall = 3`, the cavity is 110 × 66 mm. Its bottom starts at Z = 1.8, leaving the floor. Its extrusion ends 1 mm above the outside, so the opening has no top skin.

Change `wall` from 3 to 2.4. The outside stays fixed and the cavity becomes 111.2 × 67.2 mm. Change `floor` separately to see the bottom rise without changing the side walls.

## Position the cover pockets {#detents}

[Open the blade-cover exercise](../projects/08-gillette-kit/blade-cover-detents.ecky). Find `detent_engagement` and follow it into the two pocket positions. The same value moves both pockets; `pocket_radius` controls their size.

Change engagement from 0.2 to 0.3 and inspect both pockets. If only one moves, the two positions do not use the same binding. Do not compensate by changing pocket radius: position and size are different adjustments.

## Read the complete kit {#kit-solution}

[Download travel kit](../../../../sites/landing/src/models/gillette-travel-kit.ecky)

Start with the three `part` forms. In each part, find the final boolean expression and trace its inputs. Read the lid rails before the surface cutouts: rails determine how the lid engages the base.

## Test the handle clip {#fit-coupon}

[Download handle clip](../projects/08-gillette-kit/handle-snap.ecky)

The clip uses these dimensions:

```scheme
(inner_radius (+ (/ handle_diameter 2) snap_clearance))
(outer_radius (+ inner_radius clip_wall))
```

At the defaults, a 14 mm handle gets a 7.4 mm inner radius and a 9 mm outer radius. `snap_clearance` is radial here: 0.4 mm adds 0.8 mm to the bore diameter. `clip_wall` changes the material around the bore without changing that clearance.

Print this short clip with the intended filament. Adjust clearance for insertion and wall thickness for flex before using the same values in the whole kit.
