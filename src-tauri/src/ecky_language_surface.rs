use crate::contracts::GeometryBackend;
use serde::Serialize;

// Keep this list equal to the direct clauses accepted by
// ecky-render's `parse_expanded_model`/`parse_program`.
pub const MODEL_CLAUSES: &[&str] = &[
    "params",
    "verify",
    "part",
    "feature",
    "meta",
    "tag-vertex",
    "tag-face",
    "tag-edge",
    "tag-edges",
    "view",
    "analysis",
];
pub const MODEL_WRAPPERS: &[&str] = &["begin", "let", "let*"];
pub const COMPONENT_PLACEMENT_FORMS: &[&str] = &[
    "define-component",
    "ports",
    "port",
    "frame",
    "place-component",
    "port-ref",
];
pub const CORE_CONSTANTS: &[&str] = &["start", "end", "xy", "yz", "xz", "true", "false"];
pub const EXPRESSION_FORMS: &[&str] = &[
    "define",
    "lambda",
    "let",
    "let*",
    "begin",
    "if",
    "quote",
    "list",
    "append",
    "reverse",
    "range",
    "map",
    "filter",
    "fold",
    "reduce",
    "zip",
    "enumerate",
    "linspace",
    "flat-map",
    "concat-map",
    "apply",
];
pub const NUMERIC_HELPERS: &[&str] = &[
    "pi",
    "tau",
    "+",
    "-",
    "*",
    "/",
    "min",
    "max",
    "abs",
    "floor",
    "sin",
    "cos",
    "tan",
    "atan",
    "atan2",
    "deg",
    "rad",
    "deg->rad",
    "rad->deg",
    "clamp",
    "lerp",
    "invlerp",
    "remap",
    "smoothstep",
    "square",
    "cube",
    "hash01",
    "hash-signed",
    "noise2",
    "fbm2",
    "voronoi2",
    "cell-distance2",
];
pub const POINT_LIST_HELPERS: &[&str] = &[
    "vec2",
    "vec3",
    "jitter2",
    "jittered-grid",
    "polar-points",
    "organic-loop",
    "wave-loop",
    "superellipse-point",
    "voronoi-cells",
    "lorenz-points",
    "rossler-points",
    "logistic-bifurcation-points",
    "henon-points",
];
pub const BOOLEAN_HELPERS: &[&str] = &[
    "not", "and", "or", "=", ">", ">=", "<", "<=", "even?", "odd?", "zero?", "null?", "empty?",
    "list?",
];
// Manifested names must stay backed by cad::MODULE exports and guide tests.
// Keep backend-only names out of this portable list.
pub const CAD_OPS_PORTABLE: &[&str] = &[
    "box",
    "sphere",
    "cylinder",
    "cone",
    "circle",
    "ring",
    "rectangle",
    "rounded-rect",
    "rounded-polygon",
    "polygon",
    "profile",
    "make-face",
    "text",
    "svg",
    "import-stl",
    "path",
    "polyline",
    "bezier-path",
    "bspline",
    "extrude",
    "revolve",
    "loft",
    "sweep",
    "helical-ridge",
    "thread",
    "tapped-hole",
    "rib",
    "groove",
    "torus",
    "ellipse",
    "regular-polygon",
    "trapezoid",
    "wedge",
    "slot-overall",
    "slot-center-to-center",
    "slot-center-point",
    "slot-arc",
    "shell",
    "offset",
    "offset-rounded",
    "fillet",
    "chamfer",
    "taper",
    "draft",
    "twist",
    "union",
    "fuse",
    "difference",
    "cut",
    "intersection",
    "common",
    "xor",
    "compound",
    "translate",
    "rotate",
    "scale",
    "mirror",
    "linear-array",
    "radial-array",
    "grid-array",
    "arc-array",
    "repeat",
    "repeat-union",
    "repeat-compound",
    "repeat-pick",
    "for-union",
    "for-compound",
    "plane",
    "location",
    "path-frame",
    "place",
    "clip-box",
    "clip-plane",
    "build",
    "shape",
    "result",
    "sampled-radial-loft",
];
// Mesh/EckyRust-only surface. Do not add future CAD VM/OCCT names here until
// the compiler/runtime actually exports and lowers them.
pub const ECKY_RUST_ONLY_CAD_OPS: &[&str] = &[
    "mesh",
    "polyhedron",
    "protrude",
    "wall-pattern",
    "surface-trim",
    "mesh-anchor",
];
// EckyRust direct-OCCT-only surface: rendered natively (no mesh path) and
// rejected by FreeCAD interop. Unlike `wall-pattern` (mesh), these are BREP
// ops the interop backend cannot express.
pub const ECKY_RUST_DIRECT_ONLY_CAD_OPS: &[&str] = &["hull", "voronoi-cell", "import-step"];
pub const WALL_PATTERN_MODES: &[&str] = &[
    "ribs",
    "rings",
    "spiral",
    "diamond",
    "hammered",
    "fourier",
    "cellular",
    "fbm",
    "gyroid",
    "schwarz-p",
    "schwarz-d",
    "diamond-field",
    "neovius",
    "attractor-field",
];
pub const TYPED_HOLE_POLICY: &str = concat!(
    "Typed holes are supported only as CAD-VM planning placeholders: ",
    "`(hole :type solid|sketch|path|shape :goal \"...\")`. ",
    "They compile and typecheck as the requested kind, but unfilled holes intentionally reject ",
    "during render/lowering before any backend executes. ",
    "Do not emit `(hole ...)` when the user expects a finished renderable model."
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EckySupportedSurfaceManifest {
    pub backend: GeometryBackend,
    pub model_clauses: &'static [&'static str],
    pub model_wrappers: &'static [&'static str],
    pub component_placement_forms: &'static [&'static str],
    pub core_constants: &'static [&'static str],
    pub expression_forms: &'static [&'static str],
    pub numeric_helpers: &'static [&'static str],
    pub point_list_helpers: &'static [&'static str],
    pub boolean_helpers: &'static [&'static str],
    pub cad_ops: Vec<&'static str>,
    pub wall_pattern_modes: &'static [&'static str],
    pub typed_hole_policy: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceReferenceEntry {
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub returns: String,
    pub description: String,
    pub deterministic: bool,
    pub backend_support: String,
    pub example: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EckySupportedSurfaceReference {
    pub backend: GeometryBackend,
    pub entries: Vec<SurfaceReferenceEntry>,
}

pub fn cad_ops_for_backend(backend: GeometryBackend) -> Vec<&'static str> {
    let mut ops = CAD_OPS_PORTABLE.to_vec();
    if matches!(backend, GeometryBackend::EckyRust) {
        ops.extend(ECKY_RUST_ONLY_CAD_OPS);
        ops.extend(ECKY_RUST_DIRECT_ONLY_CAD_OPS);
    }
    ops
}

pub fn wall_pattern_modes_for_backend(backend: GeometryBackend) -> &'static [&'static str] {
    if matches!(backend, GeometryBackend::EckyRust) {
        WALL_PATTERN_MODES
    } else {
        &[]
    }
}

pub fn supported_surface_manifest(backend: GeometryBackend) -> EckySupportedSurfaceManifest {
    EckySupportedSurfaceManifest {
        backend,
        model_clauses: MODEL_CLAUSES,
        model_wrappers: MODEL_WRAPPERS,
        component_placement_forms: COMPONENT_PLACEMENT_FORMS,
        core_constants: CORE_CONSTANTS,
        expression_forms: EXPRESSION_FORMS,
        numeric_helpers: NUMERIC_HELPERS,
        point_list_helpers: POINT_LIST_HELPERS,
        boolean_helpers: BOOLEAN_HELPERS,
        cad_ops: cad_ops_for_backend(backend),
        wall_pattern_modes: wall_pattern_modes_for_backend(backend),
        typed_hole_policy: TYPED_HOLE_POLICY,
    }
}

pub fn supported_surface_reference(backend: GeometryBackend) -> EckySupportedSurfaceReference {
    let mut entries = Vec::new();

    entries.extend(
        MODEL_CLAUSES
            .iter()
            .map(|name| model_clause_reference(name)),
    );
    entries.extend(
        MODEL_WRAPPERS
            .iter()
            .map(|name| model_wrapper_reference(name)),
    );
    entries.extend(
        COMPONENT_PLACEMENT_FORMS
            .iter()
            .map(|name| component_placement_reference(name)),
    );
    entries.extend(
        CORE_CONSTANTS
            .iter()
            .map(|name| core_constant_reference(name)),
    );
    entries.extend(
        EXPRESSION_FORMS
            .iter()
            .map(|name| expression_reference(name)),
    );
    entries.extend(NUMERIC_HELPERS.iter().map(|name| numeric_reference(name)));
    entries.extend(
        POINT_LIST_HELPERS
            .iter()
            .map(|name| point_list_reference(name)),
    );
    entries.extend(BOOLEAN_HELPERS.iter().map(|name| boolean_reference(name)));
    entries.extend(
        cad_ops_for_backend(backend)
            .iter()
            .map(|name| cad_op_reference(name, backend)),
    );
    entries.extend(
        wall_pattern_modes_for_backend(backend)
            .iter()
            .map(|name| wall_pattern_mode_reference(name)),
    );

    EckySupportedSurfaceReference { backend, entries }
}

fn component_placement_reference(name: &str) -> SurfaceReferenceEntry {
    let (signature, returns, description, example, notes): (&str, &str, &str, &str, &[&str]) =
        match name {
            "define-component" => (
                "(define-component id (signature...) [(ports ...)] [(verify ...)] geometry)",
                "component declaration",
                "Declares closed reusable local geometry and optional ports.",
                "(define-component latch () (ports (port mount :type \"mount.v1\" :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1)))) (box 20 4 2))",
                &[],
            ),
            "ports" => (
                "(ports (port ...) ...)",
                "interface clause",
                "Groups local interfaces.",
                "(ports (port mount :type \"mount.v1\" :frame local-frame))",
                &[],
            ),
            "port" => (
                "(port id :type type-id :frame frame [:compatible-with '(type-id ...)] [:params ((name value) ...)])",
                "port declaration",
                "Declares one stable typed local interface.",
                "(port mount :type \"mount.v1\" :frame local-frame)",
                &[],
            ),
            "frame" => (
                "(frame :origin '(x y z) :x-axis '(x y z) :z-axis '(x y z))",
                "right-handed local frame",
                "Defines origin/x/z; derives y as z cross x.",
                "(frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0))",
                &[],
            ),
            "place-component" => (
                "(place-component (component :param value ...) :from port-id :to (port-ref part-id port-id) :normal aligned|opposed [:roll degrees] [:offset '(x y z)] [:mirror none|x|y])",
                "solid",
                "Mates source and target ports without Euler math.",
                "(place-component (latch) :from mount :to (port-ref enclosure side-left-latch) :normal opposed)",
                &[],
            ),
            "port-ref" => (
                "(port-ref part-id port-id)",
                "stable port reference",
                "References one target port.",
                "(port-ref enclosure side-left-latch)",
                &[],
            ),
            _ => unreachable!("component placement form registry is exhaustive"),
        };
    ref_entry(
        name,
        "componentPlacementForm",
        signature,
        returns,
        description,
        true,
        "all .ecky backends (lowered before backend planning)",
        example,
        notes,
    )
}

fn core_constant_reference(name: &str) -> SurfaceReferenceEntry {
    let (returns, description, example) = match name {
        "start" | "end" => (
            "path position",
            "Symbolic endpoint accepted by path-frame `:at`.",
            format!("(path-frame rail :at {name} :up '(0 0 1))"),
        ),
        "xy" | "yz" | "xz" => (
            "plane symbol",
            "Symbolic principal plane value.",
            format!("(list {name})"),
        ),
        "true" | "false" => (
            "boolean",
            "Boolean constant equivalent to `#t` or `#f`.",
            format!("(if {name} body fallback)"),
        ),
        _ => ("value", "Built-in Ecky constant.", name.to_string()),
    };
    ref_entry(
        name,
        "coreConstant",
        name,
        returns,
        description,
        true,
        "all .ecky backends",
        &example,
        &[],
    )
}

fn backend_support(backend: GeometryBackend) -> &'static str {
    match backend {
        GeometryBackend::Build123d => ".ecky; legacy build123d metadata migrates to Ecky Native",
        GeometryBackend::Freecad => ".ecky with geometryBackend=freecad",
        GeometryBackend::EckyRust => ".ecky with geometryBackend=mesh/eckyRust",
    }
}

fn ref_entry(
    name: &str,
    kind: &str,
    signature: &str,
    returns: &str,
    description: &str,
    deterministic: bool,
    backend_support: &str,
    example: &str,
    notes: &[&str],
) -> SurfaceReferenceEntry {
    SurfaceReferenceEntry {
        name: name.to_string(),
        kind: kind.to_string(),
        signature: signature.to_string(),
        returns: returns.to_string(),
        description: description.to_string(),
        deterministic,
        backend_support: backend_support.to_string(),
        example: example.to_string(),
        notes: notes.iter().map(|note| note.to_string()).collect(),
    }
}

fn model_clause_reference(name: &str) -> SurfaceReferenceEntry {
    match name {
        "params" => ref_entry(
            name,
            "modelClause",
            "(params control...)",
            "model clause",
            "Declares user-visible controls and default parameter values for the model.",
            true,
            "all .ecky backends",
            "(params (number radius 20 :label \"Radius\" :min 5 :max 80))",
            &["Only valid inside `(model ...)` or model-level wrappers."],
        ),
        "verify" => ref_entry(
            name,
            "modelClause",
            "(verify (tag id) [(intent text)] [(severity error|warning)] [(when bool-expr)] (metric id metric-expr) (expect id predicate))",
            "model clause",
            "Declares one conditional runtime check with intent, severity, and typed evidence.",
            true,
            "all .ecky backends",
            "(verify (tag mesh-clean) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= 0)))",
            &["Generated models keep authored verification clauses during repair."],
        ),
        "part" => ref_entry(
            name,
            "modelClause",
            "(part id geometry)",
            "model clause",
            "Declares a named renderable part from a solid, sketch, path, or compound expression.",
            true,
            "all .ecky backends",
            "(part body (cylinder radius height 48))",
            &["Part ids should be semantic and stable across edits."],
        ),
        "feature" => ref_entry(
            name,
            "modelClause",
            "(feature id :role role [:params (key...)] geometry)",
            "model clause",
            "Declares renderable geometry plus semantic role and primary control metadata.",
            true,
            "all .ecky backends",
            "(feature shell :role enclosure :params (width wall) (box width 40 wall))",
            &["Inferred dependencies remain authoritative; `:params` records design intent."],
        ),
        "meta" => ref_entry(
            name,
            "modelClause",
            "(meta key value)",
            "model clause",
            "Stores literal model metadata in Core IR; `:title` labels the exported document and `units strict` enables dimensional checks.",
            true,
            "all .ecky backends",
            "(meta :title \"Bottle cage\")",
            &[
                "Metadata does not create geometry.",
                "Values must be string, symbol, number, or boolean literals; duplicate keys reject.",
            ],
        ),
        "tag-vertex" => selector_tag_reference(
            name,
            ":vertex",
            "vertex",
            "(tag-vertex datum :vertex \"top\" body)",
        ),
        "tag-face" => selector_tag_reference(
            name,
            ":face or :faces",
            "face",
            "(tag-face mounting :faces \"bottom\" body)",
        ),
        "tag-edge" | "tag-edges" => selector_tag_reference(
            name,
            ":edge or :edges",
            "edge",
            &format!("({name} rim :edges \"top\" body)"),
        ),
        "view" => ref_entry(
            name,
            "modelClause",
            "(view id (offset-part part dx dy dz)...) ",
            "model clause",
            "Declares a preview-only exploded or print-layout view without changing export geometry.",
            true,
            "all .ecky backends",
            "(view print-layout (offset-part lid 90 0 0) (offset-part body 0 0 0))",
            &["`offset-part` references stable part ids."],
        ),
        "analysis" => ref_entry(
            name,
            "modelClause",
            "(analysis id analysis-clause...)",
            "model clause",
            "Declares an authored FEM/engineering analysis contract tied to model parts and selector tags.",
            true,
            "all .ecky backends",
            "(analysis load-case (linear-static :part body) (fixed :face-tag mounting) (solve :method direct))",
            &["Analysis evidence does not replace geometry verification."],
        ),
        _ => generic_reference(name, "modelClause", "model clause"),
    }
}

fn selector_tag_reference(
    name: &str,
    selector_keyword: &str,
    topology_kind: &str,
    example: &str,
) -> SurfaceReferenceEntry {
    ref_entry(
        name,
        "modelClause",
        &format!("({name} id {selector_keyword} selector target)"),
        "model clause",
        &format!(
            "Names a stable {topology_kind} selection for downstream operations and analysis."
        ),
        true,
        "all .ecky backends",
        example,
        &["Use semantic selectors or stable target ids; raw topology indices are brittle."],
    )
}

fn model_wrapper_reference(name: &str) -> SurfaceReferenceEntry {
    match name {
        "begin" => ref_entry(
            name,
            "modelWrapper",
            "(begin clause...)",
            "model clauses",
            "Groups multiple model clauses where a single clause position is expected.",
            true,
            "all .ecky backends",
            "(model (begin (params ...) (part body ...)))",
            &["Wrapper bodies splice model clauses into the model."],
        ),
        "let" => ref_entry(
            name,
            "modelWrapper",
            "(let ((name value)...) clause...)",
            "model clauses",
            "Binds model-level constants for following clauses; bindings in one let are parallel.",
            true,
            "all .ecky backends",
            "(model (let ((r 20)) (part body (sphere r))))",
            &["Use `let*` when a binding depends on an earlier binding."],
        ),
        "let*" => ref_entry(
            name,
            "modelWrapper",
            "(let* ((name value)...) clause...)",
            "model clauses",
            "Sequential model-level binding form; later bindings can use earlier bindings.",
            true,
            "all .ecky backends",
            "(model (let* ((r 20) (h (* r 3))) (part body (cylinder r h))))",
            &["Preferred for derived dimensions."],
        ),
        _ => generic_reference(name, "modelWrapper", "model clauses"),
    }
}

fn expression_reference(name: &str) -> SurfaceReferenceEntry {
    match name {
        "define" => ref_entry(
            name,
            "expressionForm",
            "(define name value)",
            "binding",
            "Defines a helper value or function in expression scope.",
            true,
            "all .ecky backends",
            "(define wall 2)",
            &[],
        ),
        "lambda" => ref_entry(
            name,
            "expressionForm",
            "(lambda (arg...) body)",
            "function",
            "Creates an anonymous function for map/filter/fold helpers.",
            true,
            "all .ecky backends",
            "(lambda (i) (translate (* i pitch) 0 0 cutter))",
            &[],
        ),
        "let" => ref_entry(
            name,
            "expressionForm",
            "(let ((name value)...) body)",
            "expression",
            "Parallel local bindings inside an expression.",
            true,
            "all .ecky backends",
            "(let ((r 10) (h 30)) (cylinder r h))",
            &["Bindings in the same `let` cannot depend on one another."],
        ),
        "let*" => ref_entry(
            name,
            "expressionForm",
            "(let* ((name value)...) body)",
            "expression",
            "Sequential local bindings inside an expression.",
            true,
            "all .ecky backends",
            "(let* ((r 10) (h (* r 3))) (cylinder r h))",
            &[],
        ),
        "begin" => ref_entry(
            name,
            "expressionForm",
            "(begin expr...)",
            "last expression",
            "Evaluates expressions in order and returns the final value.",
            true,
            "all .ecky backends",
            "(begin (define r 10) (sphere r))",
            &["Keep expressions pure; no mutation side effects."],
        ),
        "if" => ref_entry(
            name,
            "expressionForm",
            "(if condition then else)",
            "expression",
            "Chooses between two expressions from a boolean condition.",
            true,
            "all .ecky backends",
            "(if useCap (sphere r) (cylinder r h))",
            &[],
        ),
        "quote" => ref_entry(
            name,
            "expressionForm",
            "(quote value) or 'value",
            "literal",
            "Prevents evaluation of symbols/lists for literal data such as align tuples.",
            true,
            "all .ecky backends",
            "'(center center min)",
            &[],
        ),
        "list" => ref_entry(
            name,
            "expressionForm",
            "(list value...)",
            "list",
            "Builds a list value.",
            true,
            "all .ecky backends",
            "(list x y z)",
            &[],
        ),
        "append" => ref_entry(
            name,
            "expressionForm",
            "(append list...)",
            "list",
            "Concatenates lists.",
            true,
            "all .ecky backends",
            "(append front-points back-points)",
            &[],
        ),
        "reverse" => ref_entry(
            name,
            "expressionForm",
            "(reverse list)",
            "list",
            "Returns list items in reverse order.",
            true,
            "all .ecky backends",
            "(reverse points)",
            &[],
        ),
        "range" => ref_entry(
            name,
            "expressionForm",
            "(range count)",
            "list<number>",
            "Builds integer indices from 0 to count - 1.",
            true,
            "all .ecky backends",
            "(range 8)",
            &["Keep counts bounded small literals where possible."],
        ),
        "map" => ref_entry(
            name,
            "expressionForm",
            "(map fn list ...)",
            "list",
            "Transforms each list item with a function.",
            true,
            "all .ecky backends",
            "(map (lambda (i) (* i 10)) (range 4))",
            &[
                "Multiple source lists bind one lambda parameter per source.",
                "Static tuple destructuring is supported for `(map (lambda ((x y)) ...) (zip xs ys))` and `(map (lambda ((index value)) ...) (enumerate static-list))`.",
                "Do not use `map` at model-clause level to generate `part`, `params`, or `meta` clauses.",
            ],
        ),
        "filter" => ref_entry(
            name,
            "expressionForm",
            "(filter fn list)",
            "list",
            "Keeps list items where predicate returns true.",
            true,
            "all .ecky backends",
            "(filter (lambda (i) (even? i)) (range 8))",
            &[],
        ),
        "fold" | "reduce" => ref_entry(
            name,
            "expressionForm",
            &format!("({name} fn initial list)"),
            "value",
            "Reduces a list into a single accumulated value.",
            true,
            "all .ecky backends",
            "(fold + 0 (range 5))",
            &[],
        ),
        "zip" => ref_entry(
            name,
            "expressionForm",
            "(zip list-a list-b)",
            "list<pair>",
            "Pairs items from two lists by index.",
            true,
            "all .ecky backends",
            "(map (lambda ((x y)) (list x y)) (zip xs ys))",
            &["Use with `map` destructuring for static paired list inputs."],
        ),
        "enumerate" => ref_entry(
            name,
            "expressionForm",
            "(enumerate list)",
            "list<pair>",
            "Pairs each index with its list item.",
            true,
            "all .ecky backends",
            "(map (lambda ((index value)) (list index value)) (enumerate (range 4)))",
            &["`map` destructuring over `enumerate` requires a statically sized source list today."],
        ),
        "linspace" => ref_entry(
            name,
            "expressionForm",
            "(linspace start end count)",
            "list<number>",
            "Builds evenly spaced samples including endpoints.",
            true,
            "all .ecky backends",
            "(linspace 0 360 12)",
            &[],
        ),
        "flat-map" | "concat-map" => ref_entry(
            name,
            "expressionForm",
            &format!("({name} fn list)"),
            "list",
            "Maps each item to a list and concatenates the results.",
            true,
            "all .ecky backends",
            "(flat-map (lambda (i) (list i (- i))) (range 3))",
            &[],
        ),
        "apply" => ref_entry(
            name,
            "expressionForm",
            "(apply fn args)",
            "value",
            "Calls a function with arguments from a list.",
            true,
            "all .ecky backends",
            "(apply union cutters)",
            &["Useful for unions/differences built from mapped lists."],
        ),
        _ => generic_reference(name, "expressionForm", "expression"),
    }
}

fn numeric_reference(name: &str) -> SurfaceReferenceEntry {
    match name {
        "pi" | "tau" => ref_entry(
            name,
            "numericConstant",
            name,
            "number",
            "Built-in circle constant.",
            true,
            "all .ecky backends",
            &format!("(* radius {name})"),
            &[],
        ),
        "+" => ref_entry(
            name,
            "numericHelper",
            "(+ a b...)",
            "number",
            "Adds numbers.",
            true,
            "all .ecky backends",
            "(+ width clearance)",
            &[],
        ),
        "-" => ref_entry(
            name,
            "numericHelper",
            "(- a b...)",
            "number",
            "Subtracts numbers or negates one number.",
            true,
            "all .ecky backends",
            "(- outer inner)",
            &[],
        ),
        "*" => ref_entry(
            name,
            "numericHelper",
            "(* a b...)",
            "number",
            "Multiplies numbers.",
            true,
            "all .ecky backends",
            "(* radius 2)",
            &[],
        ),
        "/" => ref_entry(
            name,
            "numericHelper",
            "(/ a b...)",
            "number",
            "Divides numbers.",
            true,
            "all .ecky backends",
            "(/ width 2)",
            &["Avoid zero divisors."],
        ),
        "min" => ref_entry(
            name,
            "numericHelper",
            "(min a b...)",
            "number",
            "Returns smallest number.",
            true,
            "all .ecky backends",
            "(min wall max-wall)",
            &[],
        ),
        "max" => ref_entry(
            name,
            "numericHelper",
            "(max a b...)",
            "number",
            "Returns largest number.",
            true,
            "all .ecky backends",
            "(max wall 1.2)",
            &[],
        ),
        "abs" => ref_entry(
            name,
            "numericHelper",
            "(abs value)",
            "number",
            "Returns absolute value.",
            true,
            "all .ecky backends",
            "(abs offset)",
            &[],
        ),
        "floor" => ref_entry(
            name,
            "numericHelper",
            "(floor value)",
            "number",
            "Rounds down to an integer-valued number.",
            true,
            "all .ecky backends",
            "(floor segments)",
            &[],
        ),
        "sin" | "cos" | "tan" => ref_entry(
            name,
            "numericHelper",
            &format!("({name} radians)"),
            "number",
            "Trigonometric helper using radians.",
            true,
            "all .ecky backends",
            &format!("({name} (deg->rad 45))"),
            &[],
        ),
        "atan" => ref_entry(
            name,
            "numericHelper",
            "(atan value)",
            "number",
            "Single-argument arctangent returning radians.",
            true,
            "all .ecky backends",
            "(atan slope)",
            &[],
        ),
        "atan2" => ref_entry(
            name,
            "numericHelper",
            "(atan2 y x)",
            "number",
            "Two-argument arctangent returning radians.",
            true,
            "all .ecky backends",
            "(atan2 y x)",
            &[],
        ),
        "deg" => ref_entry(
            name,
            "numericHelper",
            "(deg radians)",
            "number",
            "Converts radians to degrees.",
            true,
            "all .ecky backends",
            "(deg angle-rad)",
            &[],
        ),
        "rad" => ref_entry(
            name,
            "numericHelper",
            "(rad degrees)",
            "number",
            "Converts degrees to radians.",
            true,
            "all .ecky backends",
            "(rad 90)",
            &[],
        ),
        "deg->rad" => ref_entry(
            name,
            "numericHelper",
            "(deg->rad degrees)",
            "number",
            "Converts degrees to radians.",
            true,
            "all .ecky backends",
            "(deg->rad 90)",
            &[],
        ),
        "rad->deg" => ref_entry(
            name,
            "numericHelper",
            "(rad->deg radians)",
            "number",
            "Converts radians to degrees.",
            true,
            "all .ecky backends",
            "(rad->deg pi-angle)",
            &[],
        ),
        "clamp" => ref_entry(
            name,
            "numericHelper",
            "(clamp value min max)",
            "number",
            "Constrains value to a numeric interval.",
            true,
            "all .ecky backends",
            "(clamp depth 0 3)",
            &[],
        ),
        "lerp" => ref_entry(
            name,
            "numericHelper",
            "(lerp a b t)",
            "number",
            "Linear interpolation from a to b by t.",
            true,
            "all .ecky backends",
            "(lerp 10 20 0.25)",
            &[],
        ),
        "invlerp" => ref_entry(
            name,
            "numericHelper",
            "(invlerp start end value)",
            "number",
            "Maps a value from an interval to its unbounded interpolation factor.",
            true,
            "all .ecky backends",
            "(invlerp 0 100 height)",
            &[],
        ),
        "remap" => ref_entry(
            name,
            "numericHelper",
            "(remap value in-start in-end out-start out-end)",
            "number",
            "Linearly maps a value between two intervals.",
            true,
            "all .ecky backends",
            "(remap height 0 100 1 3)",
            &[],
        ),
        "smoothstep" => ref_entry(
            name,
            "numericHelper",
            "(smoothstep edge0 edge1 x)",
            "number",
            "Smooth Hermite interpolation useful for soft transitions.",
            true,
            "all .ecky backends",
            "(smoothstep 0 1 t)",
            &[],
        ),
        "square" | "cube" => ref_entry(
            name,
            "numericHelper",
            &format!("({name} value)"),
            "number",
            "Raises a number to a small fixed power.",
            true,
            "all .ecky backends",
            &format!("({name} radius)"),
            &[],
        ),
        "hash01" => ref_entry(
            name,
            "numericHelper",
            "(hash01 x y seed)",
            "number 0..1",
            "Deterministic hash value in the 0..1 range for procedural variation.",
            true,
            "all .ecky backends",
            "(hash01 ix iy seed)",
            &["Use explicit seed params; no unseeded randomness."],
        ),
        "hash-signed" => ref_entry(
            name,
            "numericHelper",
            "(hash-signed x y seed)",
            "number -1..1",
            "Deterministic signed hash value for offsets and jitter.",
            true,
            "all .ecky backends",
            "(hash-signed ix iy seed)",
            &["Use explicit seed params; no unseeded randomness."],
        ),
        "noise2" => ref_entry(
            name,
            "numericHelper",
            "(noise2 x y seed)",
            "number 0..1",
            "smooth deterministic value noise sampled at 2D coordinates.",
            true,
            "all .ecky backends",
            "(noise2 (* x 0.1) (* y 0.1) seed)",
            &["Deterministic helper shared by native and FreeCAD interop."],
        ),
        "fbm2" => ref_entry(
            name,
            "numericHelper",
            "(fbm2 x y seed octaves lacunarity gain)",
            "number 0..1",
            "fractal Brownian motion built from deterministic noise2 octaves.",
            true,
            "all .ecky backends",
            "(fbm2 x y seed 4 2.0 0.5)",
            &["Octaves are clamped by runtime; keep small literal counts."],
        ),
        "voronoi2" => ref_entry(
            name,
            "numericHelper",
            "(voronoi2 x y seed)",
            "number 0..1",
            "Deterministic cellular field: high near cell centers, lower near cell borders.",
            true,
            "all .ecky backends",
            "(voronoi2 (* x 0.15) (* y 0.15) seed)",
            &["Use for procedural panel patterns, not exact geometric Voronoi cells."],
        ),
        "cell-distance2" => ref_entry(
            name,
            "numericHelper",
            "(cell-distance2 x y seed)",
            "number 0..1",
            "Distance-like deterministic value to nearest jittered cellular site.",
            true,
            "all .ecky backends",
            "(cell-distance2 x y seed)",
            &["`voronoi2` is derived from this helper."],
        ),
        _ => generic_reference(name, "numericHelper", "number"),
    }
}

fn point_list_reference(name: &str) -> SurfaceReferenceEntry {
    match name {
        "vec2" => ref_entry(
            name,
            "pointListHelper",
            "(vec2 x y)",
            "point2",
            "Constructs a two-coordinate point list.",
            true,
            "all .ecky backends",
            "(vec2 10 20)",
            &[],
        ),
        "vec3" => ref_entry(
            name,
            "pointListHelper",
            "(vec3 x y z)",
            "point3",
            "Constructs a three-coordinate point list.",
            true,
            "all .ecky backends",
            "(vec3 10 20 30)",
            &[],
        ),
        "jitter2" => ref_entry(
            name,
            "pointListHelper",
            "(jitter2 x y amount seed)",
            "point2",
            "Returns a deterministic jittered 2D point from a base coordinate.",
            true,
            "all .ecky backends",
            "(jitter2 10 20 2 seed)",
            &[],
        ),
        "jittered-grid" => ref_entry(
            name,
            "pointListHelper",
            "(jittered-grid rows cols dx dy amount seed)",
            "list<point2>",
            "Builds a deterministic grid of jittered 2D points.",
            true,
            "all .ecky backends",
            "(jittered-grid 4 6 12 12 2 seed)",
            &["Keep rows/cols bounded small literals."],
        ),
        "polar-points" => ref_entry(
            name,
            "pointListHelper",
            "(polar-points count radius)",
            "list<point2>",
            "Builds evenly spaced points around a circle.",
            true,
            "all .ecky backends",
            "(polar-points 32 20)",
            &[],
        ),
        "organic-loop" => ref_entry(
            name,
            "pointListHelper",
            "(organic-loop count radius amount seed)",
            "closed-ish list<point2>",
            "Builds a deterministic irregular loop around a radius.",
            true,
            "all .ecky backends",
            "(organic-loop 32 30 4 seed)",
            &["Useful as a polygon profile."],
        ),
        "wave-loop" => ref_entry(
            name,
            "pointListHelper",
            "(wave-loop count radius amplitude frequency phase)",
            "closed-ish list<point2>",
            "Builds a circular wave profile.",
            true,
            "all .ecky backends",
            "(wave-loop 48 20 3 5 0)",
            &[],
        ),
        "superellipse-point" => ref_entry(
            name,
            "pointListHelper",
            "(superellipse-point angle rx ry exponent)",
            "point2",
            "Samples one point from a superellipse.",
            true,
            "all .ecky backends",
            "(superellipse-point (deg->rad 45) 30 20 4)",
            &[],
        ),
        "voronoi-cells" => ref_entry(
            name,
            "pointListHelper",
            "(voronoi-cells rows cols dx dy amount seed)",
            "list<point2>",
            "Builds jittered grid points suitable as Voronoi-ish perforation centers.",
            true,
            "all .ecky backends",
            "(voronoi-cells 4 6 14 12 2 seed)",
            &["This returns points, not polygon cell boundaries."],
        ),
        "lorenz-points" => ref_entry(
            name,
            "pointListHelper",
            "(lorenz-points count dt scale)",
            "list<point2>",
            "Samples a deterministic Lorenz attractor projection.",
            true,
            "all .ecky backends",
            "(lorenz-points 80 0.01 4)",
            &["No seed argument in current runtime."],
        ),
        "rossler-points" => ref_entry(
            name,
            "pointListHelper",
            "(rossler-points count dt scale)",
            "list<point2>",
            "Samples a deterministic Rossler attractor projection.",
            true,
            "all .ecky backends",
            "(rossler-points 80 0.03 6)",
            &["No seed argument in current runtime."],
        ),
        "logistic-bifurcation-points" => ref_entry(
            name,
            "pointListHelper",
            "(logistic-bifurcation-points r-count samples transient scale)",
            "list<point2>",
            "Builds deterministic points from the logistic map bifurcation diagram.",
            true,
            "all .ecky backends",
            "(logistic-bifurcation-points 24 8 16 30)",
            &["Keep counts bounded small literals."],
        ),
        "henon-points" => ref_entry(
            name,
            "pointListHelper",
            "(henon-points count scale)",
            "list<point2>",
            "Samples deterministic Henon map points.",
            true,
            "all .ecky backends",
            "(henon-points 100 12)",
            &[],
        ),
        _ => generic_reference(name, "pointListHelper", "list"),
    }
}

fn boolean_reference(name: &str) -> SurfaceReferenceEntry {
    let signature = match name {
        "not" => "(not value)".to_string(),
        "and" | "or" => format!("({name} value...)"),
        "=" | ">" | ">=" | "<" | "<=" => format!("({name} a b)"),
        "even?" | "odd?" | "zero?" => format!("({name} number)"),
        "null?" | "empty?" | "list?" => format!("({name} value)"),
        _ => format!("({name} value...)"),
    };
    let example = match name {
        "not" => "(not false)".to_string(),
        "and" | "or" => format!("({name} true false)"),
        "=" | ">" | ">=" | "<" | "<=" => format!("({name} 2 1)"),
        "even?" | "odd?" | "zero?" => format!("({name} 2)"),
        "null?" | "empty?" | "list?" => format!("({name} '())"),
        _ => format!("({name} true)"),
    };
    ref_entry(
        name,
        "booleanHelper",
        &signature,
        "boolean",
        "Boolean predicate or comparator for conditionals and filtering.",
        true,
        "all .ecky backends",
        &example,
        &[],
    )
}

fn cad_op_reference(name: &str, backend: GeometryBackend) -> SurfaceReferenceEntry {
    let support = if matches!(
        name,
        "mesh" | "polyhedron" | "protrude" | "wall-pattern" | "mesh-anchor"
    ) {
        "native mesh only; rejected by FreeCAD interop"
    } else if matches!(name, "hull" | "voronoi-cell" | "import-step") {
        "native direct OCCT only; rejected by FreeCAD interop"
    } else if matches!(name, "sampled-radial-loft") {
        "native direct OCCT and FreeCAD interop; not the mesh runtime"
    } else {
        backend_support(backend)
    };
    match name {
        "mesh" => ref_entry(name, "cadOp", "(mesh :vertices ((x y z) ...) :triangles ((a b c) ...))", "mesh", "Creates bounded indexed triangle geometry. Open orientable surfaces are allowed; invalid indices, degenerate faces, duplicates, non-manifold edges, or inconsistent winding reject.", true, support, "(mesh :vertices ((0 0 0) (10 0 0) (0 10 0)) :triangles ((0 1 2)))", &["Use zero-based integer triangle indices.", "Open mesh preview may render but remains non-printable until boundary edges are closed."]),
        "polyhedron" => ref_entry(name, "cadOp", "(polyhedron :vertices ((x y z) ...) :triangles ((a b c) ...))", "solid", "Creates one closed orientable indexed triangle solid after deterministic topology validation.", true, support, "(polyhedron :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10)) :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))", &["Requires zero boundary edges, zero non-manifold edges, consistent winding, one component, and non-zero signed volume."]),
        "protrude" => ref_entry(name, "cadOp", "(protrude image-path height [:width w] [:depth d] [:fit contain|stretch] [:foreground dark|light])", "solid/mesh", "Raises continuous raster foreground coverage above local Z=0. One physical dimension preserves source aspect ratio; two contain and center by default. `:fit stretch` explicitly fills a non-matching box. Transparent pixels remain empty; an internal closure epsilon stays below the authored base plane.", true, support, "(protrude image-path 4 :width 100 :depth 70 :fit contain :foreground dark)", &["At least one of `:width` or `:depth` is required.", "Dark maps alpha*(1-luminance); light maps alpha*luminance.", "Image path must reference a readable staged PNG, JPEG, or WebP asset."]),
        "mesh-anchor" => ref_entry(name, "cadOp", "(mesh-anchor triangle-index barycentric-0 barycentric-1 barycentric-2)", "surface-trim anchor", "Declares one exact triangle seed used inside a native mesh `surface-trim` path.", true, support, "(mesh-anchor 42 0.2 0.3 0.5)", &["Nested helper for `surface-trim`; not standalone geometry."]),
        "box" => ref_entry(name, "cadOp", "(box x y z :align '(x y z))", "solid", "Creates an axis-aligned rectangular solid.", true, support, "(box 40 20 10 :align '(min center min))", &[]),
        "sphere" => ref_entry(name, "cadOp", "(sphere radius)", "solid", "Creates a sphere.", true, support, "(sphere 12)", &[]),
        "cylinder" => ref_entry(name, "cadOp", "(cylinder radius height segments)", "solid", "Creates a cylinder along local Z.", true, support, "(cylinder 8 30 48)", &[]),
        "cone" => ref_entry(name, "cadOp", "(cone r1 r2 height segments)", "solid", "Creates a cone or tapered cylinder along local Z.", true, support, "(cone 12 6 30 48)", &[]),
        "circle" => ref_entry(name, "cadOp", "(circle radius segments)", "sketch", "Creates a circular sketch/profile.", true, support, "(circle 20 64)", &[]),
        "ring" => ref_entry(name, "cadOp", "(ring outer-radius inner-radius segments)", "sketch", "Creates an annular sketch aliasing to a profile with one outer and one hole circle.", true, support, "(ring 20 10 64)", &[]),
        "rectangle" => ref_entry(name, "cadOp", "(rectangle width height)", "sketch", "Creates a rectangular sketch/profile.", true, support, "(rectangle 40 20)", &[]),
        "rounded-rect" => ref_entry(name, "cadOp", "(rounded-rect width height radius)", "sketch", "Creates a rectangle profile with rounded corners.", true, support, "(rounded-rect 40 20 3)", &[]),
        "rounded-polygon" => ref_entry(name, "cadOp", "(rounded-polygon points radius)", "sketch", "Creates a polygon profile with rounded corners.", true, support, "(rounded-polygon points 2)", &[]),
        "polygon" => ref_entry(name, "cadOp", "(polygon ((x y)...))", "sketch", "Creates a closed polygon sketch from 2D points.", true, support, "(polygon ((0 0) (40 0) (40 20) (0 20)))", &[]),
        "profile" => ref_entry(name, "cadOp", "(profile :outer sketch :holes sketch-or-list)", "sketch", "Builds a face profile from an outer loop and optional hole loops.", true, support, "(profile :outer (circle 20) :holes (circle 6))", &[]),
        "make-face" => ref_entry(name, "cadOp", "(make-face sketch)", "face/sketch", "Turns a closed sketch into a face-like profile for downstream ops.", true, support, "(make-face (polygon points))", &[]),
        "text" => ref_entry(
            name,
            "cadOp",
            "(text value size [:font selector])",
            "sketch/solid",
            "Creates a text profile. `:font` selects the face for this call before downstream extrusion.",
            true,
            support,
            "(extrude (text \"A\" 12 :font \"Arial\") 2)",
            &[
                "The selector accepts an installed font family name or an absolute font-file path.",
                "Put `:font` on the single `text` call that should change; it is not an `extrude` keyword.",
            ],
        ),
        "svg" => ref_entry(name, "cadOp", "(svg path-or-data)", "sketch/solid", "Imports SVG profile/path data where backend lowering supports it.", true, support, "(svg iconData)", &[]),
        "import-stl" => ref_entry(name, "cadOp", "(import-stl path [:target-triangles n :max-error d [:preserve-boundaries #t|#f]])", "mesh/solid", "Imports an STL file as geometry. Optional preparation keywords keep the raw source and derive a bounded indexed mesh.", true, support, "(import-stl \"/tmp/part.stl\" :target-triangles 4000 :max-error 0.05 :preserve-boundaries #t)", &["Use absolute paths from attachments or app artifacts.", "Preparation keywords require both `:target-triangles` and `:max-error`."]),
        "import-step" => ref_entry(name, "cadOp", "(import-step path)", "solid/BRep", "Imports an exact STEP payload through native Direct OCCT.", true, support, "(import-step \"/absolute/path/component.step\")", &["Prefer locked `import-component` packages; raw paths are not portable dependencies."]),
        "path" => ref_entry(name, "cadOp", "(path segment...)", "path", "Builds a path from path segments.", true, support, "(path (polyline points))", &[]),
        "polyline" => ref_entry(name, "cadOp", "(polyline points)", "path/sketch", "Builds a connected line path from points.", true, support, "(polyline ((0 0) (10 0) (10 5)))", &[]),
        "bezier-path" => ref_entry(name, "cadOp", "(bezier-path points)", "path", "Builds a cubic Bézier path from control points; native lowering uses a fixed 16 samples per cubic, so the path is an approximation.", true, support, "(bezier-path ((0 0 0) (8 0 0) (8 8 12) (16 8 12)))", &["Do not claim an exact curve from a native plan."]),
        "bspline" => ref_entry(name, "cadOp", "(bspline points :closed #t|#f)", "sketch", "Builds a 2D B-spline sketch from control points.", true, support, "(bspline points :closed #t)", &[]),
        "extrude" => ref_entry(name, "cadOp", "(extrude sketch-or-image height [:symmetric #t|#f] [:width w] [:depth d] [:fit contain|stretch] [:threshold 0..1] [:foreground dark|light])", "solid", "Extrudes a sketch, or traces raster foreground coverage into contours before the same extrusion. One raster dimension preserves source aspect ratio; two contain and center by default. `:fit stretch` explicitly fills a non-matching box.", true, support, "(extrude image-path 3 :width 40 :depth 30 :fit contain :threshold 0.5 :foreground dark)", &["Raster options apply only when the first operand is an image path.", "Raster input requires at least one of `:width` or `:depth`.", "Transparent pixels remain empty for both foreground modes."]),
        "revolve" => ref_entry(name, "cadOp", "(revolve sketch angle)", "solid", "Revolves a sketch profile around an axis.", true, support, "(revolve profile 360)", &[]),
        "loft" => ref_entry(name, "cadOp", "(loft sketch...)", "solid", "Creates a solid through multiple sketch sections.", true, support, "(loft bottom top)", &[]),
        "sweep" => ref_entry(name, "cadOp", "(sweep profile path)", "solid", "Sweeps a profile along a path.", true, support, "(sweep (circle 2 16) rail)", &[]),
        "helical-ridge" => ref_entry(name, "cadOp", "(helical-ridge :radius r :pitch p :height h :base-width w :crest-width w :depth d [:female #t] [:clearance c] [:lefthand #t])", "solid", "Creates a printable trapezoid ridge swept along a cylindrical helix.", true, support, "(helical-ridge :radius 32 :pitch 5.25 :height 16.8 :base-width 1.45 :crest-width 0.55 :depth 1.5)", &["Use the same radius, pitch, and height for the matching female groove cutter; set `:female #t` plus clearance to expand its envelope."]),
        "shell" => ref_entry(name, "cadOp", "(shell thickness [:faces selector] solid)", "solid", "Hollows or thickens a solid by wall thickness. Exact backends also accept `:faces` with `target-id:<id>` or `target-ids:<id>|<id>` to choose shell opening faces.", true, support, "(shell 2 :faces \"target-id:body:face:0-0-20:1256.637\" (cylinder 20 80))", &[]),
        "offset" => ref_entry(name, "cadOp", "(offset distance sketch)", "sketch", "Offsets a sketch/profile by distance.", true, support, "(offset 2 profile)", &[]),
        "offset-rounded" => ref_entry(name, "cadOp", "(offset-rounded distance sketch)", "sketch", "Offsets a sketch with rounded joins where supported.", true, support, "(offset-rounded 2 profile)", &[]),
        "fillet" => ref_entry(name, "cadOp", "(fillet radius [:edges selector] solid)", "solid", "Rounds edges of a solid. `:edges` accepts coarse selectors like `top`, `left`, `axis-z`, `x-min`, or `x-min+z-max`; exact backends also accept `target-id:<id>` and `target-ids:<id>|<id>`.", true, support, "(fillet 2 :edges \"x-min+z-max\" body)", &["Topology-sensitive post-op: if the selector matches no edges after one smaller-radius retry and one selector retry, stop retrying fillet. Rebuild with rounded source geometry such as `rounded-rect`, `rounded-polygon`, `offset-rounded`, `loft`, `taper`, `cone`, or explicit profiles."]),
        "chamfer" => ref_entry(name, "cadOp", "(chamfer distance [:edges selector] solid)", "solid", "Bevels edges of a solid. `:edges` accepts coarse selectors like `bottom`, `front`, `axis-z`, `y-max`, or `x-min+z-max`; exact backends also accept `target-id:<id>` and `target-ids:<id>|<id>`.", true, support, "(chamfer 1 :edges \"bottom\" body)", &["Topology-sensitive post-op: if the selector matches no edges after one smaller-distance retry and one selector retry, stop retrying chamfer. Rebuild with source bevel/rounding geometry such as explicit profiles, `loft`, `taper`, `cone`, `rounded-polygon`, or `offset-rounded`."]),
        "taper" => ref_entry(name, "cadOp", "(taper height scale sketch) or (taper height scale-x scale-y sketch)", "solid", "Extrudes a sketch while scaling the top section.", true, support, "(taper 30 0.7 0.7 (circle 12 32))", &[]),
        "draft" => ref_entry(name, "cadOp", "(draft angle solid)", "solid", "Applies a draft angle to a solid.", true, support, "(draft 2deg body)", &["Topology-sensitive; prefer explicit tapered source geometry when the result is unstable."]),
        "twist" => ref_entry(name, "cadOp", "(twist height angle sketch)", "solid", "Extrudes a sketch while rotating sections along height.", true, support, "(twist 40 90 profile)", &[]),
        "sampled-radial-loft" => ref_entry(name, "cadOp", "(sampled-radial-loft (theta z fz) :height h :z-steps n :theta-steps n :radius expr :z-map expr?)", "solid", "Samples radial sections across height, then lofts the wires/faces into a solid.", true, support, "(sampled-radial-loft (theta z fz) :height 40 :z-steps 24 :theta-steps 72 :radius (+ 18 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793))))))", &["Binders expose per-sample `theta`, absolute `z`, and normalized `fz` in `[0,1]`.", "Renders through native direct OCCT and FreeCAD interop for formula-driven dome/pot families."]),
        "hull" => ref_entry(name, "cadOp", "(hull solid...)", "solid", "Convex hull of the child solids as a single closed BREP solid.", true, support, "(hull (sphere 6) (translate 30 0 0 (sphere 6)))", &["Native direct OCCT only; FreeCAD interop rejects it.", "Great for organic blends/bridges (slipper lasts, fairings) without hand-lofting sections."]),
        "union" | "fuse" => ref_entry(name, "cadOp", &format!("({name} solid...)"), "solid", "Boolean union/fuse of solids.", true, support, &format!("({name} a b c)"), &[]),
        "difference" | "cut" => ref_entry(name, "cadOp", &format!("({name} base cutter...)"), "solid", "Subtracts cutter solids from a base solid.", true, support, &format!("({name} body hole)"), &[]),
        "intersection" | "common" => ref_entry(name, "cadOp", &format!("({name} solid...)"), "solid", "Keeps shared volume of solids.", true, support, &format!("({name} a b)"), &[]),
        "xor" => ref_entry(name, "cadOp", "(xor solid...)", "solid", "Boolean exclusive-or for solids where supported.", true, support, "(xor a b)", &[]),
        "compound" => ref_entry(name, "cadOp", "(compound geometry...)", "compound", "Groups geometry without fusing into one solid.", true, support, "(compound body bolts)", &[]),
        "translate" => ref_entry(name, "cadOp", "(translate x y z geometry)", "geometry", "Moves geometry by XYZ offset.", true, support, "(translate 10 0 0 body)", &[]),
        "rotate" => ref_entry(name, "cadOp", "(rotate x-deg y-deg z-deg geometry)", "geometry", "Rotates geometry in degrees around local axes.", true, support, "(rotate 0 0 45 body)", &[]),
        "scale" => ref_entry(name, "cadOp", "(scale x y z geometry)", "geometry", "Scales geometry by XYZ factors.", true, support, "(scale 1 1 0.5 body)", &[]),
        "mirror" => ref_entry(name, "cadOp", "(mirror axis offset geometry)", "geometry", "Mirrors geometry across the `x`, `y`, or `z` plane at offset.", true, support, "(mirror \"x\" 0 body)", &[]),
        "linear-array" => ref_entry(name, "cadOp", "(linear-array count dx dy dz geometry)", "compound", "Repeats geometry in a linear sequence.", true, support, "(linear-array 4 12 0 0 rib)", &[]),
        "radial-array" => ref_entry(name, "cadOp", "(radial-array count radius geometry)", "compound", "Repeats geometry around a circle.", true, support, "(radial-array 12 30 spoke)", &[]),
        "grid-array" => ref_entry(name, "cadOp", "(grid-array rows cols dx dy geometry)", "compound", "Repeats geometry on a 2D grid.", true, support, "(grid-array 3 5 12 12 hole)", &[]),
        "arc-array" => ref_entry(name, "cadOp", "(arc-array count radius start-angle end-angle geometry)", "compound", "Repeats geometry along an arc.", true, support, "(arc-array 8 30 0 180 notch)", &[]),
        "repeat" | "repeat-union" | "repeat-compound" | "repeat-pick" => ref_entry(name, "cadOp", &format!("({name} count fn-or-geometry)"), "geometry", "Repeat helper for patterned geometry generation.", true, support, &format!("({name} 6 rib)"), &["Prefer explicit arrays when they express the layout directly."]),
        "for-union" => ref_entry(name, "cadOp", "(for-union list fn)", "solid", "Maps list values to solids and unions the result.", true, support, "(for-union (range 6) (lambda (i) ...))", &[]),
        "for-compound" => ref_entry(name, "cadOp", "(for-compound list fn)", "compound", "Maps list values to geometry and compounds the result.", true, support, "(for-compound points (lambda (p) ...))", &[]),
        "plane" => ref_entry(name, "cadOp", "(plane :origin '(x y z) :x '(x y z) :normal '(x y z))", "plane", "Creates a local coordinate plane.", true, support, "(plane :origin '(80 0 6) :normal '(0 0 1))", &[]),
        "location" => ref_entry(name, "cadOp", "(location frame :offset '(x y z) :rotate '(x y z))", "location", "Creates a placement from a frame and optional local transform.", true, support, "(location (plane :origin '(80 0 6)) :rotate '(0 90 0))", &[]),
        "path-frame" => ref_entry(name, "cadOp", "(path-frame path :at start|end|t :up '(x y z))", "location", "Computes a local frame along a path parameter.", true, support, "(path-frame rail :at end :up '(0 0 1))", &[]),
        "place" => ref_entry(name, "cadOp", "(place frame geometry :offset '(x y z) :rotate '(x y z))", "geometry", "Places geometry in a local coordinate frame.", true, support, "(place end-frame (cylinder 4 18) :offset '(0 0 -9))", &[]),
        "clip-box" => ref_entry(name, "cadOp", "(clip-box geometry :x '(min max) :y '(min max) :z '(min max))", "geometry", "Clips geometry by an axis-aligned box; all three ranges are required.", true, support, "(clip-box body :x '(0 100) :y '(-30 30) :z '(0 40))", &["Omitting `:x` or `:y` is an invalid call, not an unsupported operation."]),
        "clip-plane" => ref_entry(name, "cadOp", "(clip-plane geometry :origin '(x y z) :normal '(x y z) [:keep \"positive\"|\"negative\"])", "geometry", "Clips geometry against an oriented plane. `:keep` is text; quote it to avoid unresolved local symbols.", true, support, "(clip-plane body :origin '(0 0 10) :normal '(0 0 1) :keep \"positive\")", &["The native planner accepts this known bare literal too, but an unresolved local `positive` can fail name resolution; use quoted text in authored source."]),
        "build" => ref_entry(name, "cadOp", "(build expr...)", "geometry", "Build container for grouped construction forms.", true, support, "(build (shape body) (result body))", &[]),
        "shape" => ref_entry(name, "cadOp", "(shape geometry)", "geometry", "Marks or wraps a geometry expression in build contexts.", true, support, "(shape body)", &[]),
        "result" => ref_entry(name, "cadOp", "(result geometry)", "geometry", "Selects final geometry from a build context.", true, support, "(result body)", &[]),
        "wall-pattern" => ref_entry(name, "cadOp", "(wall-pattern (:mode mode :depth n :uFreq n :vFreq n :seed n) shell-target)", "mesh", "Applies mesh/eckyRust procedural displacement/perforation-style wall patterns to supported shell surface targets.", true, support, "(wall-pattern (:mode gyroid :depth 0.6 :uFreq 4 :vFreq 5) (shell 2 (cylinder 20 80)))", &["Supported targets: shell surfaces built from cylinder, cone, sphere, extrude, revolve, loft, taper, and twist."]),
        "torus" => ref_entry(name, "cadOp", "(torus major minor)", "solid", "Creates a ring torus: tube of radius `minor` swept at distance `major` from the Z axis.", true, support, "(torus 20 5)", &[]),
        "ellipse" => ref_entry(name, "cadOp", "(ellipse rx ry)", "sketch", "Creates an elliptical 2D profile with radii along X and Y.", true, support, "(ellipse 10 4)", &[]),
        "regular-polygon" => ref_entry(name, "cadOp", "(regular-polygon sides radius :rotation deg)", "sketch", "Creates a regular n-gon 2D profile by side count and circumradius.", true, support, "(regular-polygon 6 10)", &[]),
        "voronoi-cell" => ref_entry(name, "cadOp", "(voronoi-cell sites index width height inset)", "sketch", "Creates one exact bounded Voronoi polygon, uniformly inset and expressed relative to its selected site.", true, support, "(voronoi-cell (voronoi-cells 3 3 12 12 1.5 23) 4 40 40 1.2)", &["Direct OCCT expands this profile to polygon edges before BREP construction.", "May cut or fuse analytic solids and poly-BREP solids produced by solidify(import-stl ...)."]),
        "trapezoid" => ref_entry(name, "cadOp", "(trapezoid bottom top height :skew s)", "sketch", "Creates a trapezoid 2D profile (parallel bottom/top widths, given height, optional skew).", true, support, "(trapezoid 20 10 8 :skew 3)", &[]),
        "wedge" => ref_entry(name, "cadOp", "(wedge dx dy dz xmin zmin xmax zmax :align '(x y z))", "solid", "Creates a wedge/ramp solid: a dx×dy×dz box whose top face is shrunk to the xmin..xmax / zmin..zmax window.", true, support, "(wedge 20 10 20 5 5 15 15)", &[]),
        "slot-overall" => ref_entry(name, "cadOp", "(slot-overall length width)", "sketch", "Creates an obround (stadium) 2D profile of given overall length and width.", true, support, "(slot-overall 40 10)", &[]),
        "slot-center-to-center" => ref_entry(name, "cadOp", "(slot-center-to-center separation width)", "sketch", "Obround 2D profile specified by the distance between the two end-arc centers.", true, support, "(slot-center-to-center 30 10)", &[]),
        "slot-center-point" => ref_entry(name, "cadOp", "(slot-center-point cx cy px py width)", "sketch", "Obround 2D profile from a center point to an end point, with width.", true, support, "(slot-center-point 0 0 20 0 10)", &[]),
        "slot-arc" => ref_entry(name, "cadOp", "(slot-arc radius start end width)", "sketch", "Curved (annular) obround: a circular-arc centerline of given radius from `start` to `end` degrees, thickened by width.", true, support, "(slot-arc 20 0 90 10)", &[]),
        "rib" => ref_entry(name, "cadOp", "(rib solid profile path)", "solid", "Adds material: sweeps `profile` along `path` and unions it onto `solid`.", true, support, "(rib (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))", &[]),
        "groove" => ref_entry(name, "cadOp", "(groove solid profile path)", "solid", "Removes material: sweeps `profile` along `path` and subtracts it from `solid`.", true, support, "(groove (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))", &[]),
        "thread" => ref_entry(name, "cadOp", "(thread :radius r :pitch p :length len :depth d [:base-width w] [:crest-width w] [:female #t] [:clearance c] [:lefthand #t] [:iso \"M4\"])", "solid", "Parametric helical thread: a core cylinder plus a `helical-ridge` (male), or a ridge cutter (`:female`). `:iso \"M4\"` decodes a metric designation into pitch/radius.", true, support, "(thread :radius 8 :pitch 2 :length 16 :depth 1)", &["Female threads are cut with `difference`; pair male/female with matching `:radius`/`:pitch` and add `:clearance`."]),
        "tapped-hole" => ref_entry(name, "cadOp", "(tapped-hole :iso \"M8\" :length len [:radius r] [:pitch p] [:depth d] [:base-width w] [:crest-width w] [:lefthand #t])", "solid", "A tapped (internal female) thread cut as a positive cavity: a named-radius bore cylinder at the ISO minor diameter unioned with a helical relief ridge whose crest reaches the major diameter. `:iso \"M8\"` decodes a metric designation; an equal-nominal `thread` mates with it.", true, support, "(tapped-hole :iso \"M8\" :length 14)", &["Subtract from a part with `difference` to cut the tapped hole.", "Pair with a matching `:iso` `thread`; both decode the same minor radius so the bolt core seats in the bore."]),
        _ => generic_reference(name, "cadOp", "geometry"),
    }
}

fn wall_pattern_mode_reference(name: &str) -> SurfaceReferenceEntry {
    let description = match name {
        "ribs" => "Straight rib pattern along the shell parameter direction.",
        "rings" => "Ring bands around the shell parameter direction.",
        "spiral" => "Spiral rib pattern across shell parameters.",
        "diamond" => "Cross-hatched diamond displacement field.",
        "hammered" => "Seeded hammered texture using deterministic noise.",
        "fourier" => "Layered sine/cosine Fourier-style displacement field.",
        "cellular" => "Seeded cellular/Voronoi-like displacement field.",
        "fbm" => "Fractal noise displacement field.",
        "gyroid" => "triply periodic gyroid implicit field.",
        "schwarz-p" => "Triply periodic Schwarz P implicit field.",
        "schwarz-d" => "Triply periodic Schwarz D implicit field.",
        "diamond-field" => "Alias-style diamond periodic implicit field.",
        "neovius" => "Triply periodic Neovius implicit field.",
        "attractor-field" => "Seeded chaotic attractor-style field.",
        _ => "Procedural wall pattern mode.",
    };
    ref_entry(
        name,
        "wallPatternMode",
        name,
        "wall-pattern :mode value",
        description,
        true,
        "native mesh only",
        &format!("(wall-pattern (:mode {name} :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)"),
        &["Use only when current geometryBackend is mesh."],
    )
}

fn generic_reference(name: &str, kind: &str, returns: &str) -> SurfaceReferenceEntry {
    ref_entry(
        name,
        kind,
        &format!("({name} ...)"),
        returns,
        "Supported `.ecky` surface entry. Read backend guide and validation errors for exact constraints.",
        true,
        "all .ecky backends unless gated by manifest backend",
        &format!("({name} ...)"),
        &[],
    )
}

pub fn join_backticked(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| format!("`{}`", item))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_scheme::cad;
    use crate::ecky_scheme::core;

    const CORE_BUILTIN_NUMERIC_HELPERS: &[&str] = &[
        "+", "-", "*", "/", "min", "max", "abs", "floor", "sin", "cos", "tan", "atan", "atan2",
    ];

    #[test]
    fn supported_surface_manifest_uses_canonical_arrays() {
        let manifest = supported_surface_manifest(GeometryBackend::Build123d);
        let expected_ops = CAD_OPS_PORTABLE.to_vec();

        assert_eq!(manifest.model_clauses, MODEL_CLAUSES);
        assert_eq!(manifest.model_wrappers, MODEL_WRAPPERS);
        assert_eq!(
            manifest.component_placement_forms,
            COMPONENT_PLACEMENT_FORMS
        );
        assert_eq!(manifest.core_constants, CORE_CONSTANTS);
        assert_eq!(manifest.expression_forms, EXPRESSION_FORMS);
        assert_eq!(manifest.numeric_helpers, NUMERIC_HELPERS);
        assert_eq!(manifest.point_list_helpers, POINT_LIST_HELPERS);
        assert_eq!(manifest.boolean_helpers, BOOLEAN_HELPERS);
        assert_eq!(manifest.cad_ops, expected_ops);
        assert_eq!(manifest.typed_hole_policy, TYPED_HOLE_POLICY);
    }

    #[test]
    fn backend_manifests_gate_exact_and_native_only_surfaces() {
        for backend in [GeometryBackend::Build123d, GeometryBackend::Freecad] {
            let manifest = supported_surface_manifest(backend);

            assert!(manifest.cad_ops.contains(&"sampled-radial-loft"));
            assert!(manifest.cad_ops.contains(&"helical-ridge"));
            assert!(!manifest.cad_ops.contains(&"wall-pattern"));
            assert!(!manifest.cad_ops.contains(&"hull"));
            assert!(manifest.wall_pattern_modes.is_empty());
        }

        let mesh_manifest = supported_surface_manifest(GeometryBackend::EckyRust);

        assert!(mesh_manifest.cad_ops.contains(&"sampled-radial-loft"));
        assert!(mesh_manifest.cad_ops.contains(&"helical-ridge"));
        assert!(mesh_manifest.cad_ops.contains(&"wall-pattern"));
        assert!(mesh_manifest.cad_ops.contains(&"hull"));
        assert_eq!(mesh_manifest.wall_pattern_modes, WALL_PATTERN_MODES);
    }

    #[test]
    fn text_reference_exposes_per_call_font_selector() {
        let reference = supported_surface_reference(GeometryBackend::EckyRust)
            .entries
            .into_iter()
            .find(|entry| entry.name == "text")
            .expect("text reference");

        assert!(reference.signature.contains(":font selector"));
        assert!(reference.example.contains(":font \"Arial\""));
        assert!(reference
            .notes
            .iter()
            .any(|note| note.contains("family name")));
        assert!(reference
            .notes
            .iter()
            .any(|note| note.contains("single `text` call")));
    }

    #[test]
    fn numeric_helper_manifest_names_are_core_exports_unless_steel_builtins() {
        for helper in CORE_BUILTIN_NUMERIC_HELPERS {
            assert!(
                NUMERIC_HELPERS.contains(helper),
                "stale builtin allowlist entry: {helper}"
            );
        }

        for helper in NUMERIC_HELPERS {
            if CORE_BUILTIN_NUMERIC_HELPERS.contains(helper) {
                continue;
            }

            assert!(
                core::MODULE.exports.contains(helper),
                "manifested numeric helper missing ecky/core export: {helper}"
            );
        }
    }

    #[test]
    fn point_list_helper_manifest_names_are_core_exports() {
        for helper in POINT_LIST_HELPERS {
            assert!(
                core::MODULE.exports.contains(helper),
                "manifested point/list helper missing ecky/core export: {helper}"
            );
        }
    }

    #[test]
    fn manifest_cad_ops_are_exported_by_cad_module() {
        for op in CAD_OPS_PORTABLE
            .iter()
            .chain(ECKY_RUST_ONLY_CAD_OPS.iter())
            .chain(ECKY_RUST_DIRECT_ONLY_CAD_OPS.iter())
        {
            assert!(cad::MODULE.exports.contains(op), "missing export: {op}");
        }
    }

    #[test]
    fn manifest_covers_every_public_core_export() {
        for export in core::MODULE.exports {
            assert!(
                CORE_CONSTANTS.contains(export)
                    || EXPRESSION_FORMS.contains(export)
                    || NUMERIC_HELPERS.contains(export)
                    || POINT_LIST_HELPERS.contains(export),
                "public ecky/core export lacks surface documentation: {export}"
            );
        }
    }

    #[test]
    fn manifest_or_canonical_guide_covers_every_public_cad_export() {
        const DOCUMENTED_STRUCTURAL_FORMS: &[&str] = &[
            "model",
            "part",
            "feature",
            "view",
            "offset-part",
            "tag-face",
            "tag-edge",
            "tag-edges",
            "tag",
            "build",
            "shape",
            "result",
            "hole",
        ];
        let guide = include_str!("../../public/docs/ecky-agent-reference.md");
        let documented_ops = CAD_OPS_PORTABLE
            .iter()
            .chain(ECKY_RUST_ONLY_CAD_OPS.iter())
            .chain(ECKY_RUST_DIRECT_ONLY_CAD_OPS.iter())
            .copied()
            .collect::<std::collections::BTreeSet<_>>();

        for export in cad::MODULE.exports {
            assert!(
                documented_ops.contains(export) || DOCUMENTED_STRUCTURAL_FORMS.contains(export),
                "public ecky/cad export lacks surface documentation: {export}"
            );
            if DOCUMENTED_STRUCTURAL_FORMS.contains(export) {
                assert!(
                    guide.contains(&format!("`{export}`")) || guide.contains(&format!("({export}")),
                    "canonical guide lacks structural form `{export}`"
                );
            }
        }
    }

    #[test]
    fn model_clause_manifest_matches_compiler_direct_clause_contract() {
        assert_eq!(
            MODEL_CLAUSES,
            [
                "params",
                "verify",
                "part",
                "feature",
                "meta",
                "tag-vertex",
                "tag-face",
                "tag-edge",
                "tag-edges",
                "view",
                "analysis",
            ]
        );
    }

    #[test]
    fn backend_cad_op_sets_match_actual_support() {
        assert!(cad_ops_for_backend(GeometryBackend::Build123d).contains(&"sampled-radial-loft"));
        assert!(cad_ops_for_backend(GeometryBackend::Freecad).contains(&"sampled-radial-loft"));
        assert!(cad_ops_for_backend(GeometryBackend::EckyRust).contains(&"sampled-radial-loft"));
        assert!(cad_ops_for_backend(GeometryBackend::Build123d).contains(&"helical-ridge"));
        assert!(cad_ops_for_backend(GeometryBackend::Freecad).contains(&"helical-ridge"));
        assert!(cad_ops_for_backend(GeometryBackend::EckyRust).contains(&"helical-ridge"));
        assert!(!cad_ops_for_backend(GeometryBackend::Build123d).contains(&"wall-pattern"));
        assert!(!cad_ops_for_backend(GeometryBackend::Freecad).contains(&"wall-pattern"));
        assert!(cad_ops_for_backend(GeometryBackend::EckyRust).contains(&"wall-pattern"));
        assert!(!cad_ops_for_backend(GeometryBackend::Build123d).contains(&"hull"));
        assert!(!cad_ops_for_backend(GeometryBackend::Freecad).contains(&"hull"));
        assert!(cad_ops_for_backend(GeometryBackend::EckyRust).contains(&"hull"));
    }

    #[test]
    fn surface_reference_documents_actual_cad_op_signatures() {
        let reference = supported_surface_reference(GeometryBackend::Build123d);
        let lookup = |name: &str| {
            reference
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("missing reference entry: {name}"))
        };

        assert_eq!(lookup("mirror").signature, "(mirror axis offset geometry)");
        assert_eq!(
            lookup("taper").signature,
            "(taper height scale sketch) or (taper height scale-x scale-y sketch)"
        );
        assert_eq!(
            lookup("offset-rounded").signature,
            "(offset-rounded distance sketch)"
        );
        assert_eq!(
            lookup("sampled-radial-loft").signature,
            "(sampled-radial-loft (theta z fz) :height h :z-steps n :theta-steps n :radius expr :z-map expr?)"
        );
        assert_eq!(
            lookup("helical-ridge").signature,
            "(helical-ridge :radius r :pitch p :height h :base-width w :crest-width w :depth d [:female #t] [:clearance c] [:lefthand #t])"
        );
    }

    #[test]
    fn component_placement_forms_reach_manifest_reference_and_canonical_guide() {
        let manifest = supported_surface_manifest(GeometryBackend::EckyRust);
        let reference = supported_surface_reference(GeometryBackend::EckyRust);
        let guide = include_str!("../../public/docs/ecky-agent-reference.md");
        for name in COMPONENT_PLACEMENT_FORMS {
            assert!(manifest.component_placement_forms.contains(name));
            assert!(
                reference.entries.iter().any(|entry| entry.name == *name),
                "surface reference missing `{name}`"
            );
            assert!(guide.contains(name), "canonical guide missing `{name}`");
        }
    }

    #[test]
    fn canonical_surface_names_chaotic_helpers() {
        for helper in [
            "lorenz-points",
            "rossler-points",
            "logistic-bifurcation-points",
            "henon-points",
        ] {
            assert!(
                POINT_LIST_HELPERS.contains(&helper),
                "missing helper: {helper}"
            );
        }
    }

    #[test]
    fn canonical_surface_names_mesh_only_wall_pattern_modes() {
        for mode in [
            "schwarz-p",
            "schwarz-d",
            "diamond-field",
            "neovius",
            "attractor-field",
        ] {
            assert!(WALL_PATTERN_MODES.contains(&mode), "missing mode: {mode}");
        }
    }

    #[test]
    fn surface_reference_covers_all_manifest_names() {
        let backend = GeometryBackend::EckyRust;
        let reference = supported_surface_reference(backend);

        for name in MODEL_CLAUSES
            .iter()
            .chain(MODEL_WRAPPERS.iter())
            .chain(COMPONENT_PLACEMENT_FORMS.iter())
            .chain(EXPRESSION_FORMS.iter())
            .chain(NUMERIC_HELPERS.iter())
            .chain(POINT_LIST_HELPERS.iter())
            .chain(BOOLEAN_HELPERS.iter())
            .chain(cad_ops_for_backend(backend).iter())
            .chain(wall_pattern_modes_for_backend(backend).iter())
        {
            let entry = reference
                .entries
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap_or_else(|| panic!("missing reference entry: {name}"));
            assert!(!entry.signature.is_empty(), "missing signature: {name}");
            assert!(!entry.description.is_empty(), "missing description: {name}");
            assert!(!entry.example.is_empty(), "missing example: {name}");
        }
    }

    #[test]
    fn surface_reference_documents_generative_helpers_and_mesh_modes() {
        let reference = supported_surface_reference(GeometryBackend::EckyRust);
        let lookup = |name: &str| {
            reference
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("missing reference entry: {name}"))
        };

        let noise = lookup("noise2");
        assert_eq!(noise.signature, "(noise2 x y seed)");
        assert!(noise
            .description
            .contains("smooth deterministic value noise"));
        assert!(noise.deterministic);
        assert!(noise.example.contains("(noise2"));

        let fbm = lookup("fbm2");
        assert_eq!(fbm.signature, "(fbm2 x y seed octaves lacunarity gain)");
        assert!(fbm.description.contains("fractal"));

        let voronoi = lookup("voronoi2");
        assert_eq!(voronoi.signature, "(voronoi2 x y seed)");
        assert!(voronoi.description.contains("cellular"));

        let cells = lookup("voronoi-cells");
        assert_eq!(
            cells.signature,
            "(voronoi-cells rows cols dx dy amount seed)"
        );
        assert!(cells.description.contains("jittered grid"));

        let wall = lookup("wall-pattern");
        assert_eq!(
            wall.backend_support,
            "native mesh only; rejected by FreeCAD interop"
        );

        let gyroid = lookup("gyroid");
        assert_eq!(gyroid.kind, "wallPatternMode");
        assert!(gyroid.description.contains("triply periodic"));
    }

    #[test]
    fn surface_reference_documents_portable_sampled_radial_loft() {
        let reference = supported_surface_reference(GeometryBackend::Build123d);
        let radial = reference
            .entries
            .iter()
            .find(|entry| entry.name == "sampled-radial-loft")
            .expect("missing sampled-radial-loft");

        assert_eq!(
            radial.backend_support,
            "native direct OCCT and FreeCAD interop; not the mesh runtime"
        );
        assert!(radial.description.contains("Samples radial sections"));
        assert!(radial.notes.iter().any(|note| note.contains("theta")));
    }

    #[test]
    fn surface_reference_warns_against_blind_edge_modifier_retries() {
        let reference = supported_surface_reference(GeometryBackend::Build123d);
        let lookup = |name: &str| {
            reference
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("missing reference entry: {name}"))
        };

        let fillet = lookup("fillet");
        assert!(fillet.notes.iter().any(|note| {
            note.contains("selector matches no edges")
                && note.contains("stop retrying fillet")
                && note.contains("rounded source geometry")
        }));

        let chamfer = lookup("chamfer");
        assert!(chamfer.notes.iter().any(|note| {
            note.contains("selector matches no edges")
                && note.contains("stop retrying chamfer")
                && note.contains("explicit profiles")
        }));
    }

    #[test]
    fn surface_reference_backend_gates_wall_pattern_entries() {
        let build123d = supported_surface_reference(GeometryBackend::Build123d);
        assert!(build123d
            .entries
            .iter()
            .any(|entry| entry.name == "sampled-radial-loft"));
        assert!(!build123d
            .entries
            .iter()
            .any(|entry| entry.name == "wall-pattern"));
        assert!(!build123d
            .entries
            .iter()
            .any(|entry| entry.kind == "wallPatternMode"));

        let mesh = supported_surface_reference(GeometryBackend::EckyRust);
        assert!(mesh
            .entries
            .iter()
            .any(|entry| entry.name == "sampled-radial-loft"));
        assert!(mesh
            .entries
            .iter()
            .any(|entry| entry.name == "wall-pattern"));
        assert!(mesh
            .entries
            .iter()
            .any(|entry| entry.name == "attractor-field"));
    }
}
