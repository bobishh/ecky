use ecky_cad_lib::{
    contracts::{DesignParams, GeometryBackend, ModelManifest, ParamValue},
    models::PathResolver,
    services::{render::render_cli_ecky, structural_verification::verify_structure},
};
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);
impl PathResolver for Fixture {
    fn app_config_dir(&self) -> PathBuf {
        self.0.join("config")
    }
    fn app_data_dir(&self) -> PathBuf {
        self.0.join("data")
    }
    fn resource_path(&self, _: &str) -> Option<PathBuf> {
        None
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn component_count(source: &str, params: &DesignParams) -> u32 {
    let fixture =
        Fixture(std::env::temp_dir().join(format!("ecky-book-fit-{}", uuid::Uuid::new_v4())));
    let bundle =
        render_cli_ecky(source, params, GeometryBackend::EckyRust, None, &fixture).unwrap();
    let manifest: ModelManifest =
        serde_json::from_slice(&fs::read(&bundle.manifest_path).unwrap()).unwrap();
    let verified = verify_structure(&bundle, &manifest);
    assert_eq!(verified.metrics.model_stl_non_manifold_edge_count, Some(0));
    verified.metrics.model_stl_component_count.unwrap()
}

#[test]
#[ignore = "requires the prepared native OCCT runtime"]
fn worm_joint_moves_without_fusing_and_collision_control_is_detected() {
    let worm = include_str!("../../docs/books/ecky-ir/examples/worm-farm/01-worm.ecky");
    let component = worm.split("\n(model").next().unwrap();
    // Union forces a real boolean: overlapping links become one component.
    // Rotate around the shared ball centre, not around a link's body centre.
    for (yaw, pitch, expected) in [
        (0, 0, 2),
        (35, 0, 2),
        (-35, 0, 2),
        (0, 20, 2),
        (0, -20, 2),
        (20, 10, 2),
        (90, 0, 1),
    ] {
        let source = format!(
            "{component}\n(model (part pair (union
          (worm-link :has-socket false)
          (translate 11mm 0 5mm (rotate 0 {pitch}deg {yaw}deg
            (translate 12mm 0 -5mm (worm-link :has-ball false)))))))"
        );
        assert_eq!(
            component_count(&source, &DesignParams::new()),
            expected,
            "yaw={yaw}, pitch={pitch}"
        );
    }
}

#[test]
#[ignore = "requires the prepared native OCCT runtime"]
fn assembled_tractor_retains_eleven_separate_pieces() {
    let source = include_str!("../../docs/books/ecky-ir/examples/worm-farm/04-tractor.ecky");
    let mut params = DesignParams::new();
    params.insert("print_layout".into(), ParamValue::Boolean(false));
    assert_eq!(component_count(source, &params), 11);
}

#[test]
#[ignore = "requires the prepared native OCCT runtime"]
fn snap_rivet_retains_a_free_wheel_and_a_tight_bore_is_detected() {
    let tractor = include_str!("../../docs/books/ecky-ir/examples/worm-farm/04-tractor.ecky");
    let components = tractor.split("\n(model").next().unwrap();
    for (clearance, expected) in [(0.4, 2), (-0.2, 1)] {
        // The seated head touches the axle end. Their union is one piece;
        // the wheel must remain separate. Negative control jams the wheel.
        let source = format!(
            "{components}\n(model (part coupon (union
          (tractor-axle :axle-length 16mm)
          (translate 0 0 9.4mm (tractor-wheel :running-clearance {clearance}mm))
          (translate 0 0 17.6mm (rotate 180deg 0 0 (tractor-rivet))))))"
        );
        assert_eq!(component_count(&source, &DesignParams::new()), expected);
    }
}
