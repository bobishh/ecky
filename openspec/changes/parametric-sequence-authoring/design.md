# Design

Static flat-map retains compile-time expansion. A dynamic source or dynamic
list-valued result lowers to a map followed by list append through Core Apply.
The native planner resolves list values before geometry construction. Controls
remain Core parameters; no defaults are substituted as permanent geometry.
List element validation remains strict, including invalid scalar callback results.

The regression fixture is the existing folded-cord spoon rest with restored
named dimensions and repeat counts. Compilation must preserve all controls;
native plans must change when supplied count and size parameters change.
The live source is updated only after isolated source/planner checks succeed.

Native scalar evaluation also resolves `pi`/`tau` passed as helper arguments and
parameter aliases introduced by callbacks. Lexical locals take precedence over
built-in constants. Dynamic flat-map native support does not add FreeCAD runtime
list support.

The historical solid-teeth source remains a compiler regression fixture only.
The delivered model uses the pre-teeth round-cord source, preserving open folds
and handle slots. `bend-gap` controls spacing within the upright folds; no solid
polygon teeth replace the sweep. Structural topology checks are retained, while
an unrequested zero-overhang counter is not used to redesign the model.

Thread printability advisories only traverse subtrees containing thread geometry.
They must not evaluate unrelated deferred point callbacks outside their lexical
map scope; this previously failed manifest publication after successful export.

Dialect inference skips Scheme comment lines beginning with any semicolon count,
including a single semicolon. Header comments must not route valid Ecky source
through Python control extraction and silently erase the version controls.
