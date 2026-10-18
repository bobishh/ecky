# Worm Farm: A CAD Adventure

Build a little farm for an articulated worm. Start with a rake: a handle, a bar, a row of downward teeth. Hollow out a mushroom house, grow a fence, assemble a tractor with spinning wheels and snap-on rivets, then build a captive ball joint that lets the worm really bend. Finally bring the farm together.

Each chapter introduces one new idea, asks you to predict a change, and gives you a complete model to open. You can point at a part before learning its code name. Small fit samples come before whole mechanisms. Illustrations show the actual CAD geometry.

Read in order. Use the language reference as a dictionary when a word is unfamiliar. A model that renders is a starting point: inspect its parts, run its checks, and try a small printed sample before making the whole toy. Ask an adult to help with printing and small parts.

## Chapter 1: A rake with teeth pointing down

Our **Worm Farm** needs a rake. Start with three things you can point to: a handle, a bar, and five teeth. The teeth reach the ground. The handle points up toward the farmer.

Your job: make one tooth, make a row, then change how many teeth the rake has.

### Find up, across, and forward {#rake-directions}

![A small rake standing on five downward teeth with its handle leaning upward](/docs/assets/worm-farm/3d65a054c2b3-03-rake.png)

Think of the printer bed as a table. X goes across it, Y goes away from you, Z goes up. A cylinder starts on that table and grows along Z.

Read `(cylinder 1.3mm 12mm)` as “a round tooth, radius 1.3, height 12.” Radius is half the width. The flat tip sits at Z = 0; its other end reaches the bar at Z = 12. The tooth does not need rotating.

Reference: [cylinder](https://ecky-cad.com/docs/primitive-signatures/#cylinder).

### Make the bar and handle {#rake-handle}

The bar crosses X. We turn a cylinder a quarter turn around Y: `rotate 0 90deg 0`. Its length is centred with `:align '(center center center)`, then `translate` lifts it to the tops of the teeth.

The handle starts at the bar too. A smaller turn, `rotate -35deg 0 0`, makes it lean upward and backward. `35deg` means 35 degrees. Imagine holding the real rake: the handle must rise away from the ground while the teeth stay down.

`union` joins overlapping shapes. The tops of the teeth enter the round bar, and the handle enters its middle. Touching in a picture is insufficient; these pieces need shared material.

Reference: [rotate](https://ecky-cad.com/docs/boolean-and-transform-signatures/#rotate), [union](https://ecky-cad.com/docs/boolean-and-transform-signatures/#union).

### Turn one tooth into five {#rake-repeat}

`repeat-union i tooth_count` makes a row and joins it. For five teeth, `i` takes the values 0, 1, 2, 3, 4. We subtract 2 to centre them, then multiply by the 4 mm spacing:

| Tooth | X position |
| --- | --- |
| First | −8 mm |
| Second | −4 mm |
| Middle | 0 mm |
| Fourth | 4 mm |
| Last | 8 mm |

`let*` gives dimensions names. Read `tooth-length` as a labelled ruler. Later expressions can use rulers named above them. `head-length` follows the number of teeth, so its ends stay near the outer teeth.

You can change `tooth_count` in the controls instead of copying and pasting another tooth.

### Build the rake {#rake-source}

```scheme
(model
  (params
    (number tooth_count 5 :label "Rake teeth" :min 3 :max 7 :step 1))
  (part rake
    (let* ((tooth-length 12mm)
           (tooth-radius 1.3mm)
           (head-height tooth-length)
           (head-radius 2mm)
           (tooth-spacing 4mm)
           (head-length (+ (* (- tooth_count 1) tooth-spacing) (* head-radius 2)))
           (handle (translate 0 0 head-height
             (rotate -35deg 0 0 (cylinder 2mm 44mm))))
           (crossbar (translate 0 0 head-height
             (rotate 0 90deg 0
               (cylinder head-radius head-length :align '(center center center)))))
           (teeth (repeat-union i tooth_count
             (translate (* (- i (/ (- tooth_count 1) 2)) tooth-spacing) 0 0
               (cylinder tooth-radius tooth-length)))))
      (union handle crossbar teeth)))
  (verify
    (tag connected-rake)
    (metric pieces (stl connected-component-count))
    (expect pieces (= 1))))
```

[Download the rake](/docs/examples/90811a79b403-03-rake.ecky)

### Try, predict, inspect {#rake-experiment}

1. Open the complete example. Count five teeth. Find their flat tips at Z = 0.
2. Set `tooth_count` to 7. Predict: two more teeth and a longer bar. The handle stays in the middle.
3. Turn the view sideways. The teeth point toward the bed; the handle rises away from it.
4. As a deliberate mistake, change only `head-height` to `20mm`. The teeth no longer reach the bar. Rendering may work, but the connected-piece check should fail. Restore `tooth-length`.

`verify` at the bottom asks for one connected piece. Run model verification after rendering. It catches a detached handle or row of teeth; it cannot decide whether the rake looks good.

**You can now:** read dimensions, turn and move a shape, repeat a tooth, and check that joins actually join. Next, give the farmer a hollow house.

## Chapter 2: A mushroom with a front door

The **Worm Farm** farmer has found a mushroom. Unfortunately it is solid inside. Very good mushroom; very bad house.

Last chapter we added material with `union`. Now we remove a room and doorway with `difference`, then make a separate roof that lifts off.

### Start with the outside {#house-outside}

![A hollow mushroom house with a round doorway and its hollow conical roof resting beside it](/docs/assets/worm-farm/22259b611ccf-02-mushroom-house.png)

`(cylinder 18mm 28mm)` makes a round house 36 mm across and 28 mm tall. Remember: the first number is a radius, so double it to get the width. Its bottom sits at Z = 0.

For the roof, `(cone 26mm 2mm 26mm)` narrows from a wide bottom to a small top over 26 mm of height. Both round shapes take dimensions, not a “number of sides.”

Reference: [cylinder](https://ecky-cad.com/docs/primitive-signatures/#cylinder), [cone](https://ecky-cad.com/docs/primitive-signatures/#cone).

### Scoop out a room {#house-cavity}

Imagine pressing a smaller cylinder into the big one, then removing everything it touches. That smaller shape is a **cutter**. Put the house first and its cutters after it:

```scheme
(difference outside room doorway)
```

`wall` starts at 2 mm. The room radius is `18 - wall`, or 16 mm. Its bottom starts 2 mm above the bed, leaving a floor. The room cutter ends at Z = 29, above the 28 mm house, so the top opens completely.

The doorway cutter is a cylinder turned sideways, just as we turned the rake's bar. It cuts the front wall and reaches the room. It stops before the back wall, so we get one door.

A cutter disappears after subtraction. Do not export it as another `part`, or you would fill the room again.

Reference: [difference](https://ecky-cad.com/docs/boolean-and-transform-signatures/#difference).

### Make a roof that lifts off {#house-roof}

`part house` and `part roof` keep the two pieces separate. The roof is hollow too: a short round opening makes its rim, and a cone scoops out the space above it. Its sloped inner surface rises at 45 degrees. Its wide, flat rim rests on the bed for printing.

The roof fits **over the outside** of the house. Its opening radius is `18 + roof_clearance`, or 18.3 mm. That leaves 0.3 mm per side around the 18 mm wall. Changing `wall` changes the room; changing `roof_clearance` changes this fit.

`house_print_layout` starts on, placing the roof beside the house on the bed. Turn it off to inspect the roof over the house. This changes export positions too; return to print layout before exporting the two pieces. The assembled placement shows the nominal skirt position; the sloped inside seats on the rim when printed.

### Build the house {#house-source}

```scheme
(model
  (params
    (number wall 2mm :label "House wall" :min 1.5 :max 3 :step 0.5)
    (number roof_clearance 0.3mm :label "Roof clearance" :min 0.2 :max 0.6 :step 0.1)
    (toggle house_print_layout true :label "House print layout (changes export)"))
  (part house
    (let* ((outside (cylinder 18mm 28mm))
           (room (translate 0 0 2mm (cylinder (- 18mm wall) 27mm)))
           (doorway (translate 0 -16mm 9mm
             (rotate 90deg 0 0 (cylinder 8.5mm 18mm :align '(center center center))))))
      (difference outside room doorway)))
  (part roof
    (let* ((rim-radius (+ 18mm roof_clearance))
           (skirt-height 3mm)
           (cap (cone 26mm 2mm 26mm))
           (rim-opening (translate 0 0 -1mm (cylinder rim-radius (+ skirt-height 1mm))))
           (inside (translate 0 0 skirt-height (cone rim-radius 0mm rim-radius))))
      (translate (if house_print_layout 52mm 0mm) 0 (if house_print_layout 0mm 25mm)
        (difference cap rim-opening inside))))
  (verify
    (tag separate-house-and-roof)
    (metric pieces (stl connected-component-count))
    (expect pieces (= 2))))
```

[Download the mushroom house and roof](/docs/examples/f7b8ba79e2f2-02-mushroom-house.ecky)

### Try, predict, inspect {#house-experiment}

1. Set `wall` to 3. Predict: the outside stays the same, but the room gets smaller. The roof opening stays the same.
2. Look through the doorway. Find the floor and the open top.
3. Increase `roof_clearance` to 0.5. Predict: more room around the wall, with the house unchanged.
4. Make a deliberate mistake: move the room cutter's bottom from Z = 2 to Z = 8. The model can still render, but the floor becomes much thicker. Restore 2 mm.

Run verification and expect two pieces. Print a small rim sample before choosing a final roof fit; a numerical gap does not guarantee your printer's fit.

**You can now:** remove a cavity, cut a doorway, preserve a floor, and leave a named gap between separate pieces. Next, repeat a reusable fence post.

## Chapter 3: A fence, a carrot, and a reusable recipe

The **Worm Farm** has a house and a rake. Now it needs a garden. The farmer requests one carrot and a fence to protect it from the tractor we will build next. Planning is improving.

Repeating a whole expression worked for rake teeth. A fence post adds a pointed cap and may need several dimensions. Give that recipe a name with `define-component`.

### Package one proven post {#garden-component}

![A fence with six pointed round posts and two rails, beside a carrot made by lofting profiles](/docs/assets/worm-farm/42504388ed53-05-garden.png)

A component declaration goes before `model`; its signature lists inputs and defaults. Our `fence_post` accepts `x` and `height`. It builds a cylinder plus a cone, then moves the result to X = `x`. Calling `(fence_post :x 18mm)` uses the default height; calling it with `:height 30mm` overrides that height too.

The colon marks a named input. The name `x` belongs to this component's signature; it is a scalar position, not an implicit coordinate vector.

A component's geometry sees its signature, local bindings, and allowed immutable top-level helpers. It cannot reach arbitrarily into a model's controls. Pass a needed value in the signature. This keeps the recipe usable somewhere else.

### Grow a fence from that recipe {#garden-repeat}

`posts` sets a count, and the repeat calls the component at `i × 9mm`. Six posts land at 0, 9, 18, 27, 36, 45. Two box rails cross them at Z = 8 and 16. Here the rails' length **is derived** from the count, so a longer row gets longer rails too.

The rail length is `(posts - 1) × 9mm + 4mm`. The centre is half the distance from first to last post. That gives a 49 mm rail centred at X = 22.5 for six posts. The extra 4 mm covers the post radii at both ends.

### Grow a ridiculous carrot {#garden-loft}

The carrot's bottom profile is a small circle of radius 1. Its top is an explicit six-sided polygon of radius 5. `loft 18mm` joins those profiles 18 mm apart. This is a deliberately stylised carrot, not a cylinder with an ignored facet count.

A loft joins two outlines at different heights. Changing the top profile changes the carrot's shape without changing its height.

### Build the garden {#garden-source}

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

### Reuse without hiding dependencies {#garden-experiment}

1. Increase `posts` from 6 to 8. Predict rail length: `7 × 9 + 4 = 67mm`. Render and count.
2. Add `:height 28mm` to the component call. All posts grow; their caps remain 5 mm tall. The rails stay at their original heights.
3. Change the component body to use an undeclared `magic_height`. Compilation should reject the free variable. Restore `height`.
4. Keep the component definition and copy it into a new model. It should work with its declared inputs alone.

**You can now:** declare reusable geometry, pass named inputs, derive a repeated support's size, and loft a profile into a changing solid. Next chapter uses reusable recipes for wheels, axles, and retaining rivets.

## Chapter 4: Wheels, axles, and snap-on rivets

The **Worm Farm** tractor needs wheels that turn and stay on. A wheel glued to a body cannot roll. A wheel slipped over an open rod falls off. We will print a kit and hold each wheel with a small split rivet.

### Lay out eleven pieces {#tractor-pieces}

![The tractor kit: body, two hollow-ended axles, four wheels and four split retaining rivets on the print bed](/docs/assets/worm-farm/a07b7545790b-04-tractor.png)

Count one body, two axles, four wheels, four rivets: **eleven separate pieces**. No metal rods, glue, or bought fasteners are part of this design.

The body has two sideways holes. Slide an axle through each hole, put a wheel on either end, then push a rivet into each hollow axle end. Its wide head keeps the wheel on.

Start with [the small axle–wheel–rivet sample](/docs/examples/f48ecd0bccbd-tractor-fit-coupon.ecky). It uses the exact same component recipes as the tractor, with a shorter axle. Print these three pieces before the whole kit. Put the wheel on the axle, push the rivet in, then check: does the wheel turn, and does it stay on when pulled gently?

![The short axle, flat wheel and split rivet used to test a printed fit](/docs/assets/worm-farm/cc3f0cfc956d-tractor-fit-coupon.png)

### Leave space for the wheel {#tractor-running-fit}

The axle radius is 3.2 mm. The wheel hole radius is `axle-radius + wheel_clearance`: 3.6 mm at the default. That leaves **0.4 mm per side** for rotation.

The body is 30 mm wide. Each wheel's inner face sits 2 mm away from it. A second gap, `wheel-end-play`, leaves 0.6 mm between the wheel's outer face and the rivet head. The head holds the wheel without squeezing it against the body.

These gaps have different jobs. A wheel that rubs needs room to spin. A rivet that falls out needs a better catch. Changing one should not silently change the other.

### Let the rivet squeeze, then catch {#tractor-snap-fit}

Picture the rivet in four pieces, all printed as one shape:

1. A broad, flat head stops outside the axle.
2. A narrow stem enters its hole.
3. A tapered tip guides it in. Behind that tip is a wider barb.
4. A slot splits the stem into two flexible legs; the head remains whole.

The axle mouth radius is 1.8 mm. The barb radius adds `snap_interference`, normally 0.25 mm. The legs must squeeze inward while passing through the mouth, then open inside a wider pocket. The pocket's shoulder catches the barb. The head stays outside and retains the wheel.

The source calls the opening's depth `lip-depth`; the barb sits behind that lip after insertion. The slot stops above the head, so the rivet remains one connected piece. Never fuse the rivet to the axle to make a check pass.

The small sample decides the printed fit. If the wheel binds, increase `wheel_clearance`. If the rivet takes too much force, reduce `snap_interference`; if it slips out, inspect the barb and pocket before increasing it. CAD geometry does not establish how a particular filament will bend. Let an adult help print and assemble small pieces.

Reference: [difference](https://ecky-cad.com/docs/boolean-and-transform-signatures/#difference), [define-component](https://ecky-cad.com/docs/components/#define-component).

### Print flat, inspect assembled {#tractor-layout}

`print_layout` starts **on**. Wheels lie flat with their holes facing up, rivets rest on their heads, axles stand on their ends, and the body rests on its underside. Separate the eleven shells in your slicer if it treats the STL as one object. Inspect the cabin roof and axle bores for bridging settings.

Turn `print_layout` **off** to inspect the assembled tractor. The same component recipes move and turn into position. This switch changes exported geometry too: turn it on again before exporting the kit. It is not a preview-only `view`.

![The same tractor parts in their assembled positions, with retaining heads outside the wheels](/docs/assets/worm-farm/0cada126e8df-tractor-assembled.png)

The two repeats choose a side and an axle. Four calls make four wheels, then four more calls make the rivets. All share the named fit dimensions.

### Open the complete kit {#tractor-source}

Read the component names first: `tractor-chassis`, `tractor-axle`, `tractor-wheel`, `tractor-rivet`. You do not need to type every line. Start by changing one measured gap in the sample.

```scheme
; Print the kit flat, then fit wheels to the axles and push in split rivets.
(define-component tractor-chassis ((number axle-radius 3.2mm) (number body-clearance 0.3mm))
  (let* ((axle-height 5mm)
         (body (box 30mm 50mm 10mm))
         (engine (translate 0 -11mm 10mm (box 22mm 23mm 10mm)))
         (chimney (translate 0 -15mm 20mm (cylinder 2.5mm 12mm)))
         (cabin (translate 0 14mm 10mm
           (difference (box 24mm 18mm 23mm)
             (translate 0 0 3mm (box 26mm 12mm 16mm)))))
         (axle-bores (repeat-compound i 2
           (translate 0 (+ -15mm (* i 30mm)) axle-height
             (rotate 0 90deg 0
               (cylinder (+ axle-radius body-clearance) 34mm :align '(center center center)))))))
    (difference (union body engine chimney cabin) axle-bores)))

(define-component tractor-axle
  ((number axle-radius 3.2mm) (number axle-length 47.2mm)
   (number mouth-radius 1.8mm) (number cavity-radius 2.35mm)
   (number lip-depth 2.2mm) (number socket-depth 5.5mm))
  (difference
    (cylinder axle-radius axle-length)
    (translate 0 0 -1mm (cylinder mouth-radius (+ axle-length 2mm)))
    (translate 0 0 lip-depth (cylinder cavity-radius (- socket-depth lip-depth)))
    (translate 0 0 (- axle-length socket-depth)
      (cylinder cavity-radius (- socket-depth lip-depth)))))

(define-component tractor-wheel
  ((number radius 10mm) (number width 6mm)
   (number axle-radius 3.2mm) (number running-clearance 0.4mm))
  (difference (cylinder radius width)
    (translate 0 0 -1mm
      (cylinder (+ axle-radius running-clearance) (+ width 2mm)))))

(define-component tractor-rivet
  ((number mouth-radius 1.8mm) (number snap-interference 0.25mm)
   (number head-radius 4.8mm) (number head-thickness 1.6mm))
  (let* ((stem-radius (- mouth-radius 0.2mm))
         (barb-radius (+ mouth-radius snap-interference))
         (barb-base 4.3mm)
         (slot-bottom (+ head-thickness 0.8mm)))
    (difference
      (union
        (cylinder head-radius head-thickness)
        (translate 0 0 (- head-thickness 0.2mm) (cylinder stem-radius 3.1mm))
        (translate 0 0 barb-base (cone barb-radius 1mm 1.9mm)))
      (translate 0 0 slot-bottom (box 1.1mm 8mm 5mm)))))

(model
  (meta :title "Snap-together tractor")
  (params
    (number wheel_radius 10mm :label "Wheel radius" :min 8 :max 12 :step 1)
    (number wheel_clearance 0.4mm :label "Wheel running clearance" :min 0.2 :max 0.6 :step 0.05)
    (number snap_interference 0.25mm :label "Rivet snap interference" :min 0.1 :max 0.35 :step 0.05)
    (toggle print_layout true :label "Print layout (changes export)"))
  (let* ((axle-radius 3.2mm)
         (body-half-width 15mm)
         (wheel-width 6mm)
         (wheel-side-gap 2mm)
         (wheel-end-play 0.6mm)
         (wheel-center (+ body-half-width wheel-side-gap (/ wheel-width 2)))
         (axle-half-length (+ body-half-width wheel-side-gap wheel-width wheel-end-play))
         (axle-length (* 2 axle-half-length))
         (rivet-head-thickness 1.6mm)
         (body-lift (- wheel_radius 5mm)))
    (part chassis
      (translate 0 0 (if print_layout 0mm body-lift)
        (tractor-chassis :axle-radius axle-radius)))
    (part axles
      (repeat-compound i 2
        (let* ((axle (tractor-axle :axle-radius axle-radius :axle-length axle-length)))
          (if print_layout
            (translate (+ -28mm (* i 56mm)) 46mm 0 axle)
            (translate (- axle-half-length) (+ -15mm (* i 30mm)) wheel_radius
              (rotate 0 90deg 0 axle))))))
    (part wheels
      (repeat-compound side 2
        (repeat-compound i 2
          (let* ((sign (- (* side 2) 1))
                 (wheel (tractor-wheel :radius wheel_radius :width wheel-width
                   :axle-radius axle-radius :running-clearance wheel_clearance)))
            (if print_layout
              (translate (* sign 32mm) (+ -15mm (* i 30mm)) 0 wheel)
              (translate (* sign (- wheel-center (/ wheel-width 2)))
                (+ -15mm (* i 30mm)) wheel_radius
                (rotate 0 (* sign 90deg) 0 wheel)))))))
    (part rivets
      (repeat-compound side 2
        (repeat-compound i 2
          (let* ((sign (- (* side 2) 1))
                 (rivet (tractor-rivet :snap-interference snap_interference
                   :head-thickness rivet-head-thickness)))
            (if print_layout
              (translate (+ -18mm (* side 24mm) (* i 12mm)) 46mm 0 rivet)
              (translate (* sign (+ axle-half-length rivet-head-thickness))
                (+ -15mm (* i 30mm)) wheel_radius
                (rotate 0 (* sign -90deg) 0 rivet))))))))
  (verify
    (tag eleven-printable-pieces)
    (metric pieces (stl connected-component-count))
    (expect pieces (= 11)))
  (verify
    (tag closed-surfaces)
    (metric bad-edges (stl non-manifold-edge-count))
    (expect bad-edges (= 0))))
```

[Download the tractor](/docs/examples/192f9ba3f5db-04-tractor.ecky)

### Try a useful failure {#tractor-experiment}

1. Render the kit and run verification. Expect eleven closed, separate pieces.
2. Inspect assembly mode. Follow the axle through the body and both wheels; find the two retaining heads.
3. Increase `wheel_radius` from 10 to 12. Wheels grow and the body rises; axle and rivet fits stay unchanged.
4. In the sample, temporarily make the rivet slot pass through its head. The rivet splits into two pieces: a real design error. Restore the slot's bottom before printing.

**You can now:** design a running gap and a snap catch, reuse exact mating parts in a cheap sample, and distinguish printable layout from assembled placement. Next comes a joint that bends in more than one direction.

## Chapter 5: A worm that really bends

The **Worm Farm** needs its farmer. A curved solid still stays rigid after printing. Our worm has separate links: a ball on one link sits inside a socket on the next. The links can move while staying captured.

This is the hardest mechanism in the book. Start with two links. Once that joint works, making a longer worm is repetition.

### Find the nine moving pieces {#worm-links}

![A nine-piece articulated worm with a face, captive ball joints, rounded body links and a tapered tail](/docs/assets/worm-farm/2f44ce87f671-01-worm.png)

The complete worm has a head, seven body links, and a tail. They are printed together in a straight row. They must remain **nine separate solids**, even though the balls are already inside their sockets. The straight printing position does not prevent later bending.

Try [the two-link joint sample](/docs/examples/7e8a77ab33ed-worm-joint-coupon.ecky) first. It is the same ball and socket, without the face or long row. After printing, let it cool and gently check movement. Do not force a fused sample; adjust the gap and print another pair.

![Two worm links sharing a captive ball-and-socket joint](/docs/assets/worm-farm/b27894952c17-worm-joint-coupon.png)

### A ball inside a cage {#worm-ball-socket}

Look at one connection from above. The ball belongs to the link on the left. The cage belongs to the link on the right. The gap between them is empty space, not a second material.

| Name in the recipe | Job |
| --- | --- |
| `ball-radius` | Size of the moving ball: 4 mm |
| `cavity-radius` | Ball radius plus `joint-clearance`: 4.6 mm by default |
| `socket-wall` | Material around that cavity: 2 mm |
| `socket-throat-radius` | Top opening: smaller than the ball, so it stays captive |
| `link-pitch` | Distance between neighbouring link centres: 23 mm |

`difference` scoops the cavity out of the socket housing. It removes a round opening above and a smaller opening below. The ball cannot pass through either opening. The ball and socket remain separate; a `union` between neighbouring links would lock the joint.

Reference: [sphere](https://ecky-cad.com/docs/primitive-signatures/#sphere), [difference](https://ecky-cad.com/docs/boolean-and-transform-signatures/#difference).

### Give the neck somewhere to move {#worm-neck-window}

A short neck joins the ball to its own body. The neck needs a side window in the next socket. Without that window the neck hits the wall as soon as you try to turn it.

The recipe sweeps a cutter through several sideways angles to clear a fan-shaped window. It leaves **1.6 mm of unbroken material below and above** the window. Those lips keep the ball captive. Cutting the window all the way through the top would weaken that cage.

A second cut relieves the nearby rounded body so it does not block the socket during up-and-down motion. The flat underside and a small ball pedestal put material on the print bed. The bottom opening also clears that pedestal as the socket tilts. Its radius is derived from the pedestal and motion angle; it stays smaller than the ball.

`joint-yaw-limit` and `joint-pitch-limit` size the clearance envelope. They do not animate the worm. They are design targets for side and vertical movement, not proof of a printed angle or permission to force the joint.

### Reuse one joint recipe {#worm-repeat}

`worm-link` builds one rounded body with an optional socket and ball. The head uses `:has-socket false`; the tail uses `:has-ball false`. Ordinary body links have both.

The row is the short part of the code:

```scheme
(translate link-pitch 0 0
  (linear-array body-count link-pitch 0 0
    (worm-link :joint-clearance joint-clearance)))
```

This fragment needs the component and bindings from the complete file. It says: start one pitch after the head, then place seven body links at that same spacing. `linear-array` groups copies without joining neighbours.

The head adds eyes and horns. The tail rounds three shrinking shapes into a tip. Those decorations do not replace the joint.

### Open the complete worm {#worm-source}

Use the complete example as a recipe to inspect and modify. The detailed joint is longer than the rake because it has to clear moving parts and keep them captured. Find the names from the table before reading every cutter.

```scheme
; Captive ball joints with central neck windows and unbroken lower/upper lips.
(define-component worm-link
  ((number joint-clearance 0.6mm)
   (number joint-yaw-limit 35deg)
   (number joint-pitch-limit 20deg)
   (toggle has-socket true)
   (toggle has-ball true))
  (let* ((ball-radius 4mm)
         (joint-height 5mm)
         (socket-wall 2mm)
         (cavity-radius (+ ball-radius joint-clearance))
         (socket-outer-radius (+ cavity-radius socket-wall))
         (socket-height 12mm)
         (socket-throat-radius 3mm)
         (socket-top-lever (- socket-height joint-height))
         (body-relief-radius (+ joint-clearance
           (* socket-outer-radius (cos (deg->rad joint-pitch-limit)))
           (* socket-top-lever (sin (deg->rad joint-pitch-limit)))))
         (ball-offset 11mm)
         (socket-offset -12mm)
         (neck-start 1.2mm)
         (neck-length (- ball-offset neck-start))
         (neck-width 4.8mm)
         (neck-height 2.8mm)
         (window-bottom 1.6mm)
         (window-top (- socket-height window-bottom))
         (window-height (- window-top window-bottom))
         (window-center (/ (+ window-top window-bottom) 2))
         (window-corner-radius 1mm)
         (neck-motion-clearance 1.2mm)
         (passage-half-width (+ (/ neck-width 2) neck-motion-clearance))
         (ball-bed-pedestal-radius 1.4mm)
         (ball-bed-pedestal-height 1.3mm)
         (ball-bed-flat-height 0.6mm)
         (pedestal-motion-clearance 0.3mm)
         (bottom-throat-radius (+ pedestal-motion-clearance
           (* ball-bed-pedestal-radius (cos (deg->rad joint-pitch-limit)))
           (* joint-height (sin (deg->rad joint-pitch-limit)))))
         (cutter-length 16mm))
    (build
      (shape rounded-body
        (clip-plane
          (translate 0 0 4.8mm (scale 1.15 1.12 1 (sphere 7.2mm)))
          :origin '(0 0 0) :normal '(0 0 1) :keep "positive"))
      (shape body-with-joint-relief
        (if has-ball
          (difference rounded-body
            (translate ball-offset 0 -1mm (cylinder body-relief-radius 16mm)))
          rounded-body))
      (shape socket-housing
        (if has-socket
          (union body-with-joint-relief
            (translate socket-offset 0 0 (cylinder socket-outer-radius socket-height)))
          body-with-joint-relief))
      (shape socket-cavity
        (union
          (translate socket-offset 0 joint-height (sphere cavity-radius))
          (translate socket-offset 0 joint-height
            (cylinder socket-throat-radius socket-height))
          (translate socket-offset 0 -1mm
            (cylinder bottom-throat-radius (+ window-bottom 1mm)))))
      (shape neck-passage-base
        (translate (- cutter-length) 0 window-center
          (rotate 90deg 0 90deg
            (extrude
              (rounded-rect (* passage-half-width 2) window-height window-corner-radius)
              cutter-length))))
      (shape neck-motion-envelope
        (translate socket-offset 0 0
          (apply union
            (map (lambda (yaw) (rotate 0 0 yaw neck-passage-base))
              (linspace (- joint-yaw-limit) joint-yaw-limit 7)))))
      (shape captive-socket
        (if has-socket
          (difference socket-housing socket-cavity neck-motion-envelope)
          socket-housing))
      (shape grounded-ball
        (union
          (clip-plane
            (translate ball-offset 0 joint-height (sphere ball-radius))
            :origin (list 0 0 ball-bed-flat-height) :normal '(0 0 1) :keep "positive")
          (translate ball-offset 0 0
            (cylinder ball-bed-pedestal-radius ball-bed-pedestal-height))))
      (shape central-neck-bridge
        (translate neck-start 0 window-center
          (rotate 90deg 0 90deg
            (extrude (slot-overall neck-width neck-height) neck-length))))
      (result
        (if has-ball
          (union captive-socket central-neck-bridge grounded-ball)
          captive-socket)))))

(model
  (meta :title "Articulated puzzle worm")
  (params
    (number body-count 7 :label "Body links" :min 2 :max 10 :step 1 :frozen true)
    (number joint-yaw-limit 35deg :label "Side motion envelope" :min 25deg :max 40deg :step 5deg)
    (number joint-pitch-limit 20deg :label "Vertical motion envelope" :min 15deg :max 25deg :step 5deg)
    (number joint-clearance 0.6mm :label "Joint clearance" :min 0.4mm :max 0.8mm :step 0.05mm))
  (verify
    (tag closed-link-surfaces)
    (metric bad-edges (stl non-manifold-edge-count))
    (expect bad-edges (= 0)))
  (verify
    (tag separate-moving-links)
    (intent "Head, body links and tail must remain separate solids")
    (metric link-count (stl connected-component-count))
    (expect link-count (= 9)))
  (verify
    (tag grounded-head)
    (metric contact (stl bed-contact-area-ratio head))
    (expect contact (>= 0.12)))
  (verify
    (tag grounded-body-links)
    (metric contact (stl bed-contact-area-ratio body-links))
    (expect contact (>= 0.12)))
  (verify
    (tag grounded-tail)
    (metric contact (stl bed-contact-area-ratio tail))
    (expect contact (>= 0.12)))
  (verify
    (tag preview-artifact)
    (metric present (manifest has-model-stl))
    (expect present (= true)))
  (let* ((link-pitch 23mm))
    (part head
      (build
        (shape head-body
          (worm-link :joint-yaw-limit joint-yaw-limit :joint-pitch-limit joint-pitch-limit :joint-clearance joint-clearance :has-socket false :has-ball true))
        (shape face-eyes
          (apply union (map (lambda (side)
              (translate -4.8mm (* side 3mm) 10.6mm (sphere 1.9mm))) '(-1 1))))
        (shape face-with-nose
          (union head-body face-eyes
            (translate -6.5mm 0 7mm (sphere 2.2mm))))
        (shape pupil-recesses
          (apply union (map (lambda (side)
              (translate -5.3mm (* side 3mm) 11.7mm (cylinder 0.8mm 3mm))) '(-1 1))))
        (shape rounded-horns
          (apply union
            (map (lambda (side)
              (translate -2.8mm (* side 4.8mm) 9mm
                (rotate (* side -10deg) 0 -8deg
                  (union
                    (cone 1.7mm 0.65mm 5.5mm)
                    (translate 0 0 5.3mm (sphere 0.8mm))))))
              '(-1 1))))
        (result (difference (union face-with-nose rounded-horns) pupil-recesses))))
    (part body-links
      (translate link-pitch 0 0
        (linear-array body-count link-pitch 0 0
          (worm-link :joint-yaw-limit joint-yaw-limit :joint-pitch-limit joint-pitch-limit :joint-clearance joint-clearance))))
    (part tail
      (translate (* (+ body-count 1) link-pitch) 0 0
        (build
          (shape tail-root
            (worm-link :joint-yaw-limit joint-yaw-limit :joint-pitch-limit joint-pitch-limit :joint-clearance joint-clearance :has-ball false))
          (shape tapered-tail-tip
            (clip-plane
              (hull
                (translate 5mm 0 2.4mm (sphere 3.4mm))
                (translate 10mm 0.8mm 1.8mm (sphere 2.7mm))
                (translate 14mm 1.5mm 1.4mm (sphere 2mm)))
              :origin '(0 0 0) :normal '(0 0 1) :keep "positive"))
          (result (union tail-root tapered-tail-tip))))))
  (tag-face head-bed :faces "bottom" head))
```

[Download the complete worm](/docs/examples/3c94b19e7aa3-01-worm.ecky)

### Try one joint before nine {#worm-experiment}

1. Render the two-link sample. Inspect the ball through the top opening and neck through the side window.
2. Find `joint-clearance = 0.6mm`. Predict: increasing it enlarges the cavity; it does not shrink the ball. Change it to 0.7 and inspect again.
3. Compare a printed sample before and after that change. Keep the setting that moves freely while staying captive. Tiny gaps depend on printer settings and material.
4. Render the complete worm and run verification. Expect nine closed pieces. `body-count` is frozen at seven because that count check expects seven body links plus two ends. When deliberately changing the count in source, update the expectation too.

Closed meshes and separate links establish only part of the mechanism. Check the window and body relief, then test a printed pair before the full worm.

**You can now:** explain a captive ball joint, distinguish clearance from a motion control, preserve retaining lips, and repeat a working link. Time to bring the farm together.

## Chapter 6: Open the gates, check the farm

Welcome to the complete **Worm Farm**: rake, mushroom house, garden, snap-together tractor, and articulated farmer. Every object uses the same geometry as its own chapter. We move those familiar pieces onto one map.

### Arrange the familiar pieces {#farm-layout}

![The articulated worm in front of a mushroom house, rake, assembled tractor, fence and carrot](/docs/assets/worm-farm/ac7c831703b7-06-farm.png)

The complete source is assembled from the chapter files when the book is built. Downloads are self-contained: you do not need a hidden component library. A source check catches an old tractor or rigid worm accidentally left in the farm.

Each `part` gains an outer `translate`. All tractor pieces share one placement; both house pieces share another. Their local dimensions and fits stay the same. The worm is longer than the rake because it has nine links at a 23 mm pitch.

The farm starts in assembled inspection mode: `print_layout` and `house_print_layout` are off. For printing, start with the individual chapter downloads and the two small joint samples. The scene is not a promise that every posed piece is resting on the bed. There is no exported ground slab.

### Peek under the roof {#farm-view}

The named `view` called `peek_inside` lifts the roof for inspection. A view offset is **preview-only**: STL and STEP keep their normal geometry. By comparison, `translate` inside a `part` and the kit's print-layout switches change exports.

The illustration shows the assembled exported positions. A preview effect must never quietly become extra material in the printed object.

Reference: [view](https://ecky-cad.com/docs/complete-compiler-surface/#view).

### Count what must stay separate {#farm-verify}

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

### Open the complete farm {#farm-source}

Start at the part names and their outer placements. The longer component recipes are the same ones you have already inspected.

```scheme
; Generated by scripts/worm_farm_sources.ts from the chapter models.
(define-component worm-link
  ((number joint-clearance 0.6mm)
   (number joint-yaw-limit 35deg)
   (number joint-pitch-limit 20deg)
   (toggle has-socket true)
   (toggle has-ball true))
  (let* ((ball-radius 4mm)
         (joint-height 5mm)
         (socket-wall 2mm)
         (cavity-radius (+ ball-radius joint-clearance))
         (socket-outer-radius (+ cavity-radius socket-wall))
         (socket-height 12mm)
         (socket-throat-radius 3mm)
         (socket-top-lever (- socket-height joint-height))
         (body-relief-radius (+ joint-clearance
           (* socket-outer-radius (cos (deg->rad joint-pitch-limit)))
           (* socket-top-lever (sin (deg->rad joint-pitch-limit)))))
         (ball-offset 11mm)
         (socket-offset -12mm)
         (neck-start 1.2mm)
         (neck-length (- ball-offset neck-start))
         (neck-width 4.8mm)
         (neck-height 2.8mm)
         (window-bottom 1.6mm)
         (window-top (- socket-height window-bottom))
         (window-height (- window-top window-bottom))
         (window-center (/ (+ window-top window-bottom) 2))
         (window-corner-radius 1mm)
         (neck-motion-clearance 1.2mm)
         (passage-half-width (+ (/ neck-width 2) neck-motion-clearance))
         (ball-bed-pedestal-radius 1.4mm)
         (ball-bed-pedestal-height 1.3mm)
         (ball-bed-flat-height 0.6mm)
         (pedestal-motion-clearance 0.3mm)
         (bottom-throat-radius (+ pedestal-motion-clearance
           (* ball-bed-pedestal-radius (cos (deg->rad joint-pitch-limit)))
           (* joint-height (sin (deg->rad joint-pitch-limit)))))
         (cutter-length 16mm))
    (build
      (shape rounded-body
        (clip-plane
          (translate 0 0 4.8mm (scale 1.15 1.12 1 (sphere 7.2mm)))
          :origin '(0 0 0) :normal '(0 0 1) :keep "positive"))
      (shape body-with-joint-relief
        (if has-ball
          (difference rounded-body
            (translate ball-offset 0 -1mm (cylinder body-relief-radius 16mm)))
          rounded-body))
      (shape socket-housing
        (if has-socket
          (union body-with-joint-relief
            (translate socket-offset 0 0 (cylinder socket-outer-radius socket-height)))
          body-with-joint-relief))
      (shape socket-cavity
        (union
          (translate socket-offset 0 joint-height (sphere cavity-radius))
          (translate socket-offset 0 joint-height
            (cylinder socket-throat-radius socket-height))
          (translate socket-offset 0 -1mm
            (cylinder bottom-throat-radius (+ window-bottom 1mm)))))
      (shape neck-passage-base
        (translate (- cutter-length) 0 window-center
          (rotate 90deg 0 90deg
            (extrude
              (rounded-rect (* passage-half-width 2) window-height window-corner-radius)
              cutter-length))))
      (shape neck-motion-envelope
        (translate socket-offset 0 0
          (apply union
            (map (lambda (yaw) (rotate 0 0 yaw neck-passage-base))
              (linspace (- joint-yaw-limit) joint-yaw-limit 7)))))
      (shape captive-socket
        (if has-socket
          (difference socket-housing socket-cavity neck-motion-envelope)
          socket-housing))
      (shape grounded-ball
        (union
          (clip-plane
            (translate ball-offset 0 joint-height (sphere ball-radius))
            :origin (list 0 0 ball-bed-flat-height) :normal '(0 0 1) :keep "positive")
          (translate ball-offset 0 0
            (cylinder ball-bed-pedestal-radius ball-bed-pedestal-height))))
      (shape central-neck-bridge
        (translate neck-start 0 window-center
          (rotate 90deg 0 90deg
            (extrude (slot-overall neck-width neck-height) neck-length))))
      (result
        (if has-ball
          (union captive-socket central-neck-bridge grounded-ball)
          captive-socket)))))

(define-component tractor-chassis ((number axle-radius 3.2mm) (number body-clearance 0.3mm))
  (let* ((axle-height 5mm)
         (body (box 30mm 50mm 10mm))
         (engine (translate 0 -11mm 10mm (box 22mm 23mm 10mm)))
         (chimney (translate 0 -15mm 20mm (cylinder 2.5mm 12mm)))
         (cabin (translate 0 14mm 10mm
           (difference (box 24mm 18mm 23mm)
             (translate 0 0 3mm (box 26mm 12mm 16mm)))))
         (axle-bores (repeat-compound i 2
           (translate 0 (+ -15mm (* i 30mm)) axle-height
             (rotate 0 90deg 0
               (cylinder (+ axle-radius body-clearance) 34mm :align '(center center center)))))))
    (difference (union body engine chimney cabin) axle-bores)))

(define-component tractor-axle
  ((number axle-radius 3.2mm) (number axle-length 47.2mm)
   (number mouth-radius 1.8mm) (number cavity-radius 2.35mm)
   (number lip-depth 2.2mm) (number socket-depth 5.5mm))
  (difference
    (cylinder axle-radius axle-length)
    (translate 0 0 -1mm (cylinder mouth-radius (+ axle-length 2mm)))
    (translate 0 0 lip-depth (cylinder cavity-radius (- socket-depth lip-depth)))
    (translate 0 0 (- axle-length socket-depth)
      (cylinder cavity-radius (- socket-depth lip-depth)))))

(define-component tractor-wheel
  ((number radius 10mm) (number width 6mm)
   (number axle-radius 3.2mm) (number running-clearance 0.4mm))
  (difference (cylinder radius width)
    (translate 0 0 -1mm
      (cylinder (+ axle-radius running-clearance) (+ width 2mm)))))

(define-component tractor-rivet
  ((number mouth-radius 1.8mm) (number snap-interference 0.25mm)
   (number head-radius 4.8mm) (number head-thickness 1.6mm))
  (let* ((stem-radius (- mouth-radius 0.2mm))
         (barb-radius (+ mouth-radius snap-interference))
         (barb-base 4.3mm)
         (slot-bottom (+ head-thickness 0.8mm)))
    (difference
      (union
        (cylinder head-radius head-thickness)
        (translate 0 0 (- head-thickness 0.2mm) (cylinder stem-radius 3.1mm))
        (translate 0 0 barb-base (cone barb-radius 1mm 1.9mm)))
      (translate 0 0 slot-bottom (box 1.1mm 8mm 5mm)))))

(define-component fence_post ((number x 0mm) (number height 22mm))
  (translate x 0 0
    (union
      (cylinder 2mm height)
      (translate 0 0 height
        (cone 3mm 0mm 5mm)))))

(model
  (meta :title "Worm Farm")
  (params
    (number body-count 7 :label "Body links" :min 2 :max 10 :step 1 :frozen true)
    (number joint-yaw-limit 35deg :label "Side motion envelope" :min 25deg :max 40deg :step 5deg)
    (number joint-pitch-limit 20deg :label "Vertical motion envelope" :min 15deg :max 25deg :step 5deg)
    (number joint-clearance 0.6mm :label "Joint clearance" :min 0.4mm :max 0.8mm :step 0.05mm)
    (number wall 2mm :label "House wall" :min 1.5 :max 3 :step 0.5)
    (number roof_clearance 0.3mm :label "Roof clearance" :min 0.2 :max 0.6 :step 0.1)
    (toggle house_print_layout false :label "House print layout (changes export)")
    (number tooth_count 5 :label "Rake teeth" :min 3 :max 7 :step 1)
    (number wheel_radius 10mm :label "Wheel radius" :min 8 :max 12 :step 1)
    (number wheel_clearance 0.4mm :label "Wheel running clearance" :min 0.2 :max 0.6 :step 0.05)
    (number snap_interference 0.25mm :label "Rivet snap interference" :min 0.1 :max 0.35 :step 0.05)
    (toggle print_layout false :label "Print layout (changes export)")
    (number posts 6 :label "Fence posts" :min 3 :max 9 :step 1))
  (let* ((link-pitch 23mm))
    (part worm-head
      (translate -100mm -35mm 0mm
(build
        (shape head-body
          (worm-link :joint-yaw-limit joint-yaw-limit :joint-pitch-limit joint-pitch-limit :joint-clearance joint-clearance :has-socket false :has-ball true))
        (shape face-eyes
          (apply union (map (lambda (side)
              (translate -4.8mm (* side 3mm) 10.6mm (sphere 1.9mm))) '(-1 1))))
        (shape face-with-nose
          (union head-body face-eyes
            (translate -6.5mm 0 7mm (sphere 2.2mm))))
        (shape pupil-recesses
          (apply union (map (lambda (side)
              (translate -5.3mm (* side 3mm) 11.7mm (cylinder 0.8mm 3mm))) '(-1 1))))
        (shape rounded-horns
          (apply union
            (map (lambda (side)
              (translate -2.8mm (* side 4.8mm) 9mm
                (rotate (* side -10deg) 0 -8deg
                  (union
                    (cone 1.7mm 0.65mm 5.5mm)
                    (translate 0 0 5.3mm (sphere 0.8mm))))))
              '(-1 1))))
        (result (difference (union face-with-nose rounded-horns) pupil-recesses)))))
    (part worm-body-links
      (translate -100mm -35mm 0mm
(translate link-pitch 0 0
        (linear-array body-count link-pitch 0 0
          (worm-link :joint-yaw-limit joint-yaw-limit :joint-pitch-limit joint-pitch-limit :joint-clearance joint-clearance)))))
    (part worm-tail
      (translate -100mm -35mm 0mm
(translate (* (+ body-count 1) link-pitch) 0 0
        (build
          (shape tail-root
            (worm-link :joint-yaw-limit joint-yaw-limit :joint-pitch-limit joint-pitch-limit :joint-clearance joint-clearance :has-ball false))
          (shape tapered-tail-tip
            (clip-plane
              (hull
                (translate 5mm 0 2.4mm (sphere 3.4mm))
                (translate 10mm 0.8mm 1.8mm (sphere 2.7mm))
                (translate 14mm 1.5mm 1.4mm (sphere 2mm)))
              :origin '(0 0 0) :normal '(0 0 1) :keep "positive"))
          (result (union tail-root tapered-tail-tip)))))))

  (part mushroom-house
      (translate -65mm 40mm 0mm
(let* ((outside (cylinder 18mm 28mm))
           (room (translate 0 0 2mm (cylinder (- 18mm wall) 27mm)))
           (doorway (translate 0 -16mm 9mm
             (rotate 90deg 0 0 (cylinder 8.5mm 18mm :align '(center center center))))))
      (difference outside room doorway))))

  (part mushroom-roof
      (translate -65mm 40mm 0mm
(let* ((rim-radius (+ 18mm roof_clearance))
           (skirt-height 3mm)
           (cap (cone 26mm 2mm 26mm))
           (rim-opening (translate 0 0 -1mm (cylinder rim-radius (+ skirt-height 1mm))))
           (inside (translate 0 0 skirt-height (cone rim-radius 0mm rim-radius))))
      (translate (if house_print_layout 52mm 0mm) 0 (if house_print_layout 0mm 25mm)
        (difference cap rim-opening inside)))))

  (part tool-rake
      (translate -20mm 75mm 0mm
(let* ((tooth-length 12mm)
           (tooth-radius 1.3mm)
           (head-height tooth-length)
           (head-radius 2mm)
           (tooth-spacing 4mm)
           (head-length (+ (* (- tooth_count 1) tooth-spacing) (* head-radius 2)))
           (handle (translate 0 0 head-height
             (rotate -35deg 0 0 (cylinder 2mm 44mm))))
           (crossbar (translate 0 0 head-height
             (rotate 0 90deg 0
               (cylinder head-radius head-length :align '(center center center)))))
           (teeth (repeat-union i tooth_count
             (translate (* (- i (/ (- tooth_count 1) 2)) tooth-spacing) 0 0
               (cylinder tooth-radius tooth-length)))))
      (union handle crossbar teeth))))

  (let* ((axle-radius 3.2mm)
         (body-half-width 15mm)
         (wheel-width 6mm)
         (wheel-side-gap 2mm)
         (wheel-end-play 0.6mm)
         (wheel-center (+ body-half-width wheel-side-gap (/ wheel-width 2)))
         (axle-half-length (+ body-half-width wheel-side-gap wheel-width wheel-end-play))
         (axle-length (* 2 axle-half-length))
         (rivet-head-thickness 1.6mm)
         (body-lift (- wheel_radius 5mm)))
    (part tractor-chassis
      (translate 40mm 35mm 0mm
(translate 0 0 (if print_layout 0mm body-lift)
        (tractor-chassis :axle-radius axle-radius))))
    (part tractor-axles
      (translate 40mm 35mm 0mm
(repeat-compound i 2
        (let* ((axle (tractor-axle :axle-radius axle-radius :axle-length axle-length)))
          (if print_layout
            (translate (+ -28mm (* i 56mm)) 46mm 0 axle)
            (translate (- axle-half-length) (+ -15mm (* i 30mm)) wheel_radius
              (rotate 0 90deg 0 axle)))))))
    (part tractor-wheels
      (translate 40mm 35mm 0mm
(repeat-compound side 2
        (repeat-compound i 2
          (let* ((sign (- (* side 2) 1))
                 (wheel (tractor-wheel :radius wheel_radius :width wheel-width
                   :axle-radius axle-radius :running-clearance wheel_clearance)))
            (if print_layout
              (translate (* sign 32mm) (+ -15mm (* i 30mm)) 0 wheel)
              (translate (* sign (- wheel-center (/ wheel-width 2)))
                (+ -15mm (* i 30mm)) wheel_radius
                (rotate 0 (* sign 90deg) 0 wheel))))))))
    (part tractor-rivets
      (translate 40mm 35mm 0mm
(repeat-compound side 2
        (repeat-compound i 2
          (let* ((sign (- (* side 2) 1))
                 (rivet (tractor-rivet :snap-interference snap_interference
                   :head-thickness rivet-head-thickness)))
            (if print_layout
              (translate (+ -18mm (* side 24mm) (* i 12mm)) 46mm 0 rivet)
              (translate (* sign (+ axle-half-length rivet-head-thickness))
                (+ -15mm (* i 30mm)) wheel_radius
                (rotate 0 (* sign -90deg) 0 rivet)))))))))

  (part garden-fence
      (translate -15mm 100mm 0mm
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

  (part garden-carrot
      (translate -15mm 100mm 0mm
(translate 20mm -12mm 0
      (loft 18mm
        (circle 1mm)
        (regular-polygon 6 5mm)))))
  (view peek_inside (offset-part mushroom-roof 0 0 24mm))
  (verify (tag closed-farm) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= 0)))
  (verify (tag separate-mechanisms) (metric pieces (stl connected-component-count)) (expect pieces (= 25))))
```

[Download the complete farm](/docs/examples/6f83c6da1afd-06-farm.ecky)

### Change one thing and explain it {#farm-finish}

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
