use ecky_cad_lib::contracts::{Attachment, AttachmentKind, CodexDialogueMessage};
use ecky_cad_lib::services::codex_takeover::{
    bind_owned_thread, build_provider_handoff_summary, claim_queue_item, complete_queue_item,
    defer_queue_item, enqueue_prompt, enqueue_prompt_with_attachments, ensure_schema,
    fail_queue_item, get_agent_binding_for_provider, get_binding, list_binding_lineage,
    list_provider_messages, list_queue, mark_queue_sending, pending_queue_bindings,
    persist_finished_provider_messages, persist_provider_turn_user_input, provider_message_page,
    recover_retryable_failures, recover_stale_sending, remove_queue_item, retry_queue_item,
    upsert_agent_binding, AgentThreadBindingRecord,
};
use rusqlite::{params, Connection};
use std::sync::Mutex;

use ecky_cad_lib::services::agy_provider::{
    reconcile_stale_deliveries, record_process_lease, AgyProcessIdentity, AgyProcessReapOutcome,
    AgyProcessReaper, StaleAgyDelivery,
};

fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE threads (
             id TEXT PRIMARY KEY,
             title TEXT NOT NULL,
             summary TEXT NOT NULL DEFAULT '',
             updated_at INTEGER NOT NULL
         );",
    )
    .unwrap();
    ensure_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO threads (id, title, updated_at) VALUES (?1, ?2, 1)",
        params!["ecky-1", "One"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads (id, title, updated_at) VALUES (?1, ?2, 1)",
        params!["ecky-2", "Two"],
    )
    .unwrap();
    conn
}

#[test]
fn legacy_single_provider_schema_migrates_without_losing_codex_binding_or_fifo() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, summary TEXT NOT NULL DEFAULT '', updated_at INTEGER NOT NULL);
         INSERT INTO threads (id, title, updated_at) VALUES ('ecky-1', 'One', 1);
         CREATE TABLE agent_thread_bindings (
            ecky_thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
            provider TEXT NOT NULL, external_thread_id TEXT NOT NULL, external_title TEXT NOT NULL,
            external_cwd TEXT NOT NULL, bootstrap_version INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
            UNIQUE(provider, external_thread_id)
         );
         CREATE TABLE agent_prompt_queue (
            id TEXT PRIMARY KEY,
            ecky_thread_id TEXT NOT NULL REFERENCES agent_thread_bindings(ecky_thread_id) ON DELETE CASCADE,
            prompt_text TEXT NOT NULL, status TEXT NOT NULL, error TEXT,
            created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         INSERT INTO agent_thread_bindings VALUES ('ecky-1', 'codex', 'codex-7', 'One', '/workspace', 3, 10, 10);
         INSERT INTO agent_prompt_queue VALUES ('queue-1', 'ecky-1', 'keep me', 'queued', NULL, 11, 11);",
    )
    .unwrap();

    ensure_schema(&conn).unwrap();
    assert_eq!(
        get_binding(&conn, "ecky-1")
            .unwrap()
            .unwrap()
            .codex_thread_id,
        "codex-7"
    );
    assert_eq!(
        list_queue(&conn, "ecky-1").unwrap()[0].prompt_text,
        "keep me"
    );
    upsert_agent_binding(
        &conn,
        &AgentThreadBindingRecord {
            ecky_thread_id: "ecky-1".to_string(),
            provider: "agy".to_string(),
            external_thread_id: "agy-8".to_string(),
            external_title: "One".to_string(),
            external_cwd: "/workspace".to_string(),
            bootstrap_version: 1,
            created_at: 12,
            updated_at: 12,
        },
    )
    .unwrap();
}

#[test]
fn provider_namespace_allows_one_owned_conversation_per_provider() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();

    let agy = upsert_agent_binding(
        &conn,
        &AgentThreadBindingRecord {
            ecky_thread_id: "ecky-1".to_string(),
            provider: "agy".to_string(),
            external_thread_id: "codex-7".to_string(),
            external_title: "Agy gearbox".to_string(),
            external_cwd: "/workspace/gearbox".to_string(),
            bootstrap_version: 1,
            created_at: 110,
            updated_at: 110,
        },
    )
    .unwrap();
    assert_eq!(agy.provider, "agy");
    assert_eq!(
        get_agent_binding_for_provider(&conn, "ecky-1", "agy").unwrap(),
        Some(agy)
    );
    assert_eq!(
        get_agent_binding_for_provider(&conn, "ecky-1", "codex")
            .unwrap()
            .unwrap()
            .external_thread_id,
        "codex-7"
    );

    let conflicting = AgentThreadBindingRecord {
        ecky_thread_id: "ecky-2".to_string(),
        provider: "agy".to_string(),
        external_thread_id: "codex-7".to_string(),
        external_title: "Other".to_string(),
        external_cwd: "/workspace/other".to_string(),
        bootstrap_version: 1,
        created_at: 120,
        updated_at: 120,
    };
    let error = upsert_agent_binding(&conn, &conflicting).unwrap_err();
    assert!(error
        .message
        .contains("already owned by Ecky thread ecky-1"));
}

#[test]
fn binding_is_idempotent_for_same_pair_and_conflicts_for_second_ecky_thread() {
    let conn = connection();
    let first = bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let repeated = bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        200,
    )
    .unwrap();
    assert_eq!(first.codex_thread_id, repeated.codex_thread_id);
    assert_eq!(get_binding(&conn, "ecky-1").unwrap(), Some(repeated));

    let error = bind_owned_thread(
        &conn,
        "ecky-2",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        300,
    )
    .unwrap_err();
    assert!(error.message.contains("ecky-1"));
    assert!(get_binding(&conn, "ecky-2").unwrap().is_none());
}

#[test]
fn owned_binding_uses_ecky_identity_without_importing_foreign_metadata() {
    let conn = connection();
    let binding = bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-created-1",
        "Gearbox housing",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    assert_eq!(binding.codex_thread_id, "codex-created-1");
    assert_eq!(binding.label, "Gearbox housing");
    assert_eq!(binding.cwd, "/workspace/gearbox");
}

#[test]
fn provider_handoff_merges_canonical_target_with_recent_provider_dialogue() {
    let summary = build_provider_handoff_summary(
        "Thread: Gearbox housing\nCurrent version anchor: housing-v4",
        &[
            CodexDialogueMessage {
                id: "u1".into(),
                role: "user".into(),
                content: "Keep walls at 3 mm".into(),
                status: "success".into(),
                timestamp: 1,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
            CodexDialogueMessage {
                id: "a1".into(),
                role: "assistant".into(),
                content: "Added four symmetric ribs".into(),
                status: "success".into(),
                timestamp: 2,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
        ],
    );
    assert!(summary.contains("Current version anchor: housing-v4"));
    assert!(summary.contains("CODEX USER: Keep walls at 3 mm"));
    assert!(summary.contains("CODEX ASSISTANT: Added four symmetric ribs"));
}

#[test]
fn provider_handoff_reenters_existing_api_and_mcp_context_assembler() {
    let conn = connection();
    let handoff = build_provider_handoff_summary(
        "Thread: Gearbox housing",
        &[CodexDialogueMessage {
            id: "a1".into(),
            role: "assistant".into(),
            content: "Current target has four symmetric ribs".into(),
            status: "success".into(),
            timestamp: 2,
            attachments: Vec::new(),
            provider_event_kind: None,
        }],
    );
    ecky_cad_lib::db::update_thread_summary(&conn, "ecky-1", &handoff).unwrap();
    let context =
        ecky_cad_lib::context::assemble_context(&conn, Some("ecky-1".to_string()), None, None);
    assert!(context
        .summary
        .contains("Current target has four symmetric ribs"));
}

#[test]
fn writer_conflict_retry_preserves_binding_and_finished_history() {
    let conn = connection();
    let first = bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-old",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    persist_finished_provider_messages(
        &conn,
        "ecky-1",
        "codex",
        &first.codex_thread_id,
        &[
            CodexDialogueMessage {
                id: "codex:old:turn-1:user:0".into(),
                role: "user".into(),
                content: "Keep the 3 mm walls.".into(),
                status: "success".into(),
                timestamp: 101,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
            CodexDialogueMessage {
                id: "codex:old:turn-1:assistant:a1".into(),
                role: "assistant".into(),
                content: "Walls retained.".into(),
                status: "success".into(),
                timestamp: 102,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
            CodexDialogueMessage {
                id: "codex:old:turn-2:assistant:a2".into(),
                role: "assistant".into(),
                content: "Still working.".into(),
                status: "pending".into(),
                timestamp: 103,
                attachments: Vec::new(),
                provider_event_kind: None,
            },
        ],
    )
    .unwrap();

    let queued = enqueue_prompt(&conn, "ecky-1", "Continue after lock", 200).unwrap();
    mark_queue_sending(&conn, &queued.id, 201).unwrap();
    defer_queue_item(
        &conn,
        &queued.id,
        "thread codex-old already has an active writer",
        204,
    )
    .unwrap();

    let binding = get_binding(&conn, "ecky-1").unwrap().unwrap();
    assert_eq!(binding.codex_thread_id, "codex-old");
    assert_eq!(
        list_binding_lineage(&conn, "ecky-1", "codex")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        list_provider_messages(&conn, "ecky-1", "codex", 30)
            .unwrap()
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        vec!["Keep the 3 mm walls.", "Walls retained."]
    );
    assert!(pending_queue_bindings(&conn, 203).unwrap().is_empty());
    assert_eq!(pending_queue_bindings(&conn, 204).unwrap(), vec![first]);
    assert!(claim_queue_item(&conn, &queued.id, 205).unwrap());
    complete_queue_item(&conn, &queued.id).unwrap();
    persist_provider_turn_user_input(
        &conn,
        "ecky-1",
        "codex",
        "codex-old",
        "turn-2",
        "Continue after lock",
        &[],
        206,
    )
    .unwrap();
    assert_eq!(
        get_binding(&conn, "ecky-1")
            .unwrap()
            .unwrap()
            .codex_thread_id,
        "codex-old"
    );
}

#[test]
fn durable_provider_history_is_cursor_paged_without_provider_io() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let messages = (0..35)
        .map(|index| CodexDialogueMessage {
            id: format!("codex:turn-{index}:assistant"),
            role: "assistant".into(),
            content: format!("finished-{index}"),
            status: "success".into(),
            timestamp: 1_000 + index,
            attachments: Vec::new(),
            provider_event_kind: None,
        })
        .collect::<Vec<_>>();
    persist_finished_provider_messages(&conn, "ecky-1", "codex", "codex-7", &messages).unwrap();

    let latest = provider_message_page(&conn, "ecky-1", "codex", None).unwrap();
    assert_eq!(latest.messages.len(), 30);
    assert_eq!(latest.messages.first().unwrap().content, "finished-5");
    let older =
        provider_message_page(&conn, "ecky-1", "codex", latest.next_cursor.as_deref()).unwrap();
    assert_eq!(
        older
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        vec![
            "finished-0",
            "finished-1",
            "finished-2",
            "finished-3",
            "finished-4"
        ]
    );
    assert_eq!(older.next_cursor, None);
}

#[test]
fn steer_input_is_durable_immediately_and_final_backfill_deduplicates_exact_turn_ordinal() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    persist_finished_provider_messages(
        &conn,
        "ecky-1",
        "codex",
        "codex-7",
        &[CodexDialogueMessage {
            id: "codex:codex-7:turn-1:user:0".into(),
            role: "user".into(),
            content: "Initial prompt".into(),
            status: "success".into(),
            timestamp: 101,
            attachments: Vec::new(),
            provider_event_kind: None,
        }],
    )
    .unwrap();

    let steer = persist_provider_turn_user_input(
        &conn,
        "ecky-1",
        "codex",
        "codex-7",
        "turn-1",
        "ж?",
        &[],
        105,
    )
    .unwrap();
    assert_eq!(steer.id, "codex:codex-7:turn-1:user:1");
    assert_eq!(
        list_provider_messages(&conn, "ecky-1", "codex", 30)
            .unwrap()
            .len(),
        2
    );

    persist_finished_provider_messages(
        &conn,
        "ecky-1",
        "codex",
        "codex-7",
        &[CodexDialogueMessage {
            id: steer.id,
            role: "user".into(),
            content: "ж?".into(),
            status: "success".into(),
            timestamp: 101,
            attachments: Vec::new(),
            provider_event_kind: None,
        }],
    )
    .unwrap();
    let messages = list_provider_messages(&conn, "ecky-1", "codex", 30).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].content, "ж?");
}

#[test]
fn queue_preserves_fifo_and_recovers_stale_sending_rows() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let first = enqueue_prompt(&conn, "ecky-1", "first", 110).unwrap();
    let second = enqueue_prompt(&conn, "ecky-1", "second", 120).unwrap();
    conn.execute(
        "UPDATE agent_prompt_queue SET status = 'sending' WHERE id = ?1",
        [&first.id],
    )
    .unwrap();

    assert_eq!(recover_stale_sending(&conn, 130).unwrap(), 1);
    let queue = list_queue(&conn, "ecky-1").unwrap();
    assert_eq!(
        queue
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id, second.id]
    );
    assert!(queue.iter().all(|item| item.status == "queued"));
}

#[test]
fn queue_round_trip_preserves_prompt_attachments() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let attachments = vec![
        Attachment {
            path: "/tmp/reference.png".to_string(),
            name: "reference.png".to_string(),
            explanation: "Reference image".to_string(),
            data_url: None,
            kind: AttachmentKind::Image,
        },
        Attachment {
            path: "/tmp/assembly.step".to_string(),
            name: "assembly.step".to_string(),
            explanation: String::new(),
            data_url: None,
            kind: AttachmentKind::Cad,
        },
    ];

    let queued = enqueue_prompt_with_attachments(
        &conn,
        "ecky-1",
        "Use these references.",
        &attachments,
        110,
    )
    .unwrap();
    assert_eq!(queued.attachments, attachments);
    assert_eq!(
        list_queue(&conn, "ecky-1").unwrap()[0].attachments,
        attachments
    );
    assert_eq!(
        ecky_cad_lib::services::codex_takeover::queue_head(&conn, "ecky-1")
            .unwrap()
            .unwrap()
            .attachments,
        attachments
    );
}

#[test]
fn stale_agy_delivery_fails_closed_instead_of_replaying_an_external_turn() {
    let conn = connection();
    ecky_cad_lib::services::agy_provider::bind_owned_conversation(
        &conn,
        "ecky-2",
        "agy-8",
        "Agy agent",
        "/workspace/agy",
        100,
    )
    .unwrap();
    let item = ecky_cad_lib::services::agy_provider::enqueue_prompt(
        &conn,
        "ecky-2",
        "do not replay me",
        110,
    )
    .unwrap();
    conn.execute(
        "UPDATE agent_prompt_queue SET status = 'sending' WHERE id = ?1",
        [&item.id],
    )
    .unwrap();

    assert_eq!(recover_stale_sending(&conn, 130).unwrap(), 1);
    let queue = ecky_cad_lib::services::agy_provider::list_queue(&conn, "ecky-2").unwrap();
    assert_eq!(queue[0].status, "failed");
    assert!(queue[0]
        .error
        .as_deref()
        .unwrap()
        .to_ascii_lowercase()
        .contains("automatic replay disabled"));
}

struct RecordingReaper {
    seen: Mutex<Vec<StaleAgyDelivery>>,
    outcome: AgyProcessReapOutcome,
}

impl AgyProcessReaper for RecordingReaper {
    fn reap(&self, delivery: &StaleAgyDelivery) -> AgyProcessReapOutcome {
        self.seen.lock().unwrap().push(delivery.clone());
        self.outcome.clone()
    }
}

#[test]
fn stale_agy_delivery_reaps_its_recorded_process_before_becoming_retryable() {
    let conn = connection();
    ecky_cad_lib::services::agy_provider::bind_owned_conversation(
        &conn,
        "ecky-2",
        "agy-8",
        "Agy agent",
        "/workspace/agy",
        100,
    )
    .unwrap();
    let item = ecky_cad_lib::services::agy_provider::enqueue_prompt(
        &conn,
        "ecky-2",
        "do not keep working",
        110,
    )
    .unwrap();
    assert!(claim_queue_item(&conn, &item.id, 120).unwrap());
    let process = AgyProcessIdentity {
        run_id: "run-7".to_string(),
        pid: 4123,
        process_group_id: Some(4123),
        executable: "/usr/local/bin/agy".to_string(),
    };
    record_process_lease(&conn, &item.id, "agy-8", &process, 120).unwrap();
    let reaper = RecordingReaper {
        seen: Mutex::new(Vec::new()),
        outcome: AgyProcessReapOutcome::StoppedOwnedProcessGroup(4123),
    };

    assert_eq!(reconcile_stale_deliveries(&conn, &reaper, 130).unwrap(), 1);
    let seen = reaper.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].process.as_ref(), Some(&process));
    let queue = ecky_cad_lib::services::agy_provider::list_queue(&conn, "ecky-2").unwrap();
    assert_eq!(queue[0].status, "failed");
    let error = queue[0].error.as_deref().unwrap();
    assert!(error.contains("stopped orphaned Agy process group 4123"));
    assert!(error
        .to_ascii_lowercase()
        .contains("automatic replay disabled"));
}

#[test]
fn queued_binding_is_discoverable_without_frontend_dispatch() {
    let conn = connection();
    let binding = bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    enqueue_prompt(&conn, "ecky-1", "deliver after missed event", 110).unwrap();

    assert_eq!(pending_queue_bindings(&conn, 110).unwrap(), vec![binding]);
}

#[test]
fn concurrent_dispatchers_use_atomic_queue_claim_instead_of_global_lock() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let queued = enqueue_prompt(&conn, "ecky-1", "deliver once", 110).unwrap();

    assert!(claim_queue_item(&conn, &queued.id, 120).unwrap());
    assert!(!claim_queue_item(&conn, &queued.id, 121).unwrap());
    assert_eq!(list_queue(&conn, "ecky-1").unwrap()[0].status, "sending");
}

#[test]
fn active_writer_failure_is_recovered_for_delayed_retry() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let queued = enqueue_prompt(&conn, "ecky-1", "retry me", 110).unwrap();
    mark_queue_sending(&conn, &queued.id, 120).unwrap();
    fail_queue_item(
        &conn,
        &queued.id,
        "thread codex-7 already has an active writer",
        130,
    )
    .unwrap();

    assert_eq!(recover_retryable_failures(&conn, 140).unwrap(), 1);
    assert_eq!(list_queue(&conn, "ecky-1").unwrap()[0].status, "queued");
}

#[test]
fn transient_writer_lock_retries_after_backoff_without_becoming_failed() {
    let conn = connection();
    let binding = bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let queued = enqueue_prompt(&conn, "ecky-1", "retry me", 110).unwrap();
    mark_queue_sending(&conn, &queued.id, 120).unwrap();
    defer_queue_item(
        &conn,
        &queued.id,
        "thread codex-7 already has an active writer",
        150,
    )
    .unwrap();

    assert!(pending_queue_bindings(&conn, 149).unwrap().is_empty());
    assert_eq!(pending_queue_bindings(&conn, 150).unwrap(), vec![binding]);
    let queue = list_queue(&conn, "ecky-1").unwrap();
    assert_eq!(queue[0].status, "queued");
    assert!(queue[0].error.as_deref().unwrap().contains("active writer"));
}

#[test]
fn failed_queue_head_blocks_overtaking_and_retry_remove_are_scoped() {
    let conn = connection();
    bind_owned_thread(
        &conn,
        "ecky-1",
        "codex-7",
        "Gearbox agent",
        "/workspace/gearbox",
        100,
    )
    .unwrap();
    let first = enqueue_prompt(&conn, "ecky-1", "first", 110).unwrap();
    let second = enqueue_prompt(&conn, "ecky-1", "second", 120).unwrap();

    mark_queue_sending(&conn, &first.id, 130).unwrap();
    fail_queue_item(&conn, &first.id, "provider failed", 140).unwrap();
    let overtake = retry_queue_item(&conn, "ecky-1", &second.id, 150).unwrap_err();
    assert!(overtake.message.contains("cannot overtake queue head"));

    retry_queue_item(&conn, "ecky-1", &first.id, 160).unwrap();
    remove_queue_item(&conn, "ecky-1", &first.id).unwrap();
    mark_queue_sending(&conn, &second.id, 170).unwrap();
    let sending = remove_queue_item(&conn, "ecky-1", &second.id).unwrap_err();
    assert!(sending.message.contains("Use STOP"));
}
