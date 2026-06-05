use super::ModuleSpec;

pub const MODULE: ModuleSpec = ModuleSpec {
    scheme_name: "ecky/cad",
    rust_module: "ecky_scheme::cad",
    exports: &[
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
        "compound",
        "fuse",
        "cut",
        "common",
        "box",
        "sphere",
        "cylinder",
        "cone",
        "torus",
        "wedge",
        "circle",
        "ellipse",
        "ring",
        "rectangle",
        "rounded-rect",
        "rounded-polygon",
        "regular-polygon",
        "trapezoid",
        "slot-overall",
        "slot-center-to-center",
        "slot-center-point",
        "slot-arc",
        "polygon",
        "extrude",
        "revolve",
        "loft",
        "sweep",
        "helical-ridge",
        "thread",
        "rib",
        "groove",
        "shell",
        "offset",
        "offset-rounded",
        "fillet",
        "chamfer",
        "taper",
        "draft",
        "translate",
        "rotate",
        "scale",
        "mirror",
        "sampled-radial-loft",
        "bezier-path",
        "bspline",
        "path",
        "polyline",
        "profile",
        "make-face",
        "union",
        "difference",
        "intersection",
        "xor",
        "linear-array",
        "radial-array",
        "grid-array",
        "arc-array",
        "text",
        "svg",
        "import-stl",
        "path-frame",
        "plane",
        "location",
        "place",
        "clip-box",
        "twist",
        "repeat",
        "repeat-union",
        "repeat-compound",
        "repeat-pick",
        "for-union",
        "for-compound",
        "wall-pattern",
    ],
};

pub fn source() -> String {
    let exported = MODULE.exports.join(" ");
    let mut out = format!(
        "(provide {})\n\
         (define-syntax model\n\
           (syntax-rules ()\n\
             [(_ clause ...) (list 'model clause ...)]))\n\
         (define-syntax part\n\
           (syntax-rules ()\n\
             [(_ name expr) (list 'part (quote name) expr)]\n\
             [(_ name label expr) (list 'part (quote name) label expr)]))\n\
         (define-syntax feature\n\
           (syntax-rules ()\n\
             [(_ name role-key role params-key (param ...) expr)\n\
              (list 'feature (quote name) role-key (quote role) params-key (list (quote param) ...) expr)]\n\
             [(_ name role-key role expr)\n\
              (list 'feature (quote name) role-key (quote role) expr)]))\n\
         (define-syntax view\n\
           (syntax-rules ()\n\
             [(_ name item ...) (list 'view (quote name) item ...)]))\n\
         (define-syntax offset-part\n\
           (syntax-rules ()\n\
             [(_ part dx dy dz) (list 'offset-part (quote part) dx dy dz)]))\n\
         (define-syntax tag-face\n\
           (syntax-rules ()\n\
             [(_ name selector-key selector target) (list 'tag-face (quote name) selector-key selector (quote target))]))\n\
         (define-syntax tag-edge\n\
           (syntax-rules ()\n\
             [(_ name selector-key selector target) (list 'tag-edge (quote name) selector-key selector (quote target))]))\n\
         (define-syntax tag-edges\n\
           (syntax-rules ()\n\
             [(_ name selector-key selector target) (list 'tag-edges (quote name) selector-key selector (quote target))]))\n\
         (define-syntax tag\n\
           (syntax-rules ()\n\
             [(_ name) (list 'tag (quote name))]))\n\
         (define-syntax build\n\
           (syntax-rules ()\n\
             [(_ item ...) (list 'build item ...)]))\n\
         (define-syntax shape\n\
           (syntax-rules ()\n\
             [(_ name expr) (list 'shape (quote name) expr)]))\n\
         (define-syntax result\n\
           (syntax-rules ()\n\
             [(_ expr) (list 'result expr)]))\n\
         (define-syntax for-union\n\
           (syntax-rules ()\n\
             [(_ (index count) body) (repeat-union index count body)]))\n\
         (define-syntax for-compound\n\
           (syntax-rules ()\n\
             [(_ (index count) body) (repeat-compound index count body)]))\n",
        exported
    );

    for name in MODULE.exports {
        if matches!(
            *name,
            "model"
                | "part"
                | "feature"
                | "view"
                | "offset-part"
                | "build"
                | "shape"
                | "result"
                | "for-union"
                | "for-compound"
        ) {
            continue;
        }
        out.push_str(&format!(
            "(define ({} . args) (cons '{} args))\n",
            name, name
        ));
    }

    out
}
