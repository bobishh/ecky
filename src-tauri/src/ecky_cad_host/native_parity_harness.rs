//! Reusable differential parity harness (native-build123d-differential-parity,
//! language-convenience-stdlib 5.1): render the same macro through a reference
//! engine (build123d or FreeCAD) and the native Direct OCCT backend, and
//! assert the resulting meshes agree within tolerance. Any test module in the
//! crate can call `assert_native_matches_reference` — the previous copy lived
//! only inside `direct_occt_executor::tests` and could not be reused by
//! stdlib component tests.

use std::path::{Path, PathBuf};

use super::direct_occt_executor::export_core_program_step_stl_with_params_runner_first;
use super::direct_occt_sdk::{
    bundled_build123d_runtime_root_from_repo, inspect_build123d_ocp_runtime, NativeExportOutcome,
};
use crate::ecky_core_ir::CoreProgram;
use crate::models::{DesignParams, PathResolver};

pub(crate) struct TestResolver;

impl PathResolver for TestResolver {
    fn app_config_dir(&self) -> PathBuf {
        temp_root("native-parity-harness-config")
    }

    fn app_data_dir(&self) -> PathBuf {
        temp_root("native-parity-harness-data")
    }

    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

pub(crate) fn compile(source: &str) -> CoreProgram {
    crate::ecky_scheme::compile_to_core_program(source).expect("compile")
}

pub(crate) fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
}

pub(crate) fn ascii_stl_non_manifold_edge_count(path: &Path) -> usize {
    let bytes = std::fs::read(path).expect("read stl");
    let key = |value: f32| -> u32 {
        if value == 0.0 {
            0.0_f32.to_bits()
        } else {
            value.to_bits()
        }
    };
    // Parse binary STL: 80-byte header + 4-byte triangle count + 50 bytes per triangle.
    assert!(bytes.len() >= 84, "STL too small: {} bytes", bytes.len());
    let triangle_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    assert_eq!(
        bytes.len(),
        84 + triangle_count * 50,
        "binary STL length mismatch: {} triangles, {} bytes",
        triangle_count,
        bytes.len()
    );
    let mut vertices: Vec<[u32; 3]> = Vec::new();
    let mut off = 84usize;
    for _ in 0..triangle_count {
        off += 12; // normal
        for _ in 0..3 {
            let x = f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
            let y = f32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
            let z = f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap());
            vertices.push([key(x), key(y), key(z)]);
            off += 12;
        }
        off += 2; // attribute
    }
    let mut edge_counts: std::collections::HashMap<([u32; 3], [u32; 3]), usize> =
        std::collections::HashMap::new();
    for triangle in vertices.chunks_exact(3) {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let edge = if a <= b { (a, b) } else { (b, a) };
            *edge_counts.entry(edge).or_default() += 1;
        }
    }
    edge_counts.values().filter(|count| **count != 2).count()
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StlMetrics {
    pub volume: f64,
    pub area: f64,
    pub bbox_min: [f64; 3],
    pub bbox_max: [f64; 3],
    pub components: usize,
    #[allow(dead_code)]
    pub triangles: usize,
}

/// Integral mesh metrics for ASCII or binary STL. Signed volume via the
/// divergence theorem (inverted shells subtract — intentionally, so an
/// inside-out solid diverges from the reference).
pub(crate) fn stl_metrics(path: &Path) -> StlMetrics {
    let bytes = std::fs::read(path).expect("read stl");
    let ascii = bytes.starts_with(b"solid")
        && std::str::from_utf8(&bytes[..bytes.len().min(4096)])
            .map(|head| head.contains("facet"))
            .unwrap_or(false);
    let mut triangles: Vec<[[f64; 3]; 3]> = Vec::new();
    if ascii {
        let text = String::from_utf8_lossy(&bytes);
        let mut current: Vec<[f64; 3]> = Vec::new();
        for line in text.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix("vertex ") {
                let mut coords = [0.0f64; 3];
                for (index, value) in rest.split_whitespace().take(3).enumerate() {
                    coords[index] = value.parse().expect("vertex coord");
                }
                current.push(coords);
                if current.len() == 3 {
                    triangles.push([current[0], current[1], current[2]]);
                    current.clear();
                }
            }
        }
    } else {
        let count = u32::from_le_bytes(bytes[80..84].try_into().expect("stl count")) as usize;
        let mut offset = 84;
        for _ in 0..count {
            let mut triangle = [[0.0f64; 3]; 3];
            for (vertex_index, vertex) in triangle.iter_mut().enumerate() {
                let base = offset + 12 + vertex_index * 12;
                for axis in 0..3 {
                    let start = base + axis * 4;
                    vertex[axis] =
                        f32::from_le_bytes(bytes[start..start + 4].try_into().expect("stl f32"))
                            as f64;
                }
            }
            triangles.push(triangle);
            offset += 50;
        }
    }

    let mut volume = 0.0f64;
    let mut area = 0.0f64;
    let mut bbox_min = [f64::INFINITY; 3];
    let mut bbox_max = [f64::NEG_INFINITY; 3];
    let key = |vertex: [f64; 3]| -> [u32; 3] {
        vertex.map(|value| {
            let value = value as f32;
            if value == 0.0 {
                0.0f32.to_bits()
            } else {
                value.to_bits()
            }
        })
    };
    let mut edge_map: std::collections::HashMap<([u32; 3], [u32; 3]), Vec<usize>> =
        std::collections::HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        let [a, b, c] = *triangle;
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        area += 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        for vertex in triangle {
            for axis in 0..3 {
                bbox_min[axis] = bbox_min[axis].min(vertex[axis]);
                bbox_max[axis] = bbox_max[axis].max(vertex[axis]);
            }
        }
        for (left, right) in [(a, b), (b, c), (c, a)] {
            let (ka, kb) = (key(left), key(right));
            let edge = if ka <= kb { (ka, kb) } else { (kb, ka) };
            edge_map.entry(edge).or_default().push(index);
        }
    }

    let mut parent: Vec<usize> = (0..triangles.len()).collect();
    fn find(parent: &mut Vec<usize>, node: usize) -> usize {
        if parent[node] != node {
            let root = find(parent, parent[node]);
            parent[node] = root;
        }
        parent[node]
    }
    for ids in edge_map.values() {
        for pair in ids.windows(2) {
            let (left, right) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
            if left != right {
                parent[left] = right;
            }
        }
    }
    let components = (0..triangles.len())
        .map(|index| find(&mut parent, index))
        .collect::<std::collections::HashSet<_>>()
        .len();

    StlMetrics {
        volume,
        area,
        bbox_min,
        bbox_max,
        components,
        triangles: triangles.len(),
    }
}

/// Which engine renders the reference model for differential parity.
/// build123d is the preferred reference; FreeCAD covers macros the
/// build123d lowering cannot express yet (geometry `if`, 4-arg `svg`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum ParityReference {
    Build123d,
    Freecad,
}

/// Differential parity against a reference engine
/// (native-build123d-differential-parity): identical source + params must
/// produce matching integral geometry, and native wall time must stay
/// within max(10 s, 3 × reference time). Skips (returns without asserting)
/// when the bundled reference runtime or native runner is unavailable in
/// the current environment — same guard as the pre-extraction live tests.
pub(crate) fn assert_native_matches_reference(
    macro_source: &str,
    params: &DesignParams,
    label: &str,
    reference_engine: ParityReference,
) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
    if !runtime_root.exists() {
        return;
    }
    let layout = inspect_build123d_ocp_runtime(&runtime_root);
    let runner_available =
        crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &TestResolver,
            true,
        )
        .map(|runner| runner.is_file())
        .unwrap_or(false);
    if !runner_available && !layout.can_compile_native_shim() {
        return;
    }

    let reference_started = std::time::Instant::now();
    let reference = match reference_engine {
        ParityReference::Build123d => {
            let lowered = crate::ecky_ir::lower_to_build123d(macro_source)
                .unwrap_or_else(|err| panic!("[{label}] build123d lowering failed: {err:?}"));
            crate::build123d::render_model_with_sources(
                &lowered,
                Some(macro_source),
                params,
                &TestResolver,
                crate::models::SourceLanguage::EckyIrV0,
            )
            .unwrap_or_else(|err| panic!("[{label}] build123d reference render failed: {err:?}"))
        }
        ParityReference::Freecad => {
            if crate::freecad::resolve_freecad_path(None).is_err() {
                return;
            }
            // FreeCAD lowering recurses deeply on large macros; give it a
            // dedicated stack so the suite does not depend on
            // RUST_MIN_STACK being exported.
            let source = macro_source.to_string();
            let params_owned = params.clone();
            let label_owned = label.to_string();
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    let lowered = crate::ecky_ir::lower_to_freecad(&source).unwrap_or_else(|err| {
                        panic!("[{label_owned}] FreeCAD lowering failed: {err:?}")
                    });
                    crate::freecad::render_model_with_sources_and_font_path(
                        &lowered,
                        Some(&source),
                        &params_owned,
                        None,
                        None,
                        &TestResolver,
                        crate::models::SourceLanguage::EckyIrV0,
                    )
                    .unwrap_or_else(|err| {
                        panic!("[{label_owned}] FreeCAD reference render failed: {err:?}")
                    })
                })
                .expect("spawn freecad reference thread")
                .join()
                .expect("freecad reference render thread")
        }
    };
    let reference_elapsed = reference_started.elapsed();
    let reference_metrics = stl_metrics(Path::new(&reference.preview_stl_path));

    let program = compile(macro_source);
    let output_dir = temp_root(&format!("direct-occt-diff-{label}"));
    let native_started = std::time::Instant::now();
    let outcome = export_core_program_step_stl_with_params_runner_first(
        &program,
        params,
        &layout,
        &output_dir,
        &TestResolver,
    )
    .unwrap_or_else(|err| panic!("[{label}] native render failed: {err:?}"));
    let native_elapsed = native_started.elapsed();
    let NativeExportOutcome::Exported { stl_path, .. } = outcome else {
        panic!("[{label}] expected native export, got {outcome:?}");
    };
    let native_metrics = stl_metrics(&stl_path);

    let volume_tolerance = reference_metrics.volume.abs() * 0.02;
    assert!(
        (native_metrics.volume - reference_metrics.volume).abs() <= volume_tolerance,
        "[{label}] volume diverges: native {:.2} vs build123d {:.2} (tolerance {:.2}); \
         native stl {stl_path:?}, reference {}",
        native_metrics.volume,
        reference_metrics.volume,
        volume_tolerance,
        reference.preview_stl_path,
    );
    let area_tolerance = reference_metrics.area.abs() * 0.05;
    assert!(
        (native_metrics.area - reference_metrics.area).abs() <= area_tolerance,
        "[{label}] area diverges: native {:.2} vs build123d {:.2} (tolerance {:.2})",
        native_metrics.area,
        reference_metrics.area,
        area_tolerance,
    );
    for axis in 0..3 {
        assert!(
            (native_metrics.bbox_min[axis] - reference_metrics.bbox_min[axis]).abs() <= 0.5
                && (native_metrics.bbox_max[axis] - reference_metrics.bbox_max[axis]).abs() <= 0.5,
            "[{label}] bbox axis {axis} diverges: native {:?}..{:?} vs build123d {:?}..{:?}",
            native_metrics.bbox_min,
            native_metrics.bbox_max,
            reference_metrics.bbox_min,
            reference_metrics.bbox_max,
        );
    }
    assert_eq!(
        native_metrics.components, reference_metrics.components,
        "[{label}] component count diverges: native {} vs build123d {}",
        native_metrics.components, reference_metrics.components,
    );
    let non_manifold = ascii_stl_non_manifold_edge_count(&stl_path);
    assert_eq!(
        non_manifold, 0,
        "[{label}] native STL has {non_manifold} non-manifold edge(s)"
    );
    let budget = std::time::Duration::from_secs(10).max(reference_elapsed * 3);
    assert!(
        native_elapsed <= budget,
        "[{label}] native render {native_elapsed:?} exceeds envelope {budget:?} \
         (build123d reference took {reference_elapsed:?})"
    );
}
