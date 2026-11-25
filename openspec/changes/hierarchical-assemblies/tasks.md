## 1. Hierarchy & Part Metadata

- [x] 1.1 Extract `AssemblyNode` tree in Rust compiler and include in `ModelManifest`.
- [x] 1.2 Expose part-to-mesh mapping and bounding boxes in Three.js model loader.

## 2. Viewport Part Isolation

- [x] 2.1 Add part visibility toggles (hide/show) and Solo/Isolate mode in `Model3DViewer`.
- [x] 2.2 Add ghost rendering for unselected assembly parts during isolation.
- [x] 2.3 Frame camera smoothly to isolated part bounding box.

## 3. Assembly Navigation UI

- [x] 3.1 Add Assembly Tree component to workbench sidebar/dock.
- [x] 3.2 Wire selection to parameter filtering (in-context scoping toggle; editor range highlighting deferred).
- [x] 3.3 Add automated unit and component acceptance tests for part isolation.
