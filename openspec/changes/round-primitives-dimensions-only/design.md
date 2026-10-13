# Design

The shared Core IR validator enforces exact positional arity: cylinder 2,
cone 3, circle 1. Ring expansion accepts exactly 2 radii. Existing alignment
keywords remain valid. Extra positional arguments fail compilation before
backend selection or rendering.

FreeCAD and mesh fallback evaluators also reject the removed argument.
Mesh approximation keeps its default internal resolution; analytic native
geometry and render tessellation policy remain independent of authored
primitive dimensions. Polygon side count belongs to `regular-polygon`.

The registry owns signatures and catalogue examples. The corpus owns prose;
normal generation projects it into human docs, agent references and prompts.
Shipped model sources and test fixtures use the same contract. A built-in
socket screw's six-sided recess uses an extruded polygon explicitly.

Existing saved user model history is not rewritten. Old source must be edited
to use dimensions only or explicit polygonal geometry before recompilation.
