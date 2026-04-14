use super::ModuleSpec;

pub const MODULE: ModuleSpec = ModuleSpec {
    scheme_name: "ecky/core",
    rust_module: "ecky_scheme::core",
    exports: &[
        "vec2",
        "vec3",
        "start",
        "end",
        "xy",
        "yz",
        "xz",
        "true",
        "false",
        "zip",
        "enumerate",
        "flat-map",
        "concat-map",
        "linspace",
        "pi",
        "tau",
        "clamp",
        "lerp",
        "invlerp",
        "remap",
        "deg->rad",
        "rad->deg",
        "square",
        "cube",
    ],
};

pub const SOURCE: &str = r#"
(provide vec2 vec3 start end xy yz xz true false
         zip enumerate flat-map concat-map linspace
         pi tau clamp lerp invlerp remap
         deg->rad rad->deg square cube)
(define (vec2 x y) (list x y))
(define (vec3 x y z) (list x y z))
(define start 'start)
(define end 'end)
(define xy 'xy)
(define yz 'yz)
(define xz 'xz)
(define true #t)
(define false #f)
(define (zip . lists)
  (if (null? lists)
      '()
      (apply map list lists)))
(define (enumerate . args)
  (cond
    [(= (length args) 1)
     (let ([lst (car args)])
       (map list (range 0 (length lst)) lst))]
    [(= (length args) 2)
     (let ([start-index (car args)]
           [lst (cadr args)])
       (map list (range start-index (+ start-index (length lst))) lst))]
    [else
     (error 'enumerate "expected (enumerate lst) or (enumerate start lst)")]))
(define (flat-map func . lists)
  (if (null? lists)
      '()
      (apply append (apply map func lists))))
(define (concat-map func . lists)
  (apply flat-map func lists))
(define (linspace start stop count)
  (cond
    [(<= count 0) '()]
    [(= count 1) (list start)]
    [else
     (let ([step (/ (- stop start) (- count 1))])
       (map (lambda (i) (+ start (* step i)))
            (range 0 count)))]))
(define pi 3.141592653589793)
(define tau 6.283185307179586)
(define (clamp value lower upper)
  (min upper (max lower value)))
(define (lerp start end t)
  (+ start (* (- end start) t)))
(define (invlerp start end value)
  (if (= start end)
      0.0
      (/ (- value start) (- end start))))
(define (remap value in-start in-end out-start out-end)
  (lerp out-start out-end (invlerp in-start in-end value)))
(define (deg->rad degrees)
  (* degrees (/ pi 180)))
(define (rad->deg radians)
  (* radians (/ 180 pi)))
(define (square x) (* x x))
(define (cube x) (* x x x))
"#;

#[cfg(test)]
mod tests {
    use super::super::{bootstrap, compiler};
    use steel_core::rvals::SteelVal;

    fn assert_true(expr: &str) {
        let mut engine = bootstrap::new_engine();
        let program = format!("(require \"ecky/core\")\n{expr}");
        let values = engine
            .compile_and_run_raw_program(program)
            .expect("scheme evaluation should succeed");
        assert!(
            matches!(values.last(), Some(SteelVal::BoolV(true))),
            "{expr} => {values:?}"
        );
    }

    #[test]
    fn generic_sequence_helpers_evaluate_in_core_module() {
        assert_true("(equal? (zip) '())");
        assert_true(
            "(equal? (zip (list 1 2 3) (list 4 5 6 7)) (list (list 1 4) (list 2 5) (list 3 6)))",
        );
        assert_true(
            "(equal? (enumerate (list 'a 'b 'c)) (list (list 0 'a) (list 1 'b) (list 2 'c)))",
        );
        assert_true("(equal? (enumerate 3 (list 'a 'b)) (list (list 3 'a) (list 4 'b)))");
        assert_true(
            "(equal? (flat-map (lambda (x) (list x (* x 10))) (list 1 2 3)) (list 1 10 2 20 3 30))",
        );
        assert_true("(equal? (concat-map (lambda (x) (range 0 x)) (list 1 3)) (list 0 0 1 2))");
        assert_true("(equal? (linspace 5 9 0) '())");
        assert_true("(equal? (linspace 5 9 1) (list 5))");
        assert_true(
            "(and (< (abs (- (list-ref (linspace 0 10 5) 1) 2.5)) 0.0000001) (< (abs (- (list-ref (linspace 0 10 5) 3) 7.5)) 0.0000001))",
        );
    }

    #[test]
    fn numeric_helper_bundle_evaluates_in_core_module() {
        assert_true("(= (square 4) 16)");
        assert_true("(= (cube 3) 27)");
        assert_true("(= (clamp 20 0 10) 10)");
        assert_true("(= (clamp -2 0 10) 0)");
        assert_true("(< (abs (- (lerp 10 20 0.25) 12.5)) 0.0000001)");
        assert_true("(< (abs (- (invlerp 10 20 12.5) 0.25)) 0.0000001)");
        assert_true("(< (abs (- (remap 15 10 20 0 100) 50)) 0.0000001)");
        assert_true("(< (abs (- tau (* 2 pi))) 0.0000001)");
        assert_true("(< (abs (- (deg->rad 180) pi)) 0.0000001)");
        assert_true("(< (abs (- (rad->deg pi) 180)) 0.0000001)");
    }

    #[test]
    fn helper_surface_compiles_into_model_points() {
        let program = compiler::compile_to_core_program(
            r#"
            (model
              (part body
                (build
                  (shape pts
                    (zip
                      (linspace 0 4 5)
                      (list 0 2 0 2 0)))
                  (result (polygon pts)))))
            "#,
        )
        .expect("helpers should compile in model source");

        let root = &program.parts[0].root;
        let crate::ecky_core_ir::CoreNodeKind::Build { bindings, result } = &root.kind else {
            panic!("expected build node, got {:?}", root.kind);
        };
        let crate::ecky_core_ir::CoreNodeKind::List(points) = &bindings[0].value.kind else {
            panic!("expected literal points, got {:?}", bindings[0].value.kind);
        };
        assert_eq!(points.len(), 5);
        let crate::ecky_core_ir::CoreNodeKind::Call { op, .. } = &result.kind else {
            panic!("expected polygon call, got {:?}", result.kind);
        };
        assert!(matches!(
            op,
            crate::ecky_core_ir::CoreOperation::Primitive(
                crate::ecky_core_ir::CorePrimitive::Polygon
            )
        ));
    }
}
