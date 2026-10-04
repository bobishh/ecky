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

Sequence-source parsing is context-specific. Expanded Steel syntax can erase the
distinction between a two/three-number `(list ...)` and a point-shaped list, so
`map`, `zip`, `flat-map`, `concat-map`, and `append` parse these source positions
as sequences. Geometry expressions retain existing Point2/Point3 inference.

Expanded-AST compilation remains primary. Runtime fallback may still succeed for
forms outside its expanded subset; if fallback fails too, retain a non-internal
expanded diagnostic. This keeps precise metadata and unsupported-operation errors
from being replaced by eager symbolic arithmetic errors. A quoted tuple source to
destructuring `map` remains unsupported and reports concise `zip`/static
`enumerate` guidance rather than a debug representation of the quoted AST.

The live `grown-form-zip-regression.ecky` file is an invalid-input diagnostic
fixture, not a geometry source to repair. Its `(apply loft (append ...))` omits
the required leading loft distance. Compiler coverage separately uses a valid
seven-control helper/zip/map/append/compound fixture; no live project source or
history is edited.

The published surface reference must match `loft distance profile1 profile2 ...`.
The shipped reference example and compiler-signature test provide the check; the
source-language docs must not advertise `loft` without distance.
