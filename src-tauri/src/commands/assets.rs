use base64::{engine::general_purpose, Engine as _};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use tauri::AppHandle;
use uuid::Uuid;

fn is_supported_image_extension(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp")
}

fn image_format_for_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    if !is_supported_image_extension(&ext) {
        return None;
    }
    Some(ext.to_uppercase())
}

fn asset_name_for_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.replace(['_', '-'], " "))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Local Asset".to_string())
}

fn stable_asset_id_for_path(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("asset-{}", &digest[..12])
}

pub fn app_assets_dir(app: &dyn crate::models::PathResolver) -> std::path::PathBuf {
    app.app_data_dir().join("assets")
}

pub fn collect_image_assets(
    app: &dyn crate::models::PathResolver,
) -> crate::models::AppResult<Vec<crate::models::Asset>> {
    let assets_dir = app_assets_dir(app);
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    }

    let mut assets = Vec::new();
    let entries = fs::read_dir(&assets_dir)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(format) = image_format_for_path(&path) else {
            continue;
        };
        assets.push(crate::models::Asset {
            id: stable_asset_id_for_path(&path),
            name: asset_name_for_path(&path),
            path: path.to_string_lossy().to_string(),
            format,
        });
    }
    assets.sort_by_key(|asset| asset.name.to_lowercase());
    Ok(assets)
}

pub fn sync_image_assets_into_config(
    app: &dyn crate::models::PathResolver,
    config: &mut crate::models::Config,
) -> crate::models::AppResult<bool> {
    let scanned_assets = collect_image_assets(app)?;
    let mut changed = false;

    for asset in scanned_assets {
        if config
            .assets
            .iter()
            .any(|existing| existing.path == asset.path)
        {
            continue;
        }
        config.assets.push(asset);
        changed = true;
    }

    if changed {
        config.assets.sort_by_key(|asset| asset.name.to_lowercase());
    }

    Ok(changed)
}

#[tauri::command]
#[specta::specta]
pub async fn upload_asset(
    source_path: String,
    name: String,
    format: String,
    app: AppHandle,
) -> crate::models::AppResult<crate::models::Asset> {
    let assets_dir = app_assets_dir(&app);
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    }

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.{}", id, format.to_lowercase());
    let target_path = assets_dir.join(&file_name);

    fs::copy(&source_path, &target_path)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;

    Ok(crate::models::Asset {
        id,
        name,
        path: target_path.to_string_lossy().to_string(),
        format,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn save_recorded_audio(
    base64_data: String,
    name: String,
    app: AppHandle,
) -> crate::models::AppResult<crate::models::Asset> {
    let assets_dir = app_assets_dir(&app);
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    }

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.webm", id);
    let target_path = assets_dir.join(&file_name);

    let bytes = general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|err| crate::models::AppError::validation(err.to_string()))?;
    fs::write(&target_path, bytes)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;

    Ok(crate::models::Asset {
        id,
        name,
        path: target_path.to_string_lossy().to_string(),
        format: "WEBM".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Config, McpConfig};
    use crate::models::{Asset, PathResolver};
    use std::path::PathBuf;

    struct TestPathResolver {
        root: PathBuf,
    }

    impl TestPathResolver {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("ecky-assets-test-{}", Uuid::new_v4()));
            fs::create_dir_all(root.join("config")).unwrap();
            fs::create_dir_all(root.join("data")).unwrap();
            Self { root }
        }

        fn assets_dir(&self) -> PathBuf {
            self.app_data_dir().join("assets")
        }
    }

    impl Drop for TestPathResolver {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    impl PathResolver for TestPathResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn empty_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: false,
            connection_type: None,
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    #[test]
    fn collect_image_assets_only_returns_supported_images_sorted_by_name() {
        let resolver = TestPathResolver::new();
        let assets_dir = resolver.assets_dir();
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("z-last.webp"), b"fake").unwrap();
        fs::write(assets_dir.join("alpha_one.PNG"), b"fake").unwrap();
        fs::write(assets_dir.join("note.jpeg"), b"fake").unwrap();
        fs::write(assets_dir.join("ignore.webm"), b"fake").unwrap();
        fs::create_dir_all(assets_dir.join("nested")).unwrap();
        fs::write(assets_dir.join("nested").join("inside.png"), b"fake").unwrap();

        let assets = collect_image_assets(&resolver).unwrap();

        assert_eq!(assets.len(), 3);
        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha one", "note", "z last"]
        );
        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.format.as_str())
                .collect::<Vec<_>>(),
            vec!["PNG", "JPEG", "WEBP"]
        );
        assert!(assets.iter().all(|asset| asset.id.starts_with("asset-")));
    }

    #[test]
    fn sync_image_assets_into_config_adds_new_images_without_duplicate_paths() {
        let resolver = TestPathResolver::new();
        let assets_dir = resolver.assets_dir();
        fs::create_dir_all(&assets_dir).unwrap();
        let alpha_path = assets_dir.join("alpha.png");
        let beta_path = assets_dir.join("beta.jpg");
        fs::write(&alpha_path, b"fake").unwrap();
        fs::write(&beta_path, b"fake").unwrap();

        let mut config = empty_config();
        config.assets.push(Asset {
            id: "existing-alpha".to_string(),
            name: "alpha".to_string(),
            path: alpha_path.to_string_lossy().to_string(),
            format: "PNG".to_string(),
        });

        let changed = sync_image_assets_into_config(&resolver, &mut config).unwrap();
        assert!(changed);
        assert_eq!(config.assets.len(), 2);
        assert_eq!(
            config
                .assets
                .iter()
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );

        let changed_again = sync_image_assets_into_config(&resolver, &mut config).unwrap();
        assert!(!changed_again);
        assert_eq!(config.assets.len(), 2);
    }
}
