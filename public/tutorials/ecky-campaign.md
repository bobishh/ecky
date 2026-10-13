# Ecky Campaign

Learn Ecky as six modeling levels. Each level ends in geometry you can preview,
compile, and export as STL. Finish the clear condition before moving forward;
the dry operation reference lives under `/docs/ecky-ir`.

## Level 01: Corner Bracket

**Mission:** Build one connected corner bracket from a horizontal foot and a vertical flange.

**Clear condition:** Preview shows one connected L-bracket where the foot and flange overlap, and the source compiles.

These are extra engineering exercises. For the illustrated beginner book,
start with [Worm Farm](missions/01-worm.md): a Bézier worm grows a farm with a
house, tools, a tractor and a garden. Return here to apply those ideas to
practical parts. This bracket is the shelf support for a robot's emergency
biscuit stash. Its two shapes must overlap; robots cannot eat an invisible join.

A renderable file needs a `model` root, a named `part`, and geometry. The first
useful part is a **corner bracket**: one horizontal foot and one vertical flange
that cross at a corner and fuse into a single printable L-shape. Starting with
two primitives keeps each transform and boolean obvious.

Begin with one solid so the root, the part id, and the primitive are clear.

```scheme
(model
  (part bracket
    (box 40 40 6)))
```

`model` is the root. `part` gives the geometry a stable id (`bracket`). `box`
produces one solid centered at the origin. Nothing is joined yet.

Make the bracket from two solids that share the corner. A long thin foot and a
long thin flange, each crossing the origin, overlap in a small square region.
`union` fuses that overlap into one connected part.

```scheme
(model
  (part bracket
    (union
      (box 40 8 6)
      (box 8 40 6))))
```

The foot is `40 x 8 x 6`; the flange is `8 x 40 x 6`. Both start at the origin,
so they overlap in an `8 x 8 x 6` corner. That deliberate overlap is what makes
the union produce one connected L-bracket instead of two loose pieces. `union`
merges overlapping solids; it does not glue solids that merely touch at a face.

When the foot and flange must sit at different heights, move one with
`translate` so the overlap is preserved, not destroyed.

```scheme
(model
  (part bracket
    (union
      (box 40 8 6)
      (translate 0 0 3
        (box 8 40 6)))))
```

`translate 0 0 3` lifts the flange so it still crosses the foot by half its
thickness. The overlap region shrinks but never disappears, so the union stays
one body. Move geometry to control placement; keep enough overlap so the
boolean has connected material to merge.

Use this pattern for the first real part: name the part, place primitives so
they overlap, then join them with one boolean.

> **Watch for:** primitives start at the origin. Two solids that do not overlap
> produce a union of two disconnected bodies, not one part. If a union has the
> right members but reads as two pieces, inspect placement and overlap before
> changing the boolean.
## Level 02: Mounting Plate

**Mission:** Turn a blank into a useful plate with repeated through-holes.

**Clear condition:** Every cutter crosses the plate and the exported STL remains one component.

Our robot now wants bolts, not optimism, to hold its biscuit shelf. Think of each cutter as a cookie cutter passing completely through the dough: a cutter that stops inside leaves a blind pocket.

Use `build` when a part needs several boolean stages. Each `shape` names an intermediate result; `result` identifies the final geometry. Names keep cutters and later selectors readable.

```scheme
(model
  (params
    (number plate_w 80)
    (number plate_h 48)
    (number thickness 5)
    (number hole_r 4))
  (part mount
    (build
      (shape blank
        (extrude (rounded-rect plate_w plate_h 4) thickness))
      (shape hole_left
        (translate -24 0 -0.5
          (cylinder hole_r (+ thickness 1))))
      (shape hole_right
        (translate 24 0 -0.5
          (cylinder hole_r (+ thickness 1))))
      (result
        (difference blank hole_left hole_right)))))
```

![Rendered output for Cut and Join: Mounting Plate, example 1](assets/04-cut-and-join-01.png)

`build` names each step. `difference` subtracts cutters from the blank. The cutters are slightly taller than the plate so the cut passes fully through.

Add material with `union` or `fuse`.

```scheme
(result
  (union
    (difference blank hole_left hole_right)
    (translate 0 0 thickness
      (cylinder 12 8))))
```

The result is still one part, but the intent stays readable.

> **Watch for:** make cutters cross the stock completely; coincident cutter and stock faces can leave unstable slivers. Booleans also rebuild topology, so raw face and edge indices are not durable selectors. Use geometric selectors or tags after the boolean.
## Level 03: Dovetail Fit

**Mission:** Make one named fit_clearance drive both mating sides of a dovetail rail and channel.

**Clear condition:** Changing fit_clearance widens the channel while the rail stays nominal; no second anonymous offset needs editing.

The shelf needs to slide out for crumb patrol. The rail stays nominal; the channel grows by a named clearance. That is the same fit idea as the mushroom roof plug, now with a polygonal cross-section.

A dovetail is a sliding fit between a male rail and a female channel. The two
parts mate because the channel is slightly larger than the rail, and that
slight difference **is** the fit. Make the difference one named number and the
whole fit becomes editable from one place.

The trap is authoring each side with its own hard-coded offset: the rail at its
nominal size, the channel widened by a magic literal like `0.6`. That works
once, but the moment you want a looser or tighter fit you have to find and edit
two offsets that were never linked. Worse, the two numbers drift apart with
every edit until the parts no longer mate.

The fix is a single named clearance binding shared by both sides:

- the **male** side uses the nominal profile directly;
- the **female** side is the same profile enlarged by the clearance on every
  side (`nominal + 2 * clearance`).

Change the clearance once and only the channel moves — the rail stays nominal,
so the fit changes through one relation instead of two anonymous offsets.

Reuse a proven profile instead of redesigning it. The dovetail rail in the
film-adapter mechanism is already a tested triangular profile; extracting that
profile and its clearance relation into a smaller fixture preserves the fit
math without inventing a second dovetail. The surrounding mechanism (film path,
detents, helicoid) is complexity you can drop; the mating profile and the named
clearance are what you keep.

When the two mating parts are separate exportable solids, keep any
preview-only assembly placement (a rail hovered above a channel for display)
out of the exported geometry. Each part should export as the clean solid it
really is; the assembly view is a diagnostic, not a feature of either part.
## Level 04: Procedural Workshop

**Mission:** Build generated cutters and path-driven attachments from data.

**Clear condition:** One parameter change regenerates the pattern; final geometry still exports as a valid solid.

A robot with twelve accessories should not need twelve hand-edited placements. Reuse the rake lesson: prove one shape and one location, then generate the row or path. Inspect the result after changing a count.

These fixtures combine generated cutter lists, deterministic fields, path frames, arrays, and parameter-driven cavities. Focus on how each model separates generation math from final boolean intent.

### Procedural perforated panel

This model uses `map` and `range` to generate cutters, `hash-signed` to jitter each cutter, `voronoi2` to vary cutter radius, and `apply union` to turn the generated list into one cutter body.

<!-- render-source: ../examples/voronoi-perforated-panel.ecky -->

![Rendered output for Real Model Patterns: Procedural Cuts and Arrayed Frames, example 1](assets/10-real-model-patterns-01.png)

The important line is the result expression:

```scheme
(result
  (difference
    panel
    (apply union
      (map
        (lambda (cell)
          (let* ((col (- cell (* 4 (floor (/ cell 4)))))
                 (row (floor (/ cell 4)))
                 (x (* (- col 1.5) 14))
                 (y (* (- row 1.0) 12))
                 (jx (+ x (* 2.4 (hash-signed col row 23))))
                 (jy (+ y (* 2.4 (hash-signed (+ col 19.19) (+ row 7.73) 54))))
                 (r (+ 2.2 (* 1.1 (voronoi2 (/ jx 14.0) (/ jy 12.0) 23)))))
            (translate jx jy 0
              (cylinder r 8))))
        (range 0 cell-count)))))
```

`range` decides how many cutters exist. `map` builds one cylinder per cell. `let*` is required because `jx`, `jy`, and `r` depend on earlier bindings. `apply union` converts the list of cylinders into one boolean operand for `difference`.

This is the pattern to use when the count is parametric but the result is still one printable part.

### Frame and array bracket

This fixture combines curve-driven placement with arrays. The rib is swept along a bezier path. The pad is placed at a sampled path frame. The base holes, locator posts, and fan stops use three array helpers.

<!-- render-source: ../examples/frame-array-bracket.ecky -->

![Rendered output for Real Model Patterns: Procedural Cuts and Arrayed Frames, example 2](assets/10-real-model-patterns-02.png)

The model has three distinct placement styles:

```scheme
(shape rail
  (bezier-path ((-18 0 4) (-8 7 9) (8 -7 12) (18 0 16))))
(shape rib
  (sweep (circle 1.1) rail))
(shape end-frame
  (path-frame rail :at end :up (0 0 1)))
(shape placed-pad
  (place end-frame pad :offset (0 0 -1.5) :rotate (0 0 18)))
```

`sweep` makes geometry follow the path. `path-frame` samples a pose from the path. `place` uses that pose to attach another solid.

The array helpers do the repeated work:

```scheme
(linear-array 3 14 0 0
  (translate -14 0 -2 (cylinder 2.1 10)))

(grid-array 2 3 16 10
  (translate -16 -5 4 (cylinder 1.2 8)))

(radial-array 6 60 11
  (translate 0 0 4 (cone 1.8 0.8 5)))
```

Use these when the pattern is regular. Use `map` and `range` when each instance needs custom math.

### Woodlouse hotel

This habitat uses one generated entrance list plus repeated shelves and dividers. Shared dimensions keep openings aligned when chamber count or overall width changes.

<!-- render-source: ../examples/woodlouse-hotel.ecky -->

![Rendered output for Real Model Patterns: Procedural Cuts and Arrayed Frames, example 3](assets/10-real-model-patterns-03.png)

The entrances are generated from one parametric chamber count:

```scheme
(shape entrances
  (apply union
    (map
      (lambda (cell)
        (let* ((col (- cell (* chamber_cols (floor (/ cell chamber_cols)))))
               (row (floor (/ cell chamber_cols)))
               (x (+ (* -0.5 hotel_w) wall (* (+ col 0.5) col_gap)))
               (z (+ wall (* (+ row 0.55) floor_gap))))
          (translate x (* -0.5 hotel_d) z
            (rotate 90 0 0
              (cylinder entrance_r (+ hotel_d 6))))))
      (range 0 (* chamber_cols 3)))))
```

`chamber_cols` drives both cutter count and divider spacing. `col_gap` is derived from `hotel_w` and `chamber_cols`, so openings stay centered when the model is resized.
## Level 05: Perforated Toothbrush Holder

**Mission:** Build a shelled product, prove one custom cutter, then repeat it across curved walls.

**Clear condition:** All four checkpoints compile and the final boolean is one body minus one generated cutter group.

Imagine a tiny umbrella stand for a hedgehog who keeps bringing wet toothpicks indoors. Build its hollow body first, then one drain and one wall cutter. Only repeat a cutter after it demonstrably crosses the wall.

Small examples teach syntax. This project teaches control: preserve one useful
body while shelling it, closing it, proving a custom cutter, then generating
dozens of cutters without turning the source into copied geometry.

Every stage is a complete `.ecky` file. Run a checkpoint before reading the
next one with `src-tauri/target/debug/ecky check <checkpoint.ecky>`.

The manifest at
`docs/books/ecky-ir/examples/toothbrush-holder/manifest.json` keeps the stages
ordered and machine-checkable.

### Stage 1: subtract one named cavity

The outside is a union of a centered cylinder and a rear box. The cavity uses
the same construction with dimensions derived from `wall_thickness`.

```scheme
(model
  (params
    (number width 36.0 :min 20.0 :max 60.0 :label "Width")
    (number depth 25.0 :min 15.0 :max 50.0 :label "Depth")
    (number wall_thickness 2.6 :min 1.5 :max 5.0 :label "Wall thickness")
    (number height 85.0 :min 40.0 :max 150.0 :label "Height"))
  (part toothbrush_holder
    (let* ((radius (/ width 2.0))
           (inner_width (- width (* 2.0 wall_thickness)))
           (inner_depth (- depth wall_thickness))
           (inner_radius (- radius wall_thickness))
           (outer
             (union
               (box width depth height :align '(center max center))
               (cylinder radius height :align '(center center center))))
           (cavity
             (union
               (box inner_width inner_depth (+ height 10.0)
                 :align '(center max center))
               (cylinder inner_radius (+ height 10.0)
                 :align '(center center center)))))
      (difference outer cavity))))
```

Canonical checkpoint:
`docs/books/ecky-ir/examples/toothbrush-holder/01-shell.ecky`.

The cavity is taller than the body. That deliberate overrun avoids coincident
top and bottom faces. `wall_thickness` owns every fit-critical offset; there is
no second anonymous wall value waiting to drift.

### Stage 2: close the base, then reopen a drain

Stage 1 is a tube. Stage 2 adds a bottom blank at `(- (/ height 2.0))`, cuts a
drain through it, then unions the drained bottom with the shell.

```text
(bottom_blank
  (translate 0 0 (- (/ height 2.0))
    (union
      (box width depth wall_thickness :align '(center max min))
      (cylinder radius wall_thickness :align '(center center min)))))
(drain
  (translate 0 0 (- (/ height 2.0) 1.0)
    (cylinder drain_radius (+ wall_thickness 2.0)
      :align '(center center min))))
(bottom (difference bottom_blank drain))
```

Canonical checkpoint:
`docs/books/ecky-ir/examples/toothbrush-holder/02-drained-base.ecky`.

The drain begins 1 mm below the bottom and ends 1 mm above it. Cutters should
cross material completely; exact tangency is not a useful manufacturing
constraint.

### Stage 3: prove one custom cutter

The wall pattern starts as a closed `polygon`, becomes a solid with `extrude`,
then receives a frame with `location` and `place`. Only one cutter is
subtracted.

```text
(spade_profile
  (polygon
    ((0.0 4.7) (-3.05 1.5) (-3.25 -0.8) (-2.15 -1.95)
     (-0.55 -3.35) (-1.15 -5.35) (1.15 -5.35)
     (0.55 -3.35) (2.15 -1.95) (3.25 -0.8) (3.05 1.5))))
(spade
  (place
    (location (plane :origin '(0.0 0.0 0.0))
      :rotate '(-90.0 0.0 180.0))
    (extrude spade_profile cutter_depth)))
(front_cutter (translate 0.0 front_cutter_y 0.0 spade))
```

Canonical checkpoint:
`docs/books/ecky-ir/examples/toothbrush-holder/03-single-cutter.ecky`.

Do not start with the full pattern. A wrong profile, axis, or cutter depth
multiplied fifty times only makes the failure slower to understand.

### Stage 4: generate cutters, subtract once

The production model uses nested `repeat-union` forms for staggered front,
left, and right rows. Row and column indices feed local `let*` bindings for
angle, side offset, and height. Those groups become `spade_holes`.

The final intent stays small:

```text
(spade_holes (union front_spades left_spades right_spades))
(final (difference (union shell bottom) spade_holes))
```

The planner flattens the generated tool union into one n-ary `difference`, so
independent cutters can be prepared in parallel before OCCT performs the
boolean. Source still describes one semantic cut; runtime chooses the execution
schedule.

Canonical complete model:
`src-tauri/tests/fixtures/cad/perf/toothbrush_holder_versions.ecky`.

This is the reusable project shape:

**1. Expose physical controls.** Parameters describe user decisions.

**2. Derive dimensions once.** Put dependent math in `let*`.

**3. Name body and cutter stages.** Final expressions should read as intent.

**4. Prove one repeated unit.** Debug one profile, axis, and overrun.

**5. Generate the remaining units.** Use `repeat` or arrays.

**6. Keep the final boolean boring.** One body, one cutter group, one cut.

**Watch for:** a repeated cutter that only touches a wall may disappear or
split topology unpredictably. Give cutter depth explicit overrun and keep safe
top/bottom margins as named bindings.
## Level 06: Film Adapter

**Mission:** Read and modify a production-scale multipart mechanism with named fit relations.

**Clear condition:** A fit parameter changes both mating sides while preview-only placement leaves export geometry unchanged.

This is an advanced mechanism, beyond the farm finish line. Treat it like a camera booth for a very demanding beetle: name each interface, isolate its fit, and resist changing three clearances at once.

The final example is a multi-part film adapter with sliding rail joints and a two-start helicoid. Its base, insert stack, tunnel, cover, and lens carrier share fit dimensions but remain separate printable parts.

<!-- render-source: ../examples/ecky-integrated-film-adapter-open-helicoid-v9.ecky -->

![Rendered output for Final Model: Integrated Film Adapter Open Helicoid v9, example 1](assets/11-complex-film-adapter-01.png)

Full source: `docs/books/ecky-ir/examples/ecky-integrated-film-adapter-open-helicoid-v9.ecky`. The sections below isolate the six mechanical subsystems.

### 1. Public controls define physical fit

The first block exposes dimensions that matter after printing: film format, aperture, rail geometry, insert stack, film gap, lens bore, and helicoid thread geometry.

```scheme
(params
  (select film_format "120_645" :label "film format"
    :options (("120 6x9" "120_6x9") ("120 6x6" "120_6x6")
              ("120 6x4.5" "120_645") ("135 36x24" "135") ("110" "110")))
  (number rail_tip_w 5.4 :label "joint max W" :min 3.5 :max 8 :step 0.1)
  (number rail_h 4.2 :label "joint H" :min 2 :max 6 :step 0.1)
  (number fit_clearance 0.25 :label "fit clearance" :min 0 :max 0.8 :step 0.05)
  (number film_gap 0.6 :label "film velvet gap" :min 0.1 :max 1.5 :step 0.05)
  (number lens_bore_d 59.6 :label "lens bore D" :min 50 :max 68 :step 0.1)
  (number thread_turns 3.2 :label "helicoid turns" :min 1.5 :max 5 :step 0.1)
  (number thread_clearance 0.25 :label "helicoid clearance" :min 0.15 :max 0.6 :step 0.05))
```

This is the same habit as earlier chapters: public parameters are physical, not arbitrary. `fit_clearance` appears in rail channels and detents. `film_gap` controls the clamp stack. `lens_bore_d`, `thread_turns`, and `thread_clearance` drive the helicoid interface.

### 2. Base makes recessed pockets and male rails

The base starts as a rounded plate, removes the aperture and insert pocket, then adds male triangular rail profiles on both long sides.

```scheme
(part base_recessed_male_rails
  (build
    (shape raw_plate
      (extrude (rounded-rect outer_w outer_h corner_r) base_h))
    (shape aperture_cut
      (translate 0 0 -0.1
        (box aperture_w aperture_h (+ base_h 0.2))))
    (shape frame_pocket
      (translate 0 0 (- base_h pocket_depth)
        (extrude
          (rounded-rect (+ holder_w (* 2 fit_clearance))
                        (+ holder_h (* 2 fit_clearance))
                        holder_corner_r)
          (+ pocket_depth 0.2))))
    (shape plate
      (difference raw_plate aperture_cut frame_pocket film_path_cut))
    (shape rail_left
      (translate (- (/ outer_w 2)) rail_y rail_z
        (rotate 0 90 0
          (extrude rail_profile_pos outer_w))))
    (result
      (fuse plate rail_left rail_right detent_top_left detent_top_right
            detent_bottom_left detent_bottom_right))))
```

`rail_profile_pos` and `rail_profile_neg` are small triangular sketches. They become long rails by `extrude`, then get fused onto the base. This is the same sketch-to-extrude move from chapter 2, applied to sliding joints.

### 3. Film insert is a two-piece stack

The lower insert carries the film guides. The upper insert clamps above the film gap. Both use the selected film format to derive `frame_w`, `frame_h`, and `film_strip_w`.

```scheme
(shape frame_w
  (if (= film_format "135") 36
    (if (= film_format "110") 17
      (if (= film_format "120_645") 42
        (if (= film_format "120_6x6") 56 84)))))
(shape guide_top
  (translate 0 (/ film_channel_h 2) (- (+ holder_thickness (/ film_guide_h 2)) 0.24)
    (box (- holder_w 8) film_guide_rail_w film_guide_h)))
(shape lower_frame
  (difference
    lower_raw
    aperture_cut
    notch_top_left
    notch_top_right
    notch_bottom_left
    notch_bottom_right))
```

The insert stack is why the model has `holder_thickness`, `film_gap`, and `insert_lid_thickness` as separate controls. Those are real Z layers, not a single magic height.

### 4. Tunnel joins bottom and top modules

The tunnel module has both sides of the sliding interface. Its bottom cuts female channels so it can slide onto the base rails. Its top adds male rails so the top cover can slide onto the tunnel.

```scheme
(part tunnel_female_bottom_male_top
  (build
    (shape channel_profile_pos
      (polygon
        (((/ (+ rail_h (* 2 fit_clearance)) 2) 0)
         (0 (/ (+ rail_tip_w (* 2 fit_clearance)) 2))
         ((- (/ (+ rail_h (* 2 fit_clearance)) 2)) 0))))
    (shape body
      (difference body_blank tunnel_cut))
    (shape channel_left
      (translate (- (+ (/ outer_w 2) lead_in)) rail_y channel_z
        (rotate 0 90 0
          (extrude channel_profile_pos (+ outer_w (* 2 lead_in))))))
    (shape rail_left
      (translate (- (/ outer_w 2)) rail_y rail_z
        (rotate 0 90 0
          (extrude rail_profile_pos outer_w))))
    (result
      (fuse
        (difference body channel_left channel_right)
        rail_left
        rail_right))))
```

This is the sliding-joint core. Female channels are oversized by `fit_clearance`; male rails use the nominal profile. The book built these ideas earlier as sketches, cuts, and named clearances. Here they become a printable mechanical interface.

### 5. Top cover is open and owns the female helicoid

The cover removes matching rail channels and opens the center so the helicoid socket is visible. The female thread is modeled as two clipped helical ridges subtracted from a sleeve.

```scheme
(shape female_thread_a_raw
  (translate 0 0 (+ socket_base_z thread_z0)
    (helical-ridge
      :radius female_root_r
      :pitch thread_pitch
      :height thread_len
      :base-width female_axial_width
      :crest-width (* female_axial_width 0.58)
      :depth female_depth)))
(shape female_thread_a
  (clip-box female_thread_a_raw
    :x ((- female_thread_clip_r) female_thread_clip_r)
    :y ((- female_thread_clip_r) female_thread_clip_r)
    :z ((+ socket_base_z 0.05) (+ socket_base_z sleeve_h 1))))
(shape female_thread_b
  (rotate 0 0 180 female_thread_a))
(shape socket_threaded_shell
  (difference
    (translate 0 0 socket_base_z
      (cylinder socket_outer_r sleeve_h))
    female_thread_a
    female_thread_b))
```

`thread_pitch` comes from carrier height and turn count. `female_thread_b` is the second start, made by rotating the first. The clipped ends keep the helix printable and bounded inside the socket height.

### 6. Moving lens carrier matches the cover

The carrier is separate and previewed to the side with `carrier_preview_x`. It uses the same thread pitch, height, and clearance math, but its ridges are fused onto the carrier body instead of cut out of the socket.

```scheme
(shape male_thread_a_raw
  (translate 0 0 thread_z0
    (helical-ridge
      :radius ridge_root_r
      :pitch thread_pitch
      :height thread_len
      :base-width thread_width
      :crest-width (* thread_width 0.58)
      :depth ridge_sweep_depth)))
(shape male_thread_a
  (clip-box male_thread_a_raw
    :x ((- thread_clip_r) thread_clip_r)
    :y ((- thread_clip_r) thread_clip_r)
    :z (0 carrier_h)))
(shape carrier_outer
  (fuse carrier_body male_thread_a male_thread_b))
(result
  (translate carrier_preview_x 0 socket_base_z
    (difference carrier_outer stop_aperture lens_slip_bore)))
```

That last `translate` is preview layout, not fit math. The carrier is offset so the reader can see both halves of the helicoid in one render.

### Combined mechanism

The mechanism combines earlier patterns directly: profiles become rails and channels; named clearances control sliding fits; repeated structures stay parametric; frames place mating geometry; verification records fit requirements. The carrier threads into the cover while the remaining modules stack through rail interfaces.
