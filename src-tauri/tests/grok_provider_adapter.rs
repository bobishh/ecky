use std::time::Duration;

use ecky_cad_lib::services::grok_provider::{GrokProviderSupervisor, GrokSessionRequest};

static GROK_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(unix)]
#[tokio::test]
async fn acp_session_streams_the_assistant_message_and_passes_the_ecky_endpoint() {
    use std::os::unix::fs::PermissionsExt;
    let _environment = GROK_ENV_LOCK.lock().await;
    let directory =
        std::env::temp_dir().join(format!("ecky-grok-acp-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("grok-fake.py");
    let args_file = directory.join("args.txt");
    let session_file = directory.join("session.json");
    let prompt_file = directory.join("prompt.txt");
    std::fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json
import os
import sys

if "version" in sys.argv:
    print("grok test")
    raise SystemExit(0)

with open(os.environ["ECKY_GROK_TEST_ARGS"], "w", encoding="utf-8") as output:
    output.write("\n".join(sys.argv[1:]))

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    request_id = message.get("id")
    if method == "session/new":
        with open(os.environ["ECKY_GROK_TEST_SESSION"], "w", encoding="utf-8") as output:
            json.dump(message.get("params") or {}, output)
        result = {"sessionId": "sess-1"}
    elif method == "session/prompt":
        text = ""
        prompt = (message.get("params") or {}).get("prompt") or []
        if prompt:
            text = prompt[0].get("text") or ""
        with open(os.environ["ECKY_GROK_TEST_PROMPT"], "w", encoding="utf-8") as output:
            output.write(text)
        print(json.dumps({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "hello"},
                }
            },
        }), flush=True)
        result = {"stopReason": "end_turn"}
    elif method == "initialize":
        result = {"protocolVersion": 1}
    elif request_id is None:
        continue
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    let previous_bin = std::env::var_os("ECKY_GROK_BIN");
    let previous_args = std::env::var_os("ECKY_GROK_TEST_ARGS");
    let previous_session = std::env::var_os("ECKY_GROK_TEST_SESSION");
    let previous_prompt = std::env::var_os("ECKY_GROK_TEST_PROMPT");
    std::env::set_var("ECKY_GROK_BIN", &executable);
    std::env::set_var("ECKY_GROK_TEST_ARGS", &args_file);
    std::env::set_var("ECKY_GROK_TEST_SESSION", &session_file);
    std::env::set_var("ECKY_GROK_TEST_PROMPT", &prompt_file);

    let supervisor = GrokProviderSupervisor::new();
    let outcome = async {
        let session_id = supervisor
            .ensure_session(GrokSessionRequest {
                cwd: directory.to_string_lossy().into_owned(),
                model: Some("grok-4.6".into()),
                mcp_url: "http://127.0.0.1:9/mcp?providerThreadId=thread-1".into(),
                resume_session_id: None,
            })
            .await?;
        assert_eq!(session_id, "sess-1");
        let started = supervisor.prompt(&session_id, "draw a 10mm cube").await?;
        let result = tokio::time::timeout(Duration::from_secs(5), started.result)
            .await
            .expect("Grok turn timed out")
            .expect("Grok turn channel closed")
            .expect("Grok turn failed");
        assert_eq!(result.status, "SUCCESS");
        assert_eq!(result.response, "hello");
        Ok::<(), ecky_cad_lib::contracts::AppError>(())
    }
    .await;
    supervisor.shutdown_all().await;

    match previous_bin {
        Some(value) => std::env::set_var("ECKY_GROK_BIN", value),
        None => std::env::remove_var("ECKY_GROK_BIN"),
    }
    match previous_args {
        Some(value) => std::env::set_var("ECKY_GROK_TEST_ARGS", value),
        None => std::env::remove_var("ECKY_GROK_TEST_ARGS"),
    }
    match previous_session {
        Some(value) => std::env::set_var("ECKY_GROK_TEST_SESSION", value),
        None => std::env::remove_var("ECKY_GROK_TEST_SESSION"),
    }
    match previous_prompt {
        Some(value) => std::env::set_var("ECKY_GROK_TEST_PROMPT", value),
        None => std::env::remove_var("ECKY_GROK_TEST_PROMPT"),
    }

    outcome.expect("fake Grok ACP session");
    let args = std::fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("agent"));
    assert!(args.contains("--always-approve"));
    assert!(args.contains("--model"));
    assert!(args.contains("grok-4.6"));
    assert!(args.contains("stdio"));
    let session = std::fs::read_to_string(&session_file).unwrap();
    assert!(session.contains("providerThreadId=thread-1"));
    assert!(session.contains(&directory.to_string_lossy().into_owned()));
    assert_eq!(
        std::fs::read_to_string(&prompt_file).unwrap(),
        "draw a 10mm cube"
    );
    let _ = std::fs::remove_dir_all(&directory);
}
