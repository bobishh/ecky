# Worm Farm: A CAD Adventure

Build a little farm for a bendy worm: mushroom house, rake, tractor, garden, and fence. Start with Bézier handles and round shapes, then learn cavities, placements, repetition, reusable components, and verification. Each chapter adds something the farm needs and reuses ideas you already met.

Your finish line: change the farm's dimensions deliberately, predict the result, explain a failed design, and export a checked model of your own. Six complete examples and actual CAD renders travel with the lessons. Read in order; the language reference is a dictionary to visit when needed, not homework to memorise first.

## Chapter 1: A worm with a wiggle

Welcome to **Worm Farm**. Our farmer has no arms, owns a suspiciously large rake, and considers a mushroom a perfectly respectable address. Your job is to build the farm, one useful idea at a time.

By the last chapter you will have a bendy worm, a hollow mushroom house with a removable roof, a rake, a tractor with separate wheels, and a garden fence. More importantly, you will be able to predict what a parameter changes, explain why geometry failed, and export a checked design of your own.

You need Ecky with its native geometry backend. Open a new project, put the complete source below in its code editor, and render. Manual modeling needs no AI connection. A millimetre, written `mm`, is a unit of length; ten of them make a centimetre. These examples are small desk models.

### Meet the farmer

![A continuous S-shaped Bézier worm with a round head and two raised eyes](/docs/assets/worm-farm/01-worm.png)

The worm is a tube following a curved backbone. Its head and eyes are spheres—round solids. A `sphere` takes a **radius**, the distance from its middle to its skin. `(sphere 3mm)` makes an eye 6 mm across.

A `translate` moves a shape. Its three distances are X, Y, Z: across, along, up. `(translate 52mm 8mm 14mm (sphere 3mm))` puts the eye's **centre** at that address. It does not move the eye's bottom to Z = 14. Its bottom is at 11.

`union` joins overlapping solids. The eyes overlap the head, and the head overlaps the tube. Move an eye far away and it becomes a loose eyeball. Our farmer is dramatic enough already; put it back.

### Make a backbone

A Bézier curve has handles. They pull on its direction and bend; the curve usually does **not** pass through the handles. Think of bending a soft garden hose by steering the ends, rather than sticking pins through every point.

This backbone has two cubic curves joined together. A cubic uses four control points. The second curve shares the first curve's last point, so the list has **seven points**, not eight. Point 1 is the tail; point 4 joins the bends; point 7 meets the head. Points 2, 3, 5, 6 steer the bends.

All backbone points have Z = 6. Its radius is also 6, so the tube reaches the ground at Z = 0. The `bend` control changes the Y positions of the handles. It leaves the tail, middle join, and head connection fixed.

You do not need to derive the curve equation today. Identify which numbers are handles and which are endpoints. That is enough to direct your first creature.

### Give the backbone a body

A `circle` is a flat outline. `sweep` carries that outline along the backbone to make a solid tube. The tail sphere rounds off the tube's otherwise flat end. Its centre sits 2 mm behind the backbone start, leaving a clear overlap instead of matching the tube's end rim exactly. Read the short source from inside outward: circle + curve → tube; tube + head + eyes → worm.

```scheme
(model
  (params
    (number bend 24mm :label "Wiggle" :min 12 :max 32 :step 2))

  (part worm
    (let* ((spine
             (bezier-path
               (list
                 (list 0 0 6)
                 (list 0 bend 6)
                 (list 28 bend 6)
                 (list 28 0 6)
                 (list 28 (- bend) 6)
                 (list 56 (- bend) 6)
                 (list 56 0 6))))
           (body (sweep (circle 6mm) spine))
           (tail (translate 0 -2mm 6mm (sphere 6mm)))
           (head (translate 56mm 3mm 8mm (sphere 8mm)))
           (left-eye (translate 52mm 8mm 14mm (sphere 3mm)))
           (right-eye (translate 60mm 8mm 14mm (sphere 3mm))))
      (union body tail head left-eye right-eye))))
```

[Download the complete worm](/docs/examples/4189afc54271-01-worm.ecky)

`model` encloses the whole design. `params` declares the changeable control. `part worm` names the exported object. `let*` names the backbone `spine`, then uses that name to build `body`. Later bindings can use earlier ones. Parentheses group an instruction with its inputs; they are the recipe's bowls, not decoration.

Native Ecky currently samples each cubic Bézier into 16 path segments. The preview is an approximation to the intended curve; slight bands in the render are expected. Tight bends can make a swept body intersect itself. We will keep this control in a tested range instead of promising every imaginable wiggle works.

### Bend, predict, inspect

1. Predict what `bend = 16` changes. The handles move closer to Y = 0; the body should become less wiggly.
2. Render that value. Check that both eyes still touch the head.
3. Try `bend = 30`. Compare the bends, then return to 24.
4. Move one eye's X coordinate from 52 to 90. Find the detached eye, then restore 52.

**You can now:** read a shape's position, distinguish radius from diameter, steer Bézier handles, and explain how a sweep makes a solid. Keep the worm file: the house is next.

Extra practice: [the connected bracket](/docs/examples/8060143996ad-02-connected-solution.ecky) teaches the same move-and-join idea with flat faces. Its back flange must overlap its foot; a union cannot invent material across a gap.

## Chapter 2: A mushroom with a front door

The **Worm Farm** farmer has found a mushroom. Unfortunately it is entirely solid inside. Very good mushroom; very bad house.

Last chapter we **added** material with `union`. This chapter removes it with `difference`. We also learn to keep a roof separate, so our farmer can rearrange the furniture without eating the ceiling.

### Start with the outside

![A hollow mushroom house with a round doorway and a wide removable conical roof](/docs/assets/worm-farm/02-mushroom-house.png)

A `cylinder` takes radius and height, in that order. `(cylinder 18mm 28mm)` is a round house 36 mm across and 28 mm tall. Its default bottom is Z = 0, and its X/Y centre is the origin. A `cone` takes bottom radius, top radius, height. The roof narrows from radius 26 to radius 2 over 18 mm.

Current round primitives take dimensions only. An extra “number of sides” is rejected. For a hexagonal shape, use `(extrude (regular-polygon 6 radius) height)`; we will try that in the garden chapter. A cylinder is round on purpose.

### Scoop out a room

A cutter is a shape you subtract. It does not become another exported object. Put the big cylinder first in `difference`, then the smaller room and the doorway.

Our `wall` control is 2 mm. The room radius is `18 - 2 = 16`; its diameter is 32. Its bottom starts at Z = 2, leaving a 2 mm floor. The room is 27 mm tall, so it ends at Z = 29—one millimetre above the outside. That overshoot opens the top cleanly.

The doorway cutter is another cylinder, turned sideways with `rotate`. Angles such as `90deg` are degrees. A cylinder normally points up Z; rotation around X turns its axis along Y. The cutter reaches the front wall and ends inside the room. It does not drill a second door through the back.

### Keep the roof movable

`part house` and `part roof` are separate pieces. Joining them would give our worm a permanently sealed mushroom. Ordinary `translate` affects the exported position; here it raises the roof to the house's top at Z = 28.

The short plug under the roof fits into the room. Its radius is `18 - wall - roof_clearance`. At the defaults that is 15.7 mm, against the room's 16 mm radius: 0.3 mm of space **per side**, or 0.6 mm across the diameter.

Clearance means room between mating surfaces. It is not a promise that this printer and filament will fit perfectly. Print a short ring and plug before spending time on the complete house.

### Build the house

```scheme
(model
  (params
    (number wall 2mm :label "House wall" :min 1 :max 3 :step 0.5)
    (number roof_clearance 0.3mm :label "Roof clearance" :min 0.1 :max 0.8 :step 0.1))

  (part house
    (let* ((outside (cylinder 18mm 28mm))
           (room
             (translate 0 0 2mm
               (cylinder (- 18mm wall) 27mm)))
           (doorway
             (translate 0 -16mm 8mm
               (rotate 90deg 0 0
                 (cylinder 7mm 18mm :align '(center center center))))))
      (difference outside room doorway)))

  (part roof
    (let* ((mushroom-cap (cone 26mm 2mm 18mm))
           (roof-plug
             (translate 0 0 -2mm
               (cylinder (- 18mm wall roof_clearance) 2mm))))
      (translate 0 0 28mm
        (union mushroom-cap roof-plug)))))
```

[Download the mushroom house and roof](/docs/examples/632368ba474b-02-mushroom-house.ecky)

### Invite a guest, not a wall

1. Predict `wall = 3`: the outer size stays fixed; the room and roof plug get smaller.
2. Render, then inspect the doorway and floor. The cutter should cross the wall completely.
3. Change `roof_clearance` from 0.3 to 0.5. Only the plug radius changes; the room does not.
4. For a deliberate failure of design, move the room cutter upward to Z = 8. Rendering can still succeed, but the floor becomes 8 mm thick. A valid solid can still be a bad house.

**You can now:** subtract a cavity, turn a cutter, preserve a floor, and derive a mating dimension from a named clearance. Save both parts for the farm.

Extra practice: [a bottle-frame clamp](/docs/examples/3b6762262a35-01-frame-clamp.ecky) subtracts a bore from a cylinder; [the peaked dovetail](/docs/examples/810480253332-solution-peaked-roof.ecky) adds a more demanding sliding fit. Work on small samples before a whole mount.

## Chapter 3: A rake for a farmer with no hands

The **Worm Farm** needs tools. Our farmer ordered a rake, then remembered the whole “no hands” situation. Build it anyway. Perhaps the tractor will help.

You already know cylinders, rotations, translations, and joins. We will arrange those familiar shapes and let a **repeat** do the boring copying.

### Plan a tool before copying it

![A rake with a long round handle, a horizontal crossbar, and five short tines](/docs/assets/worm-farm/03-rake.png)

The handle is a cylinder of radius 2 and length 44, rotated -90 degrees around X. It runs toward positive Y. The crossbar is another cylinder, rotated around Y to run along X. The teeth run toward negative Y. Their roots overlap the crossbar, so `union` can make one tool.

All their centres sit at Z = 2. The thicker handle and bar reach Z = 0; thinner teeth sit slightly above the ground. This is a desk model laid flat, not a steel tool tested for digging.

### Ask for five teeth

`repeat-union` makes copies and joins them. The compact form `(repeat-union i tooth_count geometry)` gives `i` the values 0 through `tooth_count - 1`. With five teeth, that is 0, 1, 2, 3, 4.

The tooth X position is `(i - (tooth_count - 1) / 2) × 4mm`. For five teeth the positions are -8, -4, 0, 4, 8. The subtraction centres the whole row; multiplying sets spacing. We did not manually place five cutters or invent five controls.

Counts are bare numbers: `5`, not `5mm`. Spacing is a length. This distinction matters when an expression combines them.

### Build and count

```scheme
(model
  (params
    (number tooth_count 5 :label "Rake teeth" :min 3 :max 7 :step 1))

  (part rake
    (let* ((handle
             (translate 0 0 2mm
               (rotate -90deg 0 0
                 (cylinder 2mm 44mm))))
           (crossbar
             (translate 0 0 2mm
               (rotate 0 90deg 0
                 (cylinder 2mm 32mm :align '(center center center)))))
           (teeth
             (repeat-union i tooth_count
               (translate
                 (* (- i (/ (- tooth_count 1) 2)) 4mm)
                 0 2mm
                 (rotate 90deg 0 0
                   (cylinder 1mm 12mm))))))
      (union handle crossbar teeth))))
```

[Download the rake](/docs/examples/7653d498d626-03-rake.ecky)

### Catch a copying mistake

1. Predict the positions for seven teeth: -12, -8, -4, 0, 4, 8, 12 mm.
2. Set `tooth_count` to 7, render, and count the teeth.
3. Temporarily replace the tooth's Y position `0` with `-5mm`. The roots move away from the crossbar and leave disconnected geometry. Rendering alone does not prove the tool is one piece.
4. Restore 0 and five teeth. Now alter one radius in the tooth expression: all teeth change together.

Changing a count can also outgrow its support. The crossbar spans -16 to 16 mm; the offered maximum of seven teeth keeps their roots within it. If you extend the range later, derive the bar length too.

### A different kind of stretching

For shapes that change outline along their length, copying is not enough. A `loft` joins profiles, rather like pulling a skin between two differently shaped hoops. Our garden carrot will use that idea. A sweep follows a backbone; a loft follows a sequence of cross-sections.

The [wing study](/docs/examples/2ba6d04fd279-worked-stations.ecky) starts with two four-point outlines. The [taper-and-twist version](/docs/examples/feb4c349e669-solution-taper-twist.ecky) derives its tip from `root_chord × taper_ratio`. Set the ratio to 0.45: a 44 mm root gives a 19.8 mm tip. No unused independent tip control remains.

**You can now:** rotate familiar solids into a tool, centre a repeated row, and recognise why a changing count can break a design. Keep the rake; the tractor needs wheels next.

## Chapter 4: The smallest tractor traffic jam

The **Worm Farm** farmer has hired a tractor. It has four wheels, one chimney, and absolutely no driving licence.

We will use a box where it is useful: the chassis is a rectangular support for an engine and cabin. Round wheels come from cylinders. Unlike rake teeth, the wheels should remain **separate** from the chassis.

### Name the pieces

![A miniature tractor with four bored wheels, a box chassis, open cabin, engine and chimney](/docs/assets/worm-farm/04-tractor.png)

The chassis is one `part`: body, engine, chimney, cabin, and two axle rods joined together. The wheel group is another `part` containing four separate wheels. A part name can hold multiple disconnected solids; two part declarations do not mean two connected pieces.

`box width depth height` makes the body. Like the house cylinder, the default box sits with its bottom at Z = 0 and X/Y centred. We raise the body to `wheel_radius`. The wheel centres use that same height, so their bottoms reach the ground. Change radius and the body rises with them.

### Two loops, four wheels

One `repeat-compound` selects the side; another selects the axle. A compound groups shapes without fusing them. `side` is 0 or 1. `2 × side - 1` becomes -1 or +1, placing wheel centres at X = -17 or +17 mm. `axle` is 0 or 1, so Y is -13 or +13 mm.

Each wheel is turned around Y to point along X. The `:align '(center center center)` option centres its height along its axis. Otherwise its starting end, rather than its middle, would be placed at X = ±17.

The axle radius is 2 mm. The wheel bore adds `axle_clearance`, so a 0.3 mm radial clearance gives a 2.3 mm bore radius. The bore cutter is 8 mm long, longer than the 6 mm wheel, so it cuts through both sides. You learned that overshoot trick in the mushroom house.

### Build the tractor

```scheme
(model
  (params
    (number wheel_radius 9mm :label "Wheel radius" :min 7 :max 12 :step 1)
    (number axle_clearance 0.3mm :label "Axle clearance" :min 0.1 :max 0.8 :step 0.1))

  (part chassis
    (let* ((body
             (translate 0 0 wheel_radius
               (box 26mm 42mm 6mm)))
           (axles
             (repeat-union axle 2
               (translate 0 (+ -13mm (* axle 26mm)) wheel_radius
                 (rotate 0 90deg 0
                   (cylinder 2mm 40mm :align '(center center center))))))
           (engine
             (translate 0 -10mm (+ wheel_radius 6mm)
               (box 18mm 20mm 12mm)))
           (chimney
             (translate 0 -14mm (+ wheel_radius 18mm)
               (cylinder 2mm 12mm)))
           (cabin
             (translate 0 12mm (+ wheel_radius 6mm)
               (difference
                 (box 20mm 14mm 22mm)
                 (translate 0 0 4mm (box 22mm 10mm 15mm))))))
      (union body axles engine chimney cabin)))

  (part wheels
    (repeat-compound side 2
      (repeat-compound axle 2
        (let* ((x (* (- (* side 2) 1) 17mm))
               (y (+ -13mm (* axle 26mm)))
               (bore_radius (+ 2mm axle_clearance))
               (wheel
                 (rotate 0 90deg 0
                   (difference
                     (cylinder wheel_radius 6mm :align '(center center center))
                     (cylinder bore_radius 8mm :align '(center center center))))))
          (translate x y wheel_radius wheel))))))
```

[Download the tractor](/docs/examples/c22df1745993-04-tractor.ecky)

### Inspect a useful failure

1. Set `wheel_radius` from 9 to 11. Predict the ground position before rendering: wheel bottoms remain at zero; chassis rises 2 mm.
2. Follow `axle_clearance` into the bore, not the outer wheel. Increase it to 0.5. The wheels' outside size stays fixed.
3. Replace the **outer** `repeat-compound` with `repeat-union`. The wheels are too far apart to fuse into one solid. Union is not an invisible axle.
4. Restore the compound. Inspect all four bores before export.

This model has no axle end caps or retaining clips; its wheels can slide off. That is a useful next design problem after the farm, not evidence of a finished rolling toy. A printed fit also needs a small axle/bore sample and appropriate adult help with making and assembly.

Extra practice: [the hollow travel shell](/docs/examples/a686d8c34647-shell-blank.ecky) uses a rounded rectangle and extrusion; [the handle clip](/docs/examples/29e210b79c55-handle-snap.ecky) has a flexible circular fit. A closed render does not establish flexible strength.

**You can now:** separate moving pieces, place a symmetric pair, nest two repeats, and keep axle clearance distinct from wheel size. Park the tractor beside the house.

## Chapter 5: A fence, a carrot, and a reusable recipe

The **Worm Farm** has a house, tools, and transport. Now it needs a garden. The farmer requests one carrot and a fence to protect it from the tractor. Planning is improving.

Repeating a whole expression worked for rake teeth. A fence post adds a pointed cap and may need several dimensions. Give that recipe a name with `define-component`.

### Package one proven post

![A fence with six pointed round posts and two rails, beside a carrot made by lofting profiles](/docs/assets/worm-farm/05-garden.png)

A component declaration goes before `model`; its signature lists inputs and defaults. Our `fence_post` accepts `x` and `height`. It builds a cylinder plus a cone, then moves the result to X = `x`. Calling `(fence_post :x 18mm)` uses the default height; calling it with `:height 30mm` overrides that height too.

The colon marks a named input. The name `x` belongs to this component's signature; it is a scalar position, not an implicit coordinate vector.

A component's geometry sees its signature, local bindings, and allowed immutable top-level helpers. It cannot reach arbitrarily into a model's controls. Pass a needed value in the signature. This keeps the recipe usable somewhere else.

### Grow a fence from that recipe

`posts` sets a count, and the repeat calls the component at `i × 9mm`. Six posts land at 0, 9, 18, 27, 36, 45. Two box rails cross them at Z = 8 and 16. Here the rails' length **is derived** from the count, so a longer row gets longer rails too.

The rail length is `(posts - 1) × 9mm + 4mm`. The centre is half the distance from first to last post. That gives a 49 mm rail centred at X = 22.5 for six posts. The extra 4 mm covers the post radii at both ends.

### Grow a ridiculous carrot

The carrot's bottom profile is a small circle of radius 1. Its top is an explicit six-sided polygon of radius 5. `loft 18mm` joins those profiles 18 mm apart. This is a deliberately stylised carrot, not a cylinder with an ignored facet count.

You met the distinction in the wing extension: sweep follows a path; loft joins outlines. Changing the top profile changes the carrot's shape without changing its height.

### Build the garden

```scheme
(define-component fence_post ((number x 0mm) (number height 22mm))
  (translate x 0 0
    (union
      (cylinder 2mm height)
      (translate 0 0 height
        (cone 3mm 0mm 5mm)))))

(model
  (params
    (number posts 6 :label "Fence posts" :min 3 :max 9 :step 1))

  (part fence
    (let* ((post_row
             (repeat-union i posts
               (fence_post :x (* i 9mm))))
           (rail_length (+ (* (- posts 1) 9mm) 4mm))
           (rail_center (/ (* (- posts 1) 9mm) 2))
           (bottom_rail
             (translate rail_center 0 8mm
               (box rail_length 2mm 3mm)))
           (top_rail
             (translate rail_center 0 16mm
               (box rail_length 2mm 3mm))))
      (union post_row bottom_rail top_rail)))

  (part carrot
    (translate 20mm -12mm 0
      (loft 18mm
        (circle 1mm)
        (regular-polygon 6 5mm)))))
```

[Download the garden](/docs/examples/352efddca32f-05-garden.ecky)

### Reuse without hiding dependencies

1. Increase `posts` from 6 to 8. Predict rail length: `7 × 9 + 4 = 67mm`. Render and count.
2. Add `:height 28mm` to the component call. All posts grow; their caps remain 5 mm tall. The rails stay at their original heights.
3. Change the component body to use an undeclared `magic_height`. Compilation should reject the free variable. Restore `height`.
4. Keep the component definition and copy it into a new model. It should work with its declared inputs alone.

In an external-agent workflow, `component_extract`, `component_search`, and `component_get` can help preserve and retrieve a proven recipe. Inspect the returned source and include it with the model; do not teach the reader an invisible library dependency.

Extra practice: [the multipart phone case](/docs/examples/014d027bd517-iphone-17e-voronoi-case.ecky) reuses `lattice-strut` across a rear web. Its endpoint data is explicit: changing strut width does not generate a new cell layout. Read part boundaries and the component definition before tackling the whole file.

**You can now:** declare reusable geometry, pass named inputs, derive a repeated support's size, and loft a profile into a changing solid. Next chapter puts the farm together and asks it measurable questions.

## Chapter 6: Open the gates, check the farm

Welcome to the complete **Worm Farm**. The worm has moved in, the mushroom has a roof, and the tractor has only run over the carrot twice.

This chapter adds no mysterious replacement models. The complete source contains the same worm, house, rake, tractor, and garden you already built. Their familiar parts are moved into a shared layout. You can trace every piece back to an earlier chapter.

### Put the objects on one map

![The complete farm: a curved worm, mushroom house, rake, tractor, fence and carrot arranged as a small scene](/docs/assets/worm-farm/06-farm.png)

The final model has seven controls. The worm's wiggle, house wall, roof clearance, axle clearance, rake count, wheel radius, and fence count retain their original jobs. Roof and axle fits have different names: a tighter roof should not silently change the tractor's wheels.

Each `part` gets an outer `translate`. The house and roof move together by the same X/Y offset. The chassis and wheel group also share an offset. Moving only a roof or only the wheels would break an assembly relationship even though each solid still renders.

This scene has no ground slab. Z = 0 is a coordinate plane, not exported soil. The real shapes remain the deliverable.

### Peek under the roof

The authored `view` named `peek_inside` offsets the roof upward for inspection. View offsets are **preview-only**; they do not change STL or STEP geometry. A normal `translate` inside a part changes export geometry. Naming a variable `preview` does not alter this rule.

The committed illustration shows exported assembled positions, not that exploded view. Inspect the house's standalone source if a particular viewer does not offer named views.

### Ask a measurable question

`verify` writes a requirement alongside geometry. Our check asks that the exported STL has zero non-manifold edges. It does **not** demand one connected component: separate roof and wheels are intentional. Do not “fix” them by fusing the farm into one blob.

A check has a tag, a metric, and an expectation. `closed_farm` names this check; `bad_edges` names the measured number; `(= 0)` defines a passing result. Rendering and verification are separate actions: use Ecky's model verification after a successful render. A command-line `check` only compiles the source; a declared verification is not evidence that it ran.

Passing mesh checks cannot certify roof fit, tyre friction, stability, strength, or print safety. Those need their own inspections and, for a physical model, small printed samples.

### Read the complete farm

```scheme
(define-component fence_post ((number x 0mm) (number height 22mm))
  (translate x 0 0
    (union
      (cylinder 2mm height)
      (translate 0 0 height
        (cone 3mm 0mm 5mm)))))

(model
  (params
    (number bend 24mm :label "Worm wiggle" :min 12 :max 32 :step 2)
    (number wall 2mm :label "House wall" :min 1 :max 3 :step 0.5)
    (number roof_clearance 0.3mm :label "Roof clearance" :min 0.1 :max 0.8 :step 0.1)
    (number axle_clearance 0.3mm :label "Axle clearance" :min 0.1 :max 0.8 :step 0.1)
    (number tooth_count 5 :label "Rake teeth" :min 3 :max 7 :step 1)
    (number wheel_radius 9mm :label "Wheel radius" :min 7 :max 12 :step 1)
    (number posts 6 :label "Fence posts" :min 3 :max 9 :step 1))

  (part worm
    (translate -25mm -30mm 0mm
      (let* ((spine
               (bezier-path
                 (list
                   (list 0 0 6)
                   (list 0 bend 6)
                   (list 28 bend 6)
                   (list 28 0 6)
                   (list 28 (- bend) 6)
                   (list 56 (- bend) 6)
                   (list 56 0 6))))
             (body (sweep (circle 6mm) spine))
             (tail (translate 0 -2mm 6mm (sphere 6mm)))
             (head (translate 56mm 3mm 8mm (sphere 8mm)))
             (left_eye (translate 52mm 8mm 14mm (sphere 3mm)))
             (right_eye (translate 60mm 8mm 14mm (sphere 3mm))))
        (union body tail head left_eye right_eye))))

  (part house
    (translate -30mm 35mm 0mm
      (let* ((outside (cylinder 18mm 28mm))
             (room
               (translate 0 0 2mm
                 (cylinder (- 18mm wall) 27mm)))
             (doorway
               (translate 0 -16mm 8mm
                 (rotate 90deg 0 0
                   (cylinder 7mm 18mm :align '(center center center))))))
        (difference outside room doorway))))

  (part roof
    (translate -30mm 35mm 0mm
      (let* ((cap (cone 26mm 2mm 18mm))
             (plug
               (translate 0 0 -2mm
                 (cylinder (- 18mm wall roof_clearance) 2mm))))
        (translate 0 0 28mm
          (union cap plug)))))

  (part rake
    (translate 35mm 20mm 0mm
      (let* ((handle
               (translate 0 0 2mm
                 (rotate -90deg 0 0 (cylinder 2mm 44mm))))
             (crossbar
               (translate 0 0 2mm
                 (rotate 0 90deg 0
                   (cylinder 2mm 32mm :align '(center center center)))))
             (teeth
               (repeat-union i tooth_count
                 (translate
                   (* (- i (/ (- tooth_count 1) 2)) 4mm)
                   0 2mm
                   (rotate 90deg 0 0 (cylinder 1mm 12mm))))))
        (union handle crossbar teeth))))

  (part chassis
    (translate 65mm -28mm 0mm
      (let* ((body
               (translate 0 0 wheel_radius
                 (box 26mm 42mm 6mm)))
             (axles
               (repeat-union axle 2
                 (translate 0 (+ -13mm (* axle 26mm)) wheel_radius
                   (rotate 0 90deg 0
                     (cylinder 2mm 40mm :align '(center center center))))))
             (engine
               (translate 0 -10mm (+ wheel_radius 6mm)
                 (box 18mm 20mm 12mm)))
             (chimney
               (translate 0 -14mm (+ wheel_radius 18mm)
                 (cylinder 2mm 12mm)))
             (cabin
               (translate 0 12mm (+ wheel_radius 6mm)
                 (difference
                   (box 20mm 14mm 22mm)
                   (translate 0 0 4mm (box 22mm 10mm 15mm))))))
        (union body axles engine chimney cabin))))

  (part wheels
    (translate 65mm -28mm 0mm
      (repeat-compound side 2
        (repeat-compound axle 2
          (let* ((x (* (- (* side 2) 1) 17mm))
                 (y (+ -13mm (* axle 26mm)))
                 (bore_radius (+ 2mm axle_clearance))
                 (wheel
                   (rotate 0 90deg 0
                     (difference
                       (cylinder wheel_radius 6mm :align '(center center center))
                       (cylinder bore_radius 8mm :align '(center center center))))))
            (translate x y wheel_radius wheel))))))

  (part fence
    (translate -20mm 68mm 0mm
      (let* ((post_row
               (repeat-union i posts
                 (fence_post :x (* i 9mm))))
             (rail_length (+ (* (- posts 1) 9mm) 4mm))
             (rail_center (/ (* (- posts 1) 9mm) 2))
             (bottom_rail
               (translate rail_center 0 8mm
                 (box rail_length 2mm 3mm)))
             (top_rail
               (translate rail_center 0 16mm
                 (box rail_length 2mm 3mm))))
        (union post_row bottom_rail top_rail))))

  (part carrot
    (translate 10mm 38mm 0mm
      (loft 18mm
        (circle 1mm)
        (regular-polygon 6 5mm))))

  (view peek_inside (offset-part roof 0 0 24mm))

  (verify
    (tag closed_farm)
    (intent "Farm exports have closed mesh edges")
    (metric bad_edges (stl non-manifold-edge-count))
    (expect bad_edges (= 0))))
```

[Download the complete farm](/docs/examples/b907a61fb1a1-06-farm.ecky)

Start at the part names. Collapse everything you already understand, then compare each outer placement with the previous chapter's local coordinates. Long source becomes readable when its pieces have names and a history.

### Make your own farm

1. Choose one change: a wider worm bend, eight fence posts, a larger wheel, or a different house wall.
2. Write a prediction before rendering. Which dimensions should change, and which should stay fixed?
3. Render and run model verification. Inspect the roof plug, door, wheel bores, and repeated joins.
4. Export STL for a slicer. Export STEP only when the rendered artifact offers it; mesh paths may not provide STEP.
5. Reopen an export in a viewer. Count expected objects, check scale, and inspect your change. Separate pieces may need separate export or separation in the receiving tool.

From a prepared repository, the command-line equivalents begin with:

```bash
src-tauri/target/debug/ecky check farm.ecky
src-tauri/target/debug/ecky render --backend native farm.ecky --stl farm.stl --step farm.step --json
```

The CLI commands compile and render; they do not execute the authored runtime verification. Keep the verification result from the app or your test harness with the exported design.

For a more demanding mechanism, [the film scanner](/docs/examples/fbc8ca05b0bc-film-scanner.ecky) combines rails, film inserts and a threaded carrier. Inspect shared pitch and clearance before editing a helix. Ordinary source translations affect exports; only `view` offsets are preview-only.

**You can now:** assemble familiar geometry, distinguish scene placement from inspection views, state a measurable requirement, and export a design with an honest account of what was checked. Your next project can be another farm building—or a worm-sized spaceship. Start with a prediction and one new idea.
