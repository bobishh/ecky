## Decisions

The parameter panel owns draft values and input timing. Rust owns preview rendering and cancellation. A preview uses the selected saved version and the current draft parameters. The workbench only projects a result for its current draft.

The preview compares resolved native part plans at the saved and draft values. Changed plans are evaluated using the same native CAD operations and geometry cache as normal renders. The runner writes one coarse display STL, with independent tessellation cache identity. The viewer aligns this mesh to the same model coordinates as the saved assembly and draws it translucently.

Apply uses the existing production workflow. Draft meshes live in a bounded temporary cache and never become export artifacts. A native preview that fails leaves the saved geometry visible.

## Scope

The first version covers saved native Ecky models whose geometry the Direct OCCT planner can evaluate. The normal Apply workflow stays available for other render routes and post-processed models.

A later display-first workflow can use this mesh output for ordinary viewport builds. On-demand production output would need a separate artifact lifecycle for exports and consumers of exact topology, verification and analysis.
