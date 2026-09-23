---
id: mission-06-film-scanner
title: Read the film scanner assembly
---

# Read the film scanner assembly

The film scanner combines rails, film inserts, a tunnel, a cover, and a threaded lens carrier. This chapter follows three settings through the source: rail clearance, film format, and thread clearance.

[Download complete scanner](../../../../sites/landing/src/models/film-scanner.ecky)

## Locate the mating dimensions {#interfaces}

Find `fit_clearance`, `film_format`, and `thread_clearance` in the parameter block. They adjust different interfaces. Rail clearance affects sliding channels; film format selects aperture dimensions; thread clearance affects the carrier and socket.

Start with one setting at a time. A changed film aperture should not require moving the lens carrier or changing its thread.

## Derive the channel profile {#rail-fit}

[Open the rail-profile study](../projects/06-film-scanner/01-worked-fit-profile.ecky). The channel derives its dimensions from the rail:

```scheme
(shape channel_h (+ rail_h (* 2 fit_clearance)))
(shape channel_w (+ rail_tip_w (* 2 fit_clearance)))
```

With `rail_h = 4.2`, `rail_tip_w = 5.4`, and `fit_clearance = 0.25`, these become 4.7 and 5.9 mm. Set clearance to 0.35: the receiving profile grows by 0.2 mm in both dimensions while the rail stays nominal.

The profile study isolates the cross-section math. Its rail and channel use different extrusion axes; inspect their transforms before treating them as an assembled pair.

## Select a film aperture {#format}

[Open the scanner subassembly](../projects/06-film-scanner/03-solution-scanner-subassembly.ecky). Its width branch is:

```scheme
(shape frame_w
  (if (= film_format "135") 36
    (if (= film_format "120_645") 42 84)))
```

The height branch gives 24 mm for 135 and 56 mm for both 120 options. Thus the apertures are 36 × 24, 42 × 56, and 84 × 56 mm. The insert blank adds a border around those dimensions; the aperture cutter uses them directly.

Change the format control and inspect both the opening and its surrounding insert. If you change the cutter alone, the border no longer follows the chosen format.

## Follow the lens thread {#scanner-final}

In the complete scanner, find `helical-ridge`. The carrier adds male ridges to a cylindrical body; the socket subtracts matching female ridge geometry. Pitch, ridge depth, and clearance must agree on both sides.

Two starts are made by rotating a second ridge by 180 degrees. This is a second helix at the same pitch, not a pitch change. Follow the shared pitch binding before editing either ridge.

The source also places parts apart for inspection. An ordinary `translate` inside a part affects its exported geometry. Only an authored `view` offset is preview-only; a variable named `preview` does not change that rule.

## Inspect the calibration study {#coupon}

[Download helicoid calibration study](../projects/06-film-scanner/04-calibration-helicoid-patterns.ecky)

This file includes a socket, repeated stops, a sampled knob, and comparison shapes. It is a geometry study, not a ready-made two-piece fit coupon. `repeat-union` joins repeated stops; `repeat-compound` groups witness ticks without joining them.

For a physical thread test, use a short section of both the carrier and its matching socket from the complete model. Preserve pitch, profile, and clearance.

## Record the printed fit {#print}

Test a rail/channel pair and a carrier/socket pair with the intended material and orientation. Record the clearance that slides and the thread clearance that turns. Apply those values to the full scanner, then recheck the aperture and part placement before exporting.
