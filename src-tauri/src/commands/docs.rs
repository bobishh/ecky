use crate::contracts::{AppError, AppResult};
use std::path::Path;
use tauri::AppHandle;

fn validated_asset_path(path: &str) -> AppResult<&str> {
    if path == "/docs/ecky-ir-field-guide.epub" {
        return Ok(path.trim_start_matches('/'));
    }
    let valid_example = path.strip_prefix("/docs/examples/").is_some_and(|name| {
        name.ends_with(".ecky")
            && !name.starts_with('.')
            && !name.contains("..")
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    });
    if !valid_example {
        return Err(AppError::validation(format!(
            "Unsupported documentation asset: {path}"
        )));
    }
    Ok(path.trim_start_matches('/'))
}

fn export_asset(
    asset_path: &str,
    target_path: &Path,
    resolve: impl FnOnce(&str) -> Option<Vec<u8>>,
) -> AppResult<()> {
    let path = validated_asset_path(asset_path)?;
    let bytes = resolve(path).ok_or_else(|| {
        AppError::not_found(format!(
            "Bundled documentation asset is missing: {asset_path}"
        ))
    })?;
    std::fs::write(target_path, bytes).map_err(|error| {
        AppError::persistence(format!(
            "Failed to save '{asset_path}' to '{}': {error}",
            target_path.display()
        ))
    })
}

#[tauri::command]
#[specta::specta]
pub async fn export_docs_asset(
    _app: AppHandle,
    asset_path: String,
    target_path: String,
) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        export_asset(&asset_path, Path::new(&target_path), |path| {
            // Development reads the same fresh public bytes Vite serves. Release
            // uses exact embedded keys, never the SPA's index.html fallback.
            if cfg!(debug_assertions) {
                std::fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../public")
                        .join(path),
                )
                .ok()
            } else {
                _app.asset_resolver()
                    .iter()
                    .find(|(key, _)| key.trim_start_matches('/') == path)
                    .map(|(_, bytes)| bytes.into_owned())
            }
        })
    })
    .await
    .map_err(|error| AppError::internal(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documentation_export_saves_exact_bytes_and_missing_assets_preserve_destination() {
        let target = std::env::temp_dir().join(format!("ecky-docs-{}.ecky", uuid::Uuid::new_v4()));
        export_asset("/docs/examples/01-worm.ecky", &target, |_| {
            Some(b"(model (sphere 6mm))".to_vec())
        })
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"(model (sphere 6mm))");
        let error = export_asset("/docs/examples/missing.ecky", &target, |_| None).unwrap_err();
        assert!(error.message.contains("missing.ecky"));
        assert_eq!(std::fs::read(&target).unwrap(), b"(model (sphere 6mm))");
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn documentation_assets_cannot_escape_the_bundled_download_catalog() {
        for path in [
            "/docs/examples/../secret.ecky",
            "/docs/examples/%2e%2e/secret.ecky",
            "/docs/examples/x/y.ecky",
            "/docs/ecky-ir.md",
            "https://other.test/worm.ecky",
        ] {
            assert!(validated_asset_path(path).is_err(), "{path}");
        }
        assert!(validated_asset_path("/docs/ecky-ir-field-guide.epub").is_ok());
        assert!(validated_asset_path("/docs/examples/4189afc54271-01-worm.ecky").is_ok());
    }
}
