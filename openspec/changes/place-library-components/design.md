## Decisions

### Keep the placement in Ecky model coordinates

The workbench renders the component as a separate translucent mesh. The viewer applies the same display rotation and centering as the active CAD model. A click on the CAD XY plane gives the component an initial position. The axis handles and numeric fields edit the same XYZ values in millimetres.

The placement window floats above the workbench and leaves the viewport interactive. The component preview stays out of model exports and saved version screenshots. Closing the window or leaving its project discards the placement. An in-flight preview can finish in the render cache without reopening the window.

### Reuse source materialization and version persistence

Rust resolves the selected component and checks the expected destination version and source digest. Preview renders an isolated model with the same reusable definitions and the component's default arguments. While the user moves it, the viewer updates the mesh transform locally.

Apply repeats the destination checks and inserts a part whose geometry wraps the component call in `translate`. The guarded manual source path saves and renders the new version. If rendering fails after the version was saved, the normal history retains the failed version and the placement window shows the error. The MCP copy-inline workflow continues to return source without saving a version.

## Scope

This change supports translation of source-backed library components. Rotation and port mating require separate interactions.
