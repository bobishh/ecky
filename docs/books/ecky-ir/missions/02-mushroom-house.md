---
id: worm-farm-02-mushroom-house
title: Chapter 2: A mushroom with a front door
---

# Chapter 2: A mushroom with a front door

The **Worm Farm** farmer has found a mushroom. Unfortunately it is solid inside. Very good mushroom; very bad house.

Last chapter we added material with `union`. Now we remove a room and doorway with `difference`, then make a separate roof that lifts off.

## Start with the outside {#house-outside}

![A hollow mushroom house with a round doorway and its hollow conical roof resting beside it](../assets/worm-farm/02-mushroom-house.png)

`(cylinder 18mm 28mm)` makes a round house 36 mm across and 28 mm tall. Remember: the first number is a radius, so double it to get the width. Its bottom sits at Z = 0.

For the roof, `(cone 26mm 2mm 26mm)` narrows from a wide bottom to a small top over 26 mm of height. Both round shapes take dimensions, not a “number of sides.”

Reference: [cylinder](https://ecky-cad.com/docs/primitive-signatures/#cylinder), [cone](https://ecky-cad.com/docs/primitive-signatures/#cone).

## Scoop out a room {#house-cavity}

Imagine pressing a smaller cylinder into the big one, then removing everything it touches. That smaller shape is a **cutter**. Put the house first and its cutters after it:

```scheme
(difference outside room doorway)
```

`wall` starts at 2 mm. The room radius is `18 - wall`, or 16 mm. Its bottom starts 2 mm above the bed, leaving a floor. The room cutter ends at Z = 29, above the 28 mm house, so the top opens completely.

The doorway cutter is a cylinder turned sideways, just as we turned the rake's bar. It cuts the front wall and reaches the room. It stops before the back wall, so we get one door.

A cutter disappears after subtraction. Do not export it as another `part`, or you would fill the room again.

Reference: [difference](https://ecky-cad.com/docs/boolean-and-transform-signatures/#difference).

## Make a roof that lifts off {#house-roof}

`part house` and `part roof` keep the two pieces separate. The roof is hollow too: a short round opening makes its rim, and a cone scoops out the space above it. Its sloped inner surface rises at 45 degrees. Its wide, flat rim rests on the bed for printing.

The roof fits **over the outside** of the house. Its opening radius is `18 + roof_clearance`, or 18.3 mm. That leaves 0.3 mm per side around the 18 mm wall. Changing `wall` changes the room; changing `roof_clearance` changes this fit.

`house_print_layout` starts on, placing the roof beside the house on the bed. Turn it off to inspect the roof over the house. This changes export positions too; return to print layout before exporting the two pieces. The assembled placement shows the nominal skirt position; the sloped inside seats on the rim when printed.

## Build the house {#house-source}

<!-- example: ../examples/worm-farm/02-mushroom-house.ecky -->

[Download the mushroom house and roof](../examples/worm-farm/02-mushroom-house.ecky)

## Try, predict, inspect {#house-experiment}

1. Set `wall` to 3. Predict: the outside stays the same, but the room gets smaller. The roof opening stays the same.
2. Look through the doorway. Find the floor and the open top.
3. Increase `roof_clearance` to 0.5. Predict: more room around the wall, with the house unchanged.
4. Make a deliberate mistake: move the room cutter's bottom from Z = 2 to Z = 8. The model can still render, but the floor becomes much thicker. Restore 2 mm.

Run verification and expect two pieces. Print a small rim sample before choosing a final roof fit; a numerical gap does not guarantee your printer's fit.

**You can now:** remove a cavity, cut a doorway, preserve a floor, and leave a named gap between separate pieces. Next, repeat a reusable fence post.
