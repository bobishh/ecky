use super::ModuleSpec;
use regex::Regex;
use steel_core::steel_vm::engine::Engine;

pub const BLOCKED_UNSAFE_OPS: &[&str] = &[
    "create-directory!",
    "delete-directory!",
    "delete-file!",
    "open-input-file",
    "open-output-file",
];

pub const ALLOWED_MODULES: &[&str] = &["ecky/core", "ecky/cad", "ecky/params"];

pub fn install_bootstrap_modules(engine: &mut Engine) {
    engine.register_steel_module(
        super::core::MODULE.scheme_name.to_owned(),
        super::core::SOURCE.to_owned(),
    );
    engine.register_steel_module(
        super::cad::MODULE.scheme_name.to_owned(),
        super::cad::source(),
    );
    engine.register_steel_module(
        super::params::MODULE.scheme_name.to_owned(),
        super::params::SOURCE.to_owned(),
    );
}

pub fn new_engine() -> Engine {
    let mut engine = Engine::new_sandboxed();
    install_bootstrap_modules(&mut engine);
    engine
}

pub fn wrap_user_source(source: &str) -> String {
    let normalized = normalize_surface_source(source);
    format!(
        "(require \"ecky/core\")\n(require \"ecky/params\")\n(require \"ecky/cad\")\n{}",
        normalized
    )
}

pub fn validate_user_source(source: &str) -> Result<(), String> {
    let require_re =
        Regex::new(r#"\((require|require-builtin)\s+"([^"]+)""#).map_err(|err| err.to_string())?;
    for capture in require_re.captures_iter(source) {
        let module = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
        if !ALLOWED_MODULES.contains(&module) {
            return Err(format!(
                "Steel front-end only allows `(require ...)` for app modules. Blocked `{}`.",
                module
            ));
        }
    }

    let set_re = Regex::new(r"\(\s*set!(?:\s|$)").map_err(|err| err.to_string())?;
    if set_re.is_match(source) {
        return Err("Steel modeling surface forbids `set!`.".to_string());
    }

    Ok(())
}

fn normalize_surface_source(source: &str) -> String {
    let keyword_re = Regex::new(r#"(^|[\s(])\:([A-Za-z][A-Za-z0-9_-]*)"#).expect("keyword regex");
    keyword_re.replace_all(source, "$1#:$2").into_owned()
}

#[allow(dead_code)]
pub const fn module_specs() -> [ModuleSpec; 3] {
    [
        super::core::MODULE,
        super::cad::MODULE,
        super::params::MODULE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_specs_match_bootstrap_shape() {
        let specs = module_specs();
        assert_eq!(specs[0].scheme_name, "ecky/core");
        assert_eq!(specs[1].scheme_name, "ecky/cad");
        assert_eq!(specs[2].scheme_name, "ecky/params");
    }

    #[test]
    fn unsafe_ops_list_is_small_and_explicit() {
        assert_eq!(BLOCKED_UNSAFE_OPS.len(), 5);
    }
}

#[cfg(test)]
mod steel_tests {
    use super::*;

    #[test]
    fn registers_app_modules_into_sandboxed_engine() {
        let mut engine = new_engine();

        for name in ALLOWED_MODULES {
            let form = format!("(require \"{name}\")");
            engine.run(form).unwrap();
        }
    }

    #[test]
    fn blocks_unsafe_ops_in_sandbox() {
        let mut engine = new_engine();
        for expr in [
            "(create-directory! \"boom\")",
            "(delete-directory! \"boom\")",
            "(delete-file! \"boom\")",
            "(open-input-file \"boom\")",
            "(open-output-file \"boom\")",
        ] {
            assert!(engine.compile_and_run_raw_program(expr).is_err(), "{expr}");
        }
    }

    #[test]
    fn blocks_foreign_requires() {
        let err = validate_user_source(r#"(require "steel/fs/fs.scm")"#).expect_err("blocked");
        assert!(err.contains("Blocked"));
    }
}
