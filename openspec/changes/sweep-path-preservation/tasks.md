## 1. Acceptance contract

- [x] 1.1 Add failing native acceptance for a sharp three-point corner.
- [x] 1.2 Add multi-corner volume and bounds coverage.
- [x] 1.3 Add sampled Bézier round-profile geometry coverage.
- [x] 1.4 Add oversized path bounded-work failure coverage.

## 2. Native implementation

- [x] 2.1 Preserve all deduplicated linear spine vertices.
- [x] 2.2 Use ruled section lofting for linear spines.
- [x] 2.3 Collapse collinear samples and enforce the 1024-section limit before loft construction.
- [x] 2.4 Keep 16-sample Bézier RTTI workaround and dedicated Frenet path branch.

## 3. Proof

- [x] 3.1 Confirm pre-fix acceptance failure from diagonal bounds/volume.
- [x] 3.2 Run standalone native integration executable after implementation.
- [x] 3.3 Run repository-level checks from the owning integration task.

Proof: native runner integration passed; actual round-fold model exported STL and
STEP through CLI and the project-folder watcher. Live MCP structural verification
passed for version `8eb9d062-bc6d-4425-8b28-2b86e0faaba1` with one component and
zero non-manifold edges. Viewport inspection confirmed open round folds. Orca
CLI crashed before slicing, so support-free printing is not established.
