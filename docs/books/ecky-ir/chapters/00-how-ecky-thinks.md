## How Ecky Thinks

Before the first model, the one-screen mental model. Everything in this book sits on three layers, and knowing which layer you are on explains every behavior you will meet.

**You write a Scheme surface.** An `.ecky` file is parenthesized Scheme: `(model (part ...))`. It is friendly to read and write, and it is _not_ the thing that gets built. It is a surface — a convenient skin over the layer below.

**It lowers to a finite Core IR.** Ecky compiles your surface into a small, fixed set of core operations — primitives, booleans, selectors, placements, repeats. "Finite" is the whole point: the kernel never sees arbitrary Scheme, only this closed vocabulary. That is why a model is reproducible, verifiable, and portable. When a feature "exists," it means it exists in the Core IR — not just in the surface syntax.

**The Core IR renders on a backend.** The default backend is the **native OCCT kernel**: an exact boundary-representation (B-rep) solid modeler. Exact means real faces and edges with identities you can select and tag — not a triangle soup. Two interop backends, **build123d** and **FreeCAD**, can also consume the Core IR for cross-checking and import, but they are followers, not the source of truth. Some features (like `:created-by`, later) live only on native because they depend on data only the native kernel tracks.

Keep the three layers in mind: when something compiles but renders oddly, ask which layer owns it. Surface typo, missing Core IR operation, or a backend that does not support it — the answer is almost always one of those three.
