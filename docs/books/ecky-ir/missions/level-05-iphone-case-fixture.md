---
id: mission-05-iphone-case-fixture
title: Read a multipart phone case
---

# Read a multipart phone case

This is a source walkthrough of a three-part case: a TPU shell and two PETG camera-frame inserts. It is an earlier design than the single-piece case on the landing page. Use the file linked here so the part names match the text.

[Download the three-part case](../../../../sites/landing/src/models/iphone-17e-voronoi-case.ecky)

## Identify the three parts {#materials}

Search for `(part` in the file. The TPU part surrounds the phone. The inner PETG island seats against it; the outer snap island captures the camera opening from the other side. Their fit depends on both thickness and the shapes of their mating grooves.

The source is long. Read its parameter declarations and part boundaries first, then return to individual cuts when the surrounding shape is clear.

## Follow the phone pocket {#phone-shell}

The `iphone-17e-tpu-case` part starts with phone dimensions, then derives its pocket and outside. `phone-pocket-clearance` adjusts space around the phone; wall and rear-panel thickness add material outside that pocket.

Find the USB and button cutters after the shell. Their sizes and positions are separate from the pocket. A wider case does not automatically imply a wider USB opening.

## Read one lattice segment {#lattice}

The rear lattice calls one component many times. This is its complete definition:

```scheme
(define-component lattice-strut((number x1 0.0)
   (number y1 0.0)
   (number x2 10.0)
   (number y2 0.0)
   (number width 2.0)
   (number height 1.8))

  (let* (
    (mid-x (/ (+ x1 x2) 2.0))
    (mid-y (/ (+ y1 y2) 2.0))
  )
  (extrude
    (slot-center-point mid-x mid-y x2 y2 width)
    height)))
```

The endpoints determine the segment midpoint. `slot-center-point` draws a rounded strip from that midpoint toward the second endpoint; `extrude` gives it thickness.

The calls below the component contain explicit endpoint data. Changing `back-voronoi-web-width` changes the strut width, but does not generate a new cell graph. To change the pattern, edit the endpoint data or replace it with a generator.

## Follow the camera-frame thickness {#islands}

The file defines a small helper for the inner insert:

```scheme
(define (flush-insert-thickness panel-thickness captured-thickness)
  (- panel-thickness captured-thickness))
```

For a 1.8 mm rear panel capturing 0.8 mm of TPU, the inner insert is 1 mm thick. That subtraction keeps the stack flush. Increasing one layer without reducing the other changes the total thickness.

The camera, microphone, and flash centers are shared by the TPU cuts and PETG components. Follow those shared coordinates before moving an individual opening.

## Inspect the assembled stack {#case-solution}

The preview places the PETG pieces beside the TPU shell to expose their shapes. Those offsets help inspection; they are not their installed positions. Check the mating groove, skirt, and captured thickness in source as well as in the view.

`lens-clamp-fit-clearance` and `lens-clamp-snap-interference-radius` serve different purposes: one provides space, the other sets snap engagement. Changing both together makes it harder to identify the cause of a tight fit.

## Print a small camera-frame sample {#fit-warning}

A useful test includes the TPU seat and both PETG mating features. Keep the full model's thicknesses and clearances in the sample. Test insertion and removal with the actual materials before printing the whole case.
