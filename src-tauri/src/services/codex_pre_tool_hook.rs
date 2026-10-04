use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;

const MAX_HOOK_INPUT_BYTES: u64 = 1_048_576;

pub fn runtime_descriptor_path() -> PathBuf {
    std::env::temp_dir().join("ecky/codex-pre-tool-hook.json")
}

pub fn write_runtime_descriptor(base_url: &str) -> std::io::Result<String> {
    let token = uuid::Uuid::new_v4().to_string();
    let callback_url = format!(
        "{}/codex-pre-tool-hook/{}",
        base_url.trim_end_matches('/'),
        token
    );
    write_runtime_descriptor_at(&runtime_descriptor_path(), &callback_url, &token)?;
    Ok(token)
}

pub fn write_runtime_descriptor_at(
    path: &PathBuf,
    callback_url: &str,
    token: &str,
) -> std::io::Result<()> {
    let parent = path.parent().expect("descriptor path parent");
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(
        &temporary,
        serde_json::to_vec(&json!({"endpoint": callback_url, "token": token}))
            .map_err(std::io::Error::other)?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(temporary, path)?;
    Ok(())
}

pub fn runtime_descriptor() -> std::io::Result<Value> {
    runtime_descriptor_at(&runtime_descriptor_path())
}

pub fn runtime_descriptor_at(path: &PathBuf) -> std::io::Result<Value> {
    let raw = std::fs::read(path)?;
    serde_json::from_slice(&raw).map_err(std::io::Error::other)
}

/// Entry point used by the Ecky executable when Codex invokes its registered
/// PreToolUse command hook. Any parse/network failure emits a deny decision.
pub fn maybe_run_from_process_args() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--ecky-codex-pre-tool-hook") {
        return None;
    }
    let endpoint = runtime_descriptor()
        .ok()
        .and_then(|descriptor| {
            descriptor
                .get("endpoint")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let mut body = Vec::new();
    let response = if endpoint.starts_with("http://127.0.0.1:")
        && endpoint.contains("/codex-pre-tool-hook/")
    {
        std::io::stdin()
            .take(MAX_HOOK_INPUT_BYTES + 1)
            .read_to_end(&mut body)
            .ok()
            .filter(|_| body.len() as u64 <= MAX_HOOK_INPUT_BYTES)
            .and_then(|_| {
                let request: Value = serde_json::from_slice(&body).ok()?;
                if request.get("hook_event_name").and_then(Value::as_str) != Some("PreToolUse")
                    || request.get("session_id").and_then(Value::as_str).is_none()
                    || request.get("tool_name").and_then(Value::as_str).is_none()
                {
                    return None;
                }
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()?;
                runtime.block_on(async move {
                    reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(3))
                        .build()
                        .ok()?
                        .post(endpoint)
                        .json(&request)
                        .send()
                        .await
                        .ok()?
                        .error_for_status()
                        .ok()?
                        .json::<Value>()
                        .await
                        .ok()
                        .filter(valid_pre_tool_response)
                })
            })
    } else {
        None
    };
    let Some(output) = response else {
        let _ = writeln!(
            std::io::stderr(),
            "Ecky hook authorization failed; denying tool call."
        );
        return Some(2);
    };
    if std::io::stdout()
        .write_all(output.to_string().as_bytes())
        .is_err()
        || std::io::stdout().write_all(b"\n").is_err()
    {
        return Some(2);
    }
    Some(0)
}

fn valid_pre_tool_response(response: &Value) -> bool {
    response.get("hookSpecificOutput").is_some_and(|specific| {
        specific.get("hookEventName").and_then(Value::as_str) == Some("PreToolUse")
            && matches!(
                specific.get("permissionDecision").and_then(Value::as_str),
                Some("allow" | "deny")
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DENY: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Ecky authorization unavailable; tool call denied."}}"#;

    #[test]
    fn fail_closed_payload_is_native_pre_tool_deny() {
        let payload: Value = serde_json::from_str(DENY).unwrap();
        assert_eq!(payload["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(payload["hookSpecificOutput"]["permissionDecision"], "deny");
    }

    #[test]
    fn malformed_callback_responses_are_not_success() {
        for response in [
            json!({}),
            json!({"hookSpecificOutput": {"permissionDecision": "allow"}}),
            json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "permissionDecision": "allow"}}),
            json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "ask"}}),
        ] {
            assert!(!valid_pre_tool_response(&response));
        }
        assert!(valid_pre_tool_response(
            &serde_json::from_str(DENY).unwrap()
        ));
    }

    #[test]
    fn runtime_descriptor_keeps_url_and_token_out_of_hook_definition() {
        let directory =
            std::env::temp_dir().join(format!("ecky-hook-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("descriptor.json");
        let token = uuid::Uuid::new_v4().to_string();
        let endpoint = format!("http://127.0.0.1:39249/codex-pre-tool-hook/{token}");
        write_runtime_descriptor_at(&path, &endpoint, &token).unwrap();
        let descriptor = runtime_descriptor_at(&path).unwrap();
        assert_eq!(descriptor["token"], token);
        assert_eq!(descriptor["endpoint"], endpoint);
        let _ = std::fs::remove_dir_all(directory);
    }
}
