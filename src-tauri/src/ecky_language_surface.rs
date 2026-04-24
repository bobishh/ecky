use crate::models::GeometryBackend;

pub const MODEL_CLAUSES: &[&str] = &["params", "part", "meta"];
pub const MODEL_WRAPPERS: &[&str] = &["begin", "let", "let*"];
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
    "smoothstep",
    "hash01",
    "hash-signed",
    "noise2",
    "fbm2",
    "voronoi2",
    "cell-distance2",
];
pub const POINT_LIST_HELPERS: &[&str] = &[
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
    "shell",
    "offset",
    "offset-rounded",
    "fillet",
    "chamfer",
    "taper",
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
    "build",
    "shape",
    "result",
];
// Mesh/EckyRust-only surface. Do not add future CAD VM/OCCT names here until
// the compiler/runtime actually exports and lowers them.
pub const ECKY_RUST_ONLY_CAD_OPS: &[&str] = &["wall-pattern"];
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
    pub expression_forms: &'static [&'static str],
    pub numeric_helpers: &'static [&'static str],
    pub point_list_helpers: &'static [&'static str],
    pub boolean_helpers: &'static [&'static str],
    pub cad_ops: Vec<&'static str>,
    pub wall_pattern_modes: &'static [&'static str],
    pub typed_hole_policy: &'static str,
}

pub fn cad_ops_for_backend(backend: GeometryBackend) -> Vec<&'static str> {
    let mut ops = CAD_OPS_PORTABLE.to_vec();
    if matches!(backend, GeometryBackend::EckyRust) {
        ops.extend(ECKY_RUST_ONLY_CAD_OPS);
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
        expression_forms: EXPRESSION_FORMS,
        numeric_helpers: NUMERIC_HELPERS,
        point_list_helpers: POINT_LIST_HELPERS,
        boolean_helpers: BOOLEAN_HELPERS,
        cad_ops: cad_ops_for_backend(backend),
        wall_pattern_modes: wall_pattern_modes_for_backend(backend),
        typed_hole_policy: TYPED_HOLE_POLICY,
    }
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

        assert_eq!(manifest.model_clauses, MODEL_CLAUSES);
        assert_eq!(manifest.model_wrappers, MODEL_WRAPPERS);
        assert_eq!(manifest.expression_forms, EXPRESSION_FORMS);
        assert_eq!(manifest.numeric_helpers, NUMERIC_HELPERS);
        assert_eq!(manifest.point_list_helpers, POINT_LIST_HELPERS);
        assert_eq!(manifest.boolean_helpers, BOOLEAN_HELPERS);
        assert_eq!(manifest.cad_ops, CAD_OPS_PORTABLE);
        assert_eq!(manifest.typed_hole_policy, TYPED_HOLE_POLICY);
    }

    #[test]
    fn backend_manifests_gate_mesh_only_wall_surface() {
        for backend in [GeometryBackend::Build123d, GeometryBackend::Freecad] {
            let manifest = supported_surface_manifest(backend);

            assert!(!manifest.cad_ops.contains(&"wall-pattern"));
            assert!(manifest.wall_pattern_modes.is_empty());
        }

        let mesh_manifest = supported_surface_manifest(GeometryBackend::EckyRust);

        assert!(mesh_manifest.cad_ops.contains(&"wall-pattern"));
        assert_eq!(mesh_manifest.wall_pattern_modes, WALL_PATTERN_MODES);
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
        for op in CAD_OPS_PORTABLE.iter().chain(ECKY_RUST_ONLY_CAD_OPS.iter()) {
            assert!(cad::MODULE.exports.contains(op), "missing export: {op}");
        }
    }

    #[test]
    fn wall_pattern_is_mesh_backend_only() {
        assert!(!cad_ops_for_backend(GeometryBackend::Build123d).contains(&"wall-pattern"));
        assert!(!cad_ops_for_backend(GeometryBackend::Freecad).contains(&"wall-pattern"));
        assert!(cad_ops_for_backend(GeometryBackend::EckyRust).contains(&"wall-pattern"));
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
}
