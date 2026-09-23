---
id: mission-03-wing-propeller-study
title: Loft a wing and repeat a blade
---

# Loft a wing and repeat a blade

A loft joins cross-sections into a solid. Here two four-point profiles become a wing-shaped study. You will change its span and taper, then read how one blade is repeated around a hub. These examples demonstrate geometry, not tested flying parts.

## Define the two sections {#stations}

A section, or station, is a profile at one position along the shape. In this example the root is wider and thicker than the tip. Each profile has four vertices in the XY plane; `loft` separates the profiles along Z.

## Build the first loft {#wing-worked-stations}

```scheme
(model
  (params
    (number span 90 :label "Span" :min 50 :max 180 :step 5)
    (number root_chord 44 :label "Root chord" :min 24 :max 80 :step 1)
    (number tip_chord 26 :label "Tip chord" :min 12 :max 60 :step 1)
    (number twist 6 :label "Tip twist" :min -12 :max 12 :step 1)
    :relations ((> root_chord tip_chord)))
  (part wing
    (let* (
      (root_thickness (* root_chord 0.12))
      (tip_thickness (* tip_chord 0.10))
      (root_station
        (polygon (list
          (list 0 0)
          (list (* 0.35 root_chord) root_thickness)
          (list root_chord 0)
          (list (* 0.35 root_chord) (* -0.45 root_thickness)))))
      (tip_station_raw
        (polygon (list
          (list 0 0)
          (list (* 0.35 tip_chord) tip_thickness)
          (list tip_chord 0)
          (list (* 0.35 tip_chord) (* -0.45 tip_thickness)))))
      (tip_station (rotate 0 0 twist tip_station_raw)))
      (loft span root_station tip_station))))
```

[Download wing study](../projects/04-parametric-wing/worked-stations.ecky)

`root_thickness` is 12% of root chord; `tip_thickness` is 10% of tip chord. The second profile rotates around Z before the loft. With two profiles, `span` is their separation.

Change `span` from 90 to 120. The sections move farther apart while their outlines stay the same. Restore it, then set `twist` to 0 to compare an untwisted tip.

## Derive the tip width {#wing-tip}

The first model lets you set root and tip chord independently. To keep a fixed taper, calculate tip chord from the root instead:

```scheme
(derived_tip_chord (* root_chord taper_ratio))
(tip_thickness (* derived_tip_chord 0.10))
```

These are `let*` bindings, not a complete program. Replace the tip profile's chord references with the derived name. A ratio of 0.6 gives a 26.4 mm tip for a 44 mm root.

## Compare the tapered model {#wing-solution}

[Download taper example](../projects/04-parametric-wing/solution-taper-twist.ecky)

Change `taper_ratio` from 0.6 to 0.45. The derived tip chord becomes 19.8 mm. This older example still declares `tip_chord` for a parameter relation; that control no longer drives the tip geometry. Follow the derived binding when checking the result.

## Repeat one blade {#propeller-worked}

[Open the repeated-blade study](../projects/05-propeller-study/01-worked-stations.ecky). Find `repeat-union`: it builds copies from one blade expression. The angular spacing is `360 / blade_count`, so three blades are 120 degrees apart and four are 90 degrees apart.

Change `blade_count` from 3 to 4. Count the blades and inspect where each meets the hub. The repeated blades must overlap the hub to form a connected solid.

## Add a different hub {#hub-variant}

[Open the hub exercise](../projects/05-propeller-study/02-starter-hub-variant.ecky). Keep the blade expression intact. The change belongs in the hub: a split, clamp ears, and bolt bore are selected when `hub_type` is `"split_bolt"`.

Use named dimensions for the bore clearance and split gap so you can adjust them independently.

## Compare the hub branches {#propeller-solution}

[Download completed hub example](../projects/05-propeller-study/03-solution-propeller.ecky)

Compare the two hub expressions and the `if` that selects them. Check each branch separately. A successful render establishes the modeled shape; it does not establish balance, allowable speed, or strength.
