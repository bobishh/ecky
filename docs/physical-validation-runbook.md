# Physical Validation Runbook

Scope: close remaining `Open Physical Decisions` with measurable print loop.
Fixture source: `model-runtime/examples/physical-decision-calibration.ecky`.

## Preconditions

1. Print coupon model with default params from fixture.
2. Use same filament/material for all coupons in one run.
3. Record printer profile values used for print.
4. Measure with calibrated digital caliper (0.01 mm resolution).

## Decision Protocols

### 1) Magnet Size + Thickness

Coupon part: `calibration_magnet_coupon`.

Protocol:
1. Measure real magnet diameter at 3 angles; compute mean.
2. Measure real magnet thickness at 3 points; compute mean.
3. Dry-fit magnets into both pockets (`pocket_n`, `pocket_s`).
4. Mark fit result: `press_fit`, `slip_fit`, or `no_fit`.

Param mapping:
- `magnet_d` <- measured mean diameter.
- `magnet_thickness` <- measured mean thickness.
- Keep `magnet_pocket_d`, `magnet_pocket_h` as design values to tune.

Pass/fail criteria:
- Constraint pass: `magnet_pocket_d >= magnet_d`.
- Constraint pass: `magnet_pocket_h >= magnet_thickness`.
- Physical pass: both pockets accept magnet with target fit class chosen by team.

### 2) Film Stock Thickness

Coupon part: `calibration_film_clamp_coupon`.

Protocol:
1. Cut 5 film strips from actual stock.
2. Measure each strip thickness at 3 points; compute per-strip mean.
3. Compute global mean of 5 strips.
4. Insert strip into `film_slot`; verify no buckle and no jam.

Param mapping:
- `film_thickness_target` <- global mean thickness.
- `film_gap` <- tuned slot gap from fit outcome.
- `film_gap_min` <- lower safety bound decided by team.

Pass/fail criteria:
- Constraint pass: `film_gap >= film_gap_min`.
- Constraint pass: `film_gap >= film_thickness_target`.
- Physical pass: strip slides full travel without visible curl damage.

### 3) Clamp Force Proxy

Coupon part: `calibration_film_clamp_coupon` (`spring_pad`).

Protocol:
1. Assemble coupon with film strip inserted.
2. Use pull gauge or hanging-weight method to measure extraction force (N).
3. Take 5 pulls; compute mean.
4. Convert measured force to proxy scale used by team (document conversion).

Param mapping:
- `clamp_force_proxy` <- converted mean proxy value.
- Bounds: `clamp_force_min`, `clamp_force_max` remain acceptance window.

Pass/fail criteria:
- Constraint pass: `clamp_force_proxy >= clamp_force_min`.
- Constraint pass: `clamp_force_proxy <= clamp_force_max`.
- Physical pass: film retained during shake test and removable without tearing.

### 4) Lens OD + Tolerance

Coupon part: `calibration_lens_thread_coupon`.

Protocol:
1. Measure lens barrel OD at 8 angular positions and 2 axial bands.
2. Compute max OD, min OD, and mean OD.
3. Set tolerance as `(max OD - min OD) / 2` unless lab standard overrides.
4. Test bore insertion/rotation in coupon.

Param mapping:
- `lens_barrel_od_measured` <- mean OD.
- `lens_od_tolerance` <- computed or standard tolerance.
- `lens_fit_floor` <- minimum accepted effective bore floor.
- `lens_bore_d` <- printed bore target.

Pass/fail criteria:
- Constraint pass: `lens_fit_floor >= lens_barrel_od_measured`.
- Constraint pass: `lens_bore_d >= lens_fit_floor`.
- Constraint pass: `lens_od_tolerance <= lens_od_tolerance_max`.
- Physical pass: lens inserts fully, no crack, rotational drag inside target band.

### 5) Nozzle + Layer Pair

Coupon part: `calibration_lens_thread_coupon` (`gauge_notch` uses `nozzle_d`, `layer_h`).

Protocol:
1. Record actual nozzle diameter from installed nozzle spec or pin gauge.
2. Print coupon with intended layer height.
3. Inspect gauge notch edge quality and thread flank consistency.
4. If artifacts appear, step layer height down and reprint.

Param mapping:
- `nozzle_d` <- installed nozzle diameter.
- `layer_h` <- selected layer height.
- `layer_h_max` <- process ceiling for quality.

Pass/fail criteria:
- Constraint pass: `layer_h <= layer_h_max`.
- Constraint pass: `layer_h <= nozzle_d`.
- Physical pass: no severe stair-stepping/chatter on coupon critical faces.

### 6) Thread Clearance (Post-Coupon)

Coupon part: `calibration_lens_thread_coupon` (`gauge_notch` width maps clearance).

Protocol:
1. Print coupon series across clearance values around baseline.
2. Assemble mating thread pair from production geometry.
3. For each clearance, measure torque-to-start and full engagement depth.
4. Pick lowest clearance with no cross-thread and repeatable full seat.

Param mapping:
- `thread_clearance` <- chosen winning clearance from series.
- `thread_clearance_min` <- hard lower bound after test evidence.

Pass/fail criteria:
- Constraint pass: `thread_clearance >= thread_clearance_min`.
- Physical pass: thread starts by hand, no galling, full seat achieved in repeated trials.

## Run Constraints Validation

After each parameter update:
1. Run `ecky_constraints_validate` on updated model.
2. Verify all relation rows pass.
3. If fail, fix offending param pair before next print.

## Print Result Log Template

| Run ID | Date | Material | Nozzle (`nozzle_d`) | Layer (`layer_h`) | Decision Area | Coupon Part | Raw Measurements | Derived Value | Param Updates | Constraint Check | Physical Verdict | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| PV-001 | YYYY-MM-DD | PLA/PETG/... | 0.40 | 0.20 | Magnet | calibration_magnet_coupon | d1,d2,d3; h1,h2,h3 | mean_d, mean_h | `magnet_d=...`, `magnet_thickness=...` | pass/fail | pass/fail | |
| PV-002 | YYYY-MM-DD | PLA/PETG/... | 0.40 | 0.20 | Film | calibration_film_clamp_coupon | t(5x3 pts) | mean_t | `film_thickness_target=...`, `film_gap=...` | pass/fail | pass/fail | |
| PV-003 | YYYY-MM-DD | PLA/PETG/... | 0.40 | 0.20 | Clamp | calibration_film_clamp_coupon | pull force x5 | mean_force/proxy | `clamp_force_proxy=...` | pass/fail | pass/fail | |
| PV-004 | YYYY-MM-DD | PLA/PETG/... | 0.40 | 0.20 | Lens OD | calibration_lens_thread_coupon | OD grid (16 pts) | mean/max/min/tol | `lens_barrel_od_measured=...`, `lens_od_tolerance=...`, `lens_bore_d=...` | pass/fail | pass/fail | |
| PV-005 | YYYY-MM-DD | PLA/PETG/... | 0.40 | 0.20 | Thread | calibration_lens_thread_coupon | torque/depth by clearance | chosen clear | `thread_clearance=...` | pass/fail | pass/fail | |
