# Native Chamfer & Fillet — Implementation Plan

## Gap Summary

The native Rust engine (EckyRust) is at feature parity with build123d for every
IR node **except** `fillet` and `chamfer`. Both currently return an
`unsupported` error directing users to switch backends.

Build123d delegates to OCCT which operates on B-rep (boundary representation)
solids — it can walk exact edge topology and blend NURBS surfaces. The native
engine works on triangle meshes (`csgrs::Mesh<()>`) which have no concept of
"edges" in the CAD sense, only dihedral angles between adjacent triangles.

The plan is split into two phases: **Phase 1 (Chamfer)** and Phase 2 (Fillet).
Chamfer is geometrically simpler (flat bevel) and validates the full edge
detection + mesh surgery pipeline that fillet will reuse.

---

## Phase 1: Chamfer

### 1.1 Edge Detection

Build a half-edge style lookup from the triangulated mesh:

```
edge (v_a, v_b) → [polygon_index_left, polygon_index_right]
```

**Steps:**

1. Call `mesh.triangulate()` to ensure all polygons are triangles.
2. Build `HashMap<(usize, usize), Vec<usize>>` mapping canonical edge keys
   `(min(a,b), max(a,b))` to the polygon indices that share that edge.
   Use `csgrs`'s `VertexIndexMap` (epsilon-based deduplication) to assign
   canonical vertex indices first.
3. For each shared edge, compute the **dihedral angle** between the two face
   normals:
   `θ = acos(clamp(n₁ · n₂, -1, 1))`
4. An edge is a "feature edge" when `θ > threshold` (e.g. > 20°).

### 1.2 Edge Selectors

Match the build123d lowerer's selectors so IR stays compatible:

| Selector   | Filter                                                      |
|------------|-------------------------------------------------------------|
| `all`      | All feature edges above the dihedral threshold              |
| `top`      | Feature edges whose midpoint Z is within ε of the max Z     |
| `bottom`   | Feature edges whose midpoint Z is within ε of the min Z     |
| `vertical` | Feature edges whose direction is within ε of ±Z (parallel)  |

### 1.3 Chamfer Geometry

For each selected feature edge `(v_a, v_b)` with chamfer distance `d`:

1. **Inset the edge into each adjacent face.** For face `f` with normal `n_f`,
   compute the inset direction: project `d` along the face plane perpendicular
   to the edge direction. This gives two new vertices per edge endpoint per
   face.

2. **Replace the original shared edge** with a chamfer strip — a quad (two
   triangles) connecting the inset vertices on both faces.

3. **Re-triangulate the modified adjacent faces** that lost their original edge
   vertices and gained inset vertices.

The simpler (and more robust for a first pass) alternative:

- **Vertex-based chamfer:** For each vertex on a selected edge, compute the
  average inset direction from all adjacent feature-edge faces, then split the
  vertex into N copies offset along each adjacent face. This is less precise
  but avoids the complex per-edge topology surgery.

**Chosen approach:** Edge-based chamfer (option 1). It produces cleaner
geometry and matches what OCCT does. The vertex-based approach introduces
visual artifacts at edge intersections.

### 1.4 Implementation Location

- New function: `fn chamfer_mesh(mesh: &IrMesh, distance: f64, selector: EdgeSelector) -> AppResult<IrMesh>`
- Place in `ecky_ir.rs` near `eval_shell_geometry` (~line 2878).
- Wire into `eval_geometry` match arm at line 3061, replacing the current
  `unsupported` error for `"chamfer"`.

### 1.5 Test Plan (BDD)

#### Outer loop: integration tests

1. **`chamfer_box_all_edges`** — `(chamfer 2 (box 20 20 10))` renders without
   error. Output mesh has more polygons than the input box. Bounding box is
   preserved (chamfer insets, doesn't grow).

2. **`chamfer_box_top_edges`** — `(chamfer 2 :edges "top" (box 20 20 10))`
   only modifies edges near Z=10. Bottom face polygon count unchanged.

3. **`chamfer_cylinder`** — `(chamfer 1 (cylinder 10 20))` chamfers the top
   and bottom rim edges.

4. **`chamfer_zero_distance_noop`** — `(chamfer 0 (box 10 10 10))` returns
   mesh identical to input.

5. **`chamfer_with_expressions`** — `(chamfer (/ width 10) (box width width height))`
   with params.

6. **`lower_to_build123d_chamfer_still_works`** — existing lowerer path
   unaffected.

#### Inner loop: unit tests

- `detect_feature_edges_box` — a box has 12 feature edges.
- `detect_feature_edges_cylinder` — a cylinder has 2 rim edges (top/bottom).
- `edge_selector_top` — only returns edges at max Z.
- `edge_selector_vertical` — filters edges aligned with Z.
- `chamfer_single_edge` — chamfer one edge of two triangles sharing it;
  verify vertex count increases by 2 and polygon count increases.
- `dihedral_angle_flat` — two coplanar triangles → angle ≈ 0 → not a
  feature edge.
- `dihedral_angle_right` — two perpendicular triangles → angle ≈ 90°.

### 1.6 Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Vertex deduplication tolerance too loose/tight | Missed edges or false merges | Use `csgrs::VertexIndexMap` with its proven epsilon |
| Non-manifold mesh after CSG booleans | Edge shared by >2 faces → skip | Filter to edges with exactly 2 adjacent faces |
| Chamfer distance > face size | Degenerate geometry | Clamp distance to min adjacent face inset |
| Edge-edge intersection at corners | Overlapping chamfer strips | Handle 3+ edge convergence at vertices with averaged corner cuts |

### 1.7 Delivery Milestones

1. ☑ Edge detection + dihedral angle computation + unit tests
2. ☑ Edge selector filtering (all/top/bottom/vertical) + unit tests  
3. ☑ Single-edge chamfer geometry surgery + unit test
4. ☑ Full mesh chamfer with corner handling + integration tests
5. ☑ Wire into `eval_geometry` + `cargo check` + full test suite green (271/271)

---

## Phase 2: Fillet (future)

Reuses Phase 1's edge detection and selector infrastructure entirely.
Replaces the flat chamfer strip with a smooth arc profile:

1. Subdivide each chamfer quad into N segments along the edge-perpendicular
   direction.
2. Displace each subdivision row along a circular arc between the two face
   planes.
3. Recompute normals for smooth shading across the fillet surface.

The arc approximation with N=6–8 segments produces visually convincing
fillets for preview/3D-printing. Not NURBS-exact, but sufficient for the
native engine's mesh-based approach.

Estimated additional effort after Phase 1: ~40% of Phase 1's work, since
the hard part (edge detection, topology surgery, corner handling) is already
done.
