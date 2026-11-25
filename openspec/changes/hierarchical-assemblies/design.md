## Decisions

### 1. Assembly Tree extraction from AST and Manifest

The compiler already tags each solid with a `part_key` and tracks `(define-component ...)` calls.
- Rust exposes an assembly tree:
  ```rust
  pub struct AssemblyNode {
      pub id: String,
      pub name: String,
      pub part_key: Option<String>,
      pub children: Vec<AssemblyNode>,
      pub bounding_box: [f64; 6],
  }
  ```
- The frontend workbench dock displays this tree alongside views and parameters.

### 2. Viewport Isolation (Solo & Ghost modes)

In `Model3DViewer`:
- When user isolates part `X`:
  - Part `X` is rendered normally with solid diffuse material.
  - Sibling parts are either hidden (`hidden: true`) or rendered as faint ghost silhouettes (opacity: 0.15, wireframe/transparent, depthWrite: false).
  - Orbit camera target smoothly animates to the center of Part `X`'s bounding box.
- Clicking "Exit Isolation" (or pressing Escape) restores the full assembly view.

### 3. Scoped Code and Parameter Editing

- Clicking a part in the Assembly Tree filters the Parameter Panel to only show parameters declared in or bound to that part.
- If the part is defined in a separate file or component package, clicking "Edit Component" opens that sub-component buffer in the editor.

## Scope

- Initial milestone covers 3D viewport part isolation, visibility toggles, and parameter filtering for single-file and Cairn-imported components.
- Sub-assembly joint rigging and exploded-view sliders belong to a subsequent milestone.
