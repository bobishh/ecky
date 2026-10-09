use crate::contracts::AppResult;
use crate::models::PathResolver;

// --- Component library tools (component-unification T5) ---

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentExtractToolRequest {
    /// Full `.ecky` model source containing the part to lift.
    pub source: String,
    pub part_key: String,
    /// Component name; defaults to the part key.
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    /// Save the extracted component into the component library.
    #[serde(default)]
    pub save: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentExtractToolResponse {
    pub name: String,
    /// Copy-inline `define-component` source, pasteable into any model.
    pub component_source: String,
    pub header: crate::component_extract::ComponentHeader,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSearchToolRequest {
    pub query: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSearchToolResponse {
    pub results: Vec<crate::component_package_runtime::ExtractedComponentSearchResult>,
    pub indexing_diagnostics: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentGetToolRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub component_id: Option<String>,
    #[serde(default)]
    pub revision_digest: Option<String>,
}

/// Copy-inline package import. `source` is the active model source to mutate;
/// returned source is self-contained and never a live package reference.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentImportToolRequest {
    pub package_id: String,
    pub version: String,
    pub component_id: String,
    pub source: String,
}

pub fn handle_component_extract(
    app: &dyn PathResolver,
    req: ComponentExtractToolRequest,
) -> AppResult<ComponentExtractToolResponse> {
    let extracted = crate::component_extract::extract_component(
        &crate::component_extract::ComponentExtractRequest {
            source: req.source,
            part_key: req.part_key,
            component_name: req.name,
            description: req.description,
            tags: req.tags,
            thread_id: req.thread_id,
            message_id: req.message_id,
        },
    )?;
    let saved_path = if req.save {
        let dir = crate::component_package_runtime::save_extracted_component(app, &extracted)?;
        Some(dir.to_string_lossy().to_string())
    } else {
        None
    };
    Ok(ComponentExtractToolResponse {
        name: extracted.name.clone(),
        component_source: extracted.component_source.clone(),
        header: extracted.header.clone(),
        saved_path,
    })
}

pub fn handle_component_search(
    app: &dyn PathResolver,
    req: ComponentSearchToolRequest,
) -> AppResult<ComponentSearchToolResponse> {
    let limit = req.limit.unwrap_or(20).clamp(1, 100);
    let results = crate::component_package_runtime::search_extracted_components(
        app,
        req.query.as_deref().unwrap_or(""),
        limit,
    )?;
    Ok(ComponentSearchToolResponse {
        results,
        indexing_diagnostics: Vec::new(),
    })
}

pub fn handle_component_get(
    app: &dyn PathResolver,
    req: ComponentGetToolRequest,
) -> AppResult<crate::component_package_runtime::ExtractedComponentRecord> {
    if let Some(component_id) = req.component_id.as_deref() {
        return crate::component_package_runtime::read_component_by_id(
            app,
            component_id,
            req.revision_digest.as_deref(),
        );
    }
    if req.revision_digest.is_some() {
        return Err(crate::contracts::AppError::validation(
            "revisionDigest requires componentId.",
        ));
    }
    let name = req
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            crate::contracts::AppError::validation("Provide a component name or componentId.")
        })?;
    crate::component_package_runtime::read_extracted_component(app, name)
}

pub fn handle_component_import(
    app: &dyn PathResolver,
    req: ComponentImportToolRequest,
) -> AppResult<crate::component_import_runtime::CopyInlineComponentImportResponse> {
    crate::component_import_runtime::copy_inline_component_import(
        crate::component_import_runtime::CopyInlineComponentImportRequest {
            package_id: req.package_id,
            version: req.version,
            component_id: req.component_id,
            authored_source: req.source,
        },
        &crate::component_import_runtime::InstalledLibraryComponentResolver { app },
    )
}
