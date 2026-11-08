# Live paper sketch

## Required behavior

iPhone Safari streams its camera continuously over WebRTC to Ecky's local TLS server. Rust decodes video, rectifies a marked sheet, recognizes black pen centerlines, and owns capture state and geometry identity. The Sketch workspace shows the corrected paper below its grid and editable points, lines, circles, and polylines.

Ecky supplies an A4 PDF with four identified perimeter fiducials, a blank 160 × 220 mm drawing region, and a 50 mm print-scale check. Marker positions establish perspective and millimeter coordinates. Printing instructions require actual size and checking the scale segment.

Stable, unambiguous observations enter automatically after repeated agreement. Joins, crossings, broken strokes, and uncertain fits remain reviewable candidates with confirm, edit, and reject actions. Accepted elements retain identity through later frames, occlusion, and local draft restoration after restart. Missing observations preserve accepted geometry. Marker loss pauses geometry updates and shows an actionable state. Session replacement and stopping invalidate late frames and close media resources. Processing retains only the newest waiting frame.

Accepted geometry can enter the existing Sketch editing and normal model-application flow. Video previews and observations remain transient; frames do not create model versions. Recognition and media processing remain local.

## Acceptance criteria

- Given a browser camera fixture containing the marked sheet, when paired through real WebRTC, Rust receives and decodes continuous video and Sketch displays corrected paper with metric geometry.
- Given known line, circle, closed outline, and point fixtures under perspective and modest noise, recognition recovers editable primitives within the declared fitting tolerance. A library comparison uses representative fixtures and records quality and native packaging evidence.
- Given repeated or stale frames, accepted geometry keeps its IDs and count. Given an occluding hand or missing markers, accepted geometry survives and marker loss pauses updates. Reacquisition resumes without duplicates.
- Given T/X crossings or broken strokes, candidates require review. Confirmed or edited geometry remains editable and can be transferred to Sketch; rejected observations do not immediately reappear.
- Given a stopped or replaced session, late results cannot change current geometry. Invalid pairing or unsupported media returns an actionable error. Loading, errors, and marker state changes preserve control positions.
- Local checks cover real RTC transport and Rust pixel recognition, session invariants, UI happy path and marker-loss/review state, and the printable PDF. Physical iPhone permission, TLS trust, camera performance, and screen behavior are reported as device validation when no device test has run.
