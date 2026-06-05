## Final Model: Integrated Film Adapter Open Helicoid v9

The last model is `Ecky integrated film adapter open helicoid v9`. It is not a single decorative adapter. It is an assembly built from sliding parts: a recessed base with male rails, a lower insert, an upper clamp, a tunnel module with female-bottom and male-top joints, an open top cover with the female helicoid socket, and a separate moving lens carrier with matching male helicoid threads.

<!-- render-source: ../examples/ecky-integrated-film-adapter-open-helicoid-v9.ecky -->

![Rendered output for Final Model: Integrated Film Adapter Open Helicoid v9, example 1](assets/11-complex-film-adapter-01.png)

The source is stored as `docs/books/ecky-ir/examples/ecky-integrated-film-adapter-open-helicoid-v9.ecky`. The chapter reads it in layers instead of dumping all 493 lines at once.

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

### What the whole book was building toward

The early ball and plate examples taught primitives and extrusion. The plate-with-hole examples taught profiles and cuts. The parameter chapter made fit dimensions editable. The repetition and placement chapters introduced authored structure instead of copied solids. The final model uses all of that for a real mechanism: rails slide into channels, film inserts locate inside a recessed pocket, the tunnel stacks onto the base, the open cover stacks onto the tunnel, and the lens carrier threads into the cover through a two-start helicoid.
