use ecky_cad_lib::contracts::{DesignParams, GeometryBackend};
use ecky_cad_lib::models::PathResolver;
use std::path::PathBuf;

#[derive(Clone)]
struct TestResolver(PathBuf);

impl PathResolver for TestResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.0.join("config")
    }
    fn app_data_dir(&self) -> PathBuf {
        self.0.join("data")
    }
    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

#[test]
fn native_mesh_render_succeeds_from_worker_thread_with_small_stack() {
    let source = r#"
        (model
          (part body
            (let*
              ((p1 (wall-pattern (:mode fbm :depth 0.6 :uFreq 8 :vFreq 8 :seed 3) (shell 1.0 (cylinder 14 30 40))))
               (b1 (box 5 5 5))
               (b2 (box 4 4 4))
               (b3 (box 3 3 3))
               (b4 (box 2 2 2))
               (b5 (box 1 1 1))
               (u1 (union p1 b1))
               (u2 (union u1 b2))
               (u3 (union u2 b3))
               (u4 (union u3 b4))
               (u5 (union u4 b5)))
              u5)))
    "#;

    let root = std::env::temp_dir().join(format!("ecky-stack-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::create_dir_all(root.join("config")).unwrap();
    let resolver = TestResolver(root.clone());

    // Spawn a worker thread with 512 KB stack (mimics tokio worker thread stack size on macOS)
    let handle = std::thread::Builder::new()
        .name("test-small-stack-worker".to_string())
        .stack_size(512 * 1024)
        .spawn(move || {
            ecky_cad_lib::services::render::render_cli_ecky(
                source,
                &DesignParams::new(),
                GeometryBackend::EckyRust,
                None,
                &resolver,
            )
        })
        .expect("spawn small stack thread");

    let result = handle
        .join()
        .expect("thread did not crash with stack overflow");
    assert!(result.is_ok(), "render succeeded: {:?}", result.err());
    let _ = std::fs::remove_dir_all(root);
}
