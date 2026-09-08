use ecky_cad_lib::contracts::CodexTakeoverRuntime;
use ecky_cad_lib::contracts::{
    Attachment, AttachmentKind, CodexDialogueMessage, CodexTakeoverBinding, ProviderEventKind,
};
use ecky_cad_lib::services::codex_app_server::{
    apply_live_notification, apply_runtime_notification, apply_start_response,
    bootstrap_instructions, message_page_params, parse_model_list_page, project_thread_messages,
    response_result_for_id, resume_params, runtime_from_turn_page, start_params,
    take_terminal_trace, CodexAppServerSupervisor,
};
use serde_json::json;

static CODEX_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn model_list_projection_uses_subscription_catalog_and_omits_hidden_entries() {
    let (models, next_cursor) = parse_model_list_page(&json!({
        "data": [
            { "model": "gpt-5.6", "hidden": false },
            { "model": "gpt-5.6-mini", "hidden": false },
            { "model": "internal-shadow", "hidden": true },
            { "model": "gpt-5.6", "hidden": false }
        ],
        "nextCursor": "next-model-page"
    }))
    .unwrap();

    assert_eq!(models, vec!["gpt-5.6", "gpt-5.6-mini"]);
    assert_eq!(next_cursor.as_deref(), Some("next-model-page"));
}

#[test]
fn response_matcher_ignores_notifications_and_returns_exact_request_result() {
    assert_eq!(
        response_result_for_id(
            r#"{"method":"thread/updated","params":{"threadId":"thread-1"}}"#,
            7,
        )
        .unwrap(),
        None,
    );
    assert_eq!(
        response_result_for_id(r#"{"id":7,"result":{"ok":true}}"#, 7).unwrap(),
        Some(json!({"ok": true})),
    );
}

#[test]
fn compaction_is_progress_and_only_matching_turn_completion_is_terminal() {
    let mut runtime = CodexTakeoverRuntime {
        phase: "active".to_string(),
        active_turn_id: Some("turn-7".to_string()),
        error: None,
    };
    apply_runtime_notification(
        &mut runtime,
        "thread/compacted",
        &json!({"threadId": "codex-7"}),
    );
    assert_eq!(runtime.phase, "active");
    assert_eq!(runtime.active_turn_id.as_deref(), Some("turn-7"));

    apply_runtime_notification(
        &mut runtime,
        "turn/completed",
        &json!({"turn": {"id": "turn-other", "status": "completed"}}),
    );
    assert_eq!(runtime.phase, "active");

    apply_runtime_notification(
        &mut runtime,
        "turn/completed",
        &json!({"turn": {"id": "turn-7", "status": "interrupted"}}),
    );
    assert_eq!(runtime.phase, "idle");
    assert_eq!(runtime.active_turn_id, None);

    apply_start_response(&mut runtime, "turn-7", true);
    assert_eq!(runtime.phase, "idle");
    assert_eq!(runtime.active_turn_id, None);
}

#[test]
fn idle_thread_status_is_terminal_when_app_server_omits_turn_completed() {
    let mut runtime = CodexTakeoverRuntime {
        phase: "active".to_string(),
        active_turn_id: Some("turn-live".to_string()),
        error: None,
    };

    apply_runtime_notification(
        &mut runtime,
        "thread/status/changed",
        &json!({"threadId": "codex-7", "status": {"type": "idle"}}),
    );

    assert_eq!(runtime.phase, "idle");
    assert_eq!(runtime.active_turn_id, None);
}

#[test]
fn live_projection_streams_readable_thoughts_answer_text_and_tool_activity() {
    let mut messages = Vec::new();
    apply_live_notification(
        &mut messages,
        "codex-7",
        "item/reasoning/summaryTextDelta",
        &json!({"itemId": "reason-1", "delta": "Проверяю глубину резьбы."}),
        100,
    );
    apply_live_notification(
        &mut messages,
        "codex-7",
        "item/started",
        &json!({"item": {"id": "tool-1", "type": "mcpToolCall", "server": "ecky_provider_mcp", "tool": "ecky_ast_inspect"}}),
        101,
    );
    apply_live_notification(
        &mut messages,
        "codex-7",
        "item/agentMessage/delta",
        &json!({"itemId": "answer-1", "delta": "Сейчас сверяю "}),
        102,
    );
    apply_live_notification(
        &mut messages,
        "codex-7",
        "item/agentMessage/delta",
        &json!({"itemId": "answer-1", "delta": "радиус."}),
        103,
    );
    apply_live_notification(
        &mut messages,
        "codex-7",
        "item/reasoning/textDelta",
        &json!({"itemId": "reason-1", "delta": "private chain of thought"}),
        104,
    );

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "THINKING · Проверяю глубину резьбы.");
    assert_eq!(
        messages[1].content,
        "USING TOOL · ecky_provider_mcp/ecky_ast_inspect"
    );
    assert_eq!(messages[2].content, "Сейчас сверяю радиус.");
    assert_eq!(
        messages
            .iter()
            .map(|message| message.provider_event_kind.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(ProviderEventKind::Activity),
            Some(ProviderEventKind::Activity),
            Some(ProviderEventKind::Assistant),
        ]
    );
    assert!(messages.iter().all(|message| message.status == "working"));

    apply_live_notification(
        &mut messages,
        "codex-7",
        "turn/completed",
        &json!({"turn": {"id": "turn-1", "status": "completed"}}),
        105,
    );
    assert_eq!(messages.len(), 3);
    assert!(messages.iter().all(|message| message.status == "success"));
    let trace = take_terminal_trace(&mut messages, "turn-1", "success", 105).unwrap();
    assert!(messages.is_empty());
    assert_eq!(trace.status, "success");
    assert_eq!(trace.messages[2].content, "Сейчас сверяю радиус.");
}

#[test]
fn live_projection_is_bounded_for_long_turns() {
    let mut messages = Vec::new();
    for index in 0..40 {
        apply_live_notification(
            &mut messages,
            "codex-7",
            "item/started",
            &json!({"item": {"id": format!("tool-{index}"), "type": "mcpToolCall", "server": "ecky", "tool": format!("inspect-{index}")}}),
            100 + index,
        );
    }
    apply_live_notification(
        &mut messages,
        "codex-7",
        "item/agentMessage/delta",
        &json!({"itemId": "answer-large", "delta": "x".repeat(20_000)}),
        200,
    );

    assert_eq!(messages.len(), 41);
    assert!(messages.last().unwrap().content.chars().count() <= 16_384);
}

#[test]
fn polling_latest_turn_clears_stale_active_runtime_after_missed_notification() {
    let runtime = runtime_from_turn_page(&json!({
        "data": [{"id": "turn-finished", "status": "completed"}]
    }))
    .unwrap();

    assert_eq!(runtime.phase, "idle");
    assert_eq!(runtime.active_turn_id, None);
}

#[test]
fn bootstrap_binds_direct_dialogue_to_exact_ecky_thread_once_as_instructions() {
    let prompt = bootstrap_instructions(
        "ecky-1",
        "Gearbox housing",
        "/workspace/gearbox",
        "THREAD SUMMARY\nCurrent target: gearbox housing\nRECENT DIALOGUE\nUSER: keep 3 mm walls",
    );
    assert!(prompt.contains("Ecky thread: ecky-1"));
    assert!(prompt.contains("already pre-bound"));
    assert!(prompt.contains("Do not call `thread_borrow`"));
    assert!(prompt.contains("read `agentBrief.primaryGuideUri`"));
    assert!(prompt.contains("edit that exact file"));
    assert!(!prompt.contains("inspect -> validate -> preview -> commit"));
    assert!(prompt.contains("ecky_provider_mcp"));
    assert!(prompt.contains("/workspace/gearbox/model.ecky"));
    assert!(prompt.contains("project mirror, not the canonical database"));
    assert!(prompt.contains("Do not start another user-prompt loop"));
    assert!(prompt.contains("[model.ecky](/workspace/gearbox/model.ecky:LINE)"));
    assert!(prompt.contains("Do not include internal `messageId` or `modelId`"));
    assert!(prompt.contains("Current target: gearbox housing"));
}

#[test]
fn start_is_persisted_named_service_with_same_bootstrap_and_live_mcp() {
    let start = start_params(
        "ecky-1",
        "Gearbox housing",
        "/workspace/gearbox",
        "http://127.0.0.1:39249/mcp",
        "THREAD SUMMARY\nCurrent target: gearbox housing",
        Some("gpt-5.6-codex"),
    );
    assert_eq!(start["cwd"], "/workspace/gearbox");
    assert_eq!(start["ephemeral"], false);
    assert_eq!(start["serviceName"], "ecky");
    assert_eq!(start["model"], "gpt-5.6-codex");
    assert_eq!(
        start["config"]["mcp_servers.ecky_provider_mcp.url"],
        "http://127.0.0.1:39249/mcp?providerThreadId=ecky-1"
    );
    assert_eq!(
        start["config"]["mcp_servers.ecky_provider_mcp.required"],
        true
    );
    assert!(start["config"].get("mcp_servers").is_none());
    assert!(start["developerInstructions"]
        .as_str()
        .unwrap()
        .contains("Current target: gearbox housing"));
}

#[test]
fn resume_reconciles_one_turn_without_loading_rollout_and_history_uses_opaque_cursor() {
    let binding = CodexTakeoverBinding {
        ecky_thread_id: "ecky-1".to_string(),
        codex_thread_id: "codex-7".to_string(),
        label: "Gearbox".to_string(),
        cwd: "/workspace/gearbox".to_string(),
        bootstrap_version: 1,
        created_at: 1,
        updated_at: 1,
    };
    let resume = resume_params(
        &binding,
        "Gearbox",
        "http://127.0.0.1:39249/mcp",
        "THREAD SUMMARY\nGearbox handoff",
        Some("gpt-5.6-codex"),
    );
    assert_eq!(resume["excludeTurns"], true);
    assert_eq!(resume["initialTurnsPage"]["limit"], 1);
    assert_eq!(resume["initialTurnsPage"]["itemsView"], "notLoaded");
    assert_eq!(resume["model"], "gpt-5.6-codex");
    assert_eq!(
        resume["config"]["mcp_servers.ecky_provider_mcp.url"],
        "http://127.0.0.1:39249/mcp?providerThreadId=ecky-1"
    );

    let page = message_page_params("codex-7", Some("opaque:older:7".to_string()), Some("older"));
    assert_eq!(page["cursor"], "opaque:older:7");
    assert_eq!(page["limit"], 30);
    assert_eq!(page["itemsView"], "full");
    assert_eq!(page["sortDirection"], "desc");
}

#[test]
fn transcript_projection_keeps_only_user_and_agent_messages_in_turn_order() {
    let thread = json!({
        "id": "codex-thread-7",
        "turns": [
            {
                "id": "turn-2",
                "status": "inProgress",
                "startedAt": 200,
                "completedAt": null,
                "items": [
                    {
                        "id": "user-2",
                        "type": "userMessage",
                        "content": [{"type": "text", "text": "Cut seat."}]
                    }
                ]
            },
            {
                "id": "turn-1",
                "status": "completed",
                "startedAt": 100,
                "completedAt": 104,
                "items": [
                    {
                        "id": "assistant-1",
                        "type": "agentMessage",
                        "phase": "final_answer",
                        "text": "Rib ready."
                    },
                    {
                        "id": "user-1",
                        "type": "userMessage",
                        "content": [
                            {"type": "text", "text": "Make a rib."},
                            {"type": "image", "url": "data:image/png;base64,abc"}
                        ]
                    },
                    {"id": "reason-1", "type": "reasoning", "summary": ["private"]}
                ]
            }
        ]
    });

    assert_eq!(
        project_thread_messages(&thread).unwrap(),
        vec![
            CodexDialogueMessage {
                id: "codex:codex-thread-7:turn-1:user:0".to_string(),
                role: "user".to_string(),
                content: "Make a rib.".to_string(),
                status: "success".to_string(),
                timestamp: 100,
                attachments: vec![Attachment {
                    path: String::new(),
                    name: "image".to_string(),
                    explanation: String::new(),
                    data_url: Some("data:image/png;base64,abc".to_string()),
                    kind: AttachmentKind::Image,
                }],
                provider_event_kind: None,
            },
            CodexDialogueMessage {
                id: "codex:codex-thread-7:turn-1:assistant:assistant-1".to_string(),
                role: "assistant".to_string(),
                content: "Rib ready.".to_string(),
                status: "success".to_string(),
                timestamp: 104,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
            CodexDialogueMessage {
                id: "codex:codex-thread-7:turn-2:user:0".to_string(),
                role: "user".to_string(),
                content: "Cut seat.".to_string(),
                status: "success".to_string(),
                timestamp: 200,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
        ],
    );
}

#[cfg(unix)]
#[tokio::test]
async fn timed_out_app_server_is_restarted_for_next_operation() {
    use std::os::unix::fs::PermissionsExt;

    let _environment = CODEX_ENV_LOCK.lock().await;

    let test_id = uuid::Uuid::new_v4().to_string();
    let directory = std::env::temp_dir().join(format!("ecky-codex-timeout-{test_id}"));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("fake-codex");
    let starts = directory.join("starts");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json, os, sys
with open(os.environ["ECKY_CODEX_TEST_STARTS"], "a", encoding="utf-8") as starts:
    starts.write("started\n")
for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        print(json.dumps({"id": message["id"], "result": {}}), flush=True)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    std::env::set_var("ECKY_CODEX_BIN", &executable);
    std::env::set_var("ECKY_CODEX_TEST_STARTS", &starts);
    std::env::set_var("ECKY_CODEX_REQUEST_TIMEOUT_MS", "500");
    let supervisor = CodexAppServerSupervisor::new();

    let first = supervisor
        .request("thread/list", json!({"limit": 1}))
        .await
        .unwrap_err();
    assert!(first.message.contains("timed out"));
    let second = supervisor
        .request("thread/list", json!({"limit": 1}))
        .await
        .unwrap_err();
    assert!(second.message.contains("timed out"));

    let start_count = std::fs::read_to_string(&starts).unwrap().lines().count();
    assert_eq!(start_count, 2);
    std::env::remove_var("ECKY_CODEX_BIN");
    std::env::remove_var("ECKY_CODEX_TEST_STARTS");
    std::env::remove_var("ECKY_CODEX_REQUEST_TIMEOUT_MS");
    let _ = std::fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[tokio::test]
async fn writer_conflicts_retry_same_bound_thread_after_foreign_release() {
    use std::os::unix::fs::PermissionsExt;

    let _environment = CODEX_ENV_LOCK.lock().await;

    let test_id = uuid::Uuid::new_v4().to_string();
    let directory = std::env::temp_dir().join(format!("ecky-codex-writer-{test_id}"));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("fake-codex");
    let requests = directory.join("requests");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json, os, sys
resume_count = 0
start_count = 0
requests = open(os.environ["ECKY_CODEX_TEST_REQUESTS"], "a", encoding="utf-8")
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method:
        requests.write(method + " " + str(message.get("params", {}).get("threadId", "")) + "\n")
        requests.flush()
    if method == "initialize":
        print(json.dumps({"id": message["id"], "result": {}}), flush=True)
    elif method == "thread/resume":
        resume_count += 1
        if resume_count == 1:
            print(json.dumps({"id": message["id"], "error": {"message": "thread codex-7 already has an active writer", "data": {"owner": "foreign-client"}}}), flush=True)
        else:
            print(json.dumps({"id": message["id"], "result": {"thread": {"id": "codex-7", "preview": "Dryer", "cwd": "/tmp/dryer", "createdAt": 1, "updatedAt": 2, "modelProvider": "openai", "status": {"type": "idle"}}, "initialTurnsPage": {"data": []}}}), flush=True)
    elif method == "turn/start":
        start_count += 1
        if start_count == 1:
            print(json.dumps({"id": message["id"], "error": {"message": "thread codex-7 already has an active writer", "data": {"owner": "foreign-client"}}}), flush=True)
        else:
            print(json.dumps({"id": message["id"], "result": {"turn": {"id": "turn-2"}}}), flush=True)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    std::env::set_var("ECKY_CODEX_BIN", &executable);
    std::env::set_var("ECKY_CODEX_TEST_REQUESTS", &requests);
    let supervisor = CodexAppServerSupervisor::new();
    let binding = CodexTakeoverBinding {
        ecky_thread_id: "ecky-1".to_string(),
        codex_thread_id: "codex-7".to_string(),
        label: "Dryer".to_string(),
        cwd: "/tmp/dryer".to_string(),
        bootstrap_version: ecky_cad_lib::services::codex_takeover::CODEX_BOOTSTRAP_VERSION,
        created_at: 1,
        updated_at: 1,
    };

    let first_resume = supervisor
        .resume_thread(
            &binding,
            "Dryer",
            "http://127.0.0.1:1234/mcp",
            "handoff",
            false,
            false,
            None,
        )
        .await
        .unwrap_err();
    assert!(first_resume
        .message
        .contains("already has an active writer"));

    supervisor
        .resume_thread(
            &binding,
            "Dryer",
            "http://127.0.0.1:1234/mcp",
            "handoff",
            false,
            false,
            None,
        )
        .await
        .unwrap();
    let first_turn = supervisor.start_turn("codex-7", "Continue", None).await;
    assert!(first_turn
        .unwrap_err()
        .message
        .contains("already has an active writer"));
    // The successful resume remains cached for this app-server generation. A
    // later retry must continue the same binding without manufacturing another
    // resume or replacement thread.
    supervisor
        .resume_thread(
            &binding,
            "Dryer",
            "http://127.0.0.1:1234/mcp",
            "handoff",
            false,
            false,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        supervisor
            .start_turn("codex-7", "Continue", None)
            .await
            .unwrap(),
        "turn-2"
    );

    let request_log = std::fs::read_to_string(&requests).unwrap();
    assert_eq!(request_log.matches("thread/start").count(), 0);
    assert_eq!(request_log.matches("thread/resume codex-7").count(), 2);
    assert_eq!(request_log.matches("turn/start codex-7").count(), 2);
    assert!(!request_log.contains("thread/resume codex-new"));

    std::env::remove_var("ECKY_CODEX_BIN");
    std::env::remove_var("ECKY_CODEX_TEST_REQUESTS");
    let _ = std::fs::remove_dir_all(directory);
}
