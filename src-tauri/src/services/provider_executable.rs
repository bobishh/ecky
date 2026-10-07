use std::ffi::{OsStr, OsString};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::contracts::{AppError, AppResult};

const COMMON_USER_BIN_SUFFIXES: &[&str] = &[
    ".local/bin",
    "bin",
    ".asdf/shims",
    ".volta/bin",
    ".npm/bin",
    ".bun/bin",
];

#[cfg(unix)]
const COMMON_SYSTEM_BIN_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

#[derive(Debug, Clone)]
pub struct ResolvedProviderExecutable {
    pub path: PathBuf,
    pub spawn_path: OsString,
}

pub fn resolve_provider_executable(
    default_command: &str,
    override_env: &str,
    provider_label: &str,
) -> AppResult<ResolvedProviderExecutable> {
    let command = std::env::var(override_env)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_command.to_string());
    let process_path = std::env::var_os("PATH").map(|value| value.to_string_lossy().into_owned());
    let login_shell_path = current_login_shell_path();
    let home_dir = current_home_dir();
    let spawn_path = build_provider_spawn_path(
        process_path.as_deref(),
        login_shell_path.as_deref(),
        home_dir.as_deref(),
    )
    .ok_or_else(|| {
        AppError::provider(format!(
            "{provider_label} could not build a process PATH for '{command}'. Set {override_env} to its absolute executable path."
        ))
    })?;
    let path = resolve_provider_executable_from_path(&command, &spawn_path).ok_or_else(|| {
        AppError::provider(format!(
            "{provider_label} executable '{command}' was not found. Searched PATH: {}. Set {override_env} to its absolute executable path.",
            spawn_path.to_string_lossy()
        ))
    })?;

    Ok(ResolvedProviderExecutable { path, spawn_path })
}

/// Use the desktop's bundled runtime so Provider models and protocol match the
/// signed-in desktop. An explicit override remains authoritative on every platform.
pub fn resolve_codex_executable() -> AppResult<ResolvedProviderExecutable> {
    let process_path = std::env::var_os("PATH").map(|value| value.to_string_lossy().into_owned());
    let login_path = current_login_shell_path();
    let home = current_home_dir();
    let spawn_path = build_provider_spawn_path(
        process_path.as_deref(),
        login_path.as_deref(),
        home.as_deref(),
    )
    .ok_or_else(|| AppError::provider("Codex could not build a process PATH."))?;
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    for applications in home
        .iter()
        .map(|home| home.join("Applications"))
        .chain([PathBuf::from("/Applications")])
    {
        for app in ["Codex.app", "ChatGPT.app"] {
            candidates.push(
                applications
                    .join(app)
                    .join("Contents/Resources/codex-cli/bin/codex"),
            );
        }
    }
    let override_command = std::env::var("ECKY_CODEX_BIN").ok();
    let path = resolve_codex_executable_from_sources(
        override_command.as_deref(),
        &candidates,
        &spawn_path,
    )?;
    Ok(ResolvedProviderExecutable { path, spawn_path })
}

pub fn resolve_codex_executable_from_sources(
    override_command: Option<&str>,
    desktop_candidates: &[PathBuf],
    spawn_path: &OsStr,
) -> AppResult<PathBuf> {
    let override_command = override_command.filter(|value| !value.trim().is_empty());
    if override_command.is_none() {
        if let Some(candidate) = desktop_candidates
            .iter()
            .find(|path| is_executable_file(path))
        {
            return Ok(candidate.clone());
        }
    }
    let command = override_command.unwrap_or("codex");
    resolve_provider_executable_from_path(command, spawn_path).ok_or_else(|| AppError::provider(format!(
        "Codex executable '{command}' was not found. Searched PATH: {}. Set ECKY_CODEX_BIN to its absolute executable path.", spawn_path.to_string_lossy()
    )))
}

pub fn resolve_provider_executable_from_sources(
    command: &str,
    process_path: Option<&str>,
    login_shell_path: Option<&str>,
    home_dir: Option<&Path>,
) -> AppResult<PathBuf> {
    let spawn_path =
        build_provider_spawn_path(process_path, login_shell_path, home_dir).ok_or_else(|| {
            AppError::provider(format!(
                "Provider executable '{command}' could not be resolved because no process PATH is available."
            ))
        })?;
    resolve_provider_executable_from_path(command, &spawn_path).ok_or_else(|| {
        AppError::provider(format!(
            "Provider executable '{command}' was not found in PATH: {}",
            spawn_path.to_string_lossy()
        ))
    })
}

fn build_provider_spawn_path(
    process_path: Option<&str>,
    login_shell_path: Option<&str>,
    home_dir: Option<&Path>,
) -> Option<OsString> {
    let mut entries = Vec::new();

    for source in [process_path, login_shell_path].into_iter().flatten() {
        for entry in std::env::split_paths(source) {
            push_unique(&mut entries, entry);
        }
    }
    if let Some(home_dir) = home_dir {
        for suffix in COMMON_USER_BIN_SUFFIXES {
            push_unique(&mut entries, home_dir.join(suffix));
        }
    }
    #[cfg(unix)]
    for directory in COMMON_SYSTEM_BIN_DIRS {
        push_unique(&mut entries, PathBuf::from(directory));
    }

    std::env::join_paths(entries).ok()
}

fn push_unique(entries: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidate.as_os_str().is_empty() && !entries.iter().any(|entry| entry == &candidate) {
        entries.push(candidate);
    }
}

fn resolve_provider_executable_from_path(command: &str, spawn_path: &OsStr) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.components().count() > 1 {
        return is_executable_file(command_path).then(|| command_path.to_path_buf());
    }

    for directory in std::env::split_paths(spawn_path) {
        let candidate = directory.join(command);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }

        #[cfg(windows)]
        if command_path.extension().is_none() {
            for extension in windows_path_extensions() {
                let candidate = directory.join(command).with_extension(extension);
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(windows)]
fn windows_path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.BAT;.CMD;.COM".to_string())
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.trim_start_matches('.').to_string())
        .collect()
}

fn current_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|path| !path.is_empty()))
        .map(PathBuf::from)
}

#[cfg(unix)]
fn current_login_shell_path() -> Option<String> {
    let configured_shell = std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .and_then(|shell| read_login_shell_path(shell.as_os_str()));
    configured_shell.or_else(|| read_login_shell_path(OsStr::new("/bin/zsh")))
}

#[cfg(not(unix))]
fn current_login_shell_path() -> Option<String> {
    None
}

#[cfg(unix)]
fn read_login_shell_path(shell: &OsStr) -> Option<String> {
    let output = Command::new(shell)
        .arg("-lc")
        .arg(r#"printf %s "$PATH""#)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ecky-provider-resolver-{}-{nonce}",
            std::process::id()
        ))
    }

    fn write_executable(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create executable parent");
        fs::write(path, "#!/bin/sh\nexit 0\n").expect("write executable");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("chmod executable");
        }
    }

    #[test]
    fn explicit_codex_override_never_silently_falls_back_to_desktop() {
        let root = temp_root();
        let desktop = root.join("desktop/codex");
        write_executable(&desktop);
        let path = std::env::join_paths([root.join("bin")]).unwrap();
        let error = resolve_codex_executable_from_sources(Some("missing-codex"), &[desktop], &path)
            .unwrap_err();
        assert!(error.message.contains("missing-codex"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_bare_command_from_login_shell_path() {
        let root = temp_root();
        let executable = root.join("bin/provider-test-cli");
        write_executable(&executable);

        let resolved = resolve_provider_executable_from_sources(
            "provider-test-cli",
            Some("/usr/bin:/bin"),
            Some(root.join("bin").to_string_lossy().as_ref()),
            Some(&root),
        )
        .expect("resolve provider");

        assert_eq!(resolved, executable);
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn explicit_path_wins_without_path_lookup() {
        let root = temp_root();
        let executable = root.join("custom/provider-test-cli");
        write_executable(&executable);

        let resolved = resolve_provider_executable_from_sources(
            executable.to_string_lossy().as_ref(),
            Some("/usr/bin:/bin"),
            None,
            Some(&root),
        )
        .expect("resolve explicit provider");

        assert_eq!(resolved, executable);
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_path_is_not_wrapped_in_quotes() {
        let path = read_login_shell_path(OsStr::new("/bin/sh")).expect("login shell PATH");

        assert!(!path.starts_with('"'));
        assert!(!path.ends_with('"'));
        assert!(std::env::split_paths(&path).next().is_some());
    }
}
