use super::ModuleSpec;

pub const MODULE: ModuleSpec = ModuleSpec {
    scheme_name: "ecky/cad",
    rust_module: "ecky_scheme::cad",
    exports: &[
        "model",
        "part",
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
        "circle",
        "rectangle",
        "rounded-rect",
        "rounded-polygon",
        "polygon",
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
            "model" | "part" | "build" | "shape" | "result" | "for-union" | "for-compound"
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
