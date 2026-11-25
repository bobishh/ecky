//! Antigravity owns audio and its conversation transcript. Ecky owns the PTY handoff.
//! The idle title handshake follows Repo City 306cb3d.
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, PtySize};
use serde::Deserialize;
use tauri::Emitter;
use uuid::Uuid;

use crate::contracts::{AgyVoicePhase as Phase, AgyVoiceSnapshot, AppError, AppResult};
use crate::mcp::runtime;
use crate::models::{AgentTerminalPty, AgentTerminalWriter, AppState};
use crate::services::{agy_provider, codex_takeover, provider_executable};

const TITLE_TOKEN: &str = "ECKY_AGY_VOICE_TITLE_TOKEN";
const TITLE_EXE: &str = "ECKY_AGY_VOICE_TITLE_EXE";
const TITLE_ENTRY: &str = "--ecky-agy-terminal-title";
const START_VOICE: &[u8] = b"/voice live\r";
const F7: &[u8] = b"\x1b[18~";
const ROWS: u16 = 30;
const COLS: u16 = 100;

#[derive(Default, Clone)]
pub struct AgyVoice {
    session: Arc<Mutex<Option<VoiceSession>>>,
}

struct VoiceSession {
    lease: Uuid,
    snapshot: AgyVoiceSnapshot,
    held: bool,
    requested: bool,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
}

impl AgyVoice {
    pub(crate) fn snapshot(&self) -> Option<AgyVoiceSnapshot> {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .map(|session| session.snapshot.clone())
    }

    pub(crate) fn owns_thread(&self, thread_id: &str) -> bool {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|session| session.held && session.snapshot.ecky_thread_id == thread_id)
    }

    pub(crate) fn reserve(
        state: &AppState,
        thread_id: &str,
    ) -> AppResult<(AgyVoiceSnapshot, Option<Uuid>)> {
        let mut slot = state.agy_voice.session.lock().unwrap();
        if let Some(session) = slot.as_mut().filter(|session| session.held) {
            if session.snapshot.ecky_thread_id != thread_id {
                return Err(AppError::conflict(
                    "Exit the Antigravity terminal in the other project before starting voice here.",
                ));
            }
            if session.snapshot.phase == Phase::Idle {
                session.requested = true;
                session.set_status(state, Phase::Starting, None);
            }
            return Ok((session.snapshot.clone(), None));
        }

        let lease = Uuid::new_v4();
        if let Some(previous) = slot
            .as_ref()
            .and_then(|session| session.snapshot.terminal_id.as_ref())
        {
            state.agent_terminals.lock().unwrap().remove(previous);
        }
        let session = VoiceSession {
            lease,
            snapshot: AgyVoiceSnapshot {
                ecky_thread_id: thread_id.to_owned(),
                phase: Phase::Waiting,
                detail: Some("Waiting for any current Antigravity text turn to finish.".into()),
                terminal_id: None,
            },
            held: true,
            requested: true,
            killer: None,
        };
        let snapshot = session.snapshot.clone();
        *slot = Some(session);
        emit(state, &snapshot);
        Ok((snapshot, Some(lease)))
    }

    pub(crate) fn stop(state: &AppState, thread_id: &str) -> AppResult<AgyVoiceSnapshot> {
        let mut slot = state.agy_voice.session.lock().unwrap();
        let session = slot
            .as_mut()
            .filter(|session| session.snapshot.ecky_thread_id == thread_id)
            .ok_or_else(|| AppError::not_found("No Antigravity voice session for this project."))?;
        session.requested = false;
        if session.held && session.snapshot.terminal_id.is_none() {
            session.set_status(state, Phase::Closed, None);
        }
        Ok(session.snapshot.clone())
    }

    pub(crate) fn is_requested(&self, lease: Uuid) -> bool {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|session| session.lease == lease && session.held && session.requested)
    }

    pub(crate) fn starting(state: &AppState, lease: Uuid) {
        if let Some(session) = state
            .agy_voice
            .session
            .lock()
            .unwrap()
            .as_mut()
            .filter(|session| session.lease == lease && session.requested)
        {
            session.set_status(state, Phase::Starting, None);
        }
    }

    pub(crate) fn finish(state: &AppState, lease: Uuid, outcome: AppResult<()>) {
        if let Some(session) = state
            .agy_voice
            .session
            .lock()
            .unwrap()
            .as_mut()
            .filter(|session| session.lease == lease)
        {
            session.held = false;
            session.requested = false;
            session.killer = None;
            match outcome {
                Ok(()) => session.set_status(state, Phase::Closed, None),
                Err(error) => session.set_status(
                    state,
                    Phase::Error,
                    Some(codex_takeover::error_text(&error)),
                ),
            }
        }
    }

    pub(crate) fn shutdown(&self) {
        if let Some(session) = self.session.lock().unwrap().as_mut() {
            session.requested = false;
            session.held = false;
            if let Some(killer) = session.killer.as_mut() {
                let _ = killer.kill();
            }
        }
    }
}

impl VoiceSession {
    fn set_status(&mut self, state: &AppState, phase: Phase, detail: Option<String>) {
        if self.snapshot.phase == phase && self.snapshot.detail == detail {
            return;
        }
        self.snapshot.phase = phase;
        self.snapshot.detail = detail;
        emit(state, &self.snapshot);
    }
}

fn emit(state: &AppState, snapshot: &AgyVoiceSnapshot) {
    if let Some(app) = state.app_handle.lock().unwrap().as_ref() {
        let _ = app.emit("agy-voice-updated", snapshot);
    }
}

#[derive(Default)]
struct Title(String);

impl vt100::Callbacks for Title {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.0 = String::from_utf8_lossy(title).into_owned();
    }
}

struct VoiceTerminal(vt100::Parser<Title>);

impl VoiceTerminal {
    fn new() -> Self {
        Self(vt100::Parser::new_with_callbacks(
            NonZeroU16::new(ROWS).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(COLS).unwrap_or(NonZeroU16::MIN),
            0,
            Title::default(),
        ))
    }

    fn process(&mut self, bytes: &[u8]) {
        self.0.process(bytes);
    }

    fn leased_title(&self, lease: Uuid) -> Option<&str> {
        let prefix = format!("Ecky Agy {lease} ");
        self.0.callbacks().0.strip_prefix(&prefix)
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        match (
            NonZeroU16::new(rows.min(200)),
            NonZeroU16::new(cols.min(500)),
        ) {
            (Some(rows), Some(cols)) if self.0.screen().size() != (rows, cols) => {
                self.0.set_size(rows, cols);
            }
            _ => {}
        }
    }

    fn connected(&self) -> bool {
        let (_, cols) = self.0.screen().size();
        self.0
            .screen()
            .rows(0, cols.get())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .filter(|row| !row.trim().is_empty())
            .take(6)
            .any(|row| live_voice_row(&row))
    }

    fn at_empty_prompt(&self) -> bool {
        let screen = self.0.screen();
        let (row, _) = screen.cursor_position();
        let (_, cols) = screen.size();
        screen
            .rows(0, cols.get())
            .nth(usize::from(row))
            .is_some_and(|line| line.trim() == ">")
    }
}

fn live_voice_row(row: &str) -> bool {
    let mut words = row
        .trim_start_matches(|ch: char| !ch.is_alphanumeric())
        .split_whitespace();
    if words.next() != Some("Live") {
        return false;
    }
    let Some((minutes, seconds)) = words.next().and_then(|time| time.split_once(':')) else {
        return false;
    };
    !minutes.is_empty()
        && minutes.bytes().all(|ch| ch.is_ascii_digit())
        && seconds.len() == 2
        && seconds.bytes().all(|ch| ch.is_ascii_digit())
        && words.next() == Some("·")
        && matches!(
            words.next(),
            Some("listening" | "thinking" | "speaking" | "muted")
        )
}

fn voice_input(requested: bool, sent: bool, ready: bool) -> Option<&'static [u8]> {
    match (requested, sent, ready) {
        (true, false, true) => Some(START_VOICE),
        (false, true, _) => Some(F7),
        _ => None,
    }
}

fn valid_conversation_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn conversation_ready<'a>(
    expected: Option<&str>,
    title: Option<&'a str>,
) -> AppResult<(bool, Option<&'a str>)> {
    let reported = title
        .and_then(|title| title.strip_prefix("ready "))
        .filter(|id| valid_conversation_id(id));
    match (expected, reported) {
        (Some(expected), Some(reported)) if expected != reported => Err(AppError::conflict(
            "Antigravity opened a different conversation. Exit the terminal and retry voice.",
        )),
        // A fresh TUI has no conversation ID until its first turn.
        (None, None) => Ok((title == Some("fresh"), None)),
        _ => Ok((reported.is_some(), reported)),
    }
}

#[derive(Deserialize)]
struct TitleState {
    conversation_id: Option<String>,
    agent_state: Option<String>,
}

fn read_idle_title() -> Option<(String, Option<String>)> {
    let token = std::env::var(TITLE_TOKEN).ok()?;
    Uuid::parse_str(&token).ok()?;
    let state: TitleState =
        serde_json::from_reader(std::io::stdin().lock().take(64 * 1024)).ok()?;
    let id = state.conversation_id.filter(|id| !id.is_empty());
    if id.as_deref().is_some_and(|id| !valid_conversation_id(id)) {
        return None;
    }
    (state.agent_state.as_deref() == Some("idle")).then_some((token, id))
}

/// Called by Antigravity's session-only title command before the app is initialized.
pub fn maybe_print_terminal_title() -> bool {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(TITLE_ENTRY)) {
        return false;
    }
    match read_idle_title() {
        Some((token, Some(id))) => println!("Ecky Agy {token} ready {id}"),
        Some((token, None)) => println!("Ecky Agy {token} fresh"),
        None => println!("Ecky Antigravity"),
    }
    true
}

pub(crate) struct VoiceWorkspace {
    pub thread_id: String,
    pub label: String,
    pub cwd: String,
    pub conversation_id: Option<String>,
    pub model: Option<String>,
}

pub(crate) async fn run(state: &AppState, lease: Uuid, workspace: VoiceWorkspace) -> AppResult<()> {
    let resolved =
        provider_executable::resolve_provider_executable("agy", "ECKY_AGY_BIN", "Antigravity CLI")?;
    let mut version = tokio::process::Command::new(&resolved.path);
    version
        .arg("--version")
        .env("PATH", &resolved.spawn_path)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(10), version.output())
        .await
        .map_err(|_| AppError::provider("Antigravity CLI version check timed out."))?
        .map_err(|error| AppError::provider(format!("Could not run Antigravity CLI: {error}")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() || agy_provider::parse_agy_version(&text)? < (1, 5, 0) {
        return Err(AppError::provider(
            "Antigravity voice requires agy 1.5.0 or later. Run agy update.",
        ));
    }
    if !state.agy_voice.is_requested(lease) {
        return Ok(());
    }

    let exe = std::env::current_exe().map_err(|error| {
        AppError::internal(format!(
            "Could not locate Ecky for the Antigravity title command: {error}"
        ))
    })?;
    let title = if cfg!(windows) {
        format!(r#""%{TITLE_EXE}%" {TITLE_ENTRY}"#)
    } else {
        format!(r#"exec "${TITLE_EXE}" {TITLE_ENTRY}"#)
    };
    let settings =
        serde_json::json!({"title": {"command": title, "type": "command", "enabled": true}});
    let mut command = CommandBuilder::new(resolved.path);
    command.args(["--sandbox", "--settings", &settings.to_string()]);
    command.cwd(&workspace.cwd);
    command.env("PATH", resolved.spawn_path);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env_remove("GEMINI_API_KEY");
    command.env_remove("GOOGLE_API_KEY");
    command.env(TITLE_EXE, exe);
    command.env(TITLE_TOKEN, lease.to_string());
    if let Some(conversation) = &workspace.conversation_id {
        command.args(["--conversation", conversation]);
    }
    if let Some(model) = &workspace.model {
        command.args(["--model", model]);
    }
    let state = state.clone();
    tauri::async_runtime::spawn_blocking(move || run_pty(&state, lease, workspace, command))
        .await
        .map_err(|error| AppError::internal(format!("Antigravity terminal task failed: {error}")))?
}

fn run_pty(
    state: &AppState,
    lease: Uuid,
    workspace: VoiceWorkspace,
    command: CommandBuilder,
) -> AppResult<()> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(pty_error)?;
    let mut reader = pair.master.try_clone_reader().map_err(pty_error)?;
    let writer: AgentTerminalWriter =
        Arc::new(Mutex::new(pair.master.take_writer().map_err(pty_error)?));
    let pty: AgentTerminalPty = Arc::new(Mutex::new(pair.master));
    let terminal_id = format!("agy-voice:{}", workspace.thread_id);
    let child = {
        let mut slot = state.agy_voice.session.lock().unwrap();
        let Some(session) = slot
            .as_mut()
            .filter(|session| session.lease == lease && session.held && session.requested)
        else {
            return Ok(());
        };
        let child = pair.slave.spawn_command(command).map_err(pty_error)?;
        drop(pair.slave);
        session.killer = Some(child.clone_killer());
        runtime::register_pty_terminal(
            state,
            &terminal_id,
            "Antigravity",
            "agy",
            Some(workspace.thread_id.clone()),
            pty.clone(),
            writer.clone(),
        );
        session.snapshot.terminal_id = Some(terminal_id.clone());
        session.set_status(state, Phase::Starting, Some(
            "Complete any sign-in or workspace trust prompts, then leave the Antigravity prompt empty for voice.".into(),
        ));
        child
    };
    let mut terminal = InteractivePty {
        id: terminal_id,
        child,
        pty,
        writer,
    };

    let (tx, rx) = mpsc::sync_channel(64);
    let reader = std::thread::Builder::new()
        .name("ecky-agy-voice-pty".into())
        .spawn(move || {
            let mut buf = [0; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(length) if tx.send(buf[..length].to_vec()).is_err() => break,
                    Ok(_) => {}
                }
            }
        });

    let outcome = match reader {
        Ok(_) => supervise_pty(state, lease, workspace, &mut terminal, rx),
        Err(error) => Err(pty_error(error)),
    };

    // EOF normally follows CLI exit. Stop the child if the reader or session failed.
    if terminal.child.try_wait().ok().flatten().is_none() {
        let _ = terminal.child.kill();
    }
    let status = terminal.child.wait().map_err(pty_error);
    runtime::close_interactive_pty(state, &terminal.id);
    outcome?;
    let status = status?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::provider(format!(
            "Antigravity terminal exited ({status}). Check its output, then retry voice."
        )))
    }
}

fn pty_error(error: impl std::fmt::Display) -> AppError {
    AppError::provider(format!("Antigravity terminal: {error}"))
}

struct InteractivePty {
    id: String,
    child: Box<dyn Child + Send + Sync>,
    pty: AgentTerminalPty,
    writer: AgentTerminalWriter,
}

#[derive(Default)]
struct Call {
    sent: bool,
    live: bool,
    mismatch: bool,
    canceled_dial: bool,
}

impl Call {
    fn update(
        &mut self,
        state: &AppState,
        session: &mut VoiceSession,
        writer: &AgentTerminalWriter,
        ready: bool,
        live: bool,
    ) -> AppResult<()> {
        if self.live && !live {
            session.requested = false;
            self.sent = false;
        } else if !self.live && live && !self.sent {
            session.requested = !self.mismatch && !self.canceled_dial;
            self.sent = true;
        }
        self.live = live;
        if let Some(input) = voice_input(session.requested, self.sent, ready && !self.mismatch) {
            let mut writer = writer.lock().unwrap();
            writer
                .write_all(input)
                .and_then(|_| writer.flush())
                .map_err(pty_error)?;
            self.canceled_dial = input == F7 && !live;
            self.sent = input == START_VOICE;
        }
        if !self.mismatch {
            let (phase, detail) = if live {
                (Phase::Connected, None)
            } else if self.sent {
                (Phase::Connecting, None)
            } else if !session.requested {
                (Phase::Idle, None)
            } else {
                (Phase::Starting, session.snapshot.detail.clone())
            };
            session.set_status(state, phase, detail);
        }
        Ok(())
    }
}

fn supervise_pty(
    state: &AppState,
    lease: Uuid,
    mut workspace: VoiceWorkspace,
    terminal: &mut InteractivePty,
    rx: mpsc::Receiver<Vec<u8>>,
) -> AppResult<()> {
    let mut term = VoiceTerminal::new();
    let mut call = Call::default();
    let mut live = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(60)) {
            Ok(data) => {
                if let Ok(size) = terminal.pty.lock().unwrap().get_size() {
                    term.resize(size.rows, size.cols);
                }
                term.process(&data);
                live = term.connected();
                runtime::append_interactive_pty_output(state, &terminal.id, &data);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if terminal.child.try_wait().map_err(pty_error)?.is_some() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let ready = match conversation_ready(
            workspace.conversation_id.as_deref(),
            term.leased_title(lease),
        ) {
            Ok((ready, reported)) => {
                if let (None, Some(id)) = (workspace.conversation_id.as_ref(), reported) {
                    bind_conversation(state, &workspace, id)?;
                    workspace.conversation_id = Some(id.to_string());
                }
                ready
            }
            Err(error) => {
                if let Some(session) = state
                    .agy_voice
                    .session
                    .lock()
                    .unwrap()
                    .as_mut()
                    .filter(|session| session.lease == lease)
                {
                    call.mismatch = true;
                    session.requested = false;
                    session.set_status(
                        state,
                        Phase::Error,
                        Some(codex_takeover::error_text(&error)),
                    );
                }
                false
            }
        };
        let mut slot = state.agy_voice.session.lock().unwrap();
        let Some(session) = slot
            .as_mut()
            .filter(|session| session.lease == lease && session.held)
        else {
            break;
        };
        // The CLI can report idle while the editor still contains an unsent draft.
        call.update(
            state,
            session,
            &terminal.writer,
            ready && term.at_empty_prompt(),
            live,
        )?;
    }
    Ok(())
}

fn bind_conversation(state: &AppState, workspace: &VoiceWorkspace, id: &str) -> AppResult<()> {
    tauri::async_runtime::block_on(async {
        let conn = state.db.lock().await;
        if agy_provider::get_binding(&conn, &workspace.thread_id)?.is_some() {
            return Err(AppError::conflict(
                "Antigravity conversation changed while voice was opening.",
            ));
        }
        agy_provider::bind_owned_conversation(
            &conn,
            &workspace.thread_id,
            id,
            &workspace.label,
            &workspace.cwd,
            agy_provider::now_seconds(),
        )?;
        Ok(())
    })?;
    if let Some(app) = state.app_handle.lock().unwrap().as_ref() {
        let _ = app.emit(
            "agy-provider-updated",
            serde_json::json!({
                "conversationId": id,
                "eckyThreadId": workspace.thread_id,
                "method": "binding/voice",
            }),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_exact_idle_conversation_before_starting_voice() {
        let token = uuid::Uuid::nil();
        let mut term = VoiceTerminal::new();
        term.process(format!("\x1b]0;Ecky Agy {token} ready saved").as_bytes());
        assert_eq!(term.leased_title(token), None);
        term.process(b"\x07");
        assert_eq!(
            conversation_ready(Some("saved"), term.leased_title(token)),
            Ok((true, Some("saved")))
        );
        assert!(conversation_ready(Some("saved"), Some("ready other")).is_err());
        assert_eq!(
            conversation_ready(Some("saved"), Some("fresh")),
            Ok((false, None))
        );
        assert_eq!(conversation_ready(None, Some("fresh")), Ok((true, None)));
    }

    #[test]
    fn recognizes_live_status_and_keeps_work_in_the_terminal_on_hangup() {
        let mut term = VoiceTerminal::new();
        term.process("🎤 Started voice chat · 7:31PM\r\n".as_bytes());
        assert!(!term.connected());
        term.process("● Live 01:53 · speaking · f6 mute · esc hang up".as_bytes());
        assert!(term.connected());
        assert_eq!(voice_input(false, true, false), Some(F7));
        term.process(b"\r\x1b[2K");
        assert!(!term.connected());
        assert_eq!(voice_input(true, false, true), Some(START_VOICE));
        assert_eq!(voice_input(true, false, false), None);
    }

    #[cfg(unix)]
    #[test]
    fn ready_pty_submits_voice_live_after_draft_is_cleared_and_hangup_keeps_it_interactive() {
        let state = AppState::new(
            serde_json::from_value(serde_json::json!({"engines": [], "selectedEngineId": ""}))
                .unwrap(),
            None,
            rusqlite::Connection::open_in_memory().unwrap(),
        );
        let thread_id = "voice-test";
        let (_, lease) = AgyVoice::reserve(&state, thread_id).unwrap();
        let lease = lease.unwrap();
        let workspace = VoiceWorkspace {
            thread_id: thread_id.into(),
            label: "Voice test".into(),
            cwd: ".".into(),
            conversation_id: Some("saved".into()),
            model: None,
        };
        let mut command = CommandBuilder::new("python3");
        command.args([
            "-c",
            r#"
import os, select, sys, tty

def read(length):
    result = b""
    while len(result) < length:
        if not select.select([0], [], [], 5)[0]:
            raise TimeoutError(result)
        result += os.read(0, length - len(result))
    return result

tty.setraw(0)
os.write(1, f"\x1b]0;Ecky Agy {sys.argv[1]} ready saved\x07> unsent draft".encode())
assert not select.select([0], [], [], 0.3)[0], "Voice must wait for an empty prompt"
os.write(1, b"\r\x1b[2K> ")
assert read(len(b"/voice live\r")) == b"/voice live\r"
os.write(1, "\r\x1b[2K● Live 00:00 · listening".encode())
assert read(5) == b"\x1b[18~"
os.write(1, b"\r\x1b[2KWork continued")
assert read(4) == b"exit"
"#,
            &lease.to_string(),
        ]);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| run_pty(&state, lease, workspace, command));
            let wait_for = |phase| {
                let deadline = std::time::Instant::now() + Duration::from_secs(4);
                while state.agy_voice.snapshot().unwrap().phase != phase {
                    assert!(std::time::Instant::now() < deadline, "Missing {phase:?}");
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            wait_for(Phase::Connected);
            AgyVoice::stop(&state, thread_id).unwrap();
            wait_for(Phase::Idle);
            let terminal_id = format!("agy-voice:{thread_id}");
            let writer = {
                let terminals = state.agent_terminals.lock().unwrap();
                let runtime = &terminals[&terminal_id];
                assert!(runtime.snapshot.active);
                assert!(runtime.snapshot.vt_stream.contains("Work continued"));
                runtime.writer.clone()
            };
            writer.lock().unwrap().write_all(b"exit").unwrap();
            worker.join().unwrap().unwrap();
            assert!(
                !state.agent_terminals.lock().unwrap()[&terminal_id]
                    .snapshot
                    .active
            );
        });
    }
}
