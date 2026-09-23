---
id: mission-02-bottle-cage-dovetail
title: Fit a clamp and a sliding rail
---

# Fit a clamp and a sliding rail

This chapter uses two small pieces of a bottle mount: a clamp around a round tube and a rail that slides into a channel. Work on the fits before opening the full cage.

## Describe the clearance {#fit-first}

Clearance is extra space between mating surfaces. If a centered rail is 16 mm wide and needs 0.3 mm on each side, its channel must be 16.6 mm wide. Keep the nominal width and clearance separate so you can change the fit without redrawing the rail.

```scheme
(+ rail_width (* 2 fit_clearance))
```

This expression computes the receiving width. It needs `rail_width` and `fit_clearance` from the enclosing model.

## Cut a clamp from a cylinder {#clamp}

```scheme
(model
  (params
    (number frame_dia 30 :label "frame diameter" :min 20 :max 50 :step 1)
    (number clip_t 3 :label "clip thickness" :min 2 :max 6 :step 0.5))
  (part frame_clamp
    (let* ((frame_r (/ frame_dia 2))
           (outer_r (+ frame_r clip_t)))
      (build
        (shape outer (cylinder outer_r 24 :align '(center center center)))
        (shape bore (cylinder frame_r 28 :align '(center center center)))
        (shape opening (box (* frame_dia 0.75) (* outer_r 2) 28 :align '(center max center)))
        (result (difference outer bore opening))))))
```

[Download frame clamp](../projects/07-bottle-cage/01-frame-clamp.ecky)

At the defaults, the inner radius is 15 mm and the outer radius is 18 mm. The bore is longer than the outer cylinder, so it cuts through both ends. The box removes one side of the ring to make an opening.

Change `frame_dia` from 30 to 32. The inner and outer radii both grow by 1 mm; the wall stays 3 mm thick. This example has no additional tube clearance: add a named clearance if your printed test needs one.

## Inspect the channel roof {#roof}

[Open the flat-roof rail exercise](../projects/07-bottle-cage/02-supportless-rail.ecky). Find the channel's cross-section before reading the surrounding solid. A horizontal ceiling requires the printer to bridge a gap. A peaked ceiling replaces that span with two rising slopes.

The result depends on print orientation. A peaked profile alone does not establish that every overhang in the complete mount can print without support.

## Read the peaked profile {#peaked-roof}

[Open the peaked-profile example](../projects/03-supportless-dovetail/solution-peaked-roof.ecky). These bindings set its channel dimensions:

```scheme
(female_width (+ rail_width (* 2 fit_clearance)))
(female_height (+ rail_height fit_clearance))
(female_half (/ female_width 2))
(roof_rise female_half)
```

The roof rises by half the channel width, making 45-degree sides. Width adds clearance twice; height adds it once. Do not assume every use of clearance has the same multiplier.

This example also contains placement offsets. Follow the channel cutter and receiving blank through their transforms before treating the preview as an assembled joint. A cutter must overlap its blank to remove material.

## Follow the fit into the cage {#full-cage}

[Download full cage example](../projects/07-bottle-cage/04-full-cage.ecky)

Find the clamp, male rail, and female channel. Compare their dimensions with the small examples. Then inspect how each is rotated and translated into the mount. The cross-section controls fit; the transforms control where the parts meet.

Print a short rail and channel first. They should slide without forcing and without excessive play. The full mount still needs a physical retention test with its intended bottle and load.
