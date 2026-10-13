---
id: worm-farm-03-rake
title: Chapter 3: A rake for a farmer with no hands
---

# Chapter 3: A rake for a farmer with no hands

The **Worm Farm** needs tools. Our farmer ordered a rake, then remembered the whole “no hands” situation. Build it anyway. Perhaps the tractor will help.

You already know cylinders, rotations, translations, and joins. We will arrange those familiar shapes and let a **repeat** do the boring copying.

## Plan a tool before copying it {#rake-plan}

![A rake with a long round handle, a horizontal crossbar, and five short tines](../assets/worm-farm/03-rake.png)

The handle is a cylinder of radius 2 and length 44, rotated -90 degrees around X. It runs toward positive Y. The crossbar is another cylinder, rotated around Y to run along X. The teeth run toward negative Y. Their roots overlap the crossbar, so `union` can make one tool.

All their centres sit at Z = 2. The thicker handle and bar reach Z = 0; thinner teeth sit slightly above the ground. This is a desk model laid flat, not a steel tool tested for digging.

## Ask for five teeth {#rake-repeat}

`repeat-union` makes copies and joins them. The compact form `(repeat-union i tooth_count geometry)` gives `i` the values 0 through `tooth_count - 1`. With five teeth, that is 0, 1, 2, 3, 4.

The tooth X position is `(i - (tooth_count - 1) / 2) × 4mm`. For five teeth the positions are -8, -4, 0, 4, 8. The subtraction centres the whole row; multiplying sets spacing. We did not manually place five cutters or invent five controls.

Counts are bare numbers: `5`, not `5mm`. Spacing is a length. This distinction matters when an expression combines them.

## Build and count {#rake-source}

<!-- example: ../examples/worm-farm/03-rake.ecky -->

[Download the rake](../examples/worm-farm/03-rake.ecky)

## Catch a copying mistake {#rake-experiment}

1. Predict the positions for seven teeth: -12, -8, -4, 0, 4, 8, 12 mm.
2. Set `tooth_count` to 7, render, and count the teeth.
3. Temporarily replace the tooth's Y position `0` with `-5mm`. The roots move away from the crossbar and leave disconnected geometry. Rendering alone does not prove the tool is one piece.
4. Restore 0 and five teeth. Now alter one radius in the tooth expression: all teeth change together.

Changing a count can also outgrow its support. The crossbar spans -16 to 16 mm; the offered maximum of seven teeth keeps their roots within it. If you extend the range later, derive the bar length too.

## A different kind of stretching {#rake-loft}

For shapes that change outline along their length, copying is not enough. A `loft` joins profiles, rather like pulling a skin between two differently shaped hoops. Our garden carrot will use that idea. A sweep follows a backbone; a loft follows a sequence of cross-sections.

The [wing study](../projects/04-parametric-wing/worked-stations.ecky) starts with two four-point outlines. The [taper-and-twist version](../projects/04-parametric-wing/solution-taper-twist.ecky) derives its tip from `root_chord × taper_ratio`. Set the ratio to 0.45: a 44 mm root gives a 19.8 mm tip. No unused independent tip control remains.

**You can now:** rotate familiar solids into a tool, centre a repeated row, and recognise why a changing count can break a design. Keep the rake; the tractor needs wheels next.
