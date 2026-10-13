//! Host-owned component import pre-resolution (component-package-imports,
//! Decision 3 & 4).
//!
//! Live package references (`(import-component ...)`) MUST be resolved through
//! this runtime before the pure, source-only compiler runs. The compiler itself
//! performs no package filesystem access and rejects unresolved imports (see
//! `ecky_render::scheme::bootstrap::validate_user_source`).
//!
//! `resolve_authoring_source` parses top-level import forms, resolves exact
//! installed source components, performs an AST-safe (namespace-keyed)
//! materialization into ephemeral `compiler_source`, and produces a canonical
//! dependency lock plus import-span evidence. `compile_authoring_source` then
//! feeds the resolved compiler source to the unchanged `SourceCompiler` and
//! attaches per-import origin evidence.
//!
//! Copy-inline vendoring (MCP/UI `component_import` / `component_get`) is a
//! separate, non-overlapping operation and is untouched here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ecky_render::SourceCompiler;
use sha2::Digest;
use steel_core::parser::ast::ExprKind;
use steel_core::parser::parser::Parser;

use crate::component_package_runtime::{
    payload_store_dir, read_component_package_manifest, read_coordinate_index,
    read_payload_inventory, safe_archive_path,
};
use crate::component_step_runtime::StepAsset;
use crate::contracts::{
    is_valid_ecky_symbol, AppError, AppResult, ComponentCoordinate, ComponentDependencyLock,
    ComponentDependencyLockComponent, ComponentDependencyLockEntry, ComponentImportOrigin,
    ComponentImportSourceSpan, ComponentPayloadKind, GeometryProvenance, GeometryRepresentation,
};
use crate::ecky_cad_host::source_compiler::NativeSourceCompiler;
use crate::ecky_scheme::compiler::{expr_head_name, expr_identifier, expr_list_items};
use crate::models::PathResolver;

/// A package source component resolved by an [`InstalledComponentResolver`].
#[derive(Clone, Debug)]
pub struct ResolvedSourceComponent {
    pub coordinate: ComponentCoordinate,
    /// Resolved export symbol (explicit `entrySymbol` or `componentId` fallback).
    pub entry_symbol: String,
    /// Package source containing the exported `(define-component ...)`.
    pub source: String,
    /// `sha256:<hex>` package payload digest the source was loaded from.
    pub payload_digest: String,
}

/// A static package STEP component resolved from the immutable payload store.
#[derive(Clone, Debug)]
pub struct ResolvedStepComponent {
    pub coordinate: ComponentCoordinate,
    pub path: PathBuf,
    /// Digest of the containing immutable package payload.
    pub package_digest: String,
    /// Digest of exactly the STEP bytes passed to Direct OCCT.
    pub payload_digest: String,
    pub geometry_provenance: GeometryProvenance,
}

/// Resolves an exact installed source component coordinate to its source export.
/// The production implementation reads from the global content-addressed store;
/// tests use an in-memory implementation.
pub trait InstalledComponentResolver {
    fn resolve_source_component(
        &self,
        coordinate: &ComponentCoordinate,
    ) -> AppResult<ResolvedSourceComponent>;

    /// Resolve a previously committed coordinate directly at its locked
    /// payload digest. The default preserves the in-memory resolver seam while
    /// enforcing that it cannot redirect a locked request. The production
    /// resolver overrides this to bypass mutable coordinate discovery.
    fn resolve_source_component_with_expected_digest(
        &self,
        coordinate: &ComponentCoordinate,
        expected_package_digest: &str,
    ) -> AppResult<ResolvedSourceComponent> {
        let component = self.resolve_source_component(coordinate)?;
        if component.payload_digest != expected_package_digest {
            return Err(AppError::validation(format!(
                "Resolved package payload digest '{}' for '{}' does not match locked digest '{}'.",
                component.payload_digest,
                coordinate.canonical_identity(),
                expected_package_digest,
            )));
        }
        Ok(component)
    }

    /// Optional static STEP payload branch. Source-only resolvers keep the
    /// default and therefore preserve the original test/API seam.
    fn resolve_step_component(
        &self,
        _coordinate: &ComponentCoordinate,
    ) -> AppResult<Option<ResolvedStepComponent>> {
        Ok(None)
    }

    fn resolve_step_component_with_expected_digest(
        &self,
        coordinate: &ComponentCoordinate,
        expected_package_digest: &str,
    ) -> AppResult<Option<ResolvedStepComponent>> {
        let resolved = self.resolve_step_component(coordinate)?;
        if let Some(component) = &resolved {
            if component.package_digest != expected_package_digest {
                return Err(AppError::validation(format!(
                    "Resolved package payload digest '{}' for '{}' does not match locked digest '{}'.",
                    component.package_digest,
                    coordinate.canonical_identity(),
                    expected_package_digest,
                )));
            }
        }
        Ok(resolved)
    }
}

/// Request to pre-resolve authored source that may contain live references.
pub struct ResolveAuthoringSourceRequest<'a> {
    pub authored_source: &'a str,
    /// Expected lock for a committed version. When present, every resolved
    /// payload digest must match it exactly; mismatches block resolution.
    pub expected_lock: Option<&'a ComponentDependencyLock>,
}

/// Authored byte span of one `(import-component ...)` form, plus its resolved
/// coordinate evidence. Used for diagnostics and provenance.
#[derive(Clone, Debug)]
pub struct ComponentImportSpan {
    pub coordinate: ComponentCoordinate,
    pub alias: String,
    pub entry_symbol: String,
    pub payload_digest: String,
    pub authored_start: u32,
    pub authored_end: u32,
    /// Byte range in ephemeral compiler source occupied by the materialized
    /// package definitions. This is transient-only span evidence.
    pub resolved_start: u32,
    pub resolved_end: u32,
}

/// Output of [`resolve_authoring_source`]: ephemeral compiler source with all
/// imports materialized, plus the canonical dependency lock and import spans.
#[derive(Clone, Debug)]
pub struct ResolvedAuthoringSource {
    pub compiler_source: String,
    pub dependency_lock: ComponentDependencyLock,
    pub import_spans: Vec<ComponentImportSpan>,
    pub step_assets: Vec<StepAsset>,
}

/// Output of [`compile_authoring_source`]: the compiled CoreProgram plus the
/// dependency lock and host-owned per-import/node origin evidence.
#[derive(Clone, Debug)]
pub struct ResolvedCompilation {
    pub compiler_source: String,
    pub program: ecky_render::core_ir::CoreProgram,
    pub dependency_lock: ComponentDependencyLock,
    /// Alias-level evidence suitable for persistence in bundle/manifest
    /// sidecars. It deliberately lives outside Core IR.
    pub origins: Vec<ComponentImportOrigin>,
    /// Transient node attribution keyed by raw Core node id. Core IR remains
    /// package-agnostic; the host owns this sidecar mapping.
    pub origins_by_node: BTreeMap<u64, ComponentImportOrigin>,
    pub step_assets: Vec<StepAsset>,
}

/// Request for the copy-inline component workflow used by MCP and Workbench.
/// This is intentionally distinct from the live-reference resolver above.
#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyInlineComponentImportRequest {
    pub package_id: String,
    pub version: String,
    pub component_id: String,
    /// Current persisted-or-draft authoring source. The inserted result remains
    /// ordinary source and carries no package dependency state.
    pub authored_source: String,
}

/// Result of a copy-inline component insertion. `authored_source` contains
/// full package definitions plus one concrete `(part ...)` instance.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyInlineComponentImportResponse {
    pub authored_source: String,
    pub component_source: String,
    pub entry_symbol: String,
    pub part_key: String,
}

/// Materialize an installed source component into authored source.
///
/// Deliberately no `import-component` form, registry coordinate, or dependency
/// lock escapes this boundary. Package source is validated by the installed
/// resolver before it is copied.
pub fn copy_inline_component_import(
    request: CopyInlineComponentImportRequest,
    resolver: &dyn InstalledComponentResolver,
) -> AppResult<CopyInlineComponentImportResponse> {
    let coordinate = ComponentCoordinate {
        package_id: request.package_id,
        version: request.version,
        component_id: request.component_id,
    };
    let component = resolver.resolve_source_component(&coordinate)?;
    if component.coordinate != coordinate {
        return Err(AppError::validation(format!(
            "Resolver returned '{}' while resolving exact component '{}'.",
            component.coordinate.canonical_identity(),
            coordinate.canonical_identity(),
        )));
    }
    inspect_package_source(&component.source, &component.entry_symbol)?;

    let package_names = collect_local_binding_names(&component.source);
    let local_names = collect_local_binding_names(&request.authored_source);
    if let Some(name) = package_names.intersection(&local_names).next() {
        return Err(AppError::validation(format!(
            "Cannot copy-inline '{}' because definition '{}' already exists in the active model.",
            coordinate.canonical_identity(),
            name
        )));
    }

    let model_end = top_level_model_end(&request.authored_source)?;
    let part_key = next_copy_inline_part_key(&request.authored_source, &coordinate.component_id);
    let instance = format!("\n  (part {part_key} ({}))", component.entry_symbol);
    let mut authored_source = String::with_capacity(
        request.authored_source.len() + component.source.len() + instance.len() + 4,
    );
    authored_source.push_str(component.source.trim());
    authored_source.push('\n');
    authored_source.push_str(&request.authored_source[..model_end]);
    authored_source.push_str(&instance);
    authored_source.push_str(&request.authored_source[model_end..]);

    // Guard the persisted boundary, not merely the source package.
    if source_has_live_component_import(&authored_source) {
        return Err(AppError::validation(
            "Copy-inline import produced a live component reference; package source must be self-contained."
                .to_string(),
        ));
    }
    Ok(CopyInlineComponentImportResponse {
        authored_source,
        component_source: component.source,
        entry_symbol: component.entry_symbol,
        part_key,
    })
}

/// Byte index immediately before the closing parenthesis of the sole active
/// top-level model. Parser locations keep comments and existing formatting
/// intact while the insertion itself remains a minimal source edit.
fn top_level_model_end(source: &str) -> AppResult<usize> {
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| AppError::parse(format!("Failed to parse active model source: {err}")))?;
    let model = forms
        .iter()
        .filter_map(|form| {
            let items = expr_list_items(form, "top-level form").ok()?;
            (items.first().and_then(expr_identifier).as_deref() == Some("model")).then_some(form)
        })
        .next()
        .ok_or_else(|| {
            AppError::validation(
                "Copy-inline import requires an active `(model ...)` source.".to_string(),
            )
        })?;
    let (_, end) = form_span(model);
    let end = end as usize;
    if end == 0 || end > source.len() {
        return Err(AppError::parse(
            "Active model has an invalid source span.".to_string(),
        ));
    }
    Ok(end - 1)
}

fn next_copy_inline_part_key(source: &str, component_id: &str) -> String {
    let mut candidate = component_id.to_string();
    let mut suffix = 2;
    while source.contains(&format!("(part {candidate}")) {
        candidate = format!("{component_id}-{suffix}");
        suffix += 1;
    }
    candidate
}

pub fn source_has_live_component_import(source: &str) -> bool {
    parse_import_declarations(source)
        .map(|imports| !imports.is_empty())
        .unwrap_or(false)
}

/// Parse and pre-resolve authored source containing live references. Returns
/// ephemeral compiler source (no `import-component` forms), a canonical
/// dependency lock, and import-span evidence.
pub fn resolve_authoring_source(
    request: ResolveAuthoringSourceRequest<'_>,
    resolver: &dyn InstalledComponentResolver,
) -> AppResult<ResolvedAuthoringSource> {
    let declarations = parse_import_declarations(request.authored_source)?;
    if let Some(expected) = request.expected_lock {
        expected.validate()?;
    }

    let mut alias_seen: BTreeMap<String, ComponentCoordinate> = BTreeMap::new();
    for declaration in &declarations {
        validate_alias(&declaration.alias)?;
        if let Some(existing) = alias_seen.get(&declaration.alias) {
            return Err(AppError::validation(format!(
                "Import alias '{}' is used by two package components: '{}@{}:{}' and '{}@{}:{}'.",
                declaration.alias,
                existing.package_id,
                existing.version,
                existing.component_id,
                declaration.coordinate.package_id,
                declaration.coordinate.version,
                declaration.coordinate.component_id,
            )));
        }
        alias_seen.insert(declaration.alias.clone(), declaration.coordinate.clone());
    }

    let mut resolved_sources: Vec<(ParsedImportDeclaration, ResolvedSourceComponent)> = Vec::new();
    let mut resolved_steps: Vec<(ParsedImportDeclaration, ResolvedStepComponent)> = Vec::new();
    for declaration in declarations {
        let expected_entry = request
            .expected_lock
            .map(|expected| expected_lock_entry(expected, &declaration.coordinate))
            .transpose()?;
        let step = if let Some(entry) = expected_entry {
            resolver.resolve_step_component_with_expected_digest(
                &declaration.coordinate,
                &entry.package_digest,
            )?
        } else {
            resolver.resolve_step_component(&declaration.coordinate)?
        };
        if let Some(component) = step {
            if component.coordinate != declaration.coordinate {
                return Err(AppError::validation(format!(
                    "Resolver returned '{}' while resolving exact component '{}'.",
                    component.coordinate.canonical_identity(),
                    declaration.coordinate.canonical_identity(),
                )));
            }
            if let Some(entry) = expected_entry {
                let expected_component = expected_lock_component(entry, &declaration.coordinate)?;
                if expected_component.payload_kind != Some(ComponentPayloadKind::Step)
                    || expected_component.payload_digest != component.payload_digest
                    || expected_component.geometry_representation
                        != Some(component.geometry_provenance.representation.clone())
                {
                    return Err(AppError::validation(format!(
                        "Resolved STEP evidence for '{}' does not match the committed dependency lock.",
                        declaration.coordinate.canonical_identity()
                    )));
                }
            }
            resolved_steps.push((declaration, component));
            continue;
        }

        let component = if let Some(entry) = expected_entry {
            resolver.resolve_source_component_with_expected_digest(
                &declaration.coordinate,
                &entry.package_digest,
            )?
        } else {
            resolver.resolve_source_component(&declaration.coordinate)?
        };
        if let Some(entry) = expected_entry {
            let expected_component = expected_lock_component(entry, &declaration.coordinate)?;
            if expected_component.payload_kind == Some(ComponentPayloadKind::Step) {
                return Err(AppError::validation(format!(
                    "Expected dependency lock records '{}' as STEP, but resolver returned source.",
                    declaration.coordinate.canonical_identity()
                )));
            }
        }
        if component.coordinate != declaration.coordinate {
            return Err(AppError::validation(format!(
                "Resolver returned '{}' while resolving exact component '{}'.",
                component.coordinate.canonical_identity(),
                declaration.coordinate.canonical_identity(),
            )));
        }
        resolved_sources.push((declaration, component));
    }

    // Local-binding collision detection: aliases must not collide with the
    // model's own top-level define / define-component / import names.
    let local_names = collect_local_binding_names(request.authored_source);
    let alias_names: BTreeSet<String> = resolved_sources
        .iter()
        .map(|(declaration, _)| declaration.alias.clone())
        .chain(
            resolved_steps
                .iter()
                .map(|(declaration, _)| declaration.alias.clone()),
        )
        .collect();
    for alias in &alias_names {
        if local_names.contains(alias) {
            return Err(AppError::validation(format!(
                "Import alias '{}' collides with a local component, helper, or reserved form in the authored model.",
                alias
            )));
        }
    }

    let step_assets = resolved_steps
        .iter()
        .map(|(declaration, component)| StepAsset {
            coordinate: component.coordinate.clone(),
            alias: declaration.alias.clone(),
            path: component.path.clone(),
            payload_digest: component.payload_digest.clone(),
            geometry_provenance: component.geometry_provenance.clone(),
        })
        .collect::<Vec<_>>();
    let step_lowered = crate::component_step_runtime::lower_step_assets_to_compiler_source(
        request.authored_source,
        &step_assets,
    )?;
    let materialized = materialize_compiler_source(&step_lowered, &resolved_sources)?;
    let (dependency_lock, mut import_spans) =
        build_lock_and_spans(&resolved_sources, &resolved_steps);
    for span in &mut import_spans {
        let (start, end) = materialized
            .resolved_ranges
            .get(&span.alias)
            .copied()
            .unwrap_or((0, 0));
        span.resolved_start = start;
        span.resolved_end = end;
    }
    let compiler_source = materialized.compiler_source;
    dependency_lock.validate()?;
    if let Some(expected) = request.expected_lock {
        verify_against_expected_lock(expected, &dependency_lock)?;
    }

    Ok(ResolvedAuthoringSource {
        compiler_source,
        dependency_lock,
        import_spans,
        step_assets,
    })
}

/// Resolve then compile authored source through the unchanged native compiler.
pub fn compile_authoring_source(
    request: ResolveAuthoringSourceRequest<'_>,
    resolver: &dyn InstalledComponentResolver,
) -> AppResult<ResolvedCompilation> {
    let resolved = resolve_authoring_source(request, resolver)?;
    let program = NativeSourceCompiler.compile(&resolved.compiler_source)?;
    let mut origins = resolved
        .import_spans
        .iter()
        .map(|span| ComponentImportOrigin {
            package_id: span.coordinate.package_id.clone(),
            version: span.coordinate.version.clone(),
            component_id: span.coordinate.component_id.clone(),
            alias: span.alias.clone(),
            payload_digest: span.payload_digest.clone(),
            authored_span: Some(ComponentImportSourceSpan {
                start: span.authored_start,
                end: span.authored_end,
            }),
            resolved_span: Some(ComponentImportSourceSpan {
                start: span.resolved_start,
                end: span.resolved_end,
            }),
            part_ids: Vec::new(),
            node_ids: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut origins_by_node = BTreeMap::new();
    for part in &program.parts {
        let mut nodes = Vec::new();
        collect_core_nodes(&part.root, &mut nodes);
        for (node_id, span) in nodes {
            let Some(span) = span else {
                continue;
            };
            let Some((origin_index, _)) =
                resolved
                    .import_spans
                    .iter()
                    .enumerate()
                    .find(|(_, import)| {
                        import.resolved_start <= span.start
                            && span.end <= import.resolved_end
                            && import.resolved_start != import.resolved_end
                    })
            else {
                continue;
            };
            let origin = &mut origins[origin_index];
            if !origin.part_ids.contains(&part.key) {
                origin.part_ids.push(part.key.clone());
            }
            if !origin.node_ids.contains(&node_id) {
                origin.node_ids.push(node_id);
            }
            origins_by_node.insert(node_id, origin.clone());
        }
    }
    for origin in &mut origins {
        origin.part_ids.sort();
        origin.node_ids.sort_unstable();
    }
    Ok(ResolvedCompilation {
        compiler_source: resolved.compiler_source,
        program,
        dependency_lock: resolved.dependency_lock,
        origins,
        origins_by_node,
        step_assets: resolved.step_assets,
    })
}

/// Persist live-import lock/origin evidence into runtime-owned sidecars after
/// rendering. Core IR remains package-agnostic. The lock digest is folded into
/// `ArtifactBundle.contentHash`, preventing cache reuse across dependency
/// changes even when authored source and parameters are equal.
pub fn attach_resolved_component_evidence(
    bundle: &mut crate::contracts::ArtifactBundle,
    manifest: &mut crate::contracts::ModelManifest,
    compilation: &ResolvedCompilation,
) -> AppResult<()> {
    if compilation.dependency_lock.dependencies.is_empty() {
        return Ok(());
    }
    let lock = compilation.dependency_lock.clone().canonical();
    lock.validate()?;
    let lock_digest = format!("sha256:{:x}", sha2::Sha256::digest(lock.canonical_bytes()?));
    let mut origins = compilation.origins.clone();
    origins.sort_by(|left, right| {
        left.package_id
            .cmp(&right.package_id)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.component_id.cmp(&right.component_id))
            .then_with(|| left.alias.cmp(&right.alias))
    });
    let mut content_hasher = sha2::Sha256::new();
    content_hasher.update(b"ecky-artifact-component-lock-v1\0");
    content_hasher.update(bundle.content_hash.as_bytes());
    content_hasher.update(lock_digest.as_bytes());
    bundle.content_hash = format!("{:x}", content_hasher.finalize());
    bundle.component_dependency_lock = Some(lock);
    bundle.component_dependency_lock_digest = Some(lock_digest);
    bundle.component_import_origins = origins.clone();
    manifest.component_import_origins = origins;

    if !compilation.step_assets.is_empty() {
        let authored_representation = bundle
            .geometry_provenance
            .as_ref()
            .map(|evidence| evidence.representation.clone())
            .unwrap_or(GeometryRepresentation::AnalyticBrep);
        let merged = crate::component_step_runtime::merge_step_geometry_provenance(
            authored_representation,
            &compilation.step_assets,
        );
        bundle.geometry_provenance = Some(merged.clone());
        manifest.geometry_provenance = Some(merged.clone());
        for artifact in &mut bundle.export_artifacts {
            if artifact.format.eq_ignore_ascii_case("step") {
                artifact.geometry_provenance = Some(merged.clone());
            }
        }
    }

    crate::contracts::validate_component_import_evidence(
        bundle.component_dependency_lock.as_ref(),
        bundle.component_dependency_lock_digest.as_deref(),
        &bundle.component_import_origins,
        &manifest.component_import_origins,
    )
}

fn collect_core_nodes(
    node: &ecky_render::core_ir::CoreNode,
    nodes: &mut Vec<(u64, Option<ecky_render::core_ir::SourceSpan>)>,
) {
    use ecky_render::core_ir::CoreNodeKind;

    nodes.push((node.id.raw(), node.span));
    match &node.kind {
        CoreNodeKind::Call { args, .. } => {
            for arg in args {
                collect_core_nodes(arg, nodes);
            }
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                collect_core_nodes(&binding.value, nodes);
            }
            collect_core_nodes(body, nodes);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_core_nodes(item, nodes);
            }
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_core_nodes(condition, nodes);
            collect_core_nodes(then_branch, nodes);
            collect_core_nodes(else_branch, nodes);
        }
        _ => {}
    }
}

/// Production resolver backed by the global content-addressed store and the
/// mutable coordinate index. Unlocked resolution flows through the index;
/// callers pass an expected lock to pin a committed version's digest.
pub struct InstalledLibraryComponentResolver<'a> {
    pub app: &'a dyn PathResolver,
}

impl<'a> InstalledLibraryComponentResolver<'a> {
    /// Resolve from a content-addressed store entry, independently of the
    /// mutable coordinate index. This is the historical-version path.
    fn resolve_at_payload_digest(
        &self,
        coordinate: &ComponentCoordinate,
        payload_digest: &str,
    ) -> AppResult<ResolvedSourceComponent> {
        let store_dir = payload_store_dir(self.app, payload_digest)?;
        let inventory = read_payload_inventory(&store_dir)?;
        if inventory.package_digest != payload_digest {
            return Err(AppError::validation(format!(
                "Package store payload '{}' has an integrity sidecar for '{}', not the requested locked digest for '{}'.",
                payload_digest,
                inventory.package_digest,
                coordinate.canonical_identity(),
            )));
        }
        let package = read_component_package_manifest(&store_dir)?;
        if package.package_id != coordinate.package_id || package.version != coordinate.version {
            return Err(AppError::validation(format!(
                "Package store payload '{}' contains '{}@{}', not locked coordinate '{}@{}'.",
                payload_digest,
                package.package_id,
                package.version,
                coordinate.package_id,
                coordinate.version,
            )));
        }
        let component = package
            .components
            .iter()
            .find(|component| component.component_id == coordinate.component_id)
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "Installed package '{}@{}' does not contain componentId '{}'.",
                    coordinate.package_id, coordinate.version, coordinate.component_id
                ))
            })?;
        let source_ref = component.source_ref.as_deref().ok_or_else(|| {
            AppError::validation(format!(
                "Component '{}' is missing a sourceRef and cannot be live-imported.",
                coordinate.canonical_identity()
            ))
        })?;
        let relative = safe_archive_path(source_ref)?;
        let entry_symbol = component
            .entry_symbol
            .clone()
            .filter(|symbol| is_valid_ecky_symbol(symbol))
            .unwrap_or_else(|| coordinate.component_id.clone());
        let source_path = store_dir.join(&relative);
        let source = std::fs::read_to_string(&source_path).map_err(|err| {
            AppError::not_found(format!(
                "Component '{}' source file '{}' was not found in the store: {}",
                coordinate.canonical_identity(),
                relative.display(),
                err
            ))
        })?;
        let source_digest = format!("sha256:{:x}", sha2::Sha256::digest(source.as_bytes()));
        let inventory_match = inventory.entries.iter().any(|entry| {
            entry.path == relative.to_string_lossy().replace('\\', "/")
                && entry.sha256 == source_digest
        });
        if !inventory_match {
            return Err(AppError::validation(format!(
                "Component '{}' source file '{}' digest does not match the package inventory.",
                coordinate.canonical_identity(),
                relative.display()
            )));
        }
        if let Err(inner) = inspect_package_source(&source, &entry_symbol) {
            return Err(AppError::validation(format!(
                "Package component '{}' {}",
                coordinate.canonical_identity(),
                inner.message
            )));
        }
        Ok(ResolvedSourceComponent {
            coordinate: coordinate.clone(),
            entry_symbol,
            source,
            payload_digest: payload_digest.to_string(),
        })
    }

    fn resolve_step_at_payload_digest(
        &self,
        coordinate: &ComponentCoordinate,
        package_digest: &str,
    ) -> AppResult<Option<ResolvedStepComponent>> {
        let store_dir = payload_store_dir(self.app, package_digest)?;
        let inventory = read_payload_inventory(&store_dir)?;
        if inventory.package_digest != package_digest {
            return Err(AppError::validation(format!(
                "Package store payload '{}' has an integrity sidecar for '{}'.",
                package_digest, inventory.package_digest
            )));
        }
        let package = read_component_package_manifest(&store_dir)?;
        if package.package_id != coordinate.package_id || package.version != coordinate.version {
            return Err(AppError::validation(format!(
                "Package store payload '{}' does not contain locked coordinate '{}@{}'.",
                package_digest, coordinate.package_id, coordinate.version
            )));
        }
        if package.visibility != crate::contracts::PackageVisibility::Source {
            return Err(AppError::validation(format!(
                "STEP component '{}' must come from a source-visible package.",
                coordinate.canonical_identity()
            )));
        }
        let component = package
            .components
            .iter()
            .find(|component| component.component_id == coordinate.component_id)
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "Installed package '{}@{}' does not contain componentId '{}'.",
                    coordinate.package_id, coordinate.version, coordinate.component_id
                ))
            })?;
        let Some(source_ref) = component.source_ref.as_deref() else {
            return Ok(None);
        };
        let relative = safe_archive_path(source_ref)?;
        let extension = relative
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(extension.as_deref(), Some("step") | Some("stp")) {
            return Ok(None);
        }
        let geometry_provenance = component.geometry_provenance.clone().ok_or_else(|| {
            AppError::validation(format!(
                "STEP component '{}' lacks geometryProvenance; repackage it before live import.",
                coordinate.canonical_identity()
            ))
        })?;
        let path = store_dir.join(&relative);
        let bytes = std::fs::read(&path).map_err(|err| {
            AppError::not_found(format!(
                "STEP component '{}' payload '{}' was not found in the store: {}",
                coordinate.canonical_identity(),
                relative.display(),
                err
            ))
        })?;
        let payload_digest = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
        let normalized_path = relative.to_string_lossy().replace('\\', "/");
        if !inventory
            .entries
            .iter()
            .any(|entry| entry.path == normalized_path && entry.sha256 == payload_digest)
        {
            return Err(AppError::validation(format!(
                "STEP component '{}' payload '{}' does not match package inventory.",
                coordinate.canonical_identity(),
                relative.display()
            )));
        }
        let resolved = ResolvedStepComponent {
            coordinate: coordinate.clone(),
            path,
            package_digest: package_digest.to_string(),
            payload_digest,
            geometry_provenance,
        };
        crate::component_step_runtime::validate_step_asset(&StepAsset {
            coordinate: resolved.coordinate.clone(),
            alias: coordinate.component_id.clone(),
            path: resolved.path.clone(),
            payload_digest: resolved.payload_digest.clone(),
            geometry_provenance: resolved.geometry_provenance.clone(),
        })?;
        Ok(Some(resolved))
    }
}

impl<'a> InstalledComponentResolver for InstalledLibraryComponentResolver<'a> {
    fn resolve_source_component(
        &self,
        coordinate: &ComponentCoordinate,
    ) -> AppResult<ResolvedSourceComponent> {
        let index_entry = read_coordinate_index(self.app, &coordinate.package_id, &coordinate.version)?
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "Installed package coordinate '{}@{}' is not indexed; install the package before referencing it.",
                    coordinate.package_id, coordinate.version
                ))
            })?;
        self.resolve_at_payload_digest(coordinate, &index_entry.package_digest)
    }

    fn resolve_source_component_with_expected_digest(
        &self,
        coordinate: &ComponentCoordinate,
        expected_package_digest: &str,
    ) -> AppResult<ResolvedSourceComponent> {
        self.resolve_at_payload_digest(coordinate, expected_package_digest)
    }

    fn resolve_step_component(
        &self,
        coordinate: &ComponentCoordinate,
    ) -> AppResult<Option<ResolvedStepComponent>> {
        let index_entry =
            read_coordinate_index(self.app, &coordinate.package_id, &coordinate.version)?
                .ok_or_else(|| {
                    AppError::not_found(format!(
                        "Installed package coordinate '{}@{}' is not indexed; install the package before referencing it.",
                        coordinate.package_id, coordinate.version
                    ))
                })?;
        self.resolve_step_at_payload_digest(coordinate, &index_entry.package_digest)
    }

    fn resolve_step_component_with_expected_digest(
        &self,
        coordinate: &ComponentCoordinate,
        expected_package_digest: &str,
    ) -> AppResult<Option<ResolvedStepComponent>> {
        self.resolve_step_at_payload_digest(coordinate, expected_package_digest)
    }
}

// --- parsing of `(import-component ...)` forms ---

#[derive(Clone, Debug)]
struct ParsedImportDeclaration {
    coordinate: ComponentCoordinate,
    alias: String,
    authored_start: u32,
    authored_end: u32,
}

/// Parse every top-level `(import-component ...)` form. All coordinate fields
/// and the alias must be literal strings; missing or dynamic fields produce a
/// field-specific literal-required diagnostic before ordinary compilation.
fn parse_import_declarations(source: &str) -> AppResult<Vec<ParsedImportDeclaration>> {
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| AppError::parse(format!("Failed to parse authored source: {err}")))?;
    let mut declarations = Vec::new();
    for form in &forms {
        let items = match expr_list_items(form, "top-level form") {
            Ok(items) => items,
            Err(_) => continue,
        };
        if items.first().and_then(expr_identifier).as_deref() != Some("import-component") {
            continue;
        }
        let (start, end) = form_span(form);
        let declaration = parse_one_import(&items, start, end)?;
        declarations.push(declaration);
    }
    Ok(declarations)
}

fn parse_one_import(
    items: &[ExprKind],
    start: u32,
    end: u32,
) -> AppResult<ParsedImportDeclaration> {
    // (import-component "pkg" :version "v" :component "c" :as alias)
    let package_id = string_literal_arg(items.get(1), "packageId")?;
    let mut version = None;
    let mut component_id = None;
    let mut alias = None;
    let mut index = 2usize;
    while index < items.len() {
        let Some(keyword) = import_keyword_name(&items[index]) else {
            return Err(AppError::validation(format!(
                "`import-component` clause must be a keyword argument; found a non-keyword at position {index}. All coordinate fields and the alias must be literal values."
            )));
        };
        let Some(value) = items.get(index + 1) else {
            return Err(AppError::validation(format!(
                "`import-component` keyword `:{keyword}` is missing its literal value."
            )));
        };
        match keyword.as_str() {
            "version" => version = Some(string_literal_value(value, "version")?),
            "component" => component_id = Some(string_literal_value(value, "component")?),
            "as" => alias = Some(identifier_literal_value(value, "alias")?),
            other => {
                return Err(AppError::validation(format!(
                    "`import-component` does not support keyword `:{other}`; supported keywords are :version, :component, :as."
                )));
            }
        }
        index += 2;
    }
    let version = version.ok_or_else(|| {
        AppError::validation("`import-component` requires a literal `:version` string.")
    })?;
    let component_id = component_id.ok_or_else(|| {
        AppError::validation("`import-component` requires a literal `:component` string.")
    })?;
    let alias = alias.ok_or_else(|| {
        AppError::validation("`import-component` requires a literal `:as` alias symbol.")
    })?;
    Ok(ParsedImportDeclaration {
        coordinate: ComponentCoordinate {
            package_id,
            version,
            component_id,
        },
        alias,
        authored_start: start,
        authored_end: end,
    })
}

fn import_keyword_name(expr: &ExprKind) -> Option<String> {
    let ExprKind::Atom(atom) = expr else {
        return None;
    };
    use steel_core::parser::tokens::TokenType;
    match &atom.syn.ty {
        TokenType::Keyword(name) => normalize_keyword(&name.to_string())
            .strip_prefix(':')
            .map(str::to_string),
        TokenType::Identifier(name) => name.to_string().strip_prefix(':').map(str::to_string),
        _ => None,
    }
}

fn string_literal_arg(expr: Option<&ExprKind>, field: &str) -> AppResult<String> {
    expr.map(|e| string_literal_value(e, field))
        .unwrap_or_else(|| {
            Err(AppError::validation(format!(
                "`import-component` requires a literal {field} string."
            )))
        })
}

fn string_literal_value(expr: &ExprKind, field: &str) -> AppResult<String> {
    let ExprKind::Atom(atom) = expr else {
        return Err(literal_required(field));
    };
    use steel_core::parser::tokens::TokenType;
    if let TokenType::StringLiteral(value) = &atom.syn.ty {
        return Ok(value.resolve().to_string());
    }
    Err(literal_required(field))
}

fn identifier_literal_value(expr: &ExprKind, field: &str) -> AppResult<String> {
    let ExprKind::Atom(atom) = expr else {
        return Err(literal_required(field));
    };
    use steel_core::parser::tokens::TokenType;
    if let TokenType::Identifier(name) = &atom.syn.ty {
        let text = name.to_string();
        if is_valid_ecky_symbol(&text) {
            return Ok(text);
        }
    }
    Err(literal_required(field))
}

fn literal_required(field: &str) -> AppError {
    AppError::validation(format!(
        "`import-component` {field} must be a literal value; dynamic coordinates are not supported."
    ))
}

fn normalize_keyword(name: &str) -> String {
    if let Some(stripped) = name.strip_prefix("#:") {
        format!(":{stripped}")
    } else {
        name.to_string()
    }
}

fn form_span(form: &ExprKind) -> (u32, u32) {
    match form {
        ExprKind::List(list) => (list.location.start, list.location.end),
        _ => (0, 0),
    }
}

fn validate_alias(alias: &str) -> AppResult<()> {
    if !is_valid_ecky_symbol(alias) {
        return Err(AppError::validation(format!(
            "Import alias '{}' must be a valid Ecky symbol (letters, digits, `_` or `-`, starting with a letter).",
            alias
        )));
    }
    Ok(())
}

/// Top-level local define / define-component names in authored source (for
/// alias collision detection).
fn collect_local_binding_names(source: &str) -> BTreeSet<String> {
    let Ok(forms) = Parser::parse_without_lowering(source) else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    for form in &forms {
        let Ok(items) = expr_list_items(form, "top-level form") else {
            continue;
        };
        let Some(head) = items.first().and_then(expr_identifier) else {
            continue;
        };
        if head == "define-component" {
            if let Some(name) = items.get(1).and_then(expr_identifier) {
                names.insert(name);
            }
        } else if head == "define" {
            if let Some(name_expr) = items.get(1) {
                if let Some(name) = define_name(name_expr) {
                    names.insert(name);
                }
            }
        }
    }
    names
}

fn define_name(name_expr: &ExprKind) -> Option<String> {
    if let Some(identifier) = expr_identifier(name_expr) {
        return Some(identifier);
    }
    if let Ok(items) = expr_list_items(name_expr, "define signature") {
        return items.first().and_then(expr_identifier);
    }
    None
}

// --- materialization (AST namespace transform, no regex) ---

struct MaterializedCompilerSource {
    compiler_source: String,
    resolved_ranges: BTreeMap<String, (u32, u32)>,
}

fn materialize_compiler_source(
    authored_source: &str,
    resolved: &[(ParsedImportDeclaration, ResolvedSourceComponent)],
) -> AppResult<MaterializedCompilerSource> {
    let forms = Parser::parse_without_lowering(authored_source)
        .map_err(|err| AppError::parse(format!("Failed to parse authored source: {err}")))?;

    let mut out = String::new();
    let mut resolved_ranges = BTreeMap::new();
    // Emit namespaced package exports first, each bound to its alias.
    for (declaration, component) in resolved {
        let resolved_start = out.len() as u32;
        let namespace = namespace_token(&component.payload_digest);
        let package_forms = Parser::parse_without_lowering(&component.source).map_err(|err| {
            AppError::parse(format!(
                "Failed to parse package source for '{}': {err}",
                component.coordinate.canonical_identity()
            ))
        })?;
        let rename = build_rename_map(
            &package_forms,
            &component.entry_symbol,
            &declaration.alias,
            &namespace,
        )?;
        for form in &package_forms {
            let Ok(items) = expr_list_items(form, "package form") else {
                continue;
            };
            let head = items.first().and_then(expr_head_name);
            // Only library definitions participate in the materialized source;
            // stray top-level models in a package payload are dropped.
            if matches!(head.as_deref(), Some("define-component") | Some("define")) {
                out.push_str(&rewrite_namespaced(form, &rename)?);
                out.push('\n');
            }
        }
        resolved_ranges.insert(
            declaration.alias.clone(),
            (resolved_start, out.len() as u32),
        );
    }
    // Emit the user's non-import top-level forms verbatim.
    for form in &forms {
        let items = match expr_list_items(form, "top-level form") {
            Ok(items) => items,
            Err(_) => continue,
        };
        if items.first().and_then(expr_identifier).as_deref() == Some("import-component") {
            continue;
        }
        out.push_str(&form.to_string());
        out.push('\n');
    }
    Ok(MaterializedCompilerSource {
        compiler_source: out,
        resolved_ranges,
    })
}

fn namespace_token(payload_digest: &str) -> String {
    let hex = payload_digest
        .strip_prefix("sha256:")
        .unwrap_or(payload_digest);
    format!("pkg{}", &hex[..hex.len().min(12)])
}

fn build_rename_map(
    package_forms: &[ExprKind],
    export_symbol: &str,
    alias: &str,
    namespace: &str,
) -> AppResult<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    let mut export_seen = false;
    for form in package_forms {
        let Ok(items) = expr_list_items(form, "package form") else {
            continue;
        };
        let head = items.first().and_then(expr_head_name);
        let name = match head.as_deref() {
            Some("define-component") => items.get(1).and_then(expr_identifier),
            Some("define") => items.get(1).and_then(define_name),
            _ => None,
        };
        let Some(name) = name else {
            continue;
        };
        if name == export_symbol {
            map.insert(name, alias.to_string());
            export_seen = true;
        } else {
            let namespaced = format!("__{namespace}__{name}");
            map.insert(name, namespaced);
        }
    }
    if !export_seen {
        return Err(AppError::validation(format!(
            "Package source does not export a `define-component` named '{export_symbol}'."
        )));
    }
    Ok(map)
}

/// Rewrite a package form, renaming top-level binding names and their internal
/// references via `rename`. Mirrors the compiler's own AST walk so it never
/// touches quoted data or performs textual replacement.
fn rewrite_namespaced(form: &ExprKind, rename: &BTreeMap<String, String>) -> AppResult<String> {
    match form {
        ExprKind::Atom(_) => Ok(rename_atom(form, rename)),
        ExprKind::Quote(_) => Ok(form.to_string()),
        ExprKind::Define(def) => {
            let name = rewrite_define_name(&def.name, rename)?;
            let body = rewrite_namespaced(&def.body, rename)?;
            Ok(format!("(define {name} {body})"))
        }
        ExprKind::Begin(begin) => {
            let rendered = begin
                .exprs
                .iter()
                .map(|item| rewrite_namespaced(item, rename))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("(begin {})", rendered.join(" ")))
        }
        ExprKind::If(if_expr) => Ok(format!(
            "(if {} {} {})",
            rewrite_namespaced(&if_expr.test_expr, rename)?,
            rewrite_namespaced(&if_expr.then_expr, rename)?,
            rewrite_namespaced(&if_expr.else_expr, rename)?
        )),
        ExprKind::Let(let_expr) => {
            let bindings = let_expr
                .bindings
                .iter()
                .map(|(name, value)| {
                    Ok(format!(
                        "({} {})",
                        rename_atom(name, rename),
                        rewrite_namespaced(value, rename)?
                    ))
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!(
                "(let ({}) {})",
                bindings.join(" "),
                rewrite_namespaced(&let_expr.body_expr, rename)?
            ))
        }
        ExprKind::LambdaFunction(lambda) => {
            let args = lambda
                .args
                .iter()
                .map(|arg| rename_atom(arg, rename))
                .collect::<Vec<_>>()
                .join(" ");
            Ok(format!(
                "(lambda ({}) {})",
                args,
                rewrite_namespaced(&lambda.body, rename)?
            ))
        }
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let Ok(items) = expr_list_items(form, "namespaced form") else {
                // A malformed list is emitted verbatim rather than failing the
                // whole materialization; well-formed package source never hits
                // this arm.
                return Ok(form.to_string());
            };
            if let Some(head) = items.first().and_then(expr_identifier) {
                if head == "quote" {
                    return Ok(form.to_string());
                }
            }
            let rendered = items
                .iter()
                .map(|item| rewrite_namespaced(item, rename))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", rendered.join(" ")))
        }
        other => Ok(other.to_string()),
    }
}

fn rewrite_define_name(
    name_expr: &ExprKind,
    rename: &BTreeMap<String, String>,
) -> AppResult<String> {
    if let Ok(items) = expr_list_items(name_expr, "define signature") {
        // Function definition: `(define (f args) body)`.
        let rendered: Vec<String> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                if index == 0 {
                    Ok(rename_atom(item, rename))
                } else {
                    rewrite_namespaced(item, rename)
                }
            })
            .collect::<AppResult<Vec<_>>>()?;
        return Ok(format!("({})", rendered.join(" ")));
    }
    Ok(rename_atom(name_expr, rename))
}

fn rename_atom(expr: &ExprKind, rename: &BTreeMap<String, String>) -> String {
    if let Some(name) = expr_identifier(expr) {
        if let Some(renamed) = rename.get(&name) {
            return renamed.clone();
        }
    }
    expr.to_string()
}

// --- lock + span evidence ---

fn build_lock_and_spans(
    resolved_sources: &[(ParsedImportDeclaration, ResolvedSourceComponent)],
    resolved_steps: &[(ParsedImportDeclaration, ResolvedStepComponent)],
) -> (ComponentDependencyLock, Vec<ComponentImportSpan>) {
    // Group components by package coordinate.
    let mut by_package: BTreeMap<(String, String), ComponentDependencyLockEntry> = BTreeMap::new();
    let mut spans = Vec::with_capacity(resolved_sources.len() + resolved_steps.len());
    for (declaration, component) in resolved_sources {
        let key = (
            component.coordinate.package_id.clone(),
            component.coordinate.version.clone(),
        );
        let entry = by_package
            .entry(key.clone())
            .or_insert_with(|| ComponentDependencyLockEntry {
                package_id: component.coordinate.package_id.clone(),
                version: component.coordinate.version.clone(),
                package_digest: component.payload_digest.clone(),
                components: Vec::new(),
            });
        let lock_component = ComponentDependencyLockComponent {
            component_id: component.coordinate.component_id.clone(),
            entry_symbol: Some(component.entry_symbol.clone()),
            payload_digest: component.payload_digest.clone(),
            payload_kind: Some(crate::contracts::ComponentPayloadKind::Source),
            geometry_representation: None,
        };
        if !entry.components.contains(&lock_component) {
            entry.components.push(lock_component);
        }
        spans.push(ComponentImportSpan {
            coordinate: component.coordinate.clone(),
            alias: declaration.alias.clone(),
            entry_symbol: component.entry_symbol.clone(),
            payload_digest: component.payload_digest.clone(),
            authored_start: declaration.authored_start,
            authored_end: declaration.authored_end,
            resolved_start: 0,
            resolved_end: 0,
        });
    }
    for (declaration, component) in resolved_steps {
        let key = (
            component.coordinate.package_id.clone(),
            component.coordinate.version.clone(),
        );
        let entry = by_package
            .entry(key)
            .or_insert_with(|| ComponentDependencyLockEntry {
                package_id: component.coordinate.package_id.clone(),
                version: component.coordinate.version.clone(),
                package_digest: component.package_digest.clone(),
                components: Vec::new(),
            });
        if entry.package_digest != component.package_digest {
            continue;
        }
        let lock_component = ComponentDependencyLockComponent {
            component_id: component.coordinate.component_id.clone(),
            entry_symbol: None,
            payload_digest: component.payload_digest.clone(),
            payload_kind: Some(ComponentPayloadKind::Step),
            geometry_representation: Some(component.geometry_provenance.representation.clone()),
        };
        if !entry.components.contains(&lock_component) {
            entry.components.push(lock_component);
        }
        spans.push(ComponentImportSpan {
            coordinate: component.coordinate.clone(),
            alias: declaration.alias.clone(),
            entry_symbol: declaration.alias.clone(),
            payload_digest: component.payload_digest.clone(),
            authored_start: declaration.authored_start,
            authored_end: declaration.authored_end,
            resolved_start: 0,
            resolved_end: 0,
        });
    }
    let lock = ComponentDependencyLock {
        schema_version: crate::contracts::COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
        dependencies: by_package.into_values().collect(),
    }
    .canonical();
    (lock, spans)
}

fn expected_lock_entry<'a>(
    expected: &'a ComponentDependencyLock,
    coordinate: &ComponentCoordinate,
) -> AppResult<&'a ComponentDependencyLockEntry> {
    expected
        .dependencies
        .iter()
        .find(|entry| {
            entry.package_id == coordinate.package_id && entry.version == coordinate.version
        })
        .ok_or_else(|| {
            AppError::validation(format!(
                "Expected dependency lock does not contain exact package coordinate for '{}'.",
                coordinate.canonical_identity()
            ))
        })
}

fn expected_lock_component<'a>(
    entry: &'a ComponentDependencyLockEntry,
    coordinate: &ComponentCoordinate,
) -> AppResult<&'a ComponentDependencyLockComponent> {
    entry
        .components
        .iter()
        .find(|component| component.component_id == coordinate.component_id)
        .ok_or_else(|| {
            AppError::validation(format!(
                "Resolved dependency set does not exactly match the expected dependency lock; it does not contain exact component '{}'.",
                coordinate.canonical_identity()
            ))
        })
}

/// A committed lock is an exact authorization record, not a digest hint. Its
/// package coordinates, payloads, component exports, and schema version must
/// equal the newly resolved candidate after canonical ordering. This blocks
/// missing/extra components as well as package-digest redirects.
fn verify_against_expected_lock(
    expected: &ComponentDependencyLock,
    candidate: &ComponentDependencyLock,
) -> AppResult<()> {
    let expected = expected.clone().canonical();
    let candidate = candidate.clone().canonical();
    if expected != candidate {
        return Err(AppError::validation(
            "Resolved dependencies do not exactly match the expected dependency lock; preview, render, and commit cannot rewrite a committed lock.".to_string(),
        ));
    }
    Ok(())
}

/// Combined AST-based inspection of a package source: parses once and reports
/// either the absence of the requested export symbol or a transitive
/// `import-component` form. Neither check performs textual matching. Returns
/// `Ok(())` when the source exports the symbol and has no transitive imports.
fn inspect_package_source(source: &str, symbol: &str) -> AppResult<()> {
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| AppError::parse(format!("Failed to parse package source: {err}")))?;
    let mut exported = false;
    for form in &forms {
        let Ok(items) = expr_list_items(form, "package form") else {
            continue;
        };
        let Some(head) = items.first().and_then(expr_identifier) else {
            continue;
        };
        match head.as_str() {
            "define-component"
                if items.get(1).and_then(expr_identifier).as_deref() == Some(symbol) =>
            {
                exported = true;
            }
            "import-component" => {
                return Err(AppError::validation(
                    "source itself contains an `import-component` form; transitive live dependencies are not supported".to_string(),
                ));
            }
            _ => {}
        }
    }
    if !exported {
        return Err(AppError::validation(format!(
            "source does not export a `define-component` named '{symbol}'; repackage it with an export, or import an independently renderable model directly"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    struct FsResolver {
        root: PathBuf,
    }

    impl PathResolver for FsResolver {
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

    /// In-memory resolver keyed by canonical identity.
    struct MemoryResolver {
        components: HashMap<String, ResolvedSourceComponent>,
    }

    impl MemoryResolver {
        fn new() -> Self {
            Self {
                components: HashMap::new(),
            }
        }

        fn with(
            mut self,
            coordinate: ComponentCoordinate,
            entry_symbol: &str,
            source: &str,
            payload_digest: &str,
        ) -> Self {
            self.components.insert(
                coordinate.canonical_identity(),
                ResolvedSourceComponent {
                    coordinate,
                    entry_symbol: entry_symbol.to_string(),
                    source: source.to_string(),
                    payload_digest: payload_digest.to_string(),
                },
            );
            self
        }
    }

    impl InstalledComponentResolver for MemoryResolver {
        fn resolve_source_component(
            &self,
            coordinate: &ComponentCoordinate,
        ) -> AppResult<ResolvedSourceComponent> {
            self.components
                .get(&coordinate.canonical_identity())
                .cloned()
                .ok_or_else(|| {
                    AppError::not_found(format!(
                        "Installed package coordinate '{}' is not available.",
                        coordinate.canonical_identity()
                    ))
                })
        }
    }

    fn cage_coordinate() -> ComponentCoordinate {
        ComponentCoordinate {
            package_id: "bike.kit".to_string(),
            version: "1.2.0".to_string(),
            component_id: "cage".to_string(),
        }
    }

    fn cage_source() -> &'static str {
        "(define-component cage ((number diameter 74)) (cylinder diameter 40))"
    }

    fn resolve(
        source: &str,
        resolver: &dyn InstalledComponentResolver,
    ) -> AppResult<ResolvedAuthoringSource> {
        resolve_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: source,
                expected_lock: None,
            },
            resolver,
        )
    }

    #[test]
    fn live_reference_materializes_export_under_alias_and_compiles() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder :diameter 74)))
        "#;
        let resolved = resolve(source, &resolver).expect("resolve");
        assert!(
            !resolved.compiler_source.contains("import-component"),
            "compiler source must not carry live import forms: {}",
            resolved.compiler_source
        );
        assert!(
            resolved
                .compiler_source
                .contains("(define-component holder"),
            "export must be materialized under the alias: {}",
            resolved.compiler_source
        );
        // The materialized source must compile through the unchanged compiler.
        crate::ecky_scheme::compile_to_core_program(&resolved.compiler_source)
            .expect("materialized source compiles");
        assert_eq!(resolved.dependency_lock.dependencies.len(), 1);
        let dep = &resolved.dependency_lock.dependencies[0];
        assert_eq!(dep.package_id, "bike.kit");
        assert_eq!(dep.version, "1.2.0");
        assert_eq!(dep.package_digest, "sha256:aaaa");
        assert_eq!(dep.components.len(), 1);
        assert_eq!(dep.components[0].component_id, "cage");
        assert_eq!(dep.components[0].payload_digest, "sha256:aaaa");
        assert_eq!(resolved.import_spans.len(), 1);
        assert_eq!(resolved.import_spans[0].alias, "holder");
    }

    #[test]
    fn missing_exact_version_fails_naming_canonical_identity() {
        let resolver = MemoryResolver::new(); // nothing installed
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder :diameter 74)))
        "#;
        let err = resolve(source, &resolver).expect_err("missing version fails");
        assert!(
            err.message.contains("bike.kit@1.2.0:cage"),
            "error must name the exact canonical identity: {}",
            err.message
        );
    }

    #[test]
    fn different_installed_version_is_not_selected() {
        // Only 1.3.0 is installed; requesting 1.2.0 must fail without fallback.
        let installed_130 = ComponentCoordinate {
            package_id: "bike.kit".to_string(),
            version: "1.3.0".to_string(),
            component_id: "cage".to_string(),
        };
        let resolver =
            MemoryResolver::new().with(installed_130, "cage", cage_source(), "sha256:bbbb");
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder)))
        "#;
        let err = resolve(source, &resolver).expect_err("version mismatch fails");
        assert!(
            err.message.contains("bike.kit@1.2.0:cage"),
            "{}",
            err.message
        );
    }

    #[test]
    fn explicit_entry_symbol_resolves_export() {
        let coordinate = cage_coordinate();
        let source = "(define-component cage-v2 ((number diameter 74)) (cylinder diameter 40))";
        let resolver = MemoryResolver::new().with(coordinate, "cage-v2", source, "sha256:cccc");
        let authored = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder :diameter 74)))
        "#;
        let resolved = resolve(authored, &resolver).expect("resolve entry symbol");
        assert!(
            resolved
                .compiler_source
                .contains("(define-component holder"),
            "{}",
            resolved.compiler_source
        );
        crate::ecky_scheme::compile_to_core_program(&resolved.compiler_source).expect("compiles");
    }

    #[test]
    fn duplicate_alias_names_both_coordinates() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (import-component "other.kit" :version "2.0.0" :component "ring" :as holder)
            (model (part body (holder)))
        "#;
        let err = resolve(source, &resolver).expect_err("duplicate alias fails");
        assert!(err.message.contains("'holder'"), "{}", err.message);
        assert!(err.message.contains("bike.kit"), "{}", err.message);
    }

    #[test]
    fn alias_collision_with_local_define_fails() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (define-component holder ((number d 1)) (box d d d))
            (model (part body (holder)))
        "#;
        let err = resolve(source, &resolver).expect_err("local collision fails");
        assert!(
            err.message.contains("collides with a local"),
            "{}",
            err.message
        );
    }

    #[test]
    fn incomplete_coordinate_field_fails_before_compile() {
        let resolver = MemoryResolver::new();
        let source = r#"
            (import-component "bike.kit" :component "cage" :as holder)
            (model (part body (holder)))
        "#;
        let err = resolve(source, &resolver).expect_err("missing version fails");
        assert!(err.message.contains(":version"), "{}", err.message);
    }

    #[test]
    fn raw_compiler_rejects_unresolved_import_without_io() {
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder)))
        "#;
        let err = crate::ecky_scheme::compile_to_core_program(source)
            .expect_err("raw compiler must reject import-component");
        assert!(
            err.to_string().contains("host pre-resolution"),
            "raw compiler must name host pre-resolution: {}",
            err
        );
    }

    #[test]
    fn copy_inline_component_import_is_not_a_live_reference() {
        // The MCP/UI copy-inline workflow inserts full define-component source
        // and must not produce an import-component declaration or a lock.
        let resolver = MemoryResolver::new();
        let vendored = r#"
            (define-component holder ((number diameter 74)) (cylinder diameter 40))
            (model (part body (holder :diameter 74)))
        "#;
        let resolved = resolve(vendored, &resolver).expect("no live imports");
        assert!(resolved.dependency_lock.dependencies.is_empty());
        assert!(resolved.import_spans.is_empty());
        assert!(
            !resolved.compiler_source.contains("import-component"),
            "{}",
            resolved.compiler_source
        );
    }

    #[test]
    fn copy_inline_import_inserts_self_contained_definition_and_instance() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        let imported = copy_inline_component_import(
            CopyInlineComponentImportRequest {
                package_id: "bike.kit".to_string(),
                version: "1.2.0".to_string(),
                component_id: "cage".to_string(),
                authored_source: "(model (part base (box 1 1 1)))".to_string(),
            },
            &resolver,
        )
        .expect("copy inline");

        assert_eq!(imported.part_key, "cage");
        assert!(imported.authored_source.contains("(define-component cage"));
        assert!(imported.authored_source.contains("(part cage (cage))"));
        assert!(!imported.authored_source.contains("import-component"));
        let resolved = resolve(&imported.authored_source, &resolver).expect("no live lock");
        assert!(resolved.dependency_lock.dependencies.is_empty());
        compile_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: &imported.authored_source,
                expected_lock: None,
            },
            &resolver,
        )
        .expect("vendored source compiles");
    }

    #[test]
    fn compile_authoring_source_returns_program_lock_and_origins() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder :diameter 74)))
        "#;
        let compiled = compile_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: source,
                expected_lock: None,
            },
            &resolver,
        )
        .expect("compile");
        assert_eq!(compiled.program.parts.len(), 1);
        assert_eq!(compiled.origins.len(), 1);
        assert_eq!(compiled.origins[0].alias, "holder");
        assert_eq!(compiled.origins[0].payload_digest, "sha256:aaaa");
        assert_eq!(
            compiled.dependency_lock.dependencies[0].package_digest,
            "sha256:aaaa"
        );
    }

    #[test]
    fn expected_lock_mismatch_blocks_resolution() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        let expected = ComponentDependencyLock {
            schema_version: crate::contracts::COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
            dependencies: vec![ComponentDependencyLockEntry {
                package_id: "bike.kit".to_string(),
                version: "1.2.0".to_string(),
                package_digest: "sha256:zzzz".to_string(),
                components: Vec::new(),
            }],
        };
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder :diameter 74)))
        "#;
        let err = resolve_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: source,
                expected_lock: Some(&expected),
            },
            &resolver,
        )
        .expect_err("lock mismatch must block");
        assert!(
            err.message.contains("does not match locked digest"),
            "{}",
            err.message
        );
    }

    #[test]
    fn expected_lock_rejects_component_not_recorded_by_the_lock() {
        let resolver =
            MemoryResolver::new().with(cage_coordinate(), "cage", cage_source(), "sha256:aaaa");
        // The package digest is correct, but an empty component list cannot
        // authorize the live `cage` import.
        let expected = ComponentDependencyLock {
            schema_version: crate::contracts::COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
            dependencies: vec![ComponentDependencyLockEntry {
                package_id: "bike.kit".to_string(),
                version: "1.2.0".to_string(),
                package_digest: "sha256:aaaa".to_string(),
                components: Vec::new(),
            }],
        };
        let source = r#"
            (import-component "bike.kit" :version "1.2.0" :component "cage" :as holder)
            (model (part body (holder :diameter 74)))
        "#;
        let err = resolve_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: source,
                expected_lock: Some(&expected),
            },
            &resolver,
        )
        .expect_err("component omitted from expected lock must fail");
        assert!(
            err.message
                .contains("does not exactly match the expected dependency lock"),
            "{}",
            err.message
        );
    }

    #[test]
    fn namespace_isolation_lets_two_packages_share_an_export_name() {
        // Two different packages both export a component literally named
        // `body`; both must import without their private helpers colliding.
        let coord_a = ComponentCoordinate {
            package_id: "pkg.a".to_string(),
            version: "1.0.0".to_string(),
            component_id: "body".to_string(),
        };
        let coord_b = ComponentCoordinate {
            package_id: "pkg.b".to_string(),
            version: "1.0.0".to_string(),
            component_id: "body".to_string(),
        };
        let source_template = "(define-component body ((number w 4)) (box w w w))";
        let resolver = MemoryResolver::new()
            .with(coord_a, "body", source_template, "sha256:aaaa")
            .with(coord_b, "body", source_template, "sha256:bbbb");
        let authored = r#"
            (import-component "pkg.a" :version "1.0.0" :component "body" :as partA)
            (import-component "pkg.b" :version "1.0.0" :component "body" :as partB)
            (model
              (part a (partA :w 6))
              (part b (partB :w 8)))
        "#;
        let resolved = resolve(authored, &resolver).expect("resolve both");
        crate::ecky_scheme::compile_to_core_program(&resolved.compiler_source)
            .expect("both exports compile under distinct aliases");
        assert_eq!(resolved.dependency_lock.dependencies.len(), 2);
    }

    #[test]
    fn committed_lock_resolves_directly_from_cas_after_discovery_uninstall() {
        let root = std::env::temp_dir().join(format!(
            "ecky-component-locked-cas-{}",
            uuid::Uuid::new_v4()
        ));
        let project = root.join("package");
        fs::create_dir_all(project.join("components")).expect("package dir");
        fs::write(
            project.join(crate::component_package_runtime::COMPONENT_PACKAGE_FILE_NAME),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "packageId": "fixture.locked",
                "version": "1.0.0",
                "displayName": "Locked fixture",
                "visibility": "source",
                "components": [{
                    "componentId": "cage",
                    "version": "1.0.0",
                    "displayName": "Cage",
                    "sourceRef": "components/cage.ecky",
                    "entrySymbol": "cage"
                }]
            }))
            .expect("manifest json"),
        )
        .expect("manifest");
        fs::write(
            project.join("components/cage.ecky"),
            "(define-component cage () (box 1 2 3))",
        )
        .expect("source");
        let archive = root.join("fixture.eckypkg");
        crate::component_package_runtime::write_component_package_archive(&project, &archive)
            .expect("archive");
        let resolver = FsResolver { root: root.clone() };
        crate::component_package_runtime::install_component_package_to_store(&resolver, &archive)
            .expect("install");
        let authored = r#"
          (import-component "fixture.locked" :version "1.0.0" :component "cage" :as holder)
          (model (part body (holder)))
        "#;
        let first = compile_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: authored,
                expected_lock: None,
            },
            &InstalledLibraryComponentResolver { app: &resolver },
        )
        .expect("initial resolution");

        crate::component_package_runtime::remove_coordinate_index(
            &resolver,
            "fixture.locked",
            "1.0.0",
        )
        .expect("uninstall");
        let missing = compile_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: authored,
                expected_lock: None,
            },
            &InstalledLibraryComponentResolver { app: &resolver },
        )
        .expect_err("unlocked discovery must stop after uninstall");
        assert!(missing.message.contains("not indexed"), "{missing}");

        let historical = compile_authoring_source(
            ResolveAuthoringSourceRequest {
                authored_source: authored,
                expected_lock: Some(&first.dependency_lock),
            },
            &InstalledLibraryComponentResolver { app: &resolver },
        )
        .expect("committed lock resolves immutable CAS directly");
        assert_eq!(historical.dependency_lock, first.dependency_lock);
        assert_eq!(historical.compiler_source, first.compiler_source);
        assert_eq!(historical.program.parts[0].key, first.program.parts[0].key);
        assert_eq!(
            historical.origins_by_node.keys().collect::<Vec<_>>(),
            first.origins_by_node.keys().collect::<Vec<_>>()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
