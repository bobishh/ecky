use ecky_cad_lib::llm_eval::{
    persist_run, EvalCase, EvalRoute, EvalRun, EvalTurnPolicy, EVAL_SCHEMA_VERSION,
};
use ecky_cad_lib::provider_turn::ProviderTurnIntent;
use std::fs;
use std::process::Command;

fn run(run_id: &str, model: &str) -> EvalRun {
    EvalRun {
        schema_version: EVAL_SCHEMA_VERSION,
        run_id: run_id.into(),
        case: EvalCase {
            case_id: "case-a".into(),
            objective: "repair fixture".into(),
            acceptance_criteria: vec!["terminal success".into()],
            starting_version_id: Some("version-a".into()),
            starting_input_digest: Some("sha256:start".into()),
            expected_red_rounds: 0,
        },
        thread_id: "thread-a".into(),
        external_thread_id: "agy-a".into(),
        turn_id: format!("turn-{run_id}"),
        route: EvalRoute {
            provider: "agy".into(),
            model: Some(model.into()),
            effort: None,
            prompt_version: "agy-provider-v2".into(),
            jev: None,
        },
        prompt: "repair fixture".into(),
        started_at: 10,
        completed_at: 11,
        status: "success".into(),
        response: Some("done".into()),
        raw_error: None,
        turn_policy: Some(EvalTurnPolicy::for_intent(ProviderTurnIntent::Modify)),
        policy_violations: Vec::new(),
        events: Vec::new(),
        versions: Vec::new(),
        usage: None,
    }
}

#[test]
fn compare_cli_reads_edn_runs_and_writes_markdown() {
    let root = std::env::temp_dir().join(format!("ecky-eval-cli-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let baseline = persist_run(&root, &run("baseline", "gemini-flash")).unwrap();
    let challenger = persist_run(&root, &run("challenger", "gemini-pro")).unwrap();
    let output = root.join("comparison.md");

    let status = Command::new(env!("CARGO_BIN_EXE_compare_llm_evals"))
        .arg(&baseline.run_dir)
        .arg(&challenger.run_dir)
        .arg(&output)
        .status()
        .unwrap();

    assert!(status.success());
    let report = fs::read_to_string(output).unwrap();
    assert!(report.contains("# LLM eval comparison: case-a"));
    assert!(report.contains("Changed variable | model"));
    assert!(!root.join("comparison.json").exists());
    fs::remove_dir_all(root).unwrap();
}
