pub mod bootstrap;
pub mod cad;
pub mod compiler;
pub mod core;
pub mod params;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSpec {
    pub scheme_name: &'static str,
    pub rust_module: &'static str,
    pub exports: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapShape {
    pub sandboxed: bool,
    pub modules: &'static [ModuleSpec],
    pub blocked_unsafe_ops: &'static [&'static str],
}

pub const APP_MODULES: [ModuleSpec; 3] = [core::MODULE, cad::MODULE, params::MODULE];

pub const BOOTSTRAP_SHAPE: BootstrapShape = BootstrapShape {
    sandboxed: true,
    modules: &APP_MODULES,
    blocked_unsafe_ops: bootstrap::BLOCKED_UNSAFE_OPS,
};

pub fn bootstrap_shape() -> &'static BootstrapShape {
    &BOOTSTRAP_SHAPE
}

pub use compiler::{
    compile_to_core_program, compile_to_legacy_source, try_compile_to_core_program,
    try_compile_to_legacy_source,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_has_app_modules_only() {
        let shape = bootstrap_shape();
        assert!(shape.sandboxed);
        assert_eq!(shape.modules.len(), 3);
        assert_eq!(shape.modules[0].scheme_name, "ecky/core");
        assert_eq!(shape.modules[1].scheme_name, "ecky/cad");
        assert_eq!(shape.modules[2].scheme_name, "ecky/params");
    }

    #[test]
    fn shape_blocks_unsafe_ops() {
        let ops = bootstrap::BLOCKED_UNSAFE_OPS;
        assert!(ops.contains(&"create-directory!"));
        assert!(ops.contains(&"delete-directory!"));
        assert!(ops.contains(&"delete-file!"));
        assert!(ops.contains(&"open-input-file"));
        assert!(ops.contains(&"open-output-file"));
    }

    #[test]
    fn compiles_old_style_model_source_via_steel() {
        let compiled = compile_to_legacy_source(
            r#"
            (model
              (params
                (number radius 10 :label "Radius")
                (toggle printed false))
              (part body
                (translate 0 0 5
                  (extrude (circle radius) 20))))
            "#,
        )
        .expect("compile");

        assert!(compiled.contains("(model"));
        assert!(compiled.contains("(number radius 10"));
        assert!(compiled.contains("(translate 0 0 5"));
        assert!(compiled.contains("(circle radius)"));
    }

    #[test]
    fn compiles_scheme_helpers_into_model_source() {
        let compiled = compile_to_legacy_source(
            r#"
            (define (cup-body radius height)
              (extrude (circle radius) height))

            (model
              (part body (cup-body 12 30)))
            "#,
        )
        .expect("compile");

        assert!(compiled.contains("(part body"));
        assert!(
            compiled.contains("(extrude (circle 12) 30)")
                || compiled.contains("(cup-body 12 30)")
                || compiled.contains("(extrude (circle ##radius2) ##height2)"),
            "{}",
            compiled
        );
    }

    #[test]
    fn scheme_source_flows_through_ecky_ir_lowerer() {
        let code = crate::ecky_ir::lower_to_build123d(
            r#"
            (define (cup-body radius height)
              (extrude (circle radius) height))

            (model
              (part body (cup-body 12 30)))
            "#,
        )
        .expect("lower");

        assert!(code.contains("Circle("), "{}", code);
        assert!(code.contains("extrude"), "{}", code);
    }
}
