# Verified authoring guidance

## Problem

Agent language guidance presents a few native examples as if prose were proof.
Recent authoring used an unquoted `clip-plane :keep positive`, omitted required
`clip-box` ranges, and treated topology checks as printability or shape proof.
The `geometryBackend=mesh` label also hides legacy native-hybrid behavior.

## Scope

- Make published native catalogue examples compile and plan through Direct OCCT.
- State exact literal and named-binding requirements, Bézier approximation, and
  artifact representation truth.
- Bound verification claims: topology, shape intent, support-free printing, and
  visual/mechanical hypotheses need their own evidence.
- Preserve parameters and exact current-version evidence; compiler errors never
  authorize a Python, STL, or hardcoded-control replacement.

No compiler, native planner, lifecycle, or frontend changes.
