use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::models::{
    AppError, AppResult, ArtifactBundle, DocumentMetadata, EngineKind, EnrichmentStatus,
    ExportArtifact, FreecadLibraryImportRequest, FreecadLibraryItem, FreecadLibrarySearchRequest,
    GeometryBackend, ManifestEnrichmentState, ModelManifest, ModelSourceKind, PartBinding,
    PathResolver, SourceLanguage, ViewerAsset, ViewerAssetFormat, MODEL_RUNTIME_SCHEMA_VERSION,
};

const SUPPORTED_EXTENSIONS: &[&str] = &["fcstd", "step", "stp", "stl", "obj", "3mf", "brep", "brp"];
const CAD_IMPORTABLE_EXTENSIONS: &[&str] = &["step", "stp", "fcstd"];
const MESH_IMPORTABLE_EXTENSIONS: &[&str] = &["stl", "obj", "3mf"];
const MODEL_RUNTIME_ROOT: &str = "model-runtime";
const IMPORTED_MESH_ARTIFACT_DIR: &str = "imported-mesh";
const BUNDLE_FILE_NAME: &str = "bundle.json";
const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Default)]
struct ItemGroup {
    paths_by_format: BTreeMap<String, PathBuf>,
}

pub fn search_freecad_library(
    request: &FreecadLibrarySearchRequest,
    configured_roots: &[String],
) -> AppResult<Vec<FreecadLibraryItem>> {
    let roots = resolve_roots(&request.roots, configured_roots)?;
    let query_tokens = tokenize(&request.query);
    let limit = request.limit.unwrap_or(80).clamp(1, 500) as usize;
    let mut items = Vec::new();

    for root in roots {
        let root = root.canonicalize().map_err(|err| {
            AppError::persistence(format!(
                "Failed to resolve FreeCAD library root '{}': {}",
                root.display(),
                err
            ))
        })?;
        if !root.is_dir() {
            return Err(AppError::validation(format!(
                "FreeCAD library root '{}' is not a directory.",
                root.display()
            )));
        }

        let groups = scan_groups(&root)?;
        for (group_key, group) in groups {
            if !request.include_architecture && is_architecture_path(&group_key) {
                continue;
            }
            let Some(item) = build_item(&root, &group_key, group)? else {
                continue;
            };
            if matches_query(&item, &query_tokens) {
                items.push(item);
            }
        }
    }

    items.sort_by(|a, b| {
        score_item(b, &query_tokens)
            .cmp(&score_item(a, &query_tokens))
            .then_with(|| a.category_path.cmp(&b.category_path))
            .then_with(|| a.name.cmp(&b.name))
    });
    items.truncate(limit);
    Ok(items)
}

pub fn import_path_from_request(request: &FreecadLibraryImportRequest) -> AppResult<PathBuf> {
    let path = PathBuf::from(&request.item.import_path);
    if !path.exists() {
        return Err(AppError::not_found(format!(
            "FreeCAD library part '{}' was not found at '{}'.",
            request.item.name,
            path.display()
        )));
    }
    let ext = extension_key(&path).ok_or_else(|| {
        AppError::validation(format!(
            "FreeCAD library part '{}' has no importable extension.",
            path.display()
        ))
    })?;
    if !CAD_IMPORTABLE_EXTENSIONS.contains(&ext.as_str())
        && !MESH_IMPORTABLE_EXTENSIONS.contains(&ext.as_str())
    {
        return Err(AppError::validation(format!(
            "FreeCAD library part '{}' uses '{}', but only FCStd, STEP, STL, OBJ, and 3MF imports are supported.",
            request.item.name, ext
        )));
    }
    Ok(path)
}

pub fn import_mesh_from_request(
    request: &FreecadLibraryImportRequest,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let source_path = import_path_from_request(request)?;
    let ext = extension_key(&source_path).ok_or_else(|| {
        AppError::validation(format!(
            "FreeCAD library mesh '{}' has no importable extension.",
            source_path.display()
        ))
    })?;
    if !MESH_IMPORTABLE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(AppError::validation(format!(
            "FreeCAD library part '{}' is a CAD import, not a mesh import.",
            request.item.name
        )));
    }

    let source_bytes =
        fs::read(&source_path).map_err(|err| AppError::persistence(err.to_string()))?;
    let content_hash = digest_segments([source_bytes.as_slice()]);
    let model_id = format!("imported-mesh-{}", short_digest(&content_hash));
    let bundle_dir = app
        .app_data_dir()
        .join(MODEL_RUNTIME_ROOT)
        .join(IMPORTED_MESH_ARTIFACT_DIR)
        .join(&model_id);
    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let mesh_file_name = format!("source.{}", ext);
    let mesh_path = bundle_dir.join(mesh_file_name);
    if !mesh_path.exists() {
        fs::copy(&source_path, &mesh_path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to persist imported mesh '{}': {}",
                source_path.display(),
                err
            ))
        })?;
    }

    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    let mesh_path_string = path_to_string(&mesh_path)?;
    let label = if request.item.name.trim().is_empty() {
        source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(humanize_name)
            .unwrap_or_else(|| "Imported Mesh".to_string())
    } else {
        request.item.name.clone()
    };
    let part_id = "mesh-body".to_string();
    let warning =
        "Imported mesh models are reference-only; CAD booleans and topology selectors are unavailable."
            .to_string();
    let manifest = ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::ImportedMesh,
        source_digest: None,
        core_digest: None,
        ast_schema_version: None,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: label.clone(),
            document_label: label.clone(),
            source_path: Some(path_to_string(&source_path)?),
            object_count: 1,
            warnings: vec![warning.clone()],
        },
        parts: vec![PartBinding {
            part_id: part_id.clone(),
            freecad_object_name: label.clone(),
            label: label.clone(),
            kind: "mesh".to_string(),
            semantic_role: Some("mesh-reference".to_string()),
            viewer_asset_path: Some(mesh_path_string.clone()),
            viewer_node_ids: vec![part_id.clone()],
            parameter_keys: Vec::new(),
            editable: false,
            bounds: None,
            volume: None,
            area: None,
        }],
        parameter_groups: Vec::new(),
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        preview_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets: Vec::new(),
        measurement_annotations: Vec::new(),
        tagged_anchors: std::collections::BTreeMap::new(),
        feature_graph: None,
        correspondence_graph: None,
        warnings: vec![warning],
        enrichment_state: ManifestEnrichmentState {
            status: EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    };
    let viewer_asset = ViewerAsset {
        part_id: part_id.clone(),
        node_id: part_id,
        object_name: label.clone(),
        label,
        path: mesh_path_string.clone(),
        format: viewer_asset_format(&ext)?,
    };
    let bundle = ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id,
        source_kind: ModelSourceKind::ImportedMesh,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash,
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: path_to_string(&manifest_path)?,
        macro_path: None,
        preview_stl_path: if ext == "stl" {
            mesh_path_string.clone()
        } else {
            String::new()
        },
        viewer_assets: vec![viewer_asset],
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: vec![ExportArtifact {
            label: "Source mesh".to_string(),
            format: ext,
            path: mesh_path_string,
            role: "source".to_string(),
        }],
    };
    crate::models::validate_model_runtime_bundle(&manifest, &bundle)?;
    write_json(&manifest_path, &manifest)?;
    write_json(&bundle_path, &bundle)?;
    Ok(bundle)
}

fn resolve_roots(request_roots: &[String], configured_roots: &[String]) -> AppResult<Vec<PathBuf>> {
    let roots = if request_roots.is_empty() {
        configured_roots
    } else {
        request_roots
    };
    let roots = roots
        .iter()
        .map(|root| root.trim())
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err(AppError::validation(
            "No FreeCAD library folder configured. Pick a local FreeCAD-library folder first.",
        ));
    }
    Ok(roots)
}

fn scan_groups(root: &Path) -> AppResult<BTreeMap<String, ItemGroup>> {
    let mut groups = BTreeMap::<String, ItemGroup>::new();
    scan_dir(root, root, &mut groups)?;
    Ok(groups)
}

fn scan_dir(root: &Path, dir: &Path, groups: &mut BTreeMap<String, ItemGroup>) -> AppResult<()> {
    let entries = fs::read_dir(dir).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read FreeCAD library folder '{}': {}",
            dir.display(),
            err
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| AppError::persistence(err.to_string()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with('.') || file_name == "thumbnails" {
            continue;
        }
        if path.is_dir() {
            scan_dir(root, &path, groups)?;
            continue;
        }
        let Some(ext) = extension_key(&path) else {
            continue;
        };
        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(parent) = path.parent() else {
            continue;
        };
        let relative_parent = parent.strip_prefix(root).unwrap_or(parent);
        let key_path = relative_parent.join(stem);
        let key = normalize_relative_path(&key_path);
        groups
            .entry(key)
            .or_default()
            .paths_by_format
            .insert(ext, path);
    }
    Ok(())
}

fn build_item(root: &Path, key: &str, group: ItemGroup) -> AppResult<Option<FreecadLibraryItem>> {
    let preferred_format = preferred_format(&group.paths_by_format);
    if preferred_format.is_empty() {
        return Ok(None);
    }
    let import_path = group
        .paths_by_format
        .get(&preferred_format)
        .ok_or_else(|| {
            AppError::internal("Preferred FreeCAD library format missing from scanned group.")
        })?;
    let relative_import_path = import_path.strip_prefix(root).unwrap_or(import_path);
    let key_path = Path::new(key);
    let name = key_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(humanize_name)
        .unwrap_or_else(|| key.to_string());
    let category_path = key_path
        .parent()
        .map(category_label)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Uncategorized".to_string());
    let formats = group.paths_by_format.keys().cloned().collect::<Vec<_>>();
    let preview_path = thumbnail_path(root, &group.paths_by_format);
    let tags = tags_for_item(key, &formats);

    Ok(Some(FreecadLibraryItem {
        id: key.to_string(),
        name,
        category_path,
        root_path: path_to_string(root)?,
        relative_path: normalize_relative_path(relative_import_path),
        formats,
        preferred_format,
        import_path: path_to_string(import_path)?,
        preview_path,
        tags,
    }))
}

fn preferred_format(paths_by_format: &BTreeMap<String, PathBuf>) -> String {
    for ext in CAD_IMPORTABLE_EXTENSIONS {
        if paths_by_format.contains_key(*ext) {
            return (*ext).to_string();
        }
    }
    for ext in MESH_IMPORTABLE_EXTENSIONS {
        if paths_by_format.contains_key(*ext) {
            return (*ext).to_string();
        }
    }
    paths_by_format.keys().next().cloned().unwrap_or_default()
}

fn thumbnail_path(root: &Path, paths_by_format: &BTreeMap<String, PathBuf>) -> Option<String> {
    let fcstd_path = paths_by_format.get("fcstd")?;
    let stem = fcstd_path.file_stem()?.to_str()?;
    let parent_preview = fcstd_path.with_file_name(format!("{stem}.png"));
    if parent_preview.exists() {
        return Some(parent_preview.to_string_lossy().to_string());
    }
    let candidate = root.join("thumbnails").join(format!("{stem}.png"));
    if candidate.exists() {
        return Some(candidate.to_string_lossy().to_string());
    }
    None
}

fn tags_for_item(key: &str, formats: &[String]) -> Vec<String> {
    let lower = key.to_ascii_lowercase();
    let mut tags = BTreeSet::new();
    if lower.contains("architectural") || lower.contains("doors") || lower.contains("windows") {
        tags.insert("architecture".to_string());
    }
    if lower.contains("electronic") || lower.contains("electrical") {
        tags.insert("electronics".to_string());
    }
    if lower.contains("mechanical") {
        tags.insert("mechanical".to_string());
    }
    if lower.contains("bearing")
        || lower.contains("fastener")
        || lower.contains("screw")
        || lower.contains("bolt")
        || lower.contains("nut")
    {
        tags.insert("hardware".to_string());
        tags.insert("reference".to_string());
    }
    if formats
        .iter()
        .all(|format| MESH_IMPORTABLE_EXTENSIONS.contains(&format.as_str()))
    {
        tags.insert("meshOnly".to_string());
    }
    if formats.iter().any(|format| {
        format == "stl" || format == "obj" || format == "3mf" || format == "step" || format == "stp"
    }) {
        tags.insert("printableCandidate".to_string());
    }
    tags.into_iter().collect()
}

fn matches_query(item: &FreecadLibraryItem, query_tokens: &[String]) -> bool {
    if query_tokens.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {} {} {}",
        item.name,
        item.category_path,
        item.relative_path,
        item.formats.join(" "),
        item.tags.join(" ")
    )
    .to_ascii_lowercase();
    query_tokens.iter().all(|token| haystack.contains(token))
}

fn score_item(item: &FreecadLibraryItem, query_tokens: &[String]) -> i32 {
    let name = item.name.to_ascii_lowercase();
    let path = item.relative_path.to_ascii_lowercase();
    let mut score = 0;
    for token in query_tokens {
        if name == *token {
            score += 50;
        } else if name.contains(token) {
            score += 20;
        } else if path.contains(token) {
            score += 5;
        }
    }
    if item
        .formats
        .iter()
        .any(|format| format == "step" || format == "stp")
    {
        score += 4;
    }
    if item.formats.iter().any(|format| format == "fcstd") {
        score += 2;
    }
    score
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn is_architecture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("architectural parts") || lower.contains("doors_windows")
}

fn extension_key(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .map(|ext| {
            if ext == "fcstd" {
                "fcstd".to_string()
            } else {
                ext
            }
        })
}

fn humanize_name(name: &str) -> String {
    name.replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn category_label(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn normalize_relative_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

fn path_to_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| AppError::internal(format!("Invalid UTF-8 path '{}'.", path.display())))
}

fn viewer_asset_format(ext: &str) -> AppResult<ViewerAssetFormat> {
    match ext {
        "stl" => Ok(ViewerAssetFormat::Stl),
        "obj" => Ok(ViewerAssetFormat::Obj),
        "3mf" => Ok(ViewerAssetFormat::ThreeMf),
        other => Err(AppError::validation(format!(
            "Unsupported mesh viewer format '{}'.",
            other
        ))),
    }
}

fn digest_segments<const N: usize>(segments: [&[u8]; N]) -> String {
    let mut hasher = Sha256::new();
    for segment in segments {
        hasher.update(segment);
    }
    format!("{:x}", hasher.finalize())
}

fn short_digest(digest: &str) -> &str {
    &digest[..12]
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let data = serde_json::to_string_pretty(value)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| {
        AppError::persistence(format!("Failed to write '{}': {}", path.display(), err))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ecky-freecad-library-{name}-{nonce}"))
    }

    #[test]
    fn search_groups_formats_and_prefers_step_import() {
        let root = temp_root("formats");
        let dir = root.join("Mechanical Parts").join("Bearings");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("608.FCStd"), b"fcstd").unwrap();
        fs::write(dir.join("608.step"), b"step").unwrap();
        fs::write(dir.join("608.stl"), b"stl").unwrap();

        let request = FreecadLibrarySearchRequest {
            query: "608 bearing".to_string(),
            roots: vec![root.to_string_lossy().to_string()],
            limit: Some(10),
            include_architecture: false,
        };
        let items = search_freecad_library(&request, &[]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].preferred_format, "step");
        assert!(items[0].formats.contains(&"fcstd".to_string()));
        assert!(items[0].tags.contains(&"hardware".to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_hides_architecture_by_default() {
        let root = temp_root("architecture");
        let dir = root.join("Architectural Parts").join("Doors");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Door.FCStd"), b"fcstd").unwrap();

        let request = FreecadLibrarySearchRequest {
            query: "door".to_string(),
            roots: vec![root.to_string_lossy().to_string()],
            limit: Some(10),
            include_architecture: false,
        };
        assert!(search_freecad_library(&request, &[]).unwrap().is_empty());

        let request = FreecadLibrarySearchRequest {
            include_architecture: true,
            ..request
        };
        assert_eq!(search_freecad_library(&request, &[]).unwrap().len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_marks_stl_obj_and_3mf_as_mesh_only_imports() {
        let root = temp_root("mesh");
        let dir = root.join("Printable");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Fan Guard.stl"), b"solid fan\nendsolid fan\n").unwrap();
        fs::write(dir.join("Cable Clip.obj"), b"o clip\n").unwrap();
        fs::write(dir.join("Case.3mf"), b"PK").unwrap();

        let request = FreecadLibrarySearchRequest {
            query: "printable".to_string(),
            roots: vec![root.to_string_lossy().to_string()],
            limit: Some(10),
            include_architecture: false,
        };
        let items = search_freecad_library(&request, &[]).unwrap();
        assert_eq!(items.len(), 3);
        assert!(items
            .iter()
            .all(|item| item.tags.contains(&"meshOnly".to_string())));
        assert!(items
            .iter()
            .any(|item| item.preferred_format == "stl" && item.name == "Fan Guard"));
        assert!(items.iter().any(|item| item.preferred_format == "obj"));
        assert!(items.iter().any(|item| item.preferred_format == "3mf"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_mesh_from_request_persists_runtime_bundle_without_freecad() {
        let root = temp_root("mesh-import-source");
        let app_root = temp_root("mesh-import-app");
        let dir = root.join("Printable");
        fs::create_dir_all(&dir).unwrap();
        let mesh_path = dir.join("Fan Guard.stl");
        fs::write(&mesh_path, b"solid fan\nendsolid fan\n").unwrap();

        let request = FreecadLibrarySearchRequest {
            query: "fan".to_string(),
            roots: vec![root.to_string_lossy().to_string()],
            limit: Some(10),
            include_architecture: false,
        };
        let item = search_freecad_library(&request, &[]).unwrap().remove(0);
        let bundle = import_mesh_from_request(
            &FreecadLibraryImportRequest {
                item,
                thread_id: None,
                title: None,
            },
            &TestResolver {
                root: app_root.clone(),
            },
        )
        .unwrap();

        assert_eq!(bundle.source_kind, ModelSourceKind::ImportedMesh);
        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert_eq!(bundle.viewer_assets[0].format, ViewerAssetFormat::Stl);
        assert!(Path::new(&bundle.viewer_assets[0].path).exists());
        assert!(Path::new(&bundle.manifest_path).exists());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(app_root);
    }
}
