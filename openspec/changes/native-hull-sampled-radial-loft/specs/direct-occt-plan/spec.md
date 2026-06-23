# Delta for direct-occt-plan

## ADDED Requirements

### Requirement: Native sampled-radial-loft execution

The system SHALL execute `sampled-radial-loft` on the EckyRust/Direct OCCT
backend by expanding it into loft sections at plan time, without rejecting
the request as exact-backend-only and without falling back to build123d or
FreeCAD.

#### Scenario: EckyRust request renders sampled-radial-loft natively

- GIVEN a `.ecky` model whose part body is a `sampled-radial-loft`
- WHEN the model is rendered with the EckyRust backend and the Direct OCCT
  runtime available
- THEN a native Direct OCCT bundle is produced
- AND its geometry matches an exact-backend reference render within the
  differential parity tolerances.

#### Scenario: sampled-radial-loft appears in the native language manifest

- GIVEN the language surface manifest for the EckyRust backend
- WHEN CAD ops are listed
- THEN `sampled-radial-loft` is present
- AND it is no longer classified as exact-backend-only.

### Requirement: Convex hull operation

The system SHALL provide a `(hull shape...)` CAD operation that produces the
convex hull of its child shapes as a closed BREP solid on the Direct OCCT
backend. Exact backends (build123d, FreeCAD) SHALL reject `hull` with a
deterministic diagnostic naming the op.

#### Scenario: Hull of two disjoint spheres

- GIVEN a model `(hull (sphere r) (translate d 0 0 (sphere r)))`
- WHEN rendered natively
- THEN the output is a single manifold solid
- AND its volume matches the analytic spherocylinder envelope within
  tessellation tolerance
- AND its bbox equals the union bbox of the inputs.

#### Scenario: Hull rejected on exact backends

- GIVEN a model containing `hull`
- WHEN lowered to build123d or FreeCAD
- THEN lowering fails with a diagnostic naming `hull` as Direct-OCCT-only
- AND no silent backend substitution occurs.

#### Scenario: Hull admitted by the runner gate

- GIVEN a plan containing a `hull` command with shape references
- WHEN the runner subset gate inspects the plan
- THEN `hull` is runner-supported and dispatches to the precompiled runner.
