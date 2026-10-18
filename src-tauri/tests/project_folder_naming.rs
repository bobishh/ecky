use ecky_cad_lib::{
    db,
    models::PathResolver,
    project_mirror::{self, PROJECT_MANIFEST_FILE_NAME},
    services::{codex_takeover, project_naming::reconcile_project_names},
    thread_source_binding,
};
use rusqlite::{params, Connection};
use std::{fs, path::PathBuf};

struct ProjectFixture {
    root: PathBuf,
    conn: Connection,
}

struct RootResolver(PathBuf);

impl PathResolver for RootResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.0.clone()
    }
    fn app_data_dir(&self) -> PathBuf {
        self.0.clone()
    }
    fn resource_path(&self, _: &str) -> Option<PathBuf> {
        None
    }
}

impl ProjectFixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("ecky-project-naming-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 1, updated_at INTEGER NOT NULL DEFAULT 1,
                deleted_at INTEGER);
            CREATE TABLE messages (id TEXT PRIMARY KEY, thread_id TEXT, content TEXT);",
        )
        .unwrap();
        thread_source_binding::ensure_schema(&conn).unwrap();
        codex_takeover::ensure_schema(&conn).unwrap();
        Self { root, conn }
    }

    fn bind(&self, id: &str, title: &str) -> thread_source_binding::ThreadSourceBinding {
        self.conn
            .execute(
                "INSERT INTO threads (id, title) VALUES (?1, ?2)",
                params![id, title],
            )
            .unwrap();
        thread_source_binding::bind_new_thread(
            &RootResolver(self.root.clone()),
            &self.conn,
            None,
            id,
            title,
        )
        .unwrap()
    }

    fn bind_legacy(&self, id: &str) -> thread_source_binding::ThreadSourceBinding {
        let binding = self.bind(id, "Untitled design");
        let folder = PathBuf::from(&binding.folder_path);
        let legacy = folder.parent().unwrap().join(format!(
            "untitled-design-{}",
            id.rsplit('-').next().unwrap()
        ));
        if folder != legacy {
            fs::rename(&folder, &legacy).unwrap();
        }
        thread_source_binding::index_export(&self.conn, id, &legacy, &binding.source_digest)
            .unwrap()
    }

    fn rename(&self, id: &str, title: &str) {
        db::update_thread_title(&self.conn, id, title).unwrap();
    }
}

impl Drop for ProjectFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn unicode_titles_make_readable_safe_folder_names() {
    let fixture = ProjectFixture::new();
    let slug = project_mirror::project_slug("Отель для червяков / V2!", "thread-12345678");
    assert_eq!(slug, "отель-для-червяков-v2-12345678");
    assert!(project_mirror::project_dir(&RootResolver(fixture.root.clone()), None, &slug).is_ok());
    let binding = fixture.bind("thread-12345678", "Отель для червяков / V2!");
    assert!(thread_source_binding::binding_info(
        &RootResolver(fixture.root.clone()),
        &fixture.conn,
        None,
        &binding.thread_id,
        "Отель для червяков / V2!",
        None,
        false,
        None
    )
    .is_ok());
}

#[test]
fn startup_renames_untitled_folder_preserving_dirty_source_manifest_and_history() {
    // Given a bound untitled project, later titled by its first conversation,
    // with an uncommitted external save and additional project files.
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    let old_folder = PathBuf::from(&old.folder_path);
    let manifest_bytes = fs::read(old_folder.join(PROJECT_MANIFEST_FILE_NAME)).unwrap();
    fs::write(&old.source_path, "unsaved external source\n").unwrap();
    fs::create_dir(old_folder.join("assets")).unwrap();
    fs::write(old_folder.join("assets/drawing.txt"), "keep me").unwrap();
    fixture
        .conn
        .execute(
            "INSERT INTO messages VALUES ('version-1', ?1, 'history')",
            ["thread-12345678"],
        )
        .unwrap();
    fixture.rename("thread-12345678", "Отель для червяков");

    // When startup reconciles names before providers and watchers start.
    let report = reconcile_project_names(&fixture.conn);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.renamed, 1);

    // Then the complete original directory moves, bindings change together,
    // and neither source bytes, source digest, manifest nor history changes.
    let bound = thread_source_binding::get_binding(&fixture.conn, "thread-12345678")
        .unwrap()
        .unwrap();
    let expected = fixture.root.join("projects/ecky-design-12345678");
    assert_eq!(PathBuf::from(&bound.folder_path), expected);
    assert!(!old_folder.exists());
    assert_eq!(
        fs::read_to_string(&bound.source_path).unwrap(),
        "unsaved external source\n"
    );
    assert_eq!(bound.source_digest, old.source_digest);
    assert_eq!(bound.created_at, old.created_at);
    assert_eq!(
        fs::read(expected.join(PROJECT_MANIFEST_FILE_NAME)).unwrap(),
        manifest_bytes
    );
    assert_eq!(
        fs::read_to_string(expected.join("assets/drawing.txt")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fixture
            .conn
            .query_row("SELECT content FROM messages", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "history"
    );
    assert_eq!(reconcile_project_names(&fixture.conn).renamed, 0);
}

#[test]
fn startup_updates_all_owned_provider_cwds_and_preserves_conversation_and_queue() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    for provider in ["codex", "agy"] {
        codex_takeover::upsert_agent_binding(
            &fixture.conn,
            &codex_takeover::AgentThreadBindingRecord {
                ecky_thread_id: old.thread_id.clone(),
                provider: provider.into(),
                external_thread_id: format!("{provider}-original"),
                external_title: "Untitled design".into(),
                external_cwd: old.folder_path.clone(),
                bootstrap_version: 7,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
    }
    codex_takeover::enqueue_prompt(&fixture.conn, &old.thread_id, "continue", 2).unwrap();
    fixture.rename(&old.thread_id, "Worm hotel");
    assert_eq!(reconcile_project_names(&fixture.conn).renamed, 1);
    let bound = thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
        .unwrap()
        .unwrap();
    for provider in ["codex", "agy"] {
        let agent =
            codex_takeover::get_agent_binding_for_provider(&fixture.conn, &old.thread_id, provider)
                .unwrap()
                .unwrap();
        assert_eq!(agent.external_cwd, bound.folder_path);
        assert_eq!(agent.external_thread_id, format!("{provider}-original"));
        assert_eq!(
            agent.bootstrap_version, 0,
            "refresh path-bearing handoff on resume"
        );
    }
    assert_eq!(
        codex_takeover::list_queue(&fixture.conn, &old.thread_id).unwrap()[0].prompt_text,
        "continue"
    );
}

#[test]
fn title_changes_keep_existing_root_even_after_default_root_changes() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind("thread-12345678", "Alpha");
    fixture.rename(&old.thread_id, "Beta");
    assert_eq!(reconcile_project_names(&fixture.conn).renamed, 0);
    let bound = thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
        .unwrap()
        .unwrap();
    assert_eq!(bound, old);
}

#[test]
fn destination_collision_preserves_both_folders_and_binding() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    fixture.rename(&old.thread_id, "Worm hotel");
    let destination = fixture.root.join("projects/ecky-design-12345678");
    fs::create_dir(&destination).unwrap();
    let report = reconcile_project_names(&fixture.conn);
    assert_eq!(report.renamed, 0);
    assert!(!report.errors.is_empty());
    assert!(PathBuf::from(&old.source_path).exists());
    assert!(destination.is_dir());
    assert_eq!(
        thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
            .unwrap()
            .unwrap(),
        old
    );
}

#[test]
fn foreign_manifest_is_not_moved_or_rebound() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    fixture.rename(&old.thread_id, "Worm hotel");
    let folder = PathBuf::from(&old.folder_path);
    let mut manifest = project_mirror::read_manifest(&folder).unwrap().unwrap();
    manifest.thread_id = "other-thread".into();
    project_mirror::write_manifest(&folder, &manifest).unwrap();
    let report = reconcile_project_names(&fixture.conn);
    assert_eq!(report.renamed, 0);
    assert!(!report.errors.is_empty());
    assert!(folder.is_dir());
}

#[test]
fn blank_projects_use_neutral_names_without_later_title_relocation() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind("thread-12345678", "Untitled design");
    assert!(old.folder_path.ends_with("ecky-design-12345678"));
    assert_eq!(
        project_mirror::project_slug("", "thread-12345678"),
        "ecky-design-12345678"
    );
    fixture.rename(&old.thread_id, "Отель для червяков");
    assert_eq!(reconcile_project_names(&fixture.conn).renamed, 0);
    assert_eq!(
        thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
            .unwrap()
            .unwrap(),
        old
    );
}

#[test]
fn interrupted_move_recovers_bindings_before_source_consumers_start() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    codex_takeover::bind_owned_thread(
        &fixture.conn,
        &old.thread_id,
        "native-original",
        "Project",
        &old.folder_path,
        1,
    )
    .unwrap();
    fixture
        .conn
        .execute_batch(
            "CREATE TRIGGER fail_binding BEFORE UPDATE ON agent_thread_bindings
        BEGIN SELECT RAISE(FAIL, 'simulated database write failure'); END;",
        )
        .unwrap();
    let report = reconcile_project_names(&fixture.conn);
    assert!(
        report.blocked,
        "moved but unindexed source must block startup"
    );
    assert_eq!(report.renamed, 0);
    assert!(!PathBuf::from(&old.folder_path).exists());
    assert_eq!(
        thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
            .unwrap()
            .unwrap()
            .folder_path,
        old.folder_path,
        "source binding update must roll back with failed provider update"
    );
    assert_eq!(
        codex_takeover::get_binding(&fixture.conn, &old.thread_id)
            .unwrap()
            .unwrap()
            .cwd,
        old.folder_path
    );
    let new = fixture.root.join("projects/ecky-design-12345678");
    assert!(new.join("model.ecky").is_file());
    assert_eq!(
        fixture
            .conn
            .query_row(
                "SELECT COUNT(*) FROM project_folder_relocations",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );

    fixture
        .conn
        .execute_batch("DROP TRIGGER fail_binding;")
        .unwrap();
    let recovered = reconcile_project_names(&fixture.conn);
    assert!(!recovered.blocked, "{:?}", recovered.errors);
    assert_eq!(recovered.renamed, 1);
    assert!(recovered.errors.is_empty());
    assert_eq!(
        thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
            .unwrap()
            .unwrap()
            .folder_path,
        new.to_string_lossy()
    );
    assert_eq!(
        fixture
            .conn
            .query_row(
                "SELECT COUNT(*) FROM project_folder_relocations",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(reconcile_project_names(&fixture.conn).renamed, 0);
}

#[test]
fn collision_can_be_resolved_and_retried_without_recreating_source() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    let destination = fixture.root.join("projects/ecky-design-12345678");
    fs::create_dir(&destination).unwrap();
    let blocked = reconcile_project_names(&fixture.conn);
    assert!(!blocked.blocked, "original binding remains usable");
    fs::remove_dir(&destination).unwrap();
    let retry = reconcile_project_names(&fixture.conn);
    assert_eq!(retry.renamed, 1);
    assert!(retry.errors.is_empty());
    assert!(!PathBuf::from(old.folder_path).exists());
}

#[cfg(unix)]
#[test]
fn symlinked_legacy_directory_is_not_moved() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    let actual = fixture.root.join("actual-source");
    fs::rename(&old.folder_path, &actual).unwrap();
    std::os::unix::fs::symlink(&actual, &old.folder_path).unwrap();
    let report = reconcile_project_names(&fixture.conn);
    assert_eq!(report.renamed, 0);
    assert!(!report.errors.is_empty());
    assert!(fs::symlink_metadata(&old.folder_path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(actual.join("model.ecky").is_file());
}

#[test]
fn legacy_unindexed_folder_is_adopted_without_replacing_external_edits() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    fixture
        .conn
        .execute("DELETE FROM thread_source_bindings", [])
        .unwrap();
    fs::write(&old.source_path, "external unsaved source").unwrap();
    let adopted = thread_source_binding::backfill_binding(
        &RootResolver(fixture.root.clone()),
        &fixture.conn,
        None,
        &old.thread_id,
        "Untitled design",
        Some("older history source"),
        Some("version-1"),
        None,
    )
    .unwrap();
    assert_eq!(adopted.folder_path, old.folder_path);
    assert_eq!(
        fs::read_to_string(adopted.source_path).unwrap(),
        "external unsaved source"
    );
    assert!(!fixture.root.join("projects/ecky-design-12345678").exists());
}

#[test]
fn migration_runs_before_app_state_source_projection_and_background_workers() {
    let source = include_str!("../src/lib.rs");
    let setup = source.split(".setup(move |app| {").nth(1).unwrap();
    let migration = setup
        .find("project_naming::reconcile_project_names")
        .unwrap();
    assert!(migration < setup.find("services::session::read_last_snapshot").unwrap());
    assert!(migration < setup.find("AppState::new_with_read_connection").unwrap());
    assert!(migration < setup.find("initialize_codex_queue_supervisor").unwrap());
    assert!(migration < setup.find("ProjectFolderWatcher::new").unwrap());
}

#[test]
fn recorded_provider_process_lease_defers_legacy_folder_move() {
    let fixture = ProjectFixture::new();
    let old = fixture.bind_legacy("thread-12345678");
    codex_takeover::bind_owned_thread(
        &fixture.conn,
        &old.thread_id,
        "native-thread",
        "Project",
        &old.folder_path,
        1,
    )
    .unwrap();
    let queued =
        codex_takeover::enqueue_prompt(&fixture.conn, &old.thread_id, "keep lease", 2).unwrap();
    fixture.conn.execute("INSERT INTO agent_provider_process_leases VALUES (?1, 'codex', 'native-thread', 'run-1', 123, 123, 'provider', 1)", [&queued.id]).unwrap();
    let report = reconcile_project_names(&fixture.conn);
    assert_eq!(report.renamed, 0);
    assert!(!report.blocked);
    assert!(!report.errors.is_empty());
    assert!(PathBuf::from(&old.source_path).exists());
    assert_eq!(
        thread_source_binding::get_binding(&fixture.conn, &old.thread_id)
            .unwrap()
            .unwrap(),
        old
    );
}

#[test]
fn unindexed_backfill_never_overwrites_a_foreign_neutral_folder() {
    let fixture = ProjectFixture::new();
    let other = fixture.bind("other-12345678", "Other");
    let foreign = fixture.root.join("projects/ecky-design-12345678");
    fs::rename(&other.folder_path, &foreign).unwrap();
    let source_before = fs::read(foreign.join("model.ecky")).unwrap();
    fixture
        .conn
        .execute(
            "INSERT INTO threads (id, title) VALUES ('thread-12345678', 'Untitled design')",
            [],
        )
        .unwrap();
    let result = thread_source_binding::backfill_binding(
        &RootResolver(fixture.root.clone()),
        &fixture.conn,
        None,
        "thread-12345678",
        "Untitled design",
        Some("must not overwrite"),
        None,
        None,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(foreign.join("model.ecky")).unwrap(), source_before);
}
