use super::ModuleSpec;

pub const MODULE: ModuleSpec = ModuleSpec {
    scheme_name: "ecky/params",
    rust_module: "ecky_scheme::params",
    exports: &["params", "number", "toggle", "select", "image", "option"],
};

pub const SOURCE: &str = r#"
(provide params number toggle select image option)

(define-syntax params
  (syntax-rules ()
    [(_ decl ...)
     (list 'params decl ...)]))

(define-syntax number
  (syntax-rules ()
    [(_ key default opt ...)
     (list 'number (quote key) default opt ...)]))

(define-syntax toggle
  (syntax-rules ()
    [(_ key default opt ...)
     (list 'toggle (quote key) default opt ...)]))

(define-syntax select
  (syntax-rules ()
    [(_ key default opt ...)
     (list 'select (quote key) default opt ...)]))

(define-syntax image
  (syntax-rules ()
    [(_ key default opt ...)
     (list 'image (quote key) default opt ...)]))

(define-syntax option
  (syntax-rules ()
    [(_ label value)
     (list label value)]))
"#;
