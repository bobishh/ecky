# Complex Model Walkthrough: Film Scanning Adapter Helicoid

This chapter dissects one real Ecky IR model from parameter sheet down to boolean finish. Goal: show how a non-trivial assembly stays readable, tuneable, and manufacturable without dropping into opaque backend code.

### Why this model

- mixes 2D profiles, solid primitives, helical features, clipping, and booleans
- contains two mating parts with shared fit logic
- derives many dimensions from a smaller trusted parameter set
- demonstrates why `build` plus named `shape` bindings matter

### Full model source

```scheme
{{COMPLEX_MODEL_SOURCE}}
```

### Part map

- `top_cover_integrated_helicoid`: top plate with rail channels plus female helicoid socket
- `moving_lens_carrier`: inner moving carrier with mating male thread
- `params`: one public control surface for outer envelope, rail fit, lens bore, thread geometry, and print clearance

### Parameter strategy

The model keeps raw user knobs near physical intent, not near backend operations.

- envelope controls: `outer_w`, `outer_h`, `adapter_h`, `sleeve_h`, `carrier_h`
- fit controls: `fit_clearance`, `thread_clearance`, `joint_sink`
- optical interface controls: `lens_bore_d`, `lens_stop_lip_w`, `lens_stop_h`
- thread controls: `thread_turns`, `thread_depth`, `thread_width`, `thread_start_z`

This split matters. Public parameters describe measurable hardware decisions. Derived bindings translate those decisions into local radii, clip bounds, sweep depths, and bore offsets.

### Build pattern

Each part follows same loop.

- start with absolute reference planes like `top_z` and `socket_base_z`
- derive secondary radii from public parameters
- build primitive solids and profiles
- generate threads in overbuild form
- trim threads with `clip-box`
- subtract bores and channels
- fuse only at final assembly boundary

This is Ecky IR at usable scale: geometry becomes small named facts instead of one unreadable boolean expression.

### Female socket construction

The socket side shows strongest value from named bindings.

- `lens_slip_r`, `carrier_core_r`, `socket_bore_r`, `socket_outer_r`: radius ladder from lens interface outward
- `female_path_r`, `female_depth`, `female_root_r`: thread path geometry derived from fit and overlap rules
- `female_axial_width`: thread ridge thickness widened by clearance budget

Then model builds thread in three phases.

- `female_thread_a_raw`: create overlong helix with `helical-ridge`
- `female_thread_a`: clip usable window with `clip-box`
- `female_thread_b`: rotate first helix 180 degrees for second start

Two-start thread appears without copy-paste geometry. One authored helix plus rotation builds symmetric mate.

### Why clip before subtraction

`helical-ridge` deliberately overshoots in Z so thread lead-in stays clean across parameter changes. Raw helix can protrude through plate or beyond sleeve cap. `clip-box` cuts it back to manufacturable bounds before subtraction from socket shell.

That order prevents three failure classes.

- accidental breakthrough above top plate
- ragged thread start below socket base
- backend-specific boolean instability from far-overlapping bodies

### Socket shell boolean stack

Socket body becomes readable because each boolean has one job.

- `socket_threaded_shell`: outer cylinder minus both helices
- `socket_bore_cut`: clean central bore cylinder
- `socket_shell`: threaded shell minus central bore
- `raw`: plate minus rail channels and center opening, then fused with socket shell

When boolean stacks get longer than this, split again. Rule: every subtraction target should have semantic name.

### Moving carrier construction

Carrier mirrors socket logic but flips boolean polarity.

- start from `carrier_body` cylinder
- build one male ridge helix, clip it, duplicate with 180-degree rotation
- fuse helices onto body instead of subtracting
- cut `stop_aperture` and `lens_slip_bore` after outer thread exists

Same parameter family drives both parts. That keeps mating geometry coherent during tuning.

### Fit logic

Three values dominate mechanical behavior.

- `thread_clearance`: radial slack between male and female thread systems
- `fit_clearance`: sliding fit for top rail channels
- `ridge_overlap`: fixed overlap that keeps thread engagement printable and durable

Model does not hide fit in magic offsets buried deep in booleans. Fit lives in named bindings near derived radii. That makes failures diagnosable.

### Reading derived bindings

Look for three binding classes.

- reference bindings: zero points and anchor planes like `top_z`
- interface bindings: physical contact dimensions like `lens_slip_r`
- manufacturing bindings: safe print or assembly adjustments like `female_thread_clip_r`

If a new binding does not fit one class, naming usually needs work.

### Change workflow

Safe edit order for models like this:

- tune public `params` first
- inspect derived radius ladder next
- verify thread clip bounds after any height or pitch change
- edit booleans last

Reverse order causes hidden regressions. Most bad edits happen when someone changes subtraction geometry before checking derived fit math.

### Reusable authoring lessons

- put interface math in named `shape` bindings
- build overlong procedural geometry, then trim explicitly
- derive mates from shared parameters, not duplicated constants
- keep one boolean purpose per binding
- duplicate repeated structure with transforms, not hand-copied solids

### Where to extend this model

- add `verify` clauses for thread clearance and lens-stop minimum wall
- extract shared helicoid math into helper functions once reuse appears in second model
- convert fixed `ridge_overlap` into public parameter only if fabrication process proves it variable

Complex Ecky IR stays tractable when structure names decisions before geometry finalizes.
