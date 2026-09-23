---
id: mission-01-bracket-enclosure
title: Build a bracket, then an enclosure
---

# Build a bracket, then an enclosure

A bracket is a useful first model: two boxes overlap, and `union` joins them. You will change its width, separate its two shapes, and see why placement matters before adding holes or a lid.

Save the complete example below as `bracket.ecky` and open it in Ecky. Lengths are in millimetres. The source declares three controls: span, foot depth, and stock thickness.

## Join the corner bracket {#worked-bracket}

```scheme
(model
  (params
    (number span 64 :label "bracket span" :min 40 :max 120 :step 2)
    (number foot_d 36 :label "foot depth" :min 20 :max 70 :step 2)
    (number stock_t 6 :label "stock thickness" :min 3 :max 12 :step 0.5))
  (part bracket
    (let* ((flange_h 40)
           (overlap stock_t))
      (build
        (shape foot (box span foot_d stock_t :align '(center center min)))
        (shape flange
          (translate 0 (- (/ foot_d 2) (/ overlap 2)) 0
            (box (- span (* 2 stock_t)) overlap flange_h :align '(center center min))))
        (result (union foot flange))))))
```

[Download connected bracket](../projects/01-corner-bracket/02-connected-solution.ecky)

`box` takes width, depth, and height. Here `:align '(center center min)` centers X and Y and puts the bottom at Z = 0. The foot is 64 × 36 × 6. The flange is 52 × 6 × 40.

The flange's Y position is `36 / 2 - 6 / 2 = 15`. Its back face meets the foot's back edge at Y = 18; the bottom 6 mm overlaps the foot. `union` joins that overlapping volume into one bracket.

## Change one dimension {#bracket-solution}

Change span from 64 to 80 and render again. The foot becomes 80 mm wide; the flange becomes 68 mm wide. Depth and height stay fixed. Both widths follow `span`, so neither shape needs a separate edit.

To see a placement error, change the first number in `translate` from `0` to `60`. The flange moves away from the foot. A union cannot bridge that gap. Restore `0` before continuing.

## Read the nested expressions {#build-forms}

Read from the inside outward: `box` creates geometry, `translate` moves it, and `union` combines it. `shape` names an intermediate value inside `build`; `result` returns the final value. `let*` gives names to dimensions used by those shapes.

`part bracket` names the exported part. A `part` can contain disconnected geometry, so counting part declarations does not tell you whether a bracket is connected.

## Separate body from lid {#enclosure-shell}

An enclosure needs two parts because its lid moves independently. The body is an outer box minus a smaller cavity. For a 72 × 48 mm case with 3 mm walls, the cavity is 66 × 42 mm. Subtract twice the wall thickness: there is a wall on each side.

[Download body and lid](../projects/02-configurable-enclosure/01-worked.ecky)

Open the file and find `part body` and `part lid`. Change `case_w` from 72 to 82. Both parts widen; `wall_t` remains 3. The cavity begins above the floor and extends through the top of the body.

## Compare two closures {#joint-branch}

A snap tab and a bolt boss need different geometry. `if` can select one expression or the other. Its first argument is a condition; the next two are the true and false branches.

```scheme
(if (= joint_type "snap")
  snap_geometry
  bolt_geometry)
```

This is a fragment: the three names come from the surrounding model. [Open the complete closure coupon](../projects/02-configurable-enclosure/01b-joint-modes-worked.ecky) to see both branches and their matching receiving cuts.

## Add the closure to the enclosure {#joint}

[The enclosure starter](../projects/02-configurable-enclosure/02-joint-starter.ecky) contains fixed snap features. Find each `FIX` comment. Replace the indicated feature with the corresponding branch from the coupon, on both body and lid. Changing only the body leaves the lid with the wrong opening.

## Compare the finished source {#configurable-enclosure}

[Download configurable enclosure](../projects/02-configurable-enclosure/03-configurable-enclosure-solution.ecky)

Compare its body and lid with your edit. Follow `fit_clearance` into the receiving cuts. A larger clearance should enlarge those cuts while leaving the nominal tab or bolt geometry unchanged.

## Test the closure separately {#print-choice}

Before printing the whole enclosure, print the small closure coupon. Adjust its clearance for the material and printer you will use. Transfer that value to the enclosure after the two pieces fit.

## Add mounting details {#finish}

[The extended bracket](../projects/01-corner-bracket/06-configurable-print-bracket.ecky) adds mounting and reinforcement details. Read it after the simple bracket: start at `result`, then follow the named shapes used there. Keep the connected foot and flange visible while inspecting each cut.
