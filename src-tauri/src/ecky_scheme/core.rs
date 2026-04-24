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
        "deg",
        "rad",
        "deg->rad",
        "rad->deg",
        "smoothstep",
        "square",
        "cube",
        "hash01",
        "hash-signed",
        "noise2",
        "fbm2",
        "voronoi2",
        "cell-distance2",
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
    ],
};

pub const SOURCE: &str = r#"
(provide vec2 vec3 start end xy yz xz true false
         zip enumerate flat-map concat-map linspace
         pi tau clamp lerp invlerp remap
         deg rad deg->rad rad->deg smoothstep square cube
         hash01 hash-signed noise2 fbm2 voronoi2 cell-distance2
         jitter2 jittered-grid polar-points organic-loop wave-loop
         superellipse-point voronoi-cells
         lorenz-points rossler-points logistic-bifurcation-points henon-points)
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
(define (deg degrees)
  (deg->rad degrees))
(define (rad radians)
  (rad->deg radians))
(define (smoothstep edge0 edge1 x)
  (let ([t (clamp (/ (- x edge0) (- edge1 edge0)) 0 1)])
    (* t t (- 3 (* 2 t)))))
(define (square x) (* x x))
(define (cube x) (* x x x))
(define (fract01 value)
  (let ([wrapped (- value (floor value))])
    (if (< wrapped 0) (+ wrapped 1) wrapped)))
(define (hash01 x y seed)
  (fract01 (* (sin (+ (* x 127.1) (* y 311.7) (* seed 74.7))) 43758.5453123)))
(define (hash-signed x y seed)
  (- (* 2 (hash01 x y seed)) 1))
(define (smoothstep01 x)
  (let ([t (clamp x 0 1)])
    (* t t (- 3 (* 2 t)))))
(define (noise2 x y seed)
  (let* ([x0 (floor x)]
         [y0 (floor y)]
         [xf (- x x0)]
         [yf (- y y0)]
         [n00 (hash01 x0 y0 seed)]
         [n10 (hash01 (+ x0 1) y0 seed)]
         [n01 (hash01 x0 (+ y0 1) seed)]
         [n11 (hash01 (+ x0 1) (+ y0 1) seed)]
         [sx (smoothstep01 xf)]
         [sy (smoothstep01 yf)])
    (lerp (lerp n00 n10 sx) (lerp n01 n11 sx) sy)))
(define (fbm2-loop x y seed octaves lacunarity gain index amplitude frequency total normalizer)
  (if (>= index octaves)
      (if (= normalizer 0) 0 (clamp (/ total normalizer) 0 1))
      (fbm2-loop x y seed octaves lacunarity gain
                 (+ index 1)
                 (* amplitude gain)
                 (* frequency lacunarity)
                 (+ total (* amplitude (noise2 (* x frequency) (* y frequency) (+ seed (* index 17)))))
                 (+ normalizer amplitude))))
(define (fbm2 x y seed octaves lacunarity gain)
  (fbm2-loop x y seed octaves lacunarity gain 0 0.5 1.0 0.0 0.0))
(define (cell-point-distance x y gx gy seed)
  (let* ([px (+ gx (hash01 gx gy seed))]
         [py (+ gy (hash01 (+ gx 19.19) (+ gy 7.73) (+ seed 31)))]
         [dx (- x px)]
         [dy (- y py)])
    (sqrt (+ (* dx dx) (* dy dy)))))
(define (cell-distance2 x y seed)
  (let* ([cx (floor x)]
         [cy (floor y)])
    (clamp
      (/ (apply min
          (flat-map
            (lambda (oy)
              (map (lambda (ox)
                     (cell-point-distance x y (+ cx ox) (+ cy oy) seed))
                   (list -1 0 1)))
            (list -1 0 1)))
         1.4142135623730951)
      0 1)))
(define (voronoi2 x y seed)
  (clamp (- 1 (cell-distance2 x y seed)) 0 1))
(define (jitter2 x y amount seed)
  (list
    (+ x (* amount (hash-signed x y seed)))
    (+ y (* amount (hash-signed (+ x 19.19) (+ y 7.73) (+ seed 31))))))
(define (centered-index index count spacing)
  (* (- index (/ (- count 1) 2.0)) spacing))
(define (jittered-grid rows cols dx dy amount seed)
  (flat-map
    (lambda (row)
      (map (lambda (col)
             (jitter2
               (centered-index col cols dx)
               (centered-index row rows dy)
               amount
               (+ seed (* row 1009) col)))
           (range 0 cols)))
    (range 0 rows)))
(define (polar-points count radius)
  (map (lambda (i)
         (let ([a (* tau (/ i count))])
           (list (* radius (cos a)) (* radius (sin a)))))
       (range 0 count)))
(define (organic-loop count radius amount seed)
  (map (lambda (i)
         (let* ([t (/ i count)]
                [a (* tau t)]
                [r (+ radius (* amount (hash-signed i count seed)))])
           (list (* r (cos a)) (* r (sin a)))))
       (range 0 count)))
(define (wave-loop count rx ry amp waves seed)
  (map (lambda (i)
         (let* ([t (/ i count)]
                [a (* tau t)]
                [w (* amp (sin (+ (* waves a) (* tau (hash01 i waves seed)))))])
           (list (* (+ rx w) (cos a)) (* (+ ry w) (sin a)))))
       (range 0 count)))
(define (signed-power value exponent)
  (if (< value 0)
      (- (expt (- value) exponent))
      (expt value exponent)))
(define (superellipse-point rx ry n t)
  (let* ([a (* tau t)]
         [e (/ 2 n)])
    (list (* rx (signed-power (cos a) e))
          (* ry (signed-power (sin a) e)))))
(define (voronoi-cells rows cols dx dy amount seed)
  (jittered-grid rows cols dx dy amount seed))
(define (bounded-point2 x y scale)
  (list (clamp x (- scale) scale)
        (clamp y (- scale) scale)))
(define (bounded-point3 x y z scale)
  (list (clamp x (- scale) scale)
        (clamp y (- scale) scale)
        (clamp z (- scale) scale)))
(define (lorenz-point-state x y z dt)
  (let* ([sigma 10.0]
         [rho 28.0]
         [beta (/ 8.0 3.0)]
         [dx (* sigma (- y x))]
         [dy (- (* x (- rho z)) y)]
         [dz (- (* x y) (* beta z))])
    (list (+ x (* dt dx))
          (+ y (* dt dy))
          (+ z (* dt dz)))))
(define (lorenz-points count dt scale)
  (let loop ([index 0] [x 0.1] [y 0.0] [z 0.0] [pts '()])
    (if (>= index count)
        (reverse pts)
        (let* ([next (lorenz-point-state x y z dt)]
               [nx (list-ref next 0)]
               [ny (list-ref next 1)]
               [nz (list-ref next 2)])
          (loop (+ index 1) nx ny nz
                (cons (bounded-point3 (* scale (/ nx 30.0))
                                      (* scale (/ ny 30.0))
                                      (* scale (/ nz 50.0))
                                      scale)
                      pts))))))
(define (rossler-point-state x y z dt)
  (let* ([a 0.2]
         [b 0.2]
         [c 5.7]
         [dx (- (+ y z))]
         [dy (+ x (* a y))]
         [dz (+ b (* z (- x c)))])
    (list (+ x (* dt dx))
          (+ y (* dt dy))
          (+ z (* dt dz)))))
(define (rossler-points count dt scale)
  (let loop ([index 0] [x 0.1] [y 0.0] [z 0.0] [pts '()])
    (if (>= index count)
        (reverse pts)
        (let* ([next (rossler-point-state x y z dt)]
               [nx (list-ref next 0)]
               [ny (list-ref next 1)]
               [nz (list-ref next 2)])
          (loop (+ index 1) nx ny nz
                (cons (bounded-point3 (* scale (/ nx 15.0))
                                      (* scale (/ ny 15.0))
                                      (* scale (/ nz 30.0))
                                      scale)
                      pts))))))
(define (logistic-step r x)
  (* r x (- 1 x)))
(define (logistic-bifurcation-points r-count samples transient scale)
  (flat-map
    (lambda (ri)
      (let* ([r (lerp 2.5 4.0 (if (= r-count 1) 0 (/ ri (- r-count 1))))]
             [x0 (+ 0.2 (* 0.6 (hash01 ri samples transient)))]
             [settled
              (let loop ([i 0] [x x0])
                (if (>= i transient) x (loop (+ i 1) (logistic-step r x))))])
        (let loop ([i 0] [x settled] [pts '()])
          (if (>= i samples)
              (reverse pts)
              (let ([nx (logistic-step r x)])
                (loop (+ i 1) nx
                      (cons
                        (bounded-point2
                          (remap r 2.5 4.0 (- scale) scale)
                          (remap nx 0 1 (- scale) scale)
                          scale)
                        pts)))))))
    (range 0 r-count)))
(define (henon-points count scale)
  (let loop ([index 0] [x 0.1] [y 0.0] [pts '()])
    (if (>= index count)
        (reverse pts)
        (let* ([nx (+ 1 (- (* 1.4 x x)) y)]
               [ny (* 0.3 x)])
          (loop (+ index 1) nx ny
                (cons (bounded-point2 (* scale (/ nx 2.0))
                                      (* scale (/ ny 2.0))
                                      scale)
                      pts))))))
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
        assert_true("(< (abs (- (deg 180) pi)) 0.0000001)");
        assert_true("(< (abs (- (rad pi) 180)) 0.0000001)");
        assert_true("(= (smoothstep 0 1 0) 0)");
        assert_true("(= (smoothstep 0 1 1) 1)");
    }

    #[test]
    fn deterministic_fancy_helpers_evaluate_in_core_module() {
        assert_true("(<= 0 (hash01 2 3 9))");
        assert_true("(< (hash01 2 3 9) 1)");
        assert_true("(= (hash01 2 3 9) (hash01 2 3 9))");
        assert_true("(not (= (hash01 2 3 9) (hash01 2 3 10)))");
        assert_true("(<= -1 (hash-signed 2 3 9))");
        assert_true("(<= (hash-signed 2 3 9) 1)");
        assert_true("(<= 0 (noise2 0.25 0.75 4))");
        assert_true("(<= (noise2 0.25 0.75 4) 1)");
        assert_true("(<= 0 (fbm2 0.25 0.75 4 4 2.0 0.5))");
        assert_true("(<= (fbm2 0.25 0.75 4 4 2.0 0.5) 1)");
        assert_true("(<= 0 (voronoi2 0.25 0.75 4))");
        assert_true("(<= 0 (cell-distance2 0.25 0.75 4))");
        assert_true("(= (length (jitter2 1 2 0.5 9)) 2)");
        assert_true("(= (length (jittered-grid 2 3 10 20 0.5 9)) 6)");
        assert_true("(= (length (polar-points 8 12)) 8)");
        assert_true("(= (length (organic-loop 12 20 3 9)) 12)");
        assert_true("(= (length (wave-loop 12 20 12 2 5 9)) 12)");
        assert_true("(= (length (voronoi-cells 2 3 10 20 0.5 9)) 6)");
    }

    #[test]
    fn chaotic_point_helpers_emit_bounded_deterministic_point_lists() {
        assert_true("(equal? (lorenz-points 4 0.01 10) (lorenz-points 4 0.01 10))");
        assert_true("(= (length (lorenz-points 4 0.01 10)) 4)");
        assert_true("(= (length (car (lorenz-points 4 0.01 10))) 3)");
        assert_true("(= (length (rossler-points 4 0.05 10)) 4)");
        assert_true("(= (length (car (rossler-points 4 0.05 10))) 3)");
        assert_true("(= (length (logistic-bifurcation-points 3 4 8 10)) 12)");
        assert_true("(= (length (car (logistic-bifurcation-points 3 4 8 10))) 2)");
        assert_true("(= (length (henon-points 5 10)) 5)");
        assert_true("(= (length (car (henon-points 5 10))) 2)");
        assert_true("(let ([p (list-ref (henon-points 20 10) 10)]) (and (<= -10 (car p)) (<= (car p) 10) (<= -10 (cadr p)) (<= (cadr p) 10)))");
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
