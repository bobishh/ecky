use crate::build123d;
use crate::freecad;
use crate::models::{
    AppError, AppResult, EngineKind, GeometryBackend, PathResolver, RuntimeAuthoringContext,
    RuntimeBackendCapability, RuntimeCapabilities, SourceLanguage,
};
use std::path::{Path, PathBuf};
use std::process::Command;

const FREECAD_RUNNER_RESOURCE_PATH: &str = "server/freecad_runner.py";

pub fn collect_runtime_capabilities(
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> RuntimeCapabilities {
    let freecad = probe_freecad_runtime(configured_freecad_cmd, app);
    let build123d = probe_build123d_runtime(app);
    let direct_occt = probe_direct_occt_runtime(app);
    let ecky_rust = RuntimeBackendCapability {
        available: true,
        detail: "bundled".to_string(),
        path: None,
    };

    RuntimeCapabilities {
        recommended_authoring_context: recommended_authoring_context(
            freecad.available,
            build123d.available,
        ),
        freecad,
        build123d,
        direct_occt,
        ecky_rust,
    }
}

pub fn recommended_authoring_context(
    freecad_available: bool,
    build123d_available: bool,
) -> RuntimeAuthoringContext {
    if build123d_available {
        return RuntimeAuthoringContext {
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::Build123d,
        };
    }

    if freecad_available {
        return RuntimeAuthoringContext {
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::Freecad,
        };
    }

    RuntimeAuthoringContext {
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
    }
}

pub fn capability_for_authoring_context(
    capabilities: &RuntimeCapabilities,
    source_language: SourceLanguage,
    geometry_backend: GeometryBackend,
) -> &RuntimeBackendCapability {
    match source_language {
        SourceLanguage::LegacyPython => &capabilities.freecad,
        SourceLanguage::Build123d => &capabilities.build123d,
        SourceLanguage::EckyIrV0 => match geometry_backend {
            GeometryBackend::Freecad => &capabilities.freecad,
            GeometryBackend::Build123d => &capabilities.build123d,
            GeometryBackend::EckyRust => &capabilities.ecky_rust,
        },
    }
}

pub fn ensure_backend_available(
    geometry_backend: GeometryBackend,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<()> {
    let capabilities = collect_runtime_capabilities(configured_freecad_cmd, app);
    let capability = match geometry_backend {
        GeometryBackend::Freecad => &capabilities.freecad,
        GeometryBackend::Build123d => &capabilities.build123d,
        GeometryBackend::EckyRust => &capabilities.ecky_rust,
    };

    if capability.available {
        Ok(())
    } else {
        Err(AppError::render(capability.detail.clone()))
    }
}

pub fn probe_freecad_runtime(
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> RuntimeBackendCapability {
    if let Err(err) = freecad::resolve_resource_path(
        app,
        FREECAD_RUNNER_RESOURCE_PATH,
        &["../server/freecad_runner.py", "server/freecad_runner.py"],
    ) {
        return unavailable_capability(err.to_string());
    }

    match resolve_existing_freecad_path(configured_freecad_cmd) {
        Ok(path) => available_capability(
            format!("Ready at {}", path.display()),
            Some(path.display().to_string()),
        ),
        Err(err) => unavailable_capability(err.to_string()),
    }
}

pub fn probe_build123d_runtime(app: &dyn PathResolver) -> RuntimeBackendCapability {
    let python_cmd = match build123d::resolve_python_cmd_with_app(app) {
        Ok(path) => path,
        Err(err) => return unavailable_capability(err.to_string()),
    };

    match probe_build123d_import(&python_cmd) {
        Ok(executable) => {
            available_capability(format!("Ready at {}", executable), Some(executable))
        }
        Err(err) => unavailable_capability(err.to_string()),
    }
}

pub fn probe_direct_occt_runtime(app: &dyn PathResolver) -> RuntimeBackendCapability {
    if let Some(runner) =
        crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(app, true)
    {
        // The precompiled runner is self-contained: a responsive runner means
        // native renders are available even when the OCP/SDK compile layout
        // is absent (runner-first export never compiles a shim).
        let output = Command::new(&runner).arg("--version").output();
        return match output {
            Ok(output) if output.status.success() => available_capability(
                "available".to_string(),
                Some(runner.display().to_string()),
            ),
            Ok(output) => unavailable_capability(format!(
                "Direct OCCT unavailable: runner failed: {}\nstdout: {}\nstderr: {}",
                output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "terminated by signal".to_string()),
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(err) => unavailable_capability(format!(
                "Direct OCCT unavailable: runner start failed at '{}': {}",
                runner.display(),
                err
            )),
        };
    }

    let runtime_root = match resolve_direct_occt_runtime_root(app) {
        Ok(path) => path,
        Err(err) => {
            return unavailable_capability(format!(
                "Direct OCCT unavailable: runner missing and runtime root unresolved: {}",
                err
            ))
        }
    };
    let layout =
        crate::ecky_cad_host::direct_occt_sdk::inspect_build123d_ocp_runtime(&runtime_root);

    let blockers = layout.blocker_summary();
    unavailable_capability(format!(
        "Direct OCCT unavailable: runner missing; {}",
        if blockers.is_empty() {
            format!(
                "checked runtime root '{}'; build precompiled runner with `bash scripts/build_direct_occt_runner.sh`",
                runtime_root.display()
            )
        } else {
            blockers.join("; ")
        }
    ))
}

fn available_capability(detail: String, path: Option<String>) -> RuntimeBackendCapability {
    RuntimeBackendCapability {
        available: true,
        detail,
        path,
    }
}

fn unavailable_capability(detail: String) -> RuntimeBackendCapability {
    RuntimeBackendCapability {
        available: false,
        detail,
        path: None,
    }
}

pub(crate) fn resolve_direct_occt_runtime_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    if let Some(path) = crate::ecky_cad_host::direct_occt_sdk::scoped_env_var_os("ECKY_OCCT_ROOT")
        .map(|value| value.to_string_lossy().into_owned())
    {
        let runtime_root = PathBuf::from(path.trim()).join("runtime").join("occt");
        if runtime_root.is_dir() {
            return Ok(runtime_root);
        }
    }

    if let Some(path) = app.resource_path("runtime/occt") {
        if path.is_dir() {
            return Ok(path);
        }
    }

    if let Ok(path) = std::env::var("BUILD123D_RUNTIME_DIR") {
        let path = PathBuf::from(path.trim());
        if path.is_dir() {
            return Ok(path);
        }
    }

    if let Some(path) = app.resource_path("runtime/build123d") {
        if path.is_dir() {
            return Ok(path);
        }
    }

    for resource in [
        "runtime/build123d/bin/python3",
        "runtime/build123d/bin/python",
    ] {
        if let Some(path) = app.resource_path(resource) {
            if let Some(root) = runtime_root_from_python_path(&path) {
                return Ok(root);
            }
        }
    }

    let repo_runtime = crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    );
    if repo_runtime.is_dir() {
        return Ok(repo_runtime);
    }

    let repo_runtime =
        crate::ecky_cad_host::direct_occt_sdk::bundled_build123d_runtime_root_from_repo(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        );
    if repo_runtime.is_dir() {
        return Ok(repo_runtime);
    }

    let python = build123d::resolve_python_cmd_with_app(app)?;
    runtime_root_from_python_path(&python).ok_or_else(|| {
        AppError::render(format!(
            "Direct OCCT runtime root could not be inferred from '{}'.",
            python.display()
        ))
    })
}

fn runtime_root_from_python_path(path: &Path) -> Option<PathBuf> {
    let bin_dir = path.parent()?;
    (bin_dir.file_name()? == "bin").then(|| bin_dir.parent().map(Path::to_path_buf))?
}

fn resolve_existing_freecad_path(configured_freecad_cmd: Option<&str>) -> AppResult<PathBuf> {
    if let Some(configured_cmd) = configured_freecad_cmd.map(str::trim) {
        if configured_cmd.is_empty() {
            return freecad::resolve_freecad_path(None)
                .and_then(ensure_existing_path)
                .map_err(normalize_missing_freecad_path);
        }

        if let Some(path) = resolve_direct_or_path_command(configured_cmd) {
            return Ok(path);
        }

        return Err(AppError::render(format!(
            "FreeCAD executable not found at '{}'.",
            configured_cmd
        )));
    }

    if let Some(env_cmd) = std::env::var_os("FREECAD_CMD") {
        let trimmed = env_cmd.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            if let Some(path) = resolve_direct_or_path_command(&trimmed) {
                return Ok(path);
            }

            return Err(AppError::render(format!(
                "FreeCAD executable not found at '{}'.",
                trimmed
            )));
        }
    }

    freecad::resolve_freecad_path(None)
        .and_then(ensure_existing_path)
        .map_err(normalize_missing_freecad_path)
}

fn resolve_direct_or_path_command(value: &str) -> Option<PathBuf> {
    let normalized = freecad::resolve_freecad_path(Some(value)).ok()?;
    if normalized.exists() {
        return Some(normalized);
    }

    if !Path::new(value).components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) && !value.contains(std::path::MAIN_SEPARATOR)
    {
        return find_command_on_path(value);
    }

    None
}

fn ensure_existing_path(path: PathBuf) -> AppResult<PathBuf> {
    if path.exists() {
        Ok(path)
    } else {
        Err(AppError::render(format!(
            "FreeCAD executable not found at '{}'.",
            path.display()
        )))
    }
}

fn normalize_missing_freecad_path(err: AppError) -> AppError {
    AppError::render(err.to_string())
}

fn find_command_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn probe_build123d_import(python_cmd: &Path) -> AppResult<String> {
    let output = Command::new(python_cmd)
        .arg("-c")
        .arg("import build123d, sys; print(sys.executable)")
        .output()
        .map_err(|err| {
            AppError::render(format!(
                "Failed to execute build123d Python '{}': {}",
                python_cmd.display(),
                err
            ))
        })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() && stdout.is_empty() {
            format!(
                "build123d import check failed for '{}'.",
                python_cmd.display()
            )
        } else {
            format!(
                "build123d import check failed for '{}'. stdout: {} stderr: {}",
                python_cmd.display(),
                stdout,
                stderr
            )
        };
        return Err(AppError::render(detail));
    }

    let executable = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if executable.is_empty() {
        return Ok(python_cmd.display().to_string());
    }

    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            Some(self.root.join("resources").join(path))
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ecky-runtime-capabilities-{}-{}",
            label,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

    fn create_direct_occt_runtime_layout(root: &Path) {
        let ocp_root = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("lib")
            .join("python3.12")
            .join("site-packages")
            .join("OCP");
        let include_dir = ocp_root.join("include").join("opencascade");
        let dylib_dir = ocp_root.join(".dylibs");
        fs::create_dir_all(&include_dir).unwrap();
        fs::create_dir_all(&dylib_dir).unwrap();
        for header in crate::ecky_cad_host::direct_occt_sdk::REQUIRED_OCCT_HEADERS {
            fs::write(include_dir.join(header), "// header\n").unwrap();
        }
        for lib in crate::ecky_cad_host::direct_occt_sdk::REQUIRED_OCCT_LIBS {
            let filename = if cfg!(target_os = "macos") {
                format!("lib{lib}.dylib")
            } else if cfg!(target_os = "windows") {
                format!("{lib}.dll")
            } else {
                format!("lib{lib}.so")
            };
            fs::write(dylib_dir.join(filename), "").unwrap();
        }
    }

    #[test]
    fn recommended_authoring_context_prefers_ecky_source_over_raw_freecad() {
        let build123d = recommended_authoring_context(true, true);
        assert_eq!(build123d.engine_kind, EngineKind::EckyIrV0);
        assert_eq!(build123d.source_language, SourceLanguage::EckyIrV0);
        assert_eq!(build123d.geometry_backend, GeometryBackend::Build123d);

        let freecad = recommended_authoring_context(true, false);
        assert_eq!(freecad.engine_kind, EngineKind::EckyIrV0);
        assert_eq!(freecad.source_language, SourceLanguage::EckyIrV0);
        assert_eq!(freecad.geometry_backend, GeometryBackend::Freecad);

        let ecky_rust = recommended_authoring_context(false, false);
        assert_eq!(ecky_rust.engine_kind, EngineKind::EckyIrV0);
        assert_eq!(ecky_rust.source_language, SourceLanguage::EckyIrV0);
        assert_eq!(ecky_rust.geometry_backend, GeometryBackend::EckyRust);
    }

    #[test]
    fn probe_freecad_runtime_reports_ready_when_binary_and_runner_exist() {
        let root = temp_root("freecad-ready");
        let resolver = TestResolver { root: root.clone() };
        let runner = root.join("resources").join(FREECAD_RUNNER_RESOURCE_PATH);
        let binary = root.join("bin").join("freecadcmd");
        write_file(&runner, "# runner\n");
        write_file(&binary, "#!/bin/sh\nexit 0\n");

        let capability = probe_freecad_runtime(Some(binary.to_string_lossy().as_ref()), &resolver);

        assert!(capability.available, "{:?}", capability);
        assert!(capability.detail.contains("Ready at"), "{:?}", capability);
        assert_eq!(
            capability.path.as_deref(),
            Some(binary.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn probe_freecad_runtime_reports_missing_binary() {
        let root = temp_root("freecad-missing-binary");
        let resolver = TestResolver { root };
        let missing = "/missing/freecadcmd";

        let capability = probe_freecad_runtime(Some(missing), &resolver);

        assert!(!capability.available);
        assert!(capability.detail.contains(missing), "{:?}", capability);
    }

    #[test]
    fn probe_build123d_runtime_reports_ready_when_import_probe_succeeds() {
        let _guard = crate::build123d_test_env_lock().lock().unwrap();
        let root = temp_root("build123d-ready");
        let resolver = TestResolver { root };
        let python =
            std::env::temp_dir().join(format!("fake-build123d-python-{}", uuid::Uuid::new_v4()));
        write_file(&python, "#!/bin/sh\nprintf '%s\\n' \"$0\"\nexit 0\n");
        std::env::set_var("BUILD123D_PYTHON", &python);

        let capability = probe_build123d_runtime(&resolver);

        std::env::remove_var("BUILD123D_PYTHON");
        assert!(capability.available, "{:?}", capability);
        assert_eq!(
            capability.path.as_deref(),
            Some(python.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn probe_build123d_runtime_reports_import_failure() {
        let _guard = crate::build123d_test_env_lock().lock().unwrap();
        let root = temp_root("build123d-fail");
        let resolver = TestResolver { root };
        let python = std::env::temp_dir().join(format!(
            "fake-build123d-python-fail-{}",
            uuid::Uuid::new_v4()
        ));
        write_file(&python, "#!/bin/sh\nprintf 'boom' >&2\nexit 1\n");
        std::env::set_var("BUILD123D_PYTHON", &python);

        let capability = probe_build123d_runtime(&resolver);

        std::env::remove_var("BUILD123D_PYTHON");
        assert!(!capability.available);
        assert!(capability.detail.contains("boom"), "{:?}", capability);
    }

    #[test]
    fn collect_runtime_capabilities_prefers_build123d_when_freecad_missing() {
        let _guard = crate::build123d_test_env_lock().lock().unwrap();
        let root = temp_root("build123d-only");
        let resolver = TestResolver { root };
        let python = std::env::temp_dir().join(format!(
            "fake-build123d-python-only-{}",
            uuid::Uuid::new_v4()
        ));
        write_file(&python, "#!/bin/sh\nprintf '%s\\n' \"$0\"\nexit 0\n");
        std::env::set_var("BUILD123D_PYTHON", &python);

        let capabilities = collect_runtime_capabilities(Some("/missing/freecadcmd"), &resolver);

        std::env::remove_var("BUILD123D_PYTHON");
        assert!(
            !capabilities.freecad.available,
            "{:?}",
            capabilities.freecad
        );
        assert!(
            capabilities.build123d.available,
            "{:?}",
            capabilities.build123d
        );
        assert_eq!(
            capabilities.recommended_authoring_context.geometry_backend,
            GeometryBackend::Build123d
        );
    }

    #[test]
    fn capability_for_authoring_context_uses_source_language_then_backend() {
        let capabilities = RuntimeCapabilities {
            freecad: unavailable_capability("fc".to_string()),
            build123d: available_capability("b123d".to_string(), Some("/tmp/python".to_string())),
            direct_occt: unavailable_capability("direct OCCT blocked".to_string()),
            ecky_rust: available_capability("rust".to_string(), None),
            recommended_authoring_context: recommended_authoring_context(false, true),
        };

        assert_eq!(
            capability_for_authoring_context(
                &capabilities,
                SourceLanguage::LegacyPython,
                GeometryBackend::Freecad
            )
            .detail,
            "fc"
        );
        assert_eq!(
            capability_for_authoring_context(
                &capabilities,
                SourceLanguage::Build123d,
                GeometryBackend::Build123d
            )
            .detail,
            "b123d"
        );
        assert_eq!(
            capability_for_authoring_context(
                &capabilities,
                SourceLanguage::EckyIrV0,
                GeometryBackend::EckyRust
            )
            .detail,
            "rust"
        );
    }

    #[test]
    fn resolve_direct_occt_runtime_root_prefers_bundled_occt_resource() {
        let root = temp_root("direct-occt-resource");
        let resolver = TestResolver { root: root.clone() };
        let occt_root = root.join("resources").join("runtime").join("occt");
        fs::create_dir_all(&occt_root).unwrap();

        let resolved = resolve_direct_occt_runtime_root(&resolver).expect("runtime root");

        assert_eq!(resolved, occt_root);
    }

    #[test]
    fn probe_direct_occt_runtime_prefers_runner_when_available() {
        let root = temp_root("direct-occt-runner-ready");
        let resolver = TestResolver { root: root.clone() };
        create_direct_occt_runtime_layout(&root);
        let runner = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("bin")
            .join("direct-occt-runner");
        write_file(&runner, "#!/bin/sh\necho 'direct-occt-runner 0.1.0'\n");

        let capability = probe_direct_occt_runtime(&resolver);

        assert!(capability.available, "{capability:?}");
        assert_eq!(capability.detail, "available", "{capability:?}");
        assert_eq!(
            capability.path.as_deref(),
            Some(runner.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn probe_direct_occt_runtime_reports_runner_failure() {
        let root = temp_root("direct-occt-runner-fail");
        let resolver = TestResolver { root: root.clone() };
        create_direct_occt_runtime_layout(&root);
        let runner = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("bin")
            .join("direct-occt-runner");
        write_file(&runner, "#!/bin/sh\necho boom >&2\nexit 7\n");

        let capability = probe_direct_occt_runtime(&resolver);

        assert!(!capability.available, "{capability:?}");
        assert!(
            capability.detail.contains("runner failed"),
            "{capability:?}"
        );
        assert!(capability.detail.contains("boom"), "{capability:?}");
    }

    #[test]
    fn probe_direct_occt_runtime_runner_alone_is_available_without_sdk_layout() {
        // The precompiled runner is self-contained; the OCP/SDK compile layout
        // is only needed for the shim-compile leg. A responsive runner alone
        // means native renders are available.
        let root = temp_root("direct-occt-runner-no-layout");
        let resolver = TestResolver { root: root.clone() };
        let runner = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("bin")
            .join("direct-occt-runner");
        write_file(&runner, "#!/bin/sh\necho 'direct-occt-runner 0.1.0'\n");

        let capability = probe_direct_occt_runtime(&resolver);

        assert!(capability.available, "{capability:?}");
        assert_eq!(capability.detail, "available", "{capability:?}");
    }

    #[test]
    fn probe_direct_occt_runtime_reports_blocker_without_changing_recommendation() {
        let _no_cwd_runner =
            crate::ecky_cad_host::direct_occt_runner::test_discovery::CwdFallbackGuard::disable();
        let root = temp_root("direct-occt-blocked");
        let resolver = TestResolver { root };

        let capability = probe_direct_occt_runtime(&resolver);

        assert!(!capability.available);
        assert!(capability.detail.contains("Direct OCCT"), "{capability:?}");
        assert!(
            capability.detail.contains("runner missing"),
            "{capability:?}"
        );
        assert!(
            !capability.detail.contains("OCCT headers missing"),
            "{capability:?}"
        );

        let recommended = recommended_authoring_context(false, true);
        assert_eq!(recommended.geometry_backend, GeometryBackend::Build123d);
    }
}
