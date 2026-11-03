# Drawing photo import

## Product contract

Import a photograph or scan of an engineering drawing into the existing Sketch workspace. Automatically recover editable geometry, overlay it on the source image, and let the user correct and accept a profile before creating a normal model version. Merely displaying an image or tracing the two boundaries of a thick printed stroke does not satisfy this feature.

The initial supported geometry is straight segments, closed polygon outlines, and full circles. Other curves may remain explicitly approximate polylines. This feature does not infer unseen views or automatically reconstruct a unique solid from multiple views. Dimension text is not authoritative: the user supplies a known measurement and confirms the geometry used for modeling.

## User flow

1. In the expanded SKETCH workspace, choose Drawing photo and open a PNG, JPEG, or WebP image. Keep the FRONT / TOP / SIDE / ALL THREE view selector and independent view state.
2. Select the relevant image region and assign it to a view. A sheet can supply several regions, or each view can use a different image. Recognition operates on the assigned region, not on the entire sheet's title block and neighboring views.
3. For an oblique photograph, select four corners of a known rectangular region and supply its width-to-height ratio or physical width and height. Rectify that plane before measuring. For a flat scan, crop and rotation suffice. Arbitrary four corners without a known rectangular aspect must not be presented as metric perspective correction.
4. Select two points with a known distance, enter the positive distance in millimeters, and preview the calibrated measurement. One uniform scale applies after rectification. Do not stretch X and Y independently to make a drawing fit.
5. Select Recognize. Show automatically recovered segments, circles, and candidate closed profiles over the image. The original remains available with an opacity control. Each view has independent progress, errors, and geometry.
6. Review the candidates: select or exclude a contour, remove spurious geometry, edit segment endpoints or circle center/radius numerically, and repair a contour using the existing drawing tools. Open chains and ambiguous junctions remain visible issues. No silent gap closing, rectangular bounding-box substitution, or automatic acceptance of text as a feature.
7. Accept the selected profile, enter clearance and extrusion depth, then Apply using the existing normal version workflow. A circle stays an analytic circle in the generated CAD geometry. Image-derived provenance remains approximate even when fitted primitives are analytic.

Recognition must create useful editable geometry before manual correction. The user must not have to redraw every line for the acceptance scenario to succeed.

## Domain and ownership

One Rust drawing-import module owns image loading, bounded decoding, coordinate transforms, calibration, recognition, candidate validation, and import provenance. Its primary concepts are image, view region, calibration, primitive, profile, and evidence. Frontend state selects and edits a temporary proposal; it does not create a second model history or own rendering orchestration.

Reuse the image formats and resource limits in `raster_trace.rs`, the existing Sketch panes and edit interactions, and the guarded profile feature apply service. Keep the old filled-raster contour extractor available for its existing consumers. Its dark-pixel outlines are not the drawing recognition implementation.

The module exposes preparation, recognition, and correction-review operations. Preparation copies an explicitly selected local image to managed, digest-addressed storage and returns dimensions and a displayable preview. Recognition accepts an image identity, view, region/rectification, metric calibration, exclusion regions, and tolerance; it returns transformed image evidence, candidate primitives/profiles, and diagnostics. The backend owns the transformation math. A CPU-bound operation must run off the async command executor.

Recognition stores immutable evidence and returns its identity. `review_drawing_profile` accepts that evidence identity and edited primitives, reuses the same geometry validation/assembly owner as Apply, and returns updated profile candidates and diagnostics without processing the image again. Candidate primitive identities must refer to the user's submitted primitives, including when several lines form one closed loop. Open or invalid candidates must not prevent review of unrelated valid candidates. Reviewing corrections does not append a model version.

The editor supports vertex selection and coordinate correction for recognized polylines as well as line endpoints and circle center/radius. Editing an outline must preserve neighboring analytic circles. Repaired geometry is reviewed again before acceptance; stale candidate roles or old issues must not silently determine the result after a geometry edit.

Extend the existing profile application contract with a drawing input variant rather than introducing a separate save pipeline. Drawing input includes the reviewed primitives and their import evidence. It is mutually exclusive with exact STEP evidence and raw manual loops. Rust validates the selected geometry and evidence again, produces a profile expression using existing CAD forms, and uses the existing guarded normal version operation. Do not accept arbitrary source text from recognition.

An analytic circle is represented by a center and radius; a line by endpoints; an approximate polyline by points. Screen sampling is separate from the authoritative primitive data. The profile assembler resolves nesting and holes from accepted closed geometry. Open geometry cannot be extruded until the user repairs it. Unsupported curved chains remain approximate and must be identified as such before acceptance.

## Accuracy and persistence

- Every import carries source image digest, view assignment, crop, rectification, calibration, extractor version, fitting tolerance, and user edits/acceptance. Preserve this evidence with the generated feature and managed image reference.
- Label output Image-derived / check dimensions. Never claim `exactBrep` or measurement accuracy merely because a curve fit succeeded.
- Report measurable fitting residuals and unresolved topology, not invented confidence percentages. A blank, blurred, or unsupported image returns an actionable diagnostic rather than invented geometry.
- Preserve the source image and accepted geometry when the original selected file is moved or removed. Reopening a saved model preserves the feature and provenance. A full reusable drawing-document editor is outside this change.
- Changing the source image, region, rectification, calibration, or target model invalidates dependent recognition/acceptance. Late responses must not overwrite another view or source scope. Applying against a changed model head must fail before adding a version.
- Recognition is local and requires no cloud provider, API key, FreeCAD, or additional application installation. Do not send drawing images to a model provider implicitly.
- Reject oversized inputs before full decoding using the existing 64 MiB/40 million pixel ceiling or stricter limits justified by the recognition implementation. Bound intermediate image resolution, graph size, and work. Preserve scale mapping when downsampling.

## Recognition requirements

Recognize the center geometry of printed linework rather than both sides of its ink thickness. Separate image preprocessing, centerline extraction, primitive fitting, and profile assembly internally behind one module interface. Small noise or text-like components can be proposed as excluded candidates, but heuristic classification must remain inspectable and correctable. Dimension, center, and hidden lines are not automatically accepted as outer solid boundaries.

Circle fitting requires distributed support around a closed contour and a residual within the configured tolerance. Do not convert a partial arc or a rounded rectangle into a circle. Line fitting must preserve meaningful corners. Any automatic endpoint join is bounded by the explicit tolerance and surfaced as a repair; larger gaps remain errors.

## Acceptance scenarios

### Automatic drawing to usable CAD

Given a raster fixture of an 80 by 50 mm plate with a 10 mm circular hole, finite stroke width, dimension annotations outside the assigned view, and modest image noise; when the user imports it, calibrates the 80 mm edge, recognizes and accepts its profile, then extrudes by 3 mm; the application recovers the rectangular centerline and circular hole without manual redrawing, preserves the source model, and creates a real native STEP solid with the hole. Verify dimensions within the declared image-fitting tolerance, and verify a circular edge in STEP rather than a many-sided hole.

### Photo rectification

Given the same drawing under a known projective transform; when four rectangle corners and its known aspect are supplied, then calibrated; recovered geometry agrees with the original within the declared tolerance. Invalid, crossed, collinear, or ill-conditioned corners are rejected. A perspective-distorted image without valid rectification is not labeled metrically verified.

### Three views from a sheet

Given one sheet containing three distinct drawings; when the user assigns three regions to FRONT, TOP, and SIDE; all three panes show their own source region and recognized geometry. Editing or failing one view does not erase the others. Applying TOP and SIDE uses the correct plane and evidence. No missing view is synthesized.

### Review and failure

Given a recognized plate plus a text-like contour and an open line chain; when the user excludes the text candidate and repairs or excludes the chain; only the accepted closed profile becomes geometry. Before repair, applying the open chain returns an actionable error and creates no version. Numeric circle edits retain analytic circles. Pending/error messages keep panes and primary controls stationary.

### Persistence and stale evidence

Given a prepared image and accepted proposal; when the original file is removed, the managed image still supports recognition and saved provenance. When calibration or model scope changes while recognition is pending, an old result cannot become current or leave a permanent pending state. A stale apply cannot append a feature.

## Verification boundary

Write outside-in acceptance tests first. Prove actual recognition from raster pixels and actual native CAD generation locally; mocked transport tests alone are insufficient. Use generated, repository-owned raster fixtures with known geometry plus a separately transformed/noisy fixture whose expected dimensions are independent of extractor internals. Test a negative image and invalid calibration. Use isolated browser tests for importing, view assignment, editing, acceptance, and one failure state. Reuse the established native three-view orientation proof and guarded version tests where unchanged.

## Explicit exclusions

Automatic OCR and interpretation of dimension/tolerance callouts; automatic assignment of front/top/side based on drawing standards; hidden feature inference; general three-view-to-solid reconstruction; correction of bent paper or lens distortion; automatic semantic interpretation of every drafting convention; PDF/CAD drawing ingestion. These are not silently substituted with guessed values in this release.
