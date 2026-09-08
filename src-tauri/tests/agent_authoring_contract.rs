use ecky_cad_lib::agent_prompt::mcp_language_reference;
use ecky_cad_lib::contracts::GeometryBackend;
use ecky_cad_lib::ecky_cad_host::direct_occt::{plan_core_program, OcctOp};
use ecky_cad_lib::ecky_language_surface::supported_surface_reference;
use ecky_cad_lib::ecky_scheme::compile_to_core_program;

fn plan(source: &str) -> Vec<OcctOp> {
    let program = compile_to_core_program(source).expect("catalogue example compiles");
    let plan = plan_core_program(&program).expect("catalogue example reaches native plan");
    plan.parts[0]
        .commands
        .iter()
        .map(|command| command.op)
        .collect()
}

#[test]
fn published_native_catalogue_examples_compile_and_plan() {
    let clip_plane = supported_surface_reference(GeometryBackend::EckyRust)
        .entries
        .into_iter()
        .find(|entry| entry.name == "clip-plane")
        .expect("clip-plane catalogue entry");
    assert_eq!(
        clip_plane.example,
        "(clip-plane body :origin '(0 0 10) :normal '(0 0 1) :keep \"positive\")"
    );
    assert_eq!(
        plan(&format!(
            "(model (part enclosure (let* ((body (box 20 20 20))) {})))",
            clip_plane.example
        )),
        vec![OcctOp::Box, OcctOp::ClipPlane]
    );
    let unresolved_local = compile_to_core_program(
        "(model (part enclosure (let* ((body (box 20 20 20)) (keep positive)) (clip-plane body :origin '(0 0 10) :normal '(0 0 1) :keep keep))))",
    )
    .expect("unresolved local positive remains deferred until native planning");
    let unresolved_local = plan_core_program(&unresolved_local)
        .expect_err("unresolved local positive must remain a native planning diagnostic");
    let unresolved_local = unresolved_local.to_string();
    assert_eq!(
        unresolved_local,
        "Direct OCCT adapter could not resolve local `positive`."
    );

    let clip_box = supported_surface_reference(GeometryBackend::EckyRust)
        .entries
        .into_iter()
        .find(|entry| entry.name == "clip-box")
        .expect("clip-box catalogue entry");
    assert_eq!(
        clip_box.example,
        "(clip-box body :x '(0 100) :y '(-30 30) :z '(0 40))"
    );
    assert_eq!(
        plan(&format!(
            "(model (part enclosure (let* ((body (box 20 20 20))) {})))",
            clip_box.example
        )),
        vec![OcctOp::Box, OcctOp::ClipBox]
    );

    let bezier = supported_surface_reference(GeometryBackend::EckyRust)
        .entries
        .into_iter()
        .find(|entry| entry.name == "bezier-path")
        .expect("bezier-path catalogue entry");
    assert_eq!(
        bezier.example,
        "(bezier-path ((0 0 0) (8 0 0) (8 8 12) (16 8 12)))"
    );
    assert_eq!(
        plan(&format!(
            "(model (part body (sweep (circle 2 16) {})))",
            bezier.example
        )),
        vec![OcctOp::Circle, OcctOp::BezierPath, OcctOp::Sweep]
    );
}

#[test]
fn mcp_guidance_states_native_approximation_and_artifact_limits() {
    let guide = mcp_language_reference(GeometryBackend::EckyRust);
    for required in [
        "positive",
        "all three bounds",
        "16 samples per cubic",
        "geometryBackend",
        "legacy",
        "artifact truth",
        "topology",
        "cross-section",
        "support-free",
        "parameters",
        "preserve existing topology",
        "do not fill loops",
        "bent round profiles",
    ] {
        assert!(guide.contains(required), "MCP guide missing `{required}`");
    }
}
