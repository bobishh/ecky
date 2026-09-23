use ecky_cad_lib::llm_eval::{project_agy_eval_event, EvalEventKind, EvalValue};
use ecky_cad_lib::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};
use ecky_cad_lib::services::agy_provider::{
    parse_agy_version, project_stream_event, AgyProjectedEvent, AgyProviderSupervisor,
    MINIMUM_AGY_VERSION,
};
use serde_json::json;

static AGY_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(unix)]
#[tokio::test]
async fn answer_turn_runs_agy_without_edit_or_auto_approval_mode() {
    use std::os::unix::fs::PermissionsExt;
    let _environment = AGY_ENV_LOCK.lock().await;
    let directory = std::env::temp_dir().join(format!(
        "ecky-agy-answer-policy-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("agy-policy-fake.py");
    let args_file = directory.join("args.txt");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json
import os
import sys

if "--version" in sys.argv:
    print("Antigravity CLI 1.1.15")
    raise SystemExit(0)
with open(os.environ["ECKY_AGY_TEST_ARGS"], "w", encoding="utf-8") as output:
    output.write("\n".join(sys.argv[1:]))
print(json.dumps({"event": "init", "conversation_id": "agy-policy-1", "init": {}}), flush=True)
for line in sys.stdin:
    json.loads(line)
    print(json.dumps({"event": "result", "result": {
        "conversation_id": "agy-policy-1", "status": "SUCCESS", "response": "answer"
    }}), flush=True)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    std::env::set_var("ECKY_AGY_BIN", &executable);
    std::env::set_var("ECKY_AGY_TEST_ARGS", &args_file);

    let supervisor = AgyProviderSupervisor::new();
    let started = supervisor
        .start_new_turn_with_policy(
            directory.to_str().unwrap(),
            "answer now",
            None,
            None,
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), started.result)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.response, "answer");
    let args = std::fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("--mode\naccept-edits"));
    assert!(args.contains("--dangerously-skip-permissions"));

    std::env::remove_var("ECKY_AGY_BIN");
    std::env::remove_var("ECKY_AGY_TEST_ARGS");
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn version_gate_requires_bidirectional_stream_json_release() {
    assert_eq!(MINIMUM_AGY_VERSION, (1, 1, 15));
    assert_eq!(
        parse_agy_version("Antigravity CLI 1.1.15\n").unwrap(),
        (1, 1, 15)
    );
    assert_eq!(parse_agy_version("agy 2.0.3").unwrap(), (2, 0, 3));
    assert!(parse_agy_version("Antigravity CLI unknown").is_err());
}

#[test]
fn stream_projection_exposes_public_progress_but_not_tool_stdout() {
    let delta = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 4,
            "state": "DONE",
            "step_type": "agent_response",
            "text_delta": "Inspecting constraints."
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        delta,
        AgyProjectedEvent::AssistantDelta {
            conversation_id: "agy-7".to_string(),
            step_index: 4,
            text: "Inspecting constraints.".to_string(),
        }
    );

    let tool = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 5,
            "state": "DONE",
            "step_type": "tool",
            "tool_name": "call_mcp_tool",
            "tool_info": {
                "input": {
                    "ServerName": "ecky_mcp",
                    "ToolName": "target_meta_get",
                    "Arguments": { "threadId": "thread-7" }
                },
                "output": "secret terminal dump"
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        tool,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 5,
            text: "USING TOOL · ecky_mcp/target_meta_get".to_string(),
        }
    );
    assert!(!format!("{tool:?}").contains("secret terminal dump"));
}

#[test]
fn eval_projection_retains_safe_tool_input_output_and_exact_order_fields() {
    let event = project_agy_eval_event(
        &json!({
            "event": "step_update",
            "step_update": {
                "conversation_id": "agy-7",
                "step_index": 5,
                "state": "DONE",
                "step_type": "tool",
                "tool_name": "call_mcp_tool",
                "tool_info": {
                    "input": {
                        "ServerName": "ecky_mcp",
                        "ToolName": "target_meta_get",
                        "Arguments": { "threadId": "thread-7", "token": "private" }
                    },
                    "output": { "ok": true, "versionId": "version-7" }
                }
            }
        }),
        123,
    )
    .unwrap();
    assert_eq!(event.kind, EvalEventKind::Tool);
    assert_eq!(event.step_index, Some(5));
    assert_eq!(event.name.as_deref(), Some("ecky_mcp/target_meta_get"));
    assert_eq!(event.occurred_at, 123);
    assert!(event.input.is_some());
    assert!(event.output.is_some());
    let encoded = ecky_cad_lib::strict_edn::to_vec(event.input.as_ref().unwrap()).unwrap();
    let text = String::from_utf8(encoded).unwrap();
    assert!(text.contains("thread-7"));
    assert!(text.contains("[REDACTED]"));
    assert!(!text.contains("private"));
    assert!(matches!(
        event.output.as_ref().unwrap().value,
        EvalValue::Map(_)
    ));
}

#[test]
fn stream_projection_prefers_public_actions_and_suppresses_protocol_noise() {
    let tool = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 6,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "call_mcp_tool",
            "tool_info": {
                "input": {
                    "ServerName": "ecky_mcp",
                    "ToolName": "session_log_in",
                    "Arguments": { "threadId": "thread-7" },
                    "toolAction": "Logging into Ecky session",
                    "toolSummary": "Ecky login"
                },
                "output": "private tool output"
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        tool,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 6,
            text: "WORKING · Logging into Ecky session".to_string(),
        }
    );
    assert!(!format!("{tool:?}").contains("private tool output"));

    let encoded_tool = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 7,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "run_command",
            "tool_info": {
                "input": "{\"toolAction\":\"Inspecting fit constraints\"}",
                "output": { "toolAction": "private output must not leak" }
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        encoded_tool,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 7,
            text: "WORKING · Inspecting fit constraints".to_string(),
        }
    );
    assert!(!format!("{encoded_tool:?}").contains("private output must not leak"));

    for step_type in ["system_message", "unknown", "progress"] {
        let event = project_stream_event(&json!({
            "event": "step_update",
            "step_update": {
                "conversation_id": "agy-7",
                "step_index": 7,
                "state": "DONE",
                "step_type": step_type,
                "text_delta": "internal protocol payload"
            }
        }))
        .unwrap();
        assert_eq!(event, None, "{step_type} must not become a fake activity");
    }
}

#[test]
fn terminal_result_carries_exact_status_response_and_error() {
    let result = project_stream_event(&json!({
        "event": "result",
        "result": {
            "conversation_id": "agy-7",
            "status": "ERROR",
            "response": "partial",
            "error": "MCP transport returned 503 raw body"
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        result,
        AgyProjectedEvent::Result {
            conversation_id: "agy-7".to_string(),
            status: "ERROR".to_string(),
            response: "partial".to_string(),
            error: Some("MCP transport returned 503 raw body".to_string()),
        }
    );
}

#[cfg(unix)]
#[tokio::test]
async fn bidirectional_session_finishes_each_turn_without_waiting_for_process_exit() {
    use std::os::unix::fs::PermissionsExt;
    let _environment = AGY_ENV_LOCK.lock().await;

    let directory = std::env::temp_dir().join(format!("ecky-agy-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("agy-fake.py");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json
import sys

if "--version" in sys.argv:
    print("Antigravity CLI 1.1.15")
    raise SystemExit(0)
if "--model" not in sys.argv or sys.argv[sys.argv.index("--model") + 1] != "claude-sonnet-4-6":
    print("missing selected model", file=sys.stderr)
    raise SystemExit(2)

conversation_id = "agy-test-7"
if "--conversation" in sys.argv:
    conversation_id = sys.argv[sys.argv.index("--conversation") + 1]
print(json.dumps({"event": "init", "conversation_id": conversation_id, "init": {}}), flush=True)
turns = 0
for line in sys.stdin:
    message = json.loads(line)
    if message.get("event") != "user":
        continue
    turns += 1
    print(json.dumps({"event": "step_update", "step_update": {
        "conversation_id": conversation_id, "step_index": turns,
        "state": "DONE", "step_type": "agent_response", "text_delta": "working"
    }}), flush=True)
    print(json.dumps({"event": "result", "result": {
        "conversation_id": conversation_id, "status": "SUCCESS",
        "response": "answer-%d" % turns
    }}), flush=True)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    std::env::set_var("ECKY_AGY_BIN", &executable);
    let supervisor = AgyProviderSupervisor::new();
    let first = supervisor
        .start_new_turn(
            directory.to_str().unwrap(),
            "first",
            Some("claude-sonnet-4-6"),
            Some("http://127.0.0.1:39249/mcp?providerThreadId=thread-1"),
        )
        .await
        .unwrap();
    assert_eq!(first.conversation_id, "agy-test-7");
    let first_result = tokio::time::timeout(std::time::Duration::from_secs(2), first.result)
        .await
        .expect("result arrives while process remains open")
        .unwrap()
        .unwrap();
    assert_eq!(first_result.response, "answer-1");
    assert_eq!(first_result.eval_events.len(), 2);
    assert_eq!(first_result.eval_events[0].sequence, 1);
    assert_eq!(first_result.eval_events[0].kind, EvalEventKind::Assistant);
    assert_eq!(first_result.eval_events[1].sequence, 2);
    assert_eq!(first_result.eval_events[1].kind, EvalEventKind::Result);
    assert!(first_result.completed_at >= first_result.started_at);
    let first_traces = supervisor.turn_traces("agy-test-7").await;
    assert_eq!(first_traces.len(), 1);
    assert_eq!(first_traces[0].status, "success");
    assert_eq!(first_traces[0].messages[0].content, "working");
    assert_eq!(
        first_traces[0].messages[0].provider_event_kind,
        Some(ecky_cad_lib::contracts::ProviderEventKind::Assistant)
    );

    let second = supervisor
        .start_turn(
            "agy-test-7",
            directory.to_str().unwrap(),
            "second",
            Some("claude-sonnet-4-6"),
            Some("http://127.0.0.1:39249/mcp?providerThreadId=thread-1"),
        )
        .await
        .unwrap();
    let second_result = tokio::time::timeout(std::time::Duration::from_secs(2), second.result)
        .await
        .expect("second result reuses warm process")
        .unwrap()
        .unwrap();
    assert_eq!(second_result.response, "answer-2");
    assert_eq!(supervisor.turn_traces("agy-test-7").await.len(), 2);
    assert_eq!(supervisor.runtime("agy-test-7").await.phase, "idle");

    let after_endpoint_change = supervisor
        .start_turn(
            "agy-test-7",
            directory.to_str().unwrap(),
            "third",
            Some("claude-sonnet-4-6"),
            Some("http://127.0.0.1:39250/mcp?providerThreadId=thread-1"),
        )
        .await
        .unwrap();
    let third_result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        after_endpoint_change.result,
    )
    .await
    .expect("endpoint change respawns and resumes the provider")
    .unwrap()
    .unwrap();
    assert_eq!(third_result.response, "answer-1");

    std::env::remove_var("ECKY_AGY_BIN");
    let _ = std::fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[tokio::test]
async fn bound_conversation_activates_without_sending_a_fake_turn() {
    use std::os::unix::fs::PermissionsExt;
    let _environment = AGY_ENV_LOCK.lock().await;

    let directory =
        std::env::temp_dir().join(format!("ecky-agy-owner-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("agy-owner-fake.py");
    let prompts = directory.join("prompts");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json
import os
import sys

if "--version" in sys.argv:
    print("Antigravity CLI 1.1.15")
    raise SystemExit(0)

conversation_id = sys.argv[sys.argv.index("--conversation") + 1]
print(json.dumps({"event": "init", "conversation_id": conversation_id, "init": {}}), flush=True)
for line in sys.stdin:
    with open(os.environ["ECKY_AGY_TEST_PROMPTS"], "a", encoding="utf-8") as output:
        output.write(line)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    std::env::set_var("ECKY_AGY_BIN", &executable);
    std::env::set_var("ECKY_AGY_TEST_PROMPTS", &prompts);
    let supervisor = AgyProviderSupervisor::new();
    supervisor
        .activate_conversation("agy-owned-8", directory.to_str().unwrap(), None)
        .await
        .unwrap();
    assert_eq!(supervisor.runtime("agy-owned-8").await.phase, "idle");
    assert!(
        !prompts.exists(),
        "activation must not synthesize a user turn"
    );

    std::env::remove_var("ECKY_AGY_BIN");
    std::env::remove_var("ECKY_AGY_TEST_PROMPTS");
    let _ = std::fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[tokio::test]
async fn stop_normalizes_provider_timeout_and_resumes_next_turn_in_fresh_process() {
    use std::os::unix::fs::PermissionsExt;
    let _environment = AGY_ENV_LOCK.lock().await;

    let directory =
        std::env::temp_dir().join(format!("ecky-agy-stop-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("agy-stop-fake.py");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json
import signal
import sys
import time

if "--version" in sys.argv:
    print("Antigravity CLI 1.1.15")
    raise SystemExit(0)

conversation_id = "agy-stop-7"
print(json.dumps({"event": "init", "conversation_id": conversation_id, "init": {}}), flush=True)

if "--conversation" in sys.argv:
    for line in sys.stdin:
        json.loads(line)
        print(json.dumps({"event": "result", "result": {
            "conversation_id": conversation_id,
            "status": "SUCCESS",
            "response": "queued turn delivered"
        }}), flush=True)
    raise SystemExit(0)

def interrupted(_signal, _frame):
    print(json.dumps({"event": "result", "result": {
        "conversation_id": conversation_id,
        "status": "ERROR",
        "response": "",
        "error": "timeout waiting for response"
    }}), flush=True)
    raise SystemExit(130)

signal.signal(signal.SIGINT, interrupted)
for line in sys.stdin:
    json.loads(line)
    print(json.dumps({"event": "step_update", "step_update": {
        "conversation_id": conversation_id, "step_index": 1,
        "state": "ACTIVE", "step_type": "tool", "tool_name": "ecky_ast_inspect"
    }}), flush=True)
    time.sleep(30)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    std::env::set_var("ECKY_AGY_BIN", &executable);
    let supervisor = AgyProviderSupervisor::new();
    let started = supervisor
        .start_new_turn(directory.to_str().unwrap(), "work", None, None)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while supervisor
            .live_messages(&started.conversation_id)
            .await
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("public event arrives before interrupt");
    supervisor
        .stop_turn(&started.conversation_id, &started.turn_id)
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), started.result)
        .await
        .expect("SIGINT produces terminal result")
        .unwrap()
        .unwrap();
    assert_eq!(result.status, "INTERRUPTED");
    assert_eq!(result.error, None);
    let traces = supervisor.turn_traces("agy-stop-7").await;
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].status, "interrupted");
    assert_eq!(
        traces[0].messages[0].content,
        "USING TOOL · ecky_ast_inspect"
    );

    let resumed = supervisor
        .start_turn(
            "agy-stop-7",
            directory.to_str().unwrap(),
            "queued work",
            None,
            None,
        )
        .await
        .unwrap();
    let resumed_result = tokio::time::timeout(std::time::Duration::from_secs(2), resumed.result)
        .await
        .expect("queued turn starts after STOP")
        .unwrap()
        .unwrap();
    assert_eq!(resumed_result.status, "SUCCESS");
    assert_eq!(resumed_result.response, "queued turn delivered");

    std::env::remove_var("ECKY_AGY_BIN");
    let _ = std::fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[tokio::test]
async fn app_shutdown_stops_the_owned_agy_process_group_including_descendants() {
    use std::os::unix::fs::PermissionsExt;
    let _environment = AGY_ENV_LOCK.lock().await;

    let directory =
        std::env::temp_dir().join(format!("ecky-agy-shutdown-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("agy-shutdown-fake.py");
    let process_ids = directory.join("process-ids");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json
import os
import subprocess
import sys
import time

if "--version" in sys.argv:
    print("Antigravity CLI 1.1.15")
    raise SystemExit(0)

conversation_id = "agy-shutdown-7"
descendant = subprocess.Popen(["sleep", "30"])
with open(os.environ["ECKY_AGY_TEST_PROCESS_IDS"], "w", encoding="utf-8") as output:
    output.write("%d %d" % (os.getpid(), descendant.pid))
print(json.dumps({"event": "init", "conversation_id": conversation_id, "init": {}}), flush=True)
for line in sys.stdin:
    json.loads(line)
    time.sleep(30)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    std::env::set_var("ECKY_AGY_BIN", &executable);
    std::env::set_var("ECKY_AGY_TEST_PROCESS_IDS", &process_ids);
    let supervisor = AgyProviderSupervisor::new();
    let started = supervisor
        .start_new_turn(directory.to_str().unwrap(), "work", None, None)
        .await
        .unwrap();
    let ids = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if let Ok(raw) = std::fs::read_to_string(&process_ids) {
                break raw;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fake provider records its process tree");
    let ids = ids
        .split_whitespace()
        .map(|value| value.parse::<i32>().unwrap())
        .collect::<Vec<_>>();

    supervisor.shutdown_all().await;
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), started.result)
        .await
        .expect("shutdown resolves the active turn")
        .unwrap()
        .expect_err("shutdown cannot report success");
    assert!(result.message.contains("Ecky shutdown"));
    let failed_eval = supervisor
        .take_failed_turn_result("agy-shutdown-7", &started.turn_id)
        .await
        .expect("failed turns retain eval evidence for durable finalization");
    assert_eq!(failed_eval.status, "ERROR");
    assert!(failed_eval.completed_at >= failed_eval.started_at);
    for pid in ids {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("process {pid} survived"));
    }

    std::env::remove_var("ECKY_AGY_BIN");
    std::env::remove_var("ECKY_AGY_TEST_PROCESS_IDS");
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn agy_provider_compaction_rotates_external_id_and_retains_message_history() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE threads (
             id TEXT PRIMARY KEY,
             title TEXT NOT NULL,
             summary TEXT NOT NULL DEFAULT '',
             updated_at INTEGER NOT NULL
         );
         INSERT INTO threads (id, title, updated_at) VALUES ('ecky-1', 'One', 1);",
    )
    .unwrap();
    ecky_cad_lib::services::codex_takeover::ensure_schema(&conn).unwrap();

    let initial = ecky_cad_lib::services::agy_provider::bind_owned_conversation(
        &conn,
        "ecky-1",
        "agy-conv-1",
        "One",
        "/workspace/one",
        100,
    )
    .unwrap();
    assert_eq!(initial.agy_conversation_id, "agy-conv-1");

    // Add some messages under agy-conv-1
    for i in 1..=5 {
        ecky_cad_lib::services::agy_provider::insert_message(
            &conn,
            "ecky-1",
            "agy-conv-1",
            if i % 2 == 1 { "user" } else { "assistant" },
            &format!("Message {i}"),
            "success",
            100 + i,
        )
        .unwrap();
    }

    assert_eq!(
        ecky_cad_lib::services::agy_provider::count_conversation_messages(
            &conn,
            "ecky-1",
            "agy-conv-1"
        )
        .unwrap(),
        5
    );

    // Rotate binding (compaction)
    let rotated = ecky_cad_lib::services::agy_provider::rotate_binding(
        &conn,
        "ecky-1",
        "agy-conv-2",
        "compaction",
        200,
    )
    .unwrap();
    assert_eq!(rotated.agy_conversation_id, "agy-conv-2");

    // Conversation message count for new conversation starts at 0
    assert_eq!(
        ecky_cad_lib::services::agy_provider::count_conversation_messages(
            &conn,
            "ecky-1",
            "agy-conv-2"
        )
        .unwrap(),
        0
    );

    // Old conversation message count is still 5
    assert_eq!(
        ecky_cad_lib::services::agy_provider::count_conversation_messages(
            &conn,
            "ecky-1",
            "agy-conv-1"
        )
        .unwrap(),
        5
    );

    // Insert compaction message into agy-conv-2
    ecky_cad_lib::services::agy_provider::insert_message(
        &conn,
        "ecky-1",
        "agy-conv-2",
        "assistant",
        "Session compacted. Earlier history is available via thread_messages_get.",
        "success",
        201,
    )
    .unwrap();

    // The whole message page for ecky-1 has all 6 messages
    let page = ecky_cad_lib::services::agy_provider::message_page(&conn, "ecky-1", None).unwrap();
    assert_eq!(page.messages.len(), 6);
    assert_eq!(
        page.messages.last().unwrap().content,
        "Session compacted. Earlier history is available via thread_messages_get."
    );

    // Current binding for ecky-1 points to agy-conv-2
    let current = ecky_cad_lib::services::agy_provider::get_binding(&conn, "ecky-1")
        .unwrap()
        .unwrap();
    assert_eq!(current.agy_conversation_id, "agy-conv-2");
}

#[test]
fn stream_projection_formats_rich_details_from_parameters_and_native_tools() {
    // 1. MCP tool with escaped quotes and specific toolSummary over generic toolAction
    let mcp_summary = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 10,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "call_mcp_tool",
            "tool_info": {
                "name": "call_mcp_tool",
                "parameters": {
                    "ServerName": "\"ecky_mcp\"",
                    "ToolName": "\"macro_buffer_replace_range\"",
                    "Arguments": "{\"startLine\": 1805, \"endLine\": 1820}",
                    "toolAction": "\"Calling MCP tool\"",
                    "toolSummary": "\"Replace broken if with direct part invocation\""
                }
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        mcp_summary,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 10,
            text: "WORKING · Replace broken if with direct part invocation".to_string(),
        }
    );

    // 2. MCP tool with line range detail when summary is absent
    let mcp_detail = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 11,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "call_mcp_tool",
            "tool_info": {
                "name": "call_mcp_tool",
                "parameters": {
                    "ServerName": "\"ecky_mcp\"",
                    "ToolName": "\"target_macro_get\"",
                    "Arguments": { "startLine": 1800, "endLine": 1840 }
                }
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        mcp_detail,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 11,
            text: "USING TOOL · ecky_mcp/target_macro_get (lines 1800-1840)".to_string(),
        }
    );

    // 3. Native run_command detail
    let cmd = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 12,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "run_command",
            "tool_info": {
                "name": "run_command",
                "parameters": {
                    "CommandLine": "git status --porcelain"
                }
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        cmd,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 12,
            text: "RUNNING · git status --porcelain".to_string(),
        }
    );

    // 4. Native view_file with line numbers
    let view = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 13,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "view_file",
            "tool_info": {
                "name": "view_file",
                "parameters": {
                    "AbsolutePath": "/Users/bogdan/projects/model.ecky",
                    "StartLine": 1800,
                    "EndLine": 1840
                }
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        view,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 13,
            text: "VIEWING · model.ecky:1800-1840".to_string(),
        }
    );

    // 5. Native replace_file_content with lines
    let edit = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 14,
            "state": "ACTIVE",
            "step_type": "tool",
            "tool_name": "replace_file_content",
            "tool_info": {
                "name": "replace_file_content",
                "parameters": {
                    "TargetFile": "/Users/bogdan/projects/model.ecky",
                    "StartLine": 1805,
                    "EndLine": 1820
                }
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        edit,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 14,
            text: "EDITING · model.ecky (lines 1805-1820)".to_string(),
        }
    );

    // 6. Tool failure carrying raw error detail
    let failed = project_stream_event(&json!({
        "event": "step_update",
        "step_update": {
            "conversation_id": "agy-7",
            "step_index": 15,
            "state": "ERROR",
            "step_type": "tool",
            "tool_name": "call_mcp_tool",
            "tool_info": {
                "name": "call_mcp_tool",
                "parameters": {
                    "ServerName": "ecky_mcp",
                    "ToolName": "session_log_in"
                },
                "error": "No bound MCP session target is available."
            }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        failed,
        AgyProjectedEvent::Working {
            conversation_id: "agy-7".to_string(),
            step_index: 15,
            text: "FAILED · ecky_mcp/session_log_in: No bound MCP session target is available."
                .to_string(),
        }
    );

    // 7. Eval projection from real Agy parameters payload retains input and resolves tool name
    let eval_event = project_agy_eval_event(
        &json!({
            "event": "step_update",
            "step_update": {
                "conversation_id": "agy-7",
                "step_index": 16,
                "state": "ACTIVE",
                "step_type": "tool",
                "tool_name": "call_mcp_tool",
                "tool_info": {
                    "name": "call_mcp_tool",
                    "parameters": {
                        "ServerName": "\"ecky_mcp\"",
                        "ToolName": "\"target_macro_get\"",
                        "Arguments": { "startLine": 1800, "endLine": 1840 }
                    }
                }
            }
        }),
        456,
    )
    .unwrap();
    assert_eq!(eval_event.kind, EvalEventKind::Tool);
    assert_eq!(
        eval_event.name.as_deref(),
        Some("ecky_mcp/target_macro_get")
    );
    assert_eq!(
        eval_event.summary.as_deref(),
        Some("USING TOOL · ecky_mcp/target_macro_get (lines 1800-1840)")
    );
    assert!(eval_event.input.is_some());
}
