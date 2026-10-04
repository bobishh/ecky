//! Durable, fail-closed configuration persistence. Diagnostics contain stages only.

use crate::contracts::{
    decode_config, encode_config, normalize_legacy_config_for_edn, AppError, AppErrorCode,
    AppResult, Config, ConfigNormalizationWarning,
};
use crate::steel_data::{parse_steel_data, write_steel_data};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const CONFIG_EDN_FILE: &str = "config.edn";
pub const CONFIG_JSON_FILE: &str = "config.json";
pub const CONFIG_LOCK_FILE: &str = "config.lock";
const LOCK_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Edn,
    LegacyJson,
    Default,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyConfigMarkers {
    pub snake_case_fields: Vec<&'static str>,
    pub deprecated_start_on_demand: bool,
    pub max_verify_attempts_present: bool,
    pub mcp_mode_present: bool,
    pub primary_agent_id_present: bool,
    pub normalization_warnings: Vec<ConfigNormalizationWarning>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigLoadOutcome {
    pub config: Config,
    pub source: ConfigSource,
    pub legacy_markers: LegacyConfigMarkers,
    pub warnings: Vec<String>,
    pub cleanup_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub warnings: Vec<String>,
    pub cleanup_pending: bool,
}

trait PersistenceOps {
    fn temp_name(&self) -> String {
        format!(".config.edn.{}.tmp", uuid::Uuid::new_v4())
    }
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn sync_file(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn replace(&self, temp: &Path, target: &Path) -> AppResult<()> {
        replace_atomic(temp, target)
    }
    fn sync_parent(&self, dir: &Path) -> std::io::Result<()> {
        sync_parent_real(dir)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
}

struct RealOps;
impl PersistenceOps for RealOps {}

pub fn load_config<F>(
    config_dir: &Path,
    default: Config,
    migrate: F,
) -> AppResult<ConfigLoadOutcome>
where
    F: FnOnce(Config, LegacyConfigMarkers) -> AppResult<Config>,
{
    load_config_with_ops(config_dir, default, migrate, &RealOps)
}

fn load_config_with_ops<F, O>(
    config_dir: &Path,
    default: Config,
    migrate: F,
    ops: &O,
) -> AppResult<ConfigLoadOutcome>
where
    F: FnOnce(Config, LegacyConfigMarkers) -> AppResult<Config>,
    O: PersistenceOps,
{
    with_lock(config_dir, || {
        load_locked(config_dir, default, migrate, ops)
    })
}

pub fn save_config(config_dir: &Path, config: Config) -> AppResult<SaveOutcome> {
    save_config_transaction(config_dir, config, |_, outcome| Ok(outcome.clone()))
}

pub fn save_config_transaction<T, F>(config_dir: &Path, config: Config, commit: F) -> AppResult<T>
where
    F: FnOnce(&Config, &SaveOutcome) -> AppResult<T>,
{
    save_config_transaction_with_ops(config_dir, config, commit, &RealOps)
}

fn save_config_transaction_with_ops<T, F, O>(
    config_dir: &Path,
    mut config: Config,
    commit: F,
    ops: &O,
) -> AppResult<T>
where
    F: FnOnce(&Config, &SaveOutcome) -> AppResult<T>,
    O: PersistenceOps,
{
    with_lock(config_dir, || {
        config.jev_classifier.validate()?;
        let mut warnings = normalization_messages(normalize_legacy_config_for_edn(&mut config));
        write_canonical(config_dir, &config, &mut warnings, ops)?;
        let cleanup_pending = !delete_legacy_json(config_dir, &mut warnings, ops);
        let outcome = SaveOutcome {
            warnings,
            cleanup_pending,
        };
        commit(&config, &outcome).map_err(|_| {
            AppError::persistence("CONFIG_COMMITTED_CALLBACK_FAILED: config.edn committed")
        })
    })
}

fn load_locked<F, O>(
    config_dir: &Path,
    default: Config,
    migrate: F,
    ops: &O,
) -> AppResult<ConfigLoadOutcome>
where
    F: FnOnce(Config, LegacyConfigMarkers) -> AppResult<Config>,
    O: PersistenceOps,
{
    let edn = config_dir.join(CONFIG_EDN_FILE);
    let json = config_dir.join(CONFIG_JSON_FILE);
    if edn.exists() {
        let config = read_edn(&edn)?; // Never rescue a corrupt EDN with JSON.
        let mut warnings = Vec::new();
        let cleanup_pending = json.exists() && !delete_legacy_json(config_dir, &mut warnings, ops);
        return Ok(ConfigLoadOutcome {
            config,
            source: ConfigSource::Edn,
            legacy_markers: LegacyConfigMarkers::default(),
            warnings,
            cleanup_pending,
        });
    }
    if !json.exists() {
        return Ok(ConfigLoadOutcome {
            config: default,
            source: ConfigSource::Default,
            legacy_markers: LegacyConfigMarkers::default(),
            warnings: vec![],
            cleanup_pending: false,
        });
    }
    let raw = fs::read_to_string(&json).map_err(|_| persist("legacy-read"))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| AppError::parse("config.json: invalid-json"))?;
    let mut config: Config = serde_json::from_value(value.clone())
        .map_err(|_| AppError::parse("config.json: invalid-shape"))?;
    let mut markers = inventory_legacy(&value);
    if !markers.max_verify_attempts_present {
        config.max_verify_attempts = 2;
    }
    markers.normalization_warnings = normalize_legacy_config_for_edn(&mut config);
    config = migrate(config, markers.clone())?;
    markers
        .normalization_warnings
        .extend(normalize_legacy_config_for_edn(&mut config));
    let mut warnings = normalization_messages(markers.normalization_warnings.clone());
    write_canonical(config_dir, &config, &mut warnings, ops)?;
    let cleanup_pending = !delete_legacy_json(config_dir, &mut warnings, ops);
    Ok(ConfigLoadOutcome {
        config,
        source: ConfigSource::LegacyJson,
        legacy_markers: markers,
        warnings,
        cleanup_pending,
    })
}

fn read_edn(path: &Path) -> AppResult<Config> {
    let raw = fs::read_to_string(path).map_err(|_| persist("config.edn-read"))?;
    let value = parse_steel_data(&raw).map_err(|_| AppError::parse("config.edn: invalid-data"))?;
    decode_config(&value).map_err(|_| AppError::parse("config.edn: invalid-shape"))
}

fn write_canonical<O: PersistenceOps>(
    dir: &Path,
    config: &Config,
    _warnings: &mut Vec<String>,
    ops: &O,
) -> AppResult<()> {
    let value = encode_config(config).map_err(|error| {
        AppError::with_details(
            AppErrorCode::Persistence,
            "config.edn: encode",
            error.to_string(),
        )
    })?;
    let bytes = write_steel_data(&value).map_err(|error| {
        AppError::with_details(
            AppErrorCode::Persistence,
            "config.edn: serialize",
            error.to_string(),
        )
    })?;
    atomic_write(dir, bytes.as_bytes(), ops)?;
    let reopened = read_edn(&dir.join(CONFIG_EDN_FILE))
        .map_err(|_| AppError::persistence("config.edn: verify"))?;
    if &reopened != config {
        return Err(AppError::persistence("config.edn: verify-mismatch"));
    }
    Ok(())
}

fn atomic_write<O: PersistenceOps>(dir: &Path, bytes: &[u8], ops: &O) -> AppResult<()> {
    fs::create_dir_all(dir).map_err(|_| persist("config-dir-create"))?;
    let target = dir.join(CONFIG_EDN_FILE);
    let (temp, mut file) = create_private_temp(dir, ops)?;
    let result = ops
        .write_all(&mut file, bytes)
        .and_then(|()| ops.sync_file(&file));
    drop(file);
    if result.is_err() {
        let _ = ops.remove_file(&temp);
        return Err(persist("config.edn-atomic-write"));
    }
    if let Err(mut error) = ops.replace(&temp, &target) {
        if ops.remove_file(&temp).is_err() {
            error.details = Some(match error.details.take() {
                Some(details) => format!("{details}; config.edn-temp-cleanup"),
                None => "config.edn-temp-cleanup".into(),
            });
        }
        return Err(error);
    }
    match ops.sync_parent(dir) {
        Ok(()) => Ok(()),
        Err(_) => Err(AppError::persistence(
            "CONFIG_DURABILITY_UNCONFIRMED: config.edn parent-sync",
        )),
    }
}

#[cfg(unix)]
fn replace_atomic(temp: &Path, target: &Path) -> AppResult<()> {
    fs::rename(temp, target).map_err(|_| persist("config.edn-rename"))
}

#[cfg(windows)]
fn replace_atomic(temp: &Path, target: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // REPLACE_EXISTING + WRITE_THROUGH is Windows commit durability.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(persist("config.edn-rename"));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn replace_atomic(temp: &Path, target: &Path) -> AppResult<()> {
    fs::rename(temp, target).map_err(|_| persist("config.edn-rename"))
}

fn create_private_temp<O: PersistenceOps>(dir: &Path, ops: &O) -> AppResult<(PathBuf, File)> {
    for _ in 0..16 {
        let path = dir.join(ops.temp_name());
        let opened = private_create(&path);
        match opened {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(persist("config.edn-temp-create")),
        }
    }
    Err(persist("config.edn-temp-collision"))
}

#[cfg(unix)]
fn private_create(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
#[cfg(not(unix))]
fn private_create(path: &Path) -> std::io::Result<File> {
    // Windows relies on the current user's default ACL. POSIX 0600 has no ACL equivalent.
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn sync_parent_real(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir).and_then(|parent| parent.sync_all())
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn delete_legacy_json<O: PersistenceOps>(dir: &Path, warnings: &mut Vec<String>, ops: &O) -> bool {
    let path = dir.join(CONFIG_JSON_FILE);
    if !path.exists() {
        return true;
    }
    match ops.remove_file(&path) {
        Ok(()) => true,
        Err(_) => {
            warnings.push("config.json: cleanup-pending".into());
            false
        }
    }
}

fn inventory_legacy(value: &serde_json::Value) -> LegacyConfigMarkers {
    let mut markers = LegacyConfigMarkers::default();
    let Some(root) = value.as_object() else {
        return markers;
    };
    for (camel, snake) in [
        ("selectedEngineId", "selected_engine_id"),
        ("freecadCmd", "freecad_cmd"),
        ("cadTextFontPath", "cad_text_font_path"),
    ] {
        if root.contains_key(snake) && !root.contains_key(camel) {
            markers.snake_case_fields.push(snake);
        }
    }
    markers.max_verify_attempts_present =
        root.contains_key("maxVerifyAttempts") || root.contains_key("max_verify_attempts");
    let mcp = root.get("mcp").and_then(serde_json::Value::as_object);
    markers.mcp_mode_present = mcp.is_some_and(|mcp| mcp.contains_key("mode"));
    markers.primary_agent_id_present = mcp.is_some_and(|mcp| mcp.contains_key("primaryAgentId"));
    markers.deprecated_start_on_demand = root
        .get("mcp")
        .and_then(serde_json::Value::as_object)
        .and_then(|m| m.get("autoAgents").or_else(|| m.get("auto_agents")))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|xs| {
            xs.iter().any(|x| {
                x.get("startOnDemand")
                    .or_else(|| x.get("start_on_demand"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            })
        });
    markers
}

fn normalization_messages(warnings: Vec<ConfigNormalizationWarning>) -> Vec<String> {
    warnings
        .into_iter()
        .map(|w| format!("{}: {}", w.code, w.field))
        .collect()
}

fn persist(stage: &'static str) -> AppError {
    AppError::persistence(format!("config persistence failed: {stage}"))
}

fn with_lock<T>(dir: &Path, action: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    static IN_PROCESS: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = IN_PROCESS
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| persist("lock-poisoned"))?;
    fs::create_dir_all(dir).map_err(|_| persist("config-dir-create"))?;
    let lock_path = dir.join(CONFIG_LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(lock_path)
        .map_err(|_| persist("lock-open"))?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(AppError::conflict("config.lock: timeout"))
            }
            Err(_) => return Err(persist("lock-acquire")),
        }
    }
    let result = action();
    let _ = lock.unlock();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{EngineKind, GeometryBackend, McpConfig, SourceLanguage, VoiceConfig};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct TestOps {
        fail_parent_sync: bool,
        fail_json_delete: bool,
        fail_write: bool,
        fail_file_sync: bool,
        fail_rename: bool,
        temp_names: Mutex<VecDeque<String>>,
    }

    impl PersistenceOps for TestOps {
        fn temp_name(&self) -> String {
            self.temp_names
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| RealOps.temp_name())
        }
        fn write_all(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            if self.fail_write {
                Err(std::io::Error::other("injected"))
            } else {
                file.write_all(bytes)
            }
        }
        fn sync_file(&self, file: &File) -> std::io::Result<()> {
            if self.fail_file_sync {
                Err(std::io::Error::other("injected"))
            } else {
                file.sync_all()
            }
        }
        fn replace(&self, temp: &Path, target: &Path) -> AppResult<()> {
            if self.fail_rename {
                Err(persist("config.edn-rename"))
            } else {
                RealOps.replace(temp, target)
            }
        }
        fn sync_parent(&self, dir: &Path) -> std::io::Result<()> {
            if self.fail_parent_sync {
                Err(std::io::Error::other("injected"))
            } else {
                RealOps.sync_parent(dir)
            }
        }
        fn remove_file(&self, path: &Path) -> std::io::Result<()> {
            if self.fail_json_delete
                && path
                    .file_name()
                    .is_some_and(|name| name == CONFIG_JSON_FILE)
            {
                Err(std::io::Error::other("injected"))
            } else {
                fs::remove_file(path)
            }
        }
    }

    fn config(selected: &str) -> Config {
        Config {
            engines: vec![],
            selected_engine_id: selected.into(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: vec![],
            assets: vec![],
            microwave: None,
            voice: VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: false,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: EngineKind::EckyIrV0,
            default_source_language: SourceLanguage::EckyIrV0,
            default_geometry_backend: GeometryBackend::EckyRust,
            max_generation_attempts: 3,
            max_verify_attempts: 2,
            projects_root: None,
        }
    }
    fn dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ecky-config-store-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn finish(path: &Path) {
        fs::remove_dir_all(path).unwrap();
    }
    fn save_with_ops(path: &Path, config: Config, ops: &TestOps) -> AppResult<SaveOutcome> {
        save_config_transaction_with_ops(path, config, |_, outcome| Ok(outcome.clone()), ops)
    }
    #[test]
    fn bdd_json_only_backfills_edn_then_removes_json() {
        let path = dir("backfill");
        let expected = config("json");
        fs::write(
            path.join(CONFIG_JSON_FILE),
            serde_json::to_string(&expected).unwrap(),
        )
        .unwrap();
        let outcome = load_config(&path, config("default"), |config, _| Ok(config)).unwrap();
        assert_eq!(outcome.source, ConfigSource::LegacyJson);
        assert_eq!(outcome.config, expected);
        assert!(!path.join(CONFIG_JSON_FILE).exists());
        assert_eq!(read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(), expected);
        finish(&path);
    }

    #[test]
    fn bdd_invalid_json_is_preserved_without_edn() {
        let path = dir("invalid-json");
        fs::write(path.join(CONFIG_JSON_FILE), "{").unwrap();
        assert!(load_config(&path, config("default"), |config, _| Ok(config)).is_err());
        assert!(path.join(CONFIG_JSON_FILE).exists());
        assert!(!path.join(CONFIG_EDN_FILE).exists());
        finish(&path);
    }

    #[test]
    fn bdd_edn_wins_over_conflicting_json_and_save_cleans_json() {
        let path = dir("edn-wins");
        save_config(&path, config("edn")).unwrap();
        fs::write(
            path.join(CONFIG_JSON_FILE),
            serde_json::to_string(&config("json")).unwrap(),
        )
        .unwrap();
        let outcome = load_config(&path, config("default"), |config, _| Ok(config)).unwrap();
        assert_eq!(outcome.source, ConfigSource::Edn);
        assert_eq!(outcome.config.selected_engine_id, "edn");
        assert!(!path.join(CONFIG_JSON_FILE).exists());
        fs::write(path.join(CONFIG_JSON_FILE), "{sentinel-invalid-json").unwrap();
        let cleanup = load_config(&path, config("default"), |config, _| Ok(config)).unwrap();
        assert_eq!(cleanup.config.selected_engine_id, "edn");
        assert!(!cleanup.cleanup_pending);
        assert!(!path.join(CONFIG_JSON_FILE).exists());
        finish(&path);
    }

    #[test]
    fn bdd_invalid_edn_never_rescues_json_and_default_never_writes() {
        let path = dir("invalid-edn");
        fs::write(path.join(CONFIG_EDN_FILE), "[").unwrap();
        fs::write(
            path.join(CONFIG_JSON_FILE),
            serde_json::to_string(&config("json")).unwrap(),
        )
        .unwrap();
        assert!(load_config(&path, config("default"), |config, _| Ok(config)).is_err());
        assert!(path.join(CONFIG_JSON_FILE).exists());
        finish(&path);
        let empty = dir("default");
        let outcome = load_config(&empty, config("default"), |config, _| Ok(config)).unwrap();
        assert_eq!(outcome.source, ConfigSource::Default);
        assert!(!empty.join(CONFIG_EDN_FILE).exists());
        finish(&empty);
    }

    #[test]
    fn bdd_atomic_output_is_canonical_and_private() {
        let path = dir("atomic");
        let value = config("canonical");
        save_config(&path, value.clone()).unwrap();
        assert_eq!(read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(), value);
        assert!(fs::read_dir(&path).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path.join(CONFIG_EDN_FILE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o077,
                0
            );
        }
        finish(&path);
    }

    #[test]
    fn bdd_parent_sync_failure_reports_recovery_error_but_leaves_edn() {
        let path = dir("parent-sync");
        let ops = TestOps {
            fail_parent_sync: true,
            ..Default::default()
        };
        let error = save_with_ops(&path, config("committed"), &ops).unwrap_err();
        assert!(error.to_string().contains("CONFIG_DURABILITY_UNCONFIRMED"));
        assert_eq!(
            read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(),
            config("committed")
        );
        finish(&path);
    }

    #[test]
    fn bdd_stale_temp_collision_is_not_deleted_and_new_temp_publishes() {
        let path = dir("temp-collision");
        let stale = ".config.edn.stale.tmp";
        fs::write(path.join(stale), "keep").unwrap();
        let ops = TestOps {
            temp_names: Mutex::new(VecDeque::from([
                stale.into(),
                ".config.edn.fresh.tmp".into(),
            ])),
            ..Default::default()
        };
        save_with_ops(&path, config("fresh"), &ops).unwrap();
        assert_eq!(fs::read_to_string(path.join(stale)).unwrap(), "keep");
        assert!(!path.join(".config.edn.fresh.tmp").exists());
        finish(&path);
    }

    #[test]
    fn bdd_cleanup_pending_retries_on_save_and_snake_agent_normalizes() {
        let path = dir("cleanup-retry");
        let mut raw = serde_json::to_value(config("legacy")).unwrap();
        raw["selected_engine_id"] = raw["selectedEngineId"].take();
        raw.as_object_mut().unwrap().remove("selectedEngineId");
        raw["mcp"]["auto_agents"] = serde_json::json!([{"id":"a","label":"a","cmd":"c","args":[],"enabled":true,"start_on_demand":true}]);
        raw["mcp"].as_object_mut().unwrap().remove("autoAgents");
        fs::write(
            path.join(CONFIG_JSON_FILE),
            serde_json::to_string(&raw).unwrap(),
        )
        .unwrap();
        let failing_ops = TestOps {
            fail_json_delete: true,
            ..Default::default()
        };
        let first = load_config_with_ops(
            &path,
            config("default"),
            |config, _| Ok(config),
            &failing_ops,
        )
        .unwrap();
        assert!(first.cleanup_pending);
        assert!(first
            .legacy_markers
            .snake_case_fields
            .contains(&"selected_engine_id"));
        assert!(first.legacy_markers.deprecated_start_on_demand);
        assert!(!first.config.mcp.auto_agents[0].start_on_demand);
        assert!(!save_config(&path, config("retry")).unwrap().cleanup_pending);
        assert!(!path.join(CONFIG_JSON_FILE).exists());
        finish(&path);
    }

    #[test]
    fn bdd_lock_timeout_and_concurrent_writers_leave_canonical_config() {
        let path = dir("lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path.join(CONFIG_LOCK_FILE))
            .unwrap();
        lock.try_lock_exclusive().unwrap();
        let error = save_config(&path, config("blocked")).unwrap_err();
        assert_eq!(error.code, crate::contracts::AppErrorCode::Conflict);
        lock.unlock().unwrap();
        let first = path.clone();
        let second = path.clone();
        let one = std::thread::spawn(move || save_config(&first, config("one")));
        let two = std::thread::spawn(move || save_config(&second, config("two")));
        one.join().unwrap().unwrap();
        two.join().unwrap().unwrap();
        let final_config = read_edn(&path.join(CONFIG_EDN_FILE)).unwrap();
        assert!(matches!(
            final_config.selected_engine_id.as_str(),
            "one" | "two"
        ));
        finish(&path);
    }

    #[cfg(unix)]
    #[test]
    fn bdd_cross_process_lock_blocks_writer_then_releases_after_owner_exit() {
        let path = dir("cross-process-lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path.join(CONFIG_LOCK_FILE))
            .unwrap();
        lock.try_lock_exclusive().unwrap();

        let child = unsafe { libc::fork() };
        assert!(child >= 0, "fork must succeed");
        if child == 0 {
            let status = if save_config(&path, config("child")).is_ok() {
                0
            } else {
                1
            };
            unsafe { libc::_exit(status) };
        }

        thread::sleep(Duration::from_millis(50));
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) },
            0
        );
        lock.unlock().unwrap();
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert_eq!(
            read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(),
            config("child")
        );

        let stale_owner = unsafe { libc::fork() };
        assert!(stale_owner >= 0, "fork must succeed");
        if stale_owner == 0 {
            let stale_lock = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path.join(CONFIG_LOCK_FILE))
                .unwrap();
            stale_lock.try_lock_exclusive().unwrap();
            unsafe { libc::_exit(0) };
        }
        assert_eq!(
            unsafe { libc::waitpid(stale_owner, &mut status, 0) },
            stale_owner
        );
        save_config(&path, config("after-stale-owner-exit")).unwrap();
        assert_eq!(
            read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(),
            config("after-stale-owner-exit")
        );
        finish(&path);
    }

    #[cfg(unix)]
    #[test]
    fn bdd_two_process_writers_serialize_and_last_durable_writer_wins() {
        struct PauseBeforeWriteOps {
            entered: PathBuf,
            release: PathBuf,
        }

        impl PersistenceOps for PauseBeforeWriteOps {
            fn write_all(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
                fs::write(&self.entered, "entered")?;
                let deadline = Instant::now() + Duration::from_secs(30);
                while !self.release.exists() {
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "test pause timed out",
                        ));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                file.write_all(bytes)
            }
        }

        fn wait_for(path: &Path) {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !path.exists() {
                assert!(Instant::now() < deadline, "timed out waiting for {path:?}");
                thread::sleep(Duration::from_millis(5));
            }
        }

        let path = dir("two-process-writers");
        let entered = path.join("first-entered");
        let release = path.join("release-first");
        let first = unsafe { libc::fork() };
        assert!(first >= 0, "fork must succeed");
        if first == 0 {
            let ops = PauseBeforeWriteOps {
                entered: entered.clone(),
                release: release.clone(),
            };
            let status =
                if save_config_transaction_with_ops(&path, config("first"), |_, _| Ok(()), &ops)
                    .is_ok()
                {
                    0
                } else {
                    1
                };
            unsafe { libc::_exit(status) };
        }
        wait_for(&entered);

        let second_started = path.join("second-started");
        let second = unsafe { libc::fork() };
        assert!(second >= 0, "fork must succeed");
        if second == 0 {
            fs::write(&second_started, "started").unwrap();
            let status = if save_config(&path, config("second")).is_ok() {
                0
            } else {
                1
            };
            unsafe { libc::_exit(status) };
        }
        wait_for(&second_started);
        thread::sleep(Duration::from_millis(50));
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(second, &mut status, libc::WNOHANG) },
            0
        );
        fs::write(&release, "release").unwrap();
        assert_eq!(unsafe { libc::waitpid(first, &mut status, 0) }, first);
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert_eq!(unsafe { libc::waitpid(second, &mut status, 0) }, second);
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert_eq!(
            read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(),
            config("second")
        );
        finish(&path);
    }

    #[test]
    fn bdd_paused_backfill_blocks_newer_save_then_newer_durable_write_wins() {
        let path = dir("paused-backfill");
        fs::write(
            path.join(CONFIG_JSON_FILE),
            serde_json::to_string(&config("legacy")).unwrap(),
        )
        .unwrap();
        let (backfill_entered_tx, backfill_entered_rx) = std::sync::mpsc::channel();
        let (release_backfill_tx, release_backfill_rx) = std::sync::mpsc::channel();
        let loader_path = path.clone();
        let loader = std::thread::spawn(move || {
            load_config(&loader_path, config("default"), |config, _| {
                backfill_entered_tx.send(()).unwrap();
                release_backfill_rx.recv().unwrap();
                Ok(config)
            })
        });
        backfill_entered_rx.recv().unwrap();

        let (save_done_tx, save_done_rx) = std::sync::mpsc::channel();
        let saver_path = path.clone();
        let saver = std::thread::spawn(move || {
            let result = save_config(&saver_path, config("newer"));
            save_done_tx.send(result).unwrap();
        });
        assert!(save_done_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release_backfill_tx.send(()).unwrap();
        loader.join().unwrap().unwrap();
        save_done_rx.recv().unwrap().unwrap();
        saver.join().unwrap();
        assert_eq!(
            read_edn(&path.join(CONFIG_EDN_FILE)).unwrap(),
            config("newer")
        );
        assert!(!path.join(CONFIG_JSON_FILE).exists());
        finish(&path);
    }

    #[test]
    fn bdd_concurrent_transactions_keep_disk_and_committed_memory_on_last_locked_writer() {
        let path = dir("transactions");
        let memory = std::sync::Arc::new(Mutex::new(String::new()));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first_path = path.clone();
        let first_memory = memory.clone();
        let first = std::thread::spawn(move || {
            save_config_transaction(&first_path, config("one"), |config, _| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                *first_memory.lock().unwrap() = config.selected_engine_id.clone();
                Ok(())
            })
        });
        entered_rx.recv().unwrap();
        let second_path = path.clone();
        let second_memory = memory.clone();
        let second = std::thread::spawn(move || {
            save_config_transaction(&second_path, config("two"), |config, _| {
                *second_memory.lock().unwrap() = config.selected_engine_id.clone();
                Ok(())
            })
        });
        release_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        let disk = read_edn(&path.join(CONFIG_EDN_FILE)).unwrap();
        assert_eq!(disk.selected_engine_id, "two");
        assert_eq!(*memory.lock().unwrap(), "two");
        finish(&path);
    }

    #[test]
    fn bdd_failed_commit_callback_reports_committed_state_without_panic() {
        let path = dir("callback-failure");
        let error = save_config_transaction(&path, config("disk"), |_, _| {
            Err::<(), _>(AppError::internal("sentinel-secret"))
        })
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("CONFIG_COMMITTED_CALLBACK_FAILED"));
        assert!(!error.to_string().contains("sentinel-secret"));
        assert_eq!(
            read_edn(&path.join(CONFIG_EDN_FILE))
                .unwrap()
                .selected_engine_id,
            "disk"
        );
        finish(&path);
    }

    #[test]
    fn bdd_precommit_failures_never_swap_committed_memory() {
        for (name, ops) in [
            (
                "write",
                TestOps {
                    fail_write: true,
                    ..Default::default()
                },
            ),
            (
                "rename",
                TestOps {
                    fail_rename: true,
                    ..Default::default()
                },
            ),
            (
                "parent-sync",
                TestOps {
                    fail_parent_sync: true,
                    ..Default::default()
                },
            ),
        ] {
            let path = dir(name);
            save_config(&path, config("old")).unwrap();
            let memory = std::sync::Arc::new(Mutex::new(config("old")));
            let committed = memory.clone();
            assert!(save_config_transaction_with_ops(
                &path,
                config("new"),
                move |config, _| {
                    *committed.lock().unwrap() = config.clone();
                    Ok(())
                },
                &ops,
            )
            .is_err());
            assert_eq!(*memory.lock().unwrap(), config("old"));
            finish(&path);
        }
    }

    #[test]
    fn bdd_temp_write_or_sync_failure_keeps_old_destination_and_leaves_no_owned_temp() {
        for (name, write_failure) in [("write", true), ("sync", false)] {
            let path = dir(name);
            save_config(&path, config("old")).unwrap();
            let before = fs::read(path.join(CONFIG_EDN_FILE)).unwrap();
            let ops = TestOps {
                fail_write: write_failure,
                fail_file_sync: !write_failure,
                ..Default::default()
            };
            assert!(save_with_ops(&path, config("new"), &ops).is_err());
            assert_eq!(fs::read(path.join(CONFIG_EDN_FILE)).unwrap(), before);
            assert!(fs::read_dir(&path).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
            finish(&path);
        }
    }

    #[test]
    fn bdd_rename_failure_keeps_old_destination_and_removes_owned_temp() {
        let path = dir("rename-failure");
        save_config(&path, config("old")).unwrap();
        let before = fs::read(path.join(CONFIG_EDN_FILE)).unwrap();
        let ops = TestOps {
            fail_rename: true,
            ..Default::default()
        };
        let error = save_with_ops(&path, config("new"), &ops).unwrap_err();
        assert!(error.to_string().contains("config.edn-rename"));
        assert_eq!(fs::read(path.join(CONFIG_EDN_FILE)).unwrap(), before);
        assert!(fs::read_dir(&path).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        finish(&path);
    }

    #[test]
    fn bdd_json_max_verify_aliases_and_absent_default_reach_migration() {
        for (name, field, expected) in [
            ("camel", Some("maxVerifyAttempts"), 7),
            ("snake", Some("max_verify_attempts"), 8),
            ("absent", None, 2),
        ] {
            let path = dir(name);
            let mut raw = serde_json::to_value(config("legacy")).unwrap();
            raw.as_object_mut().unwrap().remove("maxVerifyAttempts");
            if let Some(field) = field {
                raw[field] = serde_json::json!(expected);
            }
            fs::write(
                path.join(CONFIG_JSON_FILE),
                serde_json::to_string(&raw).unwrap(),
            )
            .unwrap();
            let outcome = load_config(&path, config("default"), |config, _| {
                assert_eq!(config.max_verify_attempts, expected);
                Ok(config)
            })
            .unwrap();
            assert_eq!(outcome.config.max_verify_attempts, expected);
            finish(&path);
        }
    }

    #[test]
    fn bdd_persistence_diagnostic_matrix_redacts_every_public_sink() {
        let path = dir("diagnostics");
        let secret = "sentinel-api-key-agent-arg";
        let sensitive_path = "/private/sentinel-config-path";
        fs::write(
            path.join(CONFIG_EDN_FILE),
            format!("{{:bad \"{secret}\" :path \"{sensitive_path}\"}}"),
        )
        .unwrap();
        let error = load_config(&path, config("default"), |config, _| Ok(config)).unwrap_err();
        let tauri = serde_json::to_value(&error).unwrap();
        let message = tauri["message"].as_str().unwrap().to_owned();
        let details = tauri
            .get("details")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let stdout = format!("config save: {error}");
        let stderr = format!("config save failed: {error}");
        let log = format!("config persistence: {error}");
        for (sink, requires_location) in [
            (stdout, true),
            (stderr, true),
            (log, true),
            (message, true),
            (details, false),
            (serde_json::to_string(&tauri).unwrap(), true),
            (format!("{error:?}"), true),
        ] {
            assert!(!sink.contains(secret));
            assert!(!sink.contains(sensitive_path));
            if requires_location {
                assert!(sink.contains("config.edn"));
            }
        }
        finish(&path);

        let cleanup = dir("diagnostic-cleanup");
        fs::create_dir_all(cleanup.join(CONFIG_JSON_FILE)).unwrap();
        let outcome = save_config(&cleanup, config("safe")).unwrap();
        assert_eq!(outcome.warnings, vec!["config.json: cleanup-pending"]);
        for sink in [
            format!("config save: {:?}", outcome.warnings),
            format!("config save failed: {:?}", outcome.warnings),
            format!("config persistence: {:?}", outcome.warnings),
            serde_json::to_string(&outcome.warnings).unwrap(),
        ] {
            assert!(!sink.contains(secret));
            assert!(!sink.contains(sensitive_path));
            assert_eq!(sink.matches("config.json").count(), 1);
        }
        fs::remove_dir_all(cleanup).unwrap();
    }
}
