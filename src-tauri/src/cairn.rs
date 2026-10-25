//! Shared implementation of the `cairn` and `ecky cairn` client commands.
use crate::{
    component_package_runtime as packages,
    contracts::{
        AppError, AppResult, CairnDependency, CairnLock, CairnLockedRelease,
        ComponentDependencyLock, ComponentPackage,
    },
    models::PathResolver,
};
use cairn_registry::Release;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const DEFAULT_REGISTRY: &str = "https://hub.ecky-cad.com";

pub fn configured_registry(config: &crate::contracts::Config) -> &str {
    config
        .cairn
        .as_ref()
        .map(|cairn| cairn.registry_url.as_str())
        .unwrap_or(DEFAULT_REGISTRY)
}

pub const USAGE:&str="Cairn — reusable stones for Ecky\n\nCommands: configure --registry <url>, search [query], info <package>, init <package>, pack --out <archive>, publish <archive>, add <package>@<version>, install [--frozen] [--offline], update <package>@<version>, index [--force] [--embed]\nOptions: --registry <url>, --project <dir>, --data-dir <dir>, --local, --json\nRegistry writes read CAIRN_PUBLISH_TOKEN_FILE or CAIRN_PUBLISH_TOKEN.\n";
fn persistence(error: impl std::fmt::Display) -> AppError {
    AppError::persistence(format!("Cairn: {error}"))
}
fn usage(error: impl std::fmt::Display) -> AppError {
    AppError::validation(format!("{error}\n\n{USAGE}"))
}

#[derive(Clone)]
pub struct CliPaths {
    pub data_dir: PathBuf,
    config_dir: PathBuf,
}
impl CliPaths {
    pub fn discover() -> AppResult<Self> {
        if let Some(path) = std::env::var_os("ECKY_DATA_DIR") {
            let data_dir = PathBuf::from(path);
            return Ok(Self {
                config_dir: std::env::var_os("ECKY_CONFIG_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| data_dir.join("config")),
                data_dir,
            });
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| usage("Set ECKY_DATA_DIR when HOME is unavailable"))?;
        #[cfg(target_os = "macos")]
        let data_dir = home.join("Library/Application Support/com.alcoholics-audacious.ecky-cad");
        #[cfg(not(target_os = "macos"))]
        let data_dir = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("com.alcoholics-audacious.ecky-cad");
        #[cfg(target_os = "macos")]
        let config_dir = data_dir.clone();
        #[cfg(not(target_os = "macos"))]
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("com.alcoholics-audacious.ecky-cad");
        Ok(Self {
            data_dir,
            config_dir: std::env::var_os("ECKY_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or(config_dir),
        })
    }
}
impl PathResolver for CliPaths {
    fn app_data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
    fn app_config_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }
    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        packaged_resource_path(path)
    }
}
struct Options {
    words: Vec<String>,
    project: PathBuf,
    paths: CliPaths,
    registry: String,
    offline: bool,
    frozen: bool,
    local: bool,
    force: bool,
    embed: bool,
    output: Option<PathBuf>,
}
impl Options {
    fn parse(arguments: &[String]) -> AppResult<Self> {
        let mut value = Self {
            words: vec![],
            project: std::env::current_dir().map_err(persistence)?,
            paths: CliPaths::discover()?,
            registry: std::env::var("CAIRN_REGISTRY").unwrap_or_else(|_| DEFAULT_REGISTRY.into()),
            offline: false,
            frozen: false,
            local: false,
            force: false,
            embed: false,
            output: None,
        };
        let mut args = arguments.iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--project" => {
                    value.project = PathBuf::from(
                        args.next()
                            .ok_or_else(|| usage("--project needs a directory"))?,
                    )
                }
                "--data-dir" => {
                    value.paths.data_dir = PathBuf::from(
                        args.next()
                            .ok_or_else(|| usage("--data-dir needs a directory"))?,
                    );
                    value.paths.config_dir = value.paths.data_dir.join("config");
                }
                "--registry" => {
                    value.registry = args
                        .next()
                        .ok_or_else(|| usage("--registry needs a URL"))?
                        .trim_end_matches('/')
                        .to_string()
                }
                "--out" => {
                    value.output = Some(PathBuf::from(
                        args.next().ok_or_else(|| usage("--out needs a path"))?,
                    ))
                }
                "--offline" => value.offline = true,
                "--frozen" => value.frozen = true,
                "--local" => value.local = true,
                "--force" => value.force = true,
                "--embed" => value.embed = true,
                "--json" => {}
                "--help" | "-h" => value.words = vec!["help".into()],
                word if word.starts_with('-') => {
                    return Err(usage(format!("Unknown option {word}")))
                }
                word => value.words.push(word.into()),
            }
        }
        if std::env::var_os("CAIRN_REGISTRY").is_none()
            && !arguments.iter().any(|argument| argument == "--registry")
        {
            if let Some(config) = read_cli_config(&value.paths)? {
                value.registry = configured_registry(&config).to_string();
            }
        }
        Ok(value)
    }
}
fn read_cli_config(paths: &CliPaths) -> AppResult<Option<crate::contracts::Config>> {
    let path = paths.app_config_dir().join("config.edn");
    if !path.exists() {
        return Ok(None);
    }
    let data =
        crate::steel_data::parse_steel_data(&std::fs::read_to_string(path).map_err(persistence)?)
            .map_err(persistence)?;
    crate::contracts::decode_config(&data).map(Some)
}

pub fn packaged_resource_path(path: &str) -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("ECKY_RESOURCE_DIR") {
        return Some(PathBuf::from(root).join(path));
    }
    let executable = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    let file = executable.parent()?.parent()?.join("resources").join(path);
    file.exists().then_some(file)
}

fn dependency(word: &str) -> AppResult<CairnDependency> {
    let (package_id, version) = word
        .rsplit_once('@')
        .ok_or_else(|| usage("Use an exact package@version coordinate"))?;
    let dependency = CairnDependency {
        package_id: package_id.into(),
        version: version.into(),
    };
    crate::contracts::validate_cairn_dependencies(std::slice::from_ref(&dependency))?;
    Ok(dependency)
}
fn token() -> AppResult<String> {
    let value = if let Some(path) = std::env::var_os("CAIRN_PUBLISH_TOKEN_FILE") {
        std::fs::read_to_string(path).map_err(persistence)?
    } else {
        std::env::var("CAIRN_PUBLISH_TOKEN")
            .map_err(|_| usage("Set CAIRN_PUBLISH_TOKEN_FILE to publish"))?
    };
    if value.trim().len() < 24 {
        return Err(usage("Publish credential must contain at least 24 bytes"));
    }
    Ok(value.trim().into())
}

struct RegistryClient<'a> {
    app: &'a dyn PathResolver,
    registry: &'a str,
    http: reqwest::Client,
    offline: bool,
}
impl<'a> RegistryClient<'a> {
    fn new(app: &'a dyn PathResolver, registry: &'a str, offline: bool) -> AppResult<Self> {
        crate::contracts::validate_cairn_registry(registry)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(persistence)?;
        Ok(Self {
            app,
            registry,
            http,
            offline,
        })
    }
    async fn body(mut response: reqwest::Response, limit: usize) -> AppResult<Vec<u8>> {
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(AppError::validation(
                "Registry response exceeds transfer limit",
            ));
        }
        let status = response.status();
        let mut result = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(persistence)? {
            if result.len().saturating_add(chunk.len()) > limit {
                return Err(AppError::validation(
                    "Registry response exceeds transfer limit",
                ));
            }
            result.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(AppError::validation(format!(
                "Registry HTTP {status}: {}",
                String::from_utf8_lossy(&result)
                    .chars()
                    .take(1000)
                    .collect::<String>()
            )));
        }
        Ok(result)
    }
    async fn metadata(&self, url: &str) -> AppResult<Value> {
        let cached: Option<(Option<String>, String)> = {
            let index = packages::open_component_index(self.app)?;
            index
                .connection()
                .query_row(
                    "SELECT etag,value FROM cairn_metadata_cache WHERE url=?1",
                    [url],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(persistence)?
        };
        if self.offline {
            return cached
                .map(|(_, body)| serde_json::from_str(&body).map_err(persistence))
                .transpose()?
                .ok_or_else(|| {
                    AppError::not_found(
                        "Metadata is not cached; reconnect or install from an existing frozen lock",
                    )
                });
        }
        let mut request = self.http.get(url);
        if let Some((Some(etag), _)) = &cached {
            request = request.header("if-none-match", etag);
        }
        let response = request.send().await.map_err(persistence)?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return serde_json::from_str(
                &cached
                    .ok_or_else(|| persistence("Registry returned 304 without cached metadata"))?
                    .1,
            )
            .map_err(persistence);
        }
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = Self::body(response, 2 * 1024 * 1024).await?;
        let value: Value = serde_json::from_slice(&body).map_err(persistence)?;
        let index = packages::open_component_index(self.app)?;
        index.connection().execute("INSERT INTO cairn_metadata_cache VALUES(?1,?2,?3) ON CONFLICT(url) DO UPDATE SET etag=excluded.etag,value=excluded.value",params![url,etag,serde_json::to_string(&value).map_err(persistence)?]).map_err(persistence)?;
        Ok(value)
    }
    async fn release(&self, dependency: &CairnDependency) -> AppResult<Release> {
        let value = self
            .metadata(&format!(
                "{}/v1/packages/{}/{}",
                self.registry, dependency.package_id, dependency.version
            ))
            .await?;
        let releases: Vec<Release> =
            serde_json::from_value(value["releases"].clone()).map_err(persistence)?;
        let release = releases
            .into_iter()
            .find(|release| {
                release.package_id == dependency.package_id && release.version == dependency.version
            })
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "Release {}@{} is unavailable",
                    dependency.package_id, dependency.version
                ))
            })?;
        if release.yanked {
            return Err(AppError::validation(
                "Release is yanked; existing frozen locks remain installable",
            ));
        }
        Ok(release)
    }
    async fn resolve(&self, roots: &[CairnDependency]) -> AppResult<CairnLock> {
        crate::contracts::validate_cairn_dependencies(roots)?;
        let mut pending = roots.to_vec();
        let mut resolved = BTreeMap::<String, Release>::new();
        while let Some(dependency) = pending.pop() {
            if let Some(release) = resolved.get(&dependency.package_id) {
                if release.version != dependency.version {
                    return Err(AppError::validation(format!(
                        "Conflicting exact versions for {}",
                        dependency.package_id
                    )));
                }
                continue;
            }
            if resolved.len() >= 128 {
                return Err(AppError::validation("Cairn graph exceeds 128 releases"));
            }
            let release = self.release(&dependency).await?;
            pending.extend(release.distribution.dependencies.iter().cloned());
            resolved.insert(dependency.package_id, release);
        }
        let lock = CairnLock {
            registry: self.registry.into(),
            roots: roots.to_vec(),
            releases: resolved
                .into_values()
                .map(|release| CairnLockedRelease {
                    package_id: release.package_id,
                    version: release.version,
                    archive_digest: release.archive_digest,
                    package_digest: release.package_digest,
                    dependencies: release.distribution.dependencies,
                })
                .collect(),
        }
        .canonical();
        lock.validate()?;
        Ok(lock)
    }
    async fn archive(&self, release: &CairnLockedRelease) -> AppResult<PathBuf> {
        if !cairn_registry::valid_digest(&release.archive_digest) {
            return Err(AppError::validation("Invalid locked archive digest"));
        }
        let path = self
            .app
            .app_data_dir()
            .join("cairn-cache/archives")
            .join(format!("{}.ecky-package", &release.archive_digest[7..]));
        if path.exists() {
            if std::fs::metadata(&path).map_err(persistence)?.len()
                > cairn_registry::MAX_ARCHIVE_BYTES as u64
            {
                return Err(AppError::validation(
                    "Cached archive exceeds transfer limit",
                ));
            }
            verify_archive(&std::fs::read(&path).map_err(persistence)?, release)?;
            return Ok(path);
        }
        if self.offline {
            return Err(AppError::not_found(format!(
                "Archive {}@{} is not cached; reconnect before offline installation",
                release.package_id, release.version
            )));
        }
        let response = self
            .http
            .get(format!(
                "{}/v1/blobs/{}",
                self.registry, release.archive_digest
            ))
            .send()
            .await
            .map_err(persistence)?;
        let bytes = Self::body(response, cairn_registry::MAX_ARCHIVE_BYTES).await?;
        verify_archive(&bytes, release)?;
        packages::write_atomic(&path, &bytes)?;
        Ok(path)
    }
}
fn verify_archive(bytes: &[u8], locked: &CairnLockedRelease) -> AppResult<()> {
    let release = cairn_registry::inspect_archive(bytes).map_err(AppError::validation)?;
    if release.package_id != locked.package_id
        || release.version != locked.version
        || release.archive_digest != locked.archive_digest
        || release.package_digest != locked.package_digest
        || sorted_dependencies(&release.distribution.dependencies)
            != sorted_dependencies(&locked.dependencies)
    {
        return Err(AppError::validation(format!(
            "Archive integrity or dependency mismatch for {}@{}",
            locked.package_id, locked.version
        )));
    }
    Ok(())
}

struct PinnedResolver<'a> {
    app: &'a dyn PathResolver,
    graph: &'a CairnLock,
}
impl PinnedResolver<'_> {
    fn digest(&self, coordinate: &crate::contracts::ComponentCoordinate) -> AppResult<&str> {
        self.graph
            .releases
            .iter()
            .find(|release| {
                release.package_id == coordinate.package_id && release.version == coordinate.version
            })
            .map(|release| release.package_digest.as_str())
            .ok_or_else(|| {
                AppError::validation(format!(
                    "Import {} is outside declared packages; use cairn add first",
                    coordinate.canonical_identity()
                ))
            })
    }
}
impl crate::component_import_runtime::InstalledComponentResolver for PinnedResolver<'_> {
    fn resolve_source_component(
        &self,
        coordinate: &crate::contracts::ComponentCoordinate,
    ) -> AppResult<crate::component_import_runtime::ResolvedSourceComponent> {
        crate::component_import_runtime::InstalledLibraryComponentResolver { app: self.app }
            .resolve_source_component_with_expected_digest(coordinate, self.digest(coordinate)?)
    }
    fn resolve_step_component(
        &self,
        coordinate: &crate::contracts::ComponentCoordinate,
    ) -> AppResult<Option<crate::component_import_runtime::ResolvedStepComponent>> {
        crate::component_import_runtime::InstalledLibraryComponentResolver { app: self.app }
            .resolve_step_component_with_expected_digest(coordinate, self.digest(coordinate)?)
    }
}
fn sorted_dependencies(dependencies: &[CairnDependency]) -> Vec<CairnDependency> {
    let mut values = dependencies.to_vec();
    values.sort_by(|a, b| {
        a.package_id
            .cmp(&b.package_id)
            .then(a.version.cmp(&b.version))
    });
    values
}
async fn install(
    options: &Options,
    manifest: &ComponentPackage,
    refresh: bool,
) -> AppResult<ComponentDependencyLock> {
    let roots = manifest
        .cairn
        .as_ref()
        .map(|cairn| cairn.dependencies.as_slice())
        .unwrap_or_default();
    crate::contracts::validate_cairn_dependencies(roots)?;
    let previous = crate::project_mirror::read_project_lock(&options.project)?;
    let pinned = previous.as_ref().and_then(|lock| lock.cairn.as_ref());
    let declarations_match = pinned
        .is_some_and(|pinned| sorted_dependencies(&pinned.roots) == sorted_dependencies(roots));
    if options.frozen && (!declarations_match || refresh) {
        return Err(AppError::validation("Frozen install requires a lock matching declared exact dependencies; run cairn install after reviewing changes"));
    }
    let graph = if !refresh && declarations_match {
        pinned
            .cloned()
            .ok_or_else(|| persistence("Missing pinned graph"))?
    } else {
        RegistryClient::new(&options.paths, &options.registry, options.offline)?
            .resolve(roots)
            .await?
    };
    graph.validate()?;
    let client = RegistryClient::new(&options.paths, &graph.registry, options.offline)?;
    let mut archives = Vec::new();
    for release in &graph.releases {
        archives.push(client.archive(release).await?);
    }
    let app = options.paths.clone();
    // Package validation/immutable writes never occupy the asynchronous network executor.
    let installed = tokio::task::spawn_blocking(move || {
        packages::install_component_package_archives_to_store(&app, &archives)
    })
    .await
    .map_err(persistence)??;
    if installed.iter().any(|installed| {
        !graph.releases.iter().any(|release| {
            release.package_id == installed.package_id
                && release.version == installed.version
                && release.package_digest == installed.package_digest
        })
    }) {
        return Err(AppError::validation(
            "Installed payload differs from resolved lock",
        ));
    }
    let source_path = options
        .project
        .join(crate::project_mirror::PROJECT_SOURCE_FILE_NAME);
    let source = if source_path.is_file() {
        std::fs::read_to_string(source_path).map_err(persistence)?
    } else {
        String::new()
    };
    let resolved = crate::component_import_runtime::resolve_authoring_source(
        crate::component_import_runtime::ResolveAuthoringSourceRequest {
            authored_source: &source,
            expected_lock: None,
        },
        &PinnedResolver {
            app: &options.paths,
            graph: &graph,
        },
    )?;
    let mut lock = resolved.dependency_lock;
    lock.cairn = Some(graph);
    let lock = lock.canonical();
    lock.validate()?;
    if options.frozen && previous.as_ref() != Some(&lock) {
        return Err(AppError::validation("Frozen install cannot rewrite model component references; run cairn install explicitly after source changes"));
    }
    Ok(lock)
}
fn save_install(
    project: &Path,
    manifest: &ComponentPackage,
    lock: &ComponentDependencyLock,
    write_manifest: bool,
) -> AppResult<()> {
    let manifest_path = project.join(packages::COMPONENT_PACKAGE_FILE_NAME);
    let previous_manifest = std::fs::read(&manifest_path).ok();
    if write_manifest {
        crate::contracts::validate_component_package(manifest)?;
        packages::write_atomic(
            &manifest_path,
            &serde_json::to_vec_pretty(manifest).map_err(persistence)?,
        )?;
    }
    let current = crate::project_mirror::read_project_lock(project)?;
    if current.as_ref() != Some(lock) {
        if let Err(error) = crate::project_mirror::write_project_lock(project, lock) {
            if write_manifest {
                if let Some(previous) = previous_manifest {
                    packages::write_atomic(&manifest_path, &previous)?;
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

/// Explicit recovery reads source/compact locks from history without modifying history or rendering.
async fn index(options: &Options) -> AppResult<Value> {
    use crate::component_library_index::{ComponentIndexPhase, ComponentIndexStatus};
    let app = Arc::new(options.paths.clone());
    let force = options.force;
    let paths = app.clone();
    let indexed = tokio::task::spawn_blocking(move || -> AppResult<usize> {
        let index = packages::open_component_index(paths.as_ref())?;
        let history = paths.app_data_dir().join("history.sqlite");
        if !history.is_file() {
            return Ok(0);
        }
        let conn = rusqlite::Connection::open_with_flags(
            &history,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(persistence)?;
        conn.busy_timeout(Duration::from_secs(3))
            .map_err(persistence)?;
        let targets =
            crate::db::latest_successful_component_capture_targets(&conn).map_err(persistence)?;
        let mut status = ComponentIndexStatus {
            phase: ComponentIndexPhase::Indexing,
            completed: 0,
            total: targets.len().try_into().unwrap_or(u32::MAX),
            error: None,
        };
        index.set_status(&status)?;
        for target in &targets {
            if force
                || index
                    .capture_diagnostics(&target.thread_id, &target.message_id)?
                    .is_none()
            {
                let capture = (|| {
                    let input = crate::db::get_component_capture_input(
                        &conn,
                        &target.thread_id,
                        &target.message_id,
                    )
                    .map_err(persistence)?
                    .ok_or_else(|| AppError::not_found("Successful model source is unavailable"))?;
                    let capture = packages::prepare_component_capture_with_lock(
                        paths.as_ref(),
                        &target.thread_id,
                        &target.message_id,
                        &input.source,
                        input.lock.as_ref(),
                        input.source_language,
                        input.geometry_backend,
                    )?;
                    packages::publish_latest_component_capture(
                        paths.as_ref(),
                        &conn,
                        &target.thread_id,
                        &target.message_id,
                        capture,
                    )
                })();
                if let Err(error) = capture {
                    if crate::db::is_latest_successful_component_capture(
                        &conn,
                        &target.thread_id,
                        &target.message_id,
                    )
                    .map_err(persistence)?
                        && index
                            .capture_diagnostics(&target.thread_id, &target.message_id)?
                            .is_none()
                    {
                        index.record_failure(
                            &target.thread_id,
                            &target.message_id,
                            &error.to_string(),
                        )?;
                    }
                }
            }
            if let Some(title) =
                crate::db::get_thread_title(&conn, &target.thread_id).map_err(persistence)?
            {
                index.set_thread_title(&target.thread_id, &title)?;
            }
            status.completed += 1;
            index.set_status(&status)?;
        }
        let latest =
            crate::db::latest_successful_component_capture_targets(&conn).map_err(persistence)?;
        packages::open_component_index(paths.as_ref())?.retain_threads(
            &latest
                .into_iter()
                .map(|target| target.thread_id)
                .collect::<Vec<_>>(),
        )?;
        status.phase = if index.diagnostics()?.is_empty() {
            ComponentIndexPhase::Ready
        } else {
            ComponentIndexPhase::Failed
        };
        index.set_status(&status)?;
        Ok(targets.len())
    })
    .await
    .map_err(persistence)??;
    let embedded = if options.embed {
        crate::catalog::refresh_embeddings(app.as_ref()).await?
    } else {
        0
    };
    let index = packages::open_component_index(app.as_ref())?;
    Ok(
        json!({"indexedModels":indexed,"embeddedDescriptors":embedded,"status":index.status()?,"diagnostics":index.diagnostics()?}),
    )
}

pub fn version_text() -> String {
    let revision = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .and_then(|path| path.parent()?.parent().map(|root| root.join("VERSION")))
        .and_then(|path| std::fs::read_to_string(path).ok());
    match revision {
        Some(revision) => format!("{} ({})", env!("CARGO_PKG_VERSION"), revision.trim()),
        None => env!("CARGO_PKG_VERSION").to_string(),
    }
}

pub async fn run(arguments: &[String]) -> AppResult<Value> {
    if matches!(arguments,[argument] if argument=="--version" || argument=="-V") {
        return Ok(json!({"version":version_text()}));
    }
    let options = Options::parse(arguments)?;
    let command = options.words.first().map(String::as_str).unwrap_or("help");
    let word = |position: usize| {
        options
            .words
            .get(position)
            .map(String::as_str)
            .ok_or_else(|| usage(format!("{command} needs an argument")))
    };
    match command {
        "help" => Ok(json!({"help":USAGE})),
        "configure" => {
            crate::contracts::validate_cairn_registry(&options.registry)?;
            let mut config = read_cli_config(&options.paths)?.ok_or_else(|| {
                AppError::not_found(
                    "No Ecky config.edn found; start Ecky once or pass ECKY_CONFIG_DIR",
                )
            })?;
            config.cairn = Some(crate::contracts::CairnConfig {
                registry_url: options.registry.clone(),
            });
            crate::config_store::save_config(&options.paths.app_config_dir(), config)?;
            Ok(
                json!({"registryUrl":options.registry,"configPath":options.paths.app_config_dir().join("config.edn")}),
            )
        }
        "init" => {
            let id = word(1)?;
            if !cairn_registry::valid_segment(id) {
                return Err(usage("Invalid package ID"));
            }
            let package:ComponentPackage=serde_json::from_value(json!({"schemaVersion":1,"packageId":id,"version":"0.1.0","displayName":id,"visibility":"source","components":[],"cairn":{"dependencies":[]}})).map_err(persistence)?;
            std::fs::create_dir_all(&options.project).map_err(persistence)?;
            let path = options.project.join(packages::COMPONENT_PACKAGE_FILE_NAME);
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(|error| persistence(format!("{}: {error}", path.display())))?;
            use std::io::Write;
            file.write_all(&serde_json::to_vec_pretty(&package).map_err(persistence)?)
                .map_err(persistence)?;
            Ok(json!({"manifest":path,"packageId":id,"version":"0.1.0"}))
        }
        "pack" => {
            let output = options
                .output
                .as_ref()
                .ok_or_else(|| usage("pack requires --out <archive>"))?;
            let release =
                packages::write_distributable_component_package_archive(&options.project, output)?;
            Ok(json!({"archive":output,"release":release}))
        }
        "publish" => {
            if options.offline {
                return Err(usage("Publication requires a network connection"));
            }
            let path = Path::new(word(1)?);
            if std::fs::metadata(path).map_err(persistence)?.len()
                > cairn_registry::MAX_ARCHIVE_BYTES as u64
            {
                return Err(AppError::validation("Archive exceeds 32 MiB"));
            }
            let body = std::fs::read(path).map_err(persistence)?;
            let expected = cairn_registry::inspect_archive(&body).map_err(AppError::validation)?;
            packages::read_component_package_from_archive(path)?;
            packages::validate_payload_archive(&packages::read_decoded_package_payload(path)?)?;
            let client = RegistryClient::new(&options.paths, &options.registry, false)?;
            let response = client
                .http
                .post(format!("{}/v1/releases", options.registry))
                .bearer_auth(token()?)
                .body(body)
                .send()
                .await
                .map_err(persistence)?;
            let release: Release =
                serde_json::from_slice(&RegistryClient::body(response, 2 * 1024 * 1024).await?)
                    .map_err(persistence)?;
            if release.archive_digest != expected.archive_digest
                || release.package_digest != expected.package_digest
            {
                return Err(AppError::validation(
                    "Published registry response disagrees with local archive integrity",
                ));
            }
            Ok(json!({"release":release}))
        }
        "search" => {
            let query = options
                .words
                .iter()
                .skip(1)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            if options.local {
                return serde_json::to_value(
                    crate::catalog::search_semantic(
                        &options.paths,
                        &crate::catalog::CatalogQuery {
                            query,
                            semantic: options.embed,
                            ..Default::default()
                        },
                    )
                    .await?,
                )
                .map_err(persistence);
            }
            let client = RegistryClient::new(&options.paths, &options.registry, options.offline)?;
            let mut url = reqwest::Url::parse(&format!("{}/v1/search", options.registry))
                .map_err(persistence)?;
            url.query_pairs_mut().append_pair("q", &query);
            client.metadata(url.as_str()).await
        }
        "info" => {
            let id = word(1)?;
            if !cairn_registry::valid_segment(id) {
                return Err(usage("info requires a package ID"));
            }
            RegistryClient::new(&options.paths, &options.registry, options.offline)?
                .metadata(&format!("{}/v1/packages/{id}", options.registry))
                .await
        }
        "add" | "update" | "install" => {
            let mut manifest = packages::read_component_package_manifest(&options.project)?;
            let changed = command != "install";
            if changed {
                if options.frozen {
                    return Err(usage("add/update cannot be frozen"));
                }
                let selected = dependency(word(1)?)?;
                let distribution = manifest.cairn.get_or_insert_with(Default::default);
                if command == "update"
                    && !distribution
                        .dependencies
                        .iter()
                        .any(|entry| entry.package_id == selected.package_id)
                {
                    return Err(usage("update requires an existing direct dependency"));
                }
                distribution
                    .dependencies
                    .retain(|entry| entry.package_id != selected.package_id);
                distribution.dependencies.push(selected);
                distribution.dependencies = sorted_dependencies(&distribution.dependencies);
            }
            let lock = install(&options, &manifest, command == "update").await?;
            save_install(&options.project, &manifest, &lock, changed)?;
            Ok(
                json!({"lock":lock,"lockPath":options.project.join(crate::project_mirror::PROJECT_LOCK_FILE_NAME)}),
            )
        }
        "index" => index(&options).await,
        _ => Err(usage(format!("Unknown command {command}"))),
    }
}

pub fn print_result(value: &Value, json_output: bool) {
    if let Some(version) = value.get("version").and_then(Value::as_str) {
        println!("{version}");
        return;
    }
    if let Some(help) = value.get("help").and_then(Value::as_str) {
        print!("{help}");
        return;
    }
    if json_output {
        println!("{value}");
    } else if let Ok(text) = serde_json::to_string_pretty(value) {
        println!("{text}");
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistryReleaseSummary {
    pub header: crate::contracts::ComponentPackageHeader,
    pub license: Option<String>,
    pub archive_digest: String,
    pub package_digest: String,
}
pub async fn search_registry(
    app: &dyn PathResolver,
    registry: &str,
    query: &str,
) -> AppResult<Vec<RegistryReleaseSummary>> {
    if query.chars().count() > 512 {
        return Err(AppError::validation(
            "Registry query exceeds 512 characters",
        ));
    }
    let client = RegistryClient::new(app, registry, false)?;
    let mut url = reqwest::Url::parse(&format!("{registry}/v1/search")).map_err(persistence)?;
    url.query_pairs_mut().append_pair("q", query);
    let value = client.metadata(url.as_str()).await?;
    let releases: Vec<Release> =
        serde_json::from_value(value["releases"].clone()).map_err(persistence)?;
    releases
        .into_iter()
        .filter(|release| !release.yanked)
        .take(100)
        .map(|release| {
            let header: crate::contracts::ComponentPackageHeader =
                serde_json::from_value(release.header).map_err(persistence)?;
            crate::contracts::validate_component_package_header(&header)?;
            if header.package_id != release.package_id || header.version != release.version {
                return Err(AppError::validation(
                    "Registry release/header coordinate mismatch",
                ));
            }
            Ok(RegistryReleaseSummary {
                header,
                license: release.distribution.license,
                archive_digest: release.archive_digest,
                package_digest: release.package_digest,
            })
        })
        .collect()
}
pub async fn install_registry_package(
    app: Arc<dyn PathResolver>,
    registry: &str,
    package_id: &str,
    version: &str,
) -> AppResult<()> {
    let selected = dependency(&format!("{package_id}@{version}"))?;
    let client = RegistryClient::new(app.as_ref(), registry, false)?;
    let graph = client.resolve(&[selected]).await?;
    let mut archives = Vec::new();
    for release in &graph.releases {
        archives.push(client.archive(release).await?);
    }
    tokio::task::spawn_blocking(move || {
        packages::install_component_package_archives_to_store(app.as_ref(), &archives)
    })
    .await
    .map_err(persistence)??;
    Ok(())
}
