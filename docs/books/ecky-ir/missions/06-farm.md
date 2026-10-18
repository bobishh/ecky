---
id: worm-farm-06-farm
title: Chapter 6: Open the gates, check the farm
---

# Chapter 6: Open the gates, check the farm

Welcome to the complete **Worm Farm**: rake, mushroom house, garden, snap-together tractor, and articulated farmer. Every object uses the same geometry as its own chapter. We move those familiar pieces onto one map.

## Arrange the familiar pieces {#farm-layout}

![The articulated worm in front of a mushroom house, rake, assembled tractor, fence and carrot](../assets/worm-farm/06-farm.png)

The complete source is assembled from the chapter files when the book is built. Downloads are self-contained: you do not need a hidden component library. A source check catches an old tractor or rigid worm accidentally left in the farm.

Each `part` gains an outer `translate`. All tractor pieces share one placement; both house pieces share another. Their local dimensions and fits stay the same. The worm is longer than the rake because it has nine links at a 23 mm pitch.

The farm starts in assembled inspection mode: `print_layout` and `house_print_layout` are off. For printing, start with the individual chapter downloads and the two small joint samples. The scene is not a promise that every posed piece is resting on the bed. There is no exported ground slab.

## Peek under the roof {#farm-view}

The named `view` called `peek_inside` lifts the roof for inspection. A view offset is **preview-only**: STL and STEP keep their normal geometry. By comparison, `translate` inside a `part` and the kit's print-layout switches change exports.

The illustration shows the assembled exported positions. A preview effect must never quietly become extra material in the printed object.

Reference: [view](https://ecky-cad.com/docs/complete-compiler-surface/#view).

## Count what must stay separate {#farm-verify}

Before pressing Verify, predict the result:

| Object | Connected pieces |
| --- | --- |
| Rake | 1 |
| House and roof | 2 |
| Fence and carrot | 2 |
| Tractor kit | 11 |
| Worm | 9 |
| Total | 25 |

The farm has twelve named part groups, but **25 connected solids**. One group can contain several wheels or links. Names and connected pieces are different counts.

The source checks two things: zero non-manifold mesh edges, and 25 pieces. A broken rivet, detached decoration, or accidentally fused worm link should change that count. Do not “repair” moving parts by joining everything together.

`verify` writes the question. Rendering builds the geometry. Running model verification measures it. A declared check is not evidence that it ran. Closed meshes and a correct count still do not prove printed fit, wheel friction, or joint strength; keep the results from your small samples too.

## Open the complete farm {#farm-source}

Start at the part names and their outer placements. The longer component recipes are the same ones you have already inspected.

<!-- example: ../examples/worm-farm/06-farm.ecky -->

[Download the complete farm](../examples/worm-farm/06-farm.ecky)

## Change one thing and explain it {#farm-finish}

1. Choose a change: seven rake teeth, eight fence posts, larger wheels, or a wider joint gap.
2. Predict what should change and what should stay fixed.
3. Render, inspect the changed piece, and run verification. Still expect 25 pieces unless you deliberately changed their number.
4. Open the relevant chapter's print layout and export STL. In your slicer, separate shells when needed; check scale, orientation, bridges, and small gaps. Export STEP only when the rendered artifact offers it.
5. Print a fit sample before a whole mechanism. Keep the measured setting with your model.

From a prepared repository, the CLI can compile and render the farm:

```bash
src-tauri/target/debug/ecky check farm.ecky
src-tauri/target/debug/ecky render --backend native farm.ecky --stl farm.stl --step farm.step --json
```

These commands do not execute authored runtime verification. Run that separately in the app or a verification harness.

**You can now:** assemble familiar parts, distinguish print placement from inspection, count moving pieces, and explain what your checks actually establish. For the next project, start with one new idea and one small sample.
