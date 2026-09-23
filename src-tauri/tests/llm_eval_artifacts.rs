use ecky_cad_lib::llm_eval::{
    build_agy_eval_run, build_failed_eval_run, case_id, compare_runs, latest_version_identity,
    persist_run, read_run, score_run, version_outcomes_for_window, EvalCase, EvalEvent,
    EvalEventKind, EvalPayload, EvalRoute, EvalRun, EvalRunSeed, EvalTurnPolicy, EvalUsage,
    EvalVersionOutcome,
};
use ecky_cad_lib::provider_turn::ProviderTurnIntent;
use ecky_cad_lib::services::agy_provider::AgyTurnResult;
use serde_json::json;
use std::fs;

fn temp_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("ecky-llm-eval-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn fixture_run(run_id: &str, model: &str) -> EvalRun {
    EvalRun {
        schema_version: ecky_cad_lib::llm_eval::EVAL_SCHEMA_VERSION,
        run_id: run_id.into(),
        case: EvalCase {
            case_id: "floating-light-trap".into(),
            objective: "Attach the light trap to printable support.".into(),
            acceptance_criteria: vec!["latest changed version verifies green".into()],
            starting_version_id: Some("version-a".into()),
            starting_input_digest: Some("sha256:start".into()),
            expected_red_rounds: 1,
        },
        thread_id: "thread-7".into(),
        external_thread_id: "agy-7".into(),
        turn_id: format!("turn-{run_id}"),
        route: EvalRoute {
            provider: "agy".into(),
            model: Some(model.into()),
            effort: Some("high".into()),
            prompt_version: "agy-provider-v2".into(),
        },
        prompt: "ловушка висит в воздухе бро".into(),
        started_at: 100,
        completed_at: 112,
        status: "success".into(),
        response: Some("Исправлено.".into()),
        raw_error: None,
        turn_policy: Some(EvalTurnPolicy::for_intent(ProviderTurnIntent::Modify)),
        policy_violations: Vec::new(),
        events: vec![
            EvalEvent {
                sequence: 1,
                step_index: Some(1),
                kind: EvalEventKind::Tool,
                state: "done".into(),
                name: Some("find_by_name".into()),
                summary: Some("USING TOOL · find_by_name".into()),
                input: Some(EvalPayload::new(json!({
                    "query": "light trap",
                    "apiToken": "must-not-survive"
                }))),
                output: None,
                error: None,
                occurred_at: 101,
            },
            EvalEvent {
                sequence: 2,
                step_index: Some(2),
                kind: EvalEventKind::Tool,
                state: "done".into(),
                name: Some("find_by_name".into()),
                summary: Some("USING TOOL · find_by_name".into()),
                input: None,
                output: Some(EvalPayload::new(json!({ "text": "x".repeat(80_000) }))),
                error: None,
                occurred_at: 102,
            },
            EvalEvent {
                sequence: 3,
                step_index: Some(3),
                kind: EvalEventKind::Tool,
                state: "done".into(),
                name: Some("run_command".into()),
                summary: Some("USING TOOL · run_command".into()),
                input: None,
                output: None,
                error: None,
                occurred_at: 103,
            },
        ],
        versions: vec![
            EvalVersionOutcome {
                version_id: "version-b".into(),
                input_digest: Some("sha256:red".into()),
                status: "error".into(),
                verification_passed: Some(false),
                raw_error: Some("non-manifold".into()),
                created_at: 105,
            },
            EvalVersionOutcome {
                version_id: "version-c".into(),
                input_digest: Some("sha256:green".into()),
                status: "success".into(),
                verification_passed: Some(true),
                raw_error: None,
                created_at: 110,
            },
        ],
        usage: Some(EvalUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            estimated_cost_usd: Some(0.02),
        }),
    }
}

#[test]
fn completed_turn_persists_strict_edn_trajectory_and_markdown_report() {
    let root = temp_root();
    let files = persist_run(&root, &fixture_run("baseline", "gemini-flash")).unwrap();

    assert_eq!(files.run_edn.file_name().unwrap(), "run.edn");
    assert_eq!(files.trajectory_edn.file_name().unwrap(), "trajectory.edn");
    assert_eq!(files.report_md.file_name().unwrap(), "report.md");
    assert!(!files.run_dir.join("run.json").exists());
    assert!(!files.run_dir.join("trajectory.json").exists());

    let encoded = fs::read_to_string(&files.trajectory_edn).unwrap();
    assert!(encoded.starts_with("{:"));
    assert!(!encoded.contains("must-not-survive"));
    assert!(encoded.contains("[REDACTED]"));
    assert!(encoded.contains(":sha256"));
    assert!(encoded.contains(":truncated true"));

    let loaded = read_run(&files.run_dir).unwrap();
    assert_eq!(loaded.events.len(), 3);
    assert_eq!(loaded.events[0].name.as_deref(), Some("find_by_name"));
    assert_eq!(loaded.events[2].name.as_deref(), Some("run_command"));

    let report = fs::read_to_string(files.report_md).unwrap();
    assert!(report.contains("# LLM eval: floating-light-trap"));
    assert!(report.contains("Intent: `modify`"));
    assert!(report.contains("Tool calls | 3"));
    assert!(report.contains("Repeated adjacent tools | 1"));
    assert!(report.contains("Policy violations | 0"));
    assert!(report.contains("First build green | no"));
    assert!(report.contains("Red-to-green repair | yes"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn active_and_done_updates_for_one_provider_step_count_as_one_tool_call() {
    let mut run = fixture_run("streaming-step", "gemini-flash");
    run.events = vec![
        EvalEvent {
            sequence: 1,
            step_index: Some(44),
            kind: EvalEventKind::Tool,
            state: "active".into(),
            name: Some("view_file".into()),
            summary: None,
            input: None,
            output: None,
            error: None,
            occurred_at: 101,
        },
        EvalEvent {
            sequence: 2,
            step_index: Some(44),
            kind: EvalEventKind::Tool,
            state: "done".into(),
            name: Some("view_file".into()),
            summary: None,
            input: None,
            output: None,
            error: None,
            occurred_at: 102,
        },
    ];

    let scores = score_run(&run);
    assert_eq!(scores.tool_calls, 1);
    assert_eq!(scores.repeated_adjacent_tools, 0);
}

#[test]
fn paired_comparison_requires_same_case_and_one_route_variable() {
    let baseline = fixture_run("baseline", "gemini-flash");
    let challenger = fixture_run("challenger", "gemini-pro");
    let report = compare_runs(&baseline, &challenger).unwrap();
    assert!(report.contains("Changed variable | model"));
    assert!(report.contains("model: gemini-flash | model: gemini-pro"));
    assert!(report.contains("Completion"));
    assert!(report.contains("Tool calls"));
    assert!(report.contains("Policy violations"));
    assert!(report.contains("Tokens"));
    assert!(report.contains("Cost USD"));

    let mut wrong_case = challenger.clone();
    wrong_case.case.case_id = "different-case".into();
    assert!(compare_runs(&baseline, &wrong_case)
        .unwrap_err()
        .contains("same case"));

    let mut two_variables = challenger;
    two_variables.route.effort = Some("medium".into());
    assert!(compare_runs(&baseline, &two_variables)
        .unwrap_err()
        .contains("exactly one route variable"));
}

#[test]
fn completed_agy_turn_builds_provider_neutral_run_bound_to_changed_versions() {
    let event = fixture_run("seed", "gemini-flash").events.remove(0);
    let result = AgyTurnResult {
        conversation_id: "agy-7".into(),
        turn_id: "turn-7".into(),
        status: "SUCCESS".into(),
        response: "Исправлено.".into(),
        error: None,
        eval_events: vec![event],
        started_at: 100,
        completed_at: 112,
    };
    let changed = fixture_run("seed", "gemini-flash").versions;
    let run = build_agy_eval_run(
        EvalRunSeed {
            run_id: "queue-7".into(),
            thread_id: "thread-7".into(),
            external_thread_id: "agy-7".into(),
            provider: "agy".into(),
            model: Some("gemini-flash".into()),
            effort: None,
            prompt_version: "agy-provider-v2".into(),
            prompt: "ловушка висит в воздухе бро".into(),
            starting_version_id: Some("version-a".into()),
            starting_input_digest: Some("sha256:start".into()),
            expected_red_rounds: 0,
            turn_intent: ProviderTurnIntent::Modify,
        },
        result,
        changed,
    );

    assert_eq!(run.status, "success");
    assert_eq!(run.turn_id, "turn-7");
    assert_eq!(run.events.len(), 1);
    assert_eq!(run.versions.len(), 2);
    assert_eq!(run.case.case_id, case_id(&run.prompt, Some("sha256:start")));
}

#[test]
fn eval_seed_uses_latest_authored_version_identity() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            status TEXT NOT NULL,
            output TEXT,
            deleted_at INTEGER,
            timestamp INTEGER NOT NULL,
            version_input_digest TEXT
        );
        INSERT INTO messages VALUES
          ('old', 'thread-7', 'assistant', 'success', '{}', NULL, 10, 'sha256:old'),
          ('latest', 'thread-7', 'assistant', 'success', '{}', NULL, 20, 'sha256:latest'),
          ('deleted', 'thread-7', 'assistant', 'success', '{}', 21, 30, 'sha256:deleted');",
    )
    .unwrap();

    assert_eq!(
        latest_version_identity(&conn, "thread-7").unwrap(),
        Some(("latest".into(), Some("sha256:latest".into())))
    );
}

#[test]
fn provider_transport_failure_still_builds_terminal_eval_run() {
    let seed = EvalRunSeed {
        run_id: "queue-failed".into(),
        thread_id: "thread-7".into(),
        external_thread_id: "agy-7".into(),
        provider: "agy".into(),
        model: Some("gemini-flash".into()),
        effort: None,
        prompt_version: "agy-provider-v2".into(),
        prompt: "fix it".into(),
        starting_version_id: None,
        starting_input_digest: None,
        expected_red_rounds: 0,
        turn_intent: ProviderTurnIntent::Modify,
    };

    let run = build_failed_eval_run(seed, "turn-failed", 100, 105, "provider exited", Vec::new());
    assert_eq!(run.status, "error");
    assert_eq!(run.raw_error.as_deref(), Some("provider exited"));
    assert!(run.events.is_empty());
}

#[test]
fn answer_run_records_tool_and_version_policy_violations() {
    let mut run = fixture_run("answer-violation", "gemini-flash");
    run.turn_policy = Some(EvalTurnPolicy::for_intent(ProviderTurnIntent::Answer));
    run.policy_violations = ecky_cad_lib::llm_eval::evaluate_policy_violations(&run);

    assert!(run
        .policy_violations
        .iter()
        .any(|violation| violation.contains("tool")));
    assert!(run
        .policy_violations
        .iter()
        .any(|violation| violation.contains("version")));
    assert_eq!(score_run(&run).policy_violations, 4);
}

#[test]
fn version_linkage_uses_structural_verification_result_not_message_status() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            status TEXT NOT NULL,
            content TEXT NOT NULL,
            output TEXT,
            structural_verification TEXT,
            deleted_at INTEGER,
            timestamp INTEGER NOT NULL,
            version_input_digest TEXT
        );
        INSERT INTO messages VALUES
          ('red', 'thread-7', 'assistant', 'success', 'verification failed', '{}',
           '{\"passed\":false}', NULL, 101, 'sha256:red');",
    )
    .unwrap();

    let versions = version_outcomes_for_window(&conn, "thread-7", None, 100, 102).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].verification_passed, Some(false));
}
