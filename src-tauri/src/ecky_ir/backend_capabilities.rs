use crate::contracts::GeometryBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendRole {
    Primary,
    ExportInterop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSupport {
    Supported,
    ExplicitlyUnsupported(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendOperationCapability {
    pub op_name: &'static str,
    pub support: OperationSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapability {
    pub backend: GeometryBackend,
    pub role: BackendRole,
    pub ops: &'static [BackendOperationCapability],
}

const ALL_CORE_OPERATION_NAMES: &[&str] = &[
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
    "union",
    "difference",
    "intersection",
    "xor",
    "translate",
    "rotate",
    "scale",
    "mirror",
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
    "path",
    "bezier-path",
    "bspline",
    "linear-array",
    "radial-array",
    "grid-array",
    "arc-array",
    "repeat",
    "repeat-union",
    "repeat-compound",
    "repeat-pick",
    "plane",
    "location",
    "path-frame",
    "place",
    "clip-box",
    "compound",
    "meta",
    "build",
];

const NATIVE_OCCT_OPS: &[BackendOperationCapability] = &[
    supported("box"),
    supported("sphere"),
    supported("cylinder"),
    supported("cone"),
    supported("circle"),
    supported("rectangle"),
    supported("rounded-rect"),
    supported("rounded-polygon"),
    supported("polygon"),
    supported("profile"),
    supported("make-face"),
    unsupported(
        "text",
        "requires text-profile preprocessing before native render",
    ),
    unsupported(
        "svg",
        "rewritten to profile geometry before native render; clean profiles take \
         the fast path and artwork (self-intersecting, multi-outer, even-odd) is \
         resolved into OCCT wire-soup faces",
    ),
    unsupported(
        "import-stl",
        "interop/import primitive, not native authored geometry",
    ),
    supported("union"),
    supported("difference"),
    supported("intersection"),
    unsupported("xor", "boolean xor is rejected by native normalization"),
    supported("translate"),
    supported("rotate"),
    supported("scale"),
    supported("mirror"),
    supported("extrude"),
    supported("revolve"),
    supported("loft"),
    supported("sweep"),
    supported("shell"),
    supported("offset"),
    supported("offset-rounded"),
    supported("fillet"),
    supported("chamfer"),
    supported("taper"),
    supported("twist"),
    supported("path"),
    supported("bezier-path"),
    supported("bspline"),
    supported("linear-array"),
    supported("radial-array"),
    supported("grid-array"),
    supported("arc-array"),
    supported("repeat"),
    supported("repeat-union"),
    supported("repeat-compound"),
    supported("repeat-pick"),
    supported("plane"),
    supported("location"),
    supported("path-frame"),
    supported("place"),
    supported("clip-box"),
    supported("compound"),
    supported("meta"),
    supported("build"),
];

const BUILD123D_OPS: &[BackendOperationCapability] = &[
    supported("box"),
    supported("sphere"),
    supported("cylinder"),
    supported("cone"),
    supported("circle"),
    supported("rectangle"),
    supported("rounded-rect"),
    supported("rounded-polygon"),
    supported("polygon"),
    supported("profile"),
    supported("make-face"),
    supported("text"),
    supported("svg"),
    supported("import-stl"),
    supported("union"),
    supported("difference"),
    supported("intersection"),
    supported("xor"),
    supported("translate"),
    supported("rotate"),
    supported("scale"),
    supported("mirror"),
    supported("extrude"),
    supported("revolve"),
    supported("loft"),
    supported("sweep"),
    supported("shell"),
    supported("offset"),
    supported("offset-rounded"),
    supported("fillet"),
    supported("chamfer"),
    supported("taper"),
    supported("twist"),
    supported("path"),
    supported("bezier-path"),
    supported("bspline"),
    supported("linear-array"),
    supported("radial-array"),
    supported("grid-array"),
    supported("arc-array"),
    supported("repeat"),
    supported("repeat-union"),
    supported("repeat-compound"),
    supported("repeat-pick"),
    supported("plane"),
    supported("location"),
    supported("path-frame"),
    supported("place"),
    supported("clip-box"),
    supported("compound"),
    supported("meta"),
    supported("build"),
];

const FREECAD_OPS: &[BackendOperationCapability] = BUILD123D_OPS;

pub const BACKEND_CAPABILITIES: &[BackendCapability] = &[
    BackendCapability {
        backend: GeometryBackend::EckyRust,
        role: BackendRole::Primary,
        ops: NATIVE_OCCT_OPS,
    },
    BackendCapability {
        backend: GeometryBackend::Build123d,
        role: BackendRole::ExportInterop,
        ops: BUILD123D_OPS,
    },
    BackendCapability {
        backend: GeometryBackend::Freecad,
        role: BackendRole::ExportInterop,
        ops: FREECAD_OPS,
    },
];

pub fn known_core_operation_names() -> &'static [&'static str] {
    ALL_CORE_OPERATION_NAMES
}

pub fn backend_capability(backend: GeometryBackend) -> Option<&'static BackendCapability> {
    BACKEND_CAPABILITIES
        .iter()
        .find(|capability| capability.backend == backend)
}

const fn supported(op_name: &'static str) -> BackendOperationCapability {
    BackendOperationCapability {
        op_name,
        support: OperationSupport::Supported,
    }
}

const fn unsupported(op_name: &'static str, reason: &'static str) -> BackendOperationCapability {
    BackendOperationCapability {
        op_name,
        support: OperationSupport::ExplicitlyUnsupported(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn backend_capabilities_list_each_backend_once() {
        assert_eq!(BACKEND_CAPABILITIES.len(), 3);
        assert_eq!(
            BACKEND_CAPABILITIES
                .iter()
                .filter(|capability| capability.backend == GeometryBackend::EckyRust)
                .count(),
            1
        );
        assert_eq!(
            BACKEND_CAPABILITIES
                .iter()
                .filter(|capability| capability.backend == GeometryBackend::Build123d)
                .count(),
            1
        );
        assert_eq!(
            BACKEND_CAPABILITIES
                .iter()
                .filter(|capability| capability.backend == GeometryBackend::Freecad)
                .count(),
            1
        );
        assert_eq!(
            backend_capability(GeometryBackend::EckyRust).unwrap().role,
            BackendRole::Primary
        );
        assert_eq!(
            backend_capability(GeometryBackend::Build123d).unwrap().role,
            BackendRole::ExportInterop
        );
        assert_eq!(
            backend_capability(GeometryBackend::Freecad).unwrap().role,
            BackendRole::ExportInterop
        );
    }

    #[test]
    fn native_backend_has_decision_for_every_known_core_operation() {
        let known = known_core_operation_names()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let native = backend_capability(GeometryBackend::EckyRust).unwrap();
        let native_ops = native
            .ops
            .iter()
            .map(|capability| capability.op_name)
            .collect::<BTreeSet<_>>();

        assert_eq!(native_ops, known);
    }

    #[test]
    fn text_backend_subsets_only_name_known_core_operations() {
        let known = known_core_operation_names()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        for capability in BACKEND_CAPABILITIES {
            let op_names = capability
                .ops
                .iter()
                .map(|op| op.op_name)
                .collect::<BTreeSet<_>>();
            assert_eq!(
                op_names.len(),
                capability.ops.len(),
                "{:?} capability table contains duplicate op names",
                capability.backend
            );
            assert!(
                op_names.is_subset(&known),
                "{:?} capability table names unknown ops: {:?}",
                capability.backend,
                op_names.difference(&known).collect::<Vec<_>>()
            );
        }
    }
}
