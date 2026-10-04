use ecky_cad_lib::agent_prompt::agent_language_reference;
use ecky_cad_lib::commands::generation::design_system_prompt;
use ecky_cad_lib::contracts::{GeometryBackend, SourceLanguage};
use ecky_cad_lib::ecky_language_surface::supported_surface_reference;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

const HUMAN_REFERENCE: &str = include_str!("../../public/docs/ecky-ir.md");
const AGENT_REFERENCE: &str = include_str!("../../public/docs/ecky-agent-reference.md");

fn canonical_agent_reference() -> &'static str {
    AGENT_REFERENCE.trim()
}

#[test]
fn api_prompt_projects_the_canonical_book_agent_reference() {
    let canonical = canonical_agent_reference();
    let prompt = agent_language_reference(GeometryBackend::EckyRust);

    assert!(prompt.contains(canonical));
    assert!(canonical.contains("`mesh` and `polyhedron`"));
    assert!(canonical.contains("`protrude`"));
    assert!(!canonical.contains("`heightfield`"));
    assert!(canonical.contains("single perspective image"));
    assert!(canonical.contains("faceted poly-BRep"));
}

#[test]
fn api_design_generation_uses_the_shared_language_reference_verbatim() {
    let shared = agent_language_reference(GeometryBackend::EckyRust);
    let design = design_system_prompt(SourceLanguage::EckyIrV0, GeometryBackend::EckyRust, None);

    assert!(design.contains(&shared));
}

#[test]
fn committed_prompt_artifacts_are_fresh() {
    let generated_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/generated");
    for (filename, backend) in [
        (
            "ecky-agent-system-prompt-ecky-rust.md",
            GeometryBackend::EckyRust,
        ),
        (
            "ecky-agent-system-prompt-build123d.md",
            GeometryBackend::Build123d,
        ),
        (
            "ecky-agent-system-prompt-freecad.md",
            GeometryBackend::Freecad,
        ),
    ] {
        let generated = fs::read_to_string(generated_dir.join(filename)).expect("generated prompt");
        let generated = generated.trim_end();
        let expected = agent_language_reference(backend);
        let expected = expected.trim_end();
        if generated != expected {
            let offset = generated
                .bytes()
                .zip(expected.bytes())
                .position(|(left, right)| left != right)
                .unwrap_or_else(|| generated.len().min(expected.len()));
            panic!(
                "{filename} stale at byte {offset}; generated_len={}, expected_len={}",
                generated.len(),
                expected.len()
            );
        }
    }
}

#[test]
fn human_reference_operation_index_links_only_registered_documented_forms() {
    let mut registered = BTreeSet::new();
    for backend in [
        GeometryBackend::EckyRust,
        GeometryBackend::Build123d,
        GeometryBackend::Freecad,
    ] {
        for entry in supported_surface_reference(backend).entries {
            registered.insert(entry.name);
        }
    }

    let linked = HUMAN_REFERENCE
        .lines()
        .filter_map(|line| {
            line.strip_prefix("| [`")
                .and_then(|rest| rest.split_once("`]("))
                .map(|(name, _)| name.to_owned())
        })
        .collect::<BTreeSet<_>>();

    assert!(
        !linked.is_empty(),
        "human operation index has no linked forms"
    );
    for name in &linked {
        assert!(
            registered.contains(name),
            "human operation index links unregistered form `{name}`"
        );
    }
    for required in ["box", "params", "part", "import-stl"] {
        assert!(
            linked.contains(required),
            "human operation index lost documented form `{required}`"
        );
    }
}

#[test]
fn human_reference_covers_runtime_registry() {
    let documented_details = HUMAN_REFERENCE
        .lines()
        .filter_map(|line| {
            line.trim_start_matches('#')
                .trim_start()
                .strip_prefix('`')
                .and_then(|rest| rest.split_once('`'))
                .map(|(name, _)| name.to_owned())
        })
        .collect::<BTreeSet<_>>();
    let generated_table = HUMAN_REFERENCE
        .lines()
        .filter_map(|line| {
            line.strip_prefix("| `")
                .and_then(|rest| rest.split_once("` |"))
                .map(|(name, _)| name.to_owned())
        })
        .collect::<BTreeSet<_>>();

    for backend in [
        GeometryBackend::EckyRust,
        GeometryBackend::Build123d,
        GeometryBackend::Freecad,
    ] {
        for entry in supported_surface_reference(backend).entries {
            assert!(
                documented_details.contains(&entry.name) || generated_table.contains(&entry.name),
                "human reference is missing `{}` for {backend:?}",
                entry.name
            );
        }
    }
}

#[test]
fn canonical_references_explain_live_packages_locks_and_native_step_truth() {
    let human_reference = HUMAN_REFERENCE
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "### Live package references",
        "(import-component",
        "No semver ranges, `latest`, network fallback, or transitive package lookup",
        "application-global content-addressed store",
        "`ecky.lock.edn`",
        "never calls FreeCAD, converts through STL, invokes `solidify`",
    ] {
        assert!(
            human_reference.contains(expected),
            "human reference missing `{expected}`"
        );
    }

    let agent_reference = canonical_agent_reference()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "`component_get` is vendor mode",
        "(import-component",
        "committed exact dependency lock",
        "STEP-backed live components",
    ] {
        assert!(
            agent_reference.contains(expected),
            "agent reference missing `{expected}`"
        );
    }
}

#[test]
fn published_native_loft_reference_compiles_with_explicit_distance() {
    let reference = supported_surface_reference(GeometryBackend::EckyRust);
    let loft = reference
        .entries
        .iter()
        .find(|entry| entry.name == "loft")
        .unwrap();
    assert!(
        loft.signature.contains("distance"),
        "native loft reference must document required distance: {}",
        loft.signature
    );
    let source = format!("(model (part body {}))", loft.example);
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&source)
        .expect("published native loft example must compile without invented bindings");
}

#[test]
fn book_surface_table_matches_published_loft_signature() {
    let reference = supported_surface_reference(GeometryBackend::EckyRust);
    let loft = reference
        .entries
        .iter()
        .find(|entry| entry.name == "loft")
        .unwrap();
    let corpus = include_str!("../../docs/books/ecky-ir/ecky-ir-corpus.md");
    let row = corpus
        .lines()
        .find(|line| line.starts_with("| `loft` |"))
        .expect("book must include loft surface reference");
    assert!(
        row.contains(&format!("`{}`", loft.signature)),
        "book loft signature must match published MCP reference: {row}"
    );
}
