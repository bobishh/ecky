//! Component extraction: lifts an existing `(part ...)` subtree out of a model
//! into a closed, copy-inline `define-component` snippet plus a compact header.
//!
//! Free-variable analysis reuses the compiler's binding resolution
//! (`ecky_scheme::compiler::collect_free_variables`): referenced model params
//! become signature entries with their metadata preserved; scalar outer
//! `let`/`let*` bindings become plain defaults; any other free reference is a
//! deterministic extraction blocker.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use steel_core::parser::ast::ExprKind;
use steel_core::parser::parser::Parser;
use steel_core::parser::tokens::TokenType;

use crate::contracts::{AppError, AppResult};
use crate::ecky_scheme::compiler::{
    collect_free_variables, expr_head_name, expr_identifier, expr_list_items,
};

/// Captures only authored top-level `define-component` forms. The stored source
/// includes each component's transitive local definitions in deterministic
/// order so a library entry remains vendorable after the model changes.
#[derive(Clone)]
struct CapturedDefinition {
    form: String,
    free: BTreeSet<String>,
    params: Vec<ComponentHeaderParam>,
    ports: Vec<ComponentHeaderPort>,
    is_component: bool,
}

pub fn extract_defined_components(
    source: &str,
    thread_id: &str,
    message_id: &str,
) -> AppResult<Vec<ExtractedComponent>> {
    let Ok(forms) = Parser::parse_without_lowering(source) else {
        return Ok(Vec::new());
    };
    let top_level_names = forms
        .iter()
        .filter_map(|form| {
            let items = expr_list_items(form, "top-level form").ok()?;
            let head = items.first().and_then(expr_head_name)?;
            if !matches!(head.as_str(), "define-component" | "define") {
                return None;
            }
            let target = items.get(1)?;
            if head == "define-component" {
                expr_identifier(target)
            } else if let Ok(signature) = expr_list_items(target, "define signature") {
                signature.first().and_then(expr_identifier)
            } else {
                expr_identifier(target)
            }
        })
        .collect::<BTreeSet<_>>();
    let mut definitions = BTreeMap::<String, CapturedDefinition>::new();
    for form in &forms {
        let Ok(items) = expr_list_items(form, "top-level form") else {
            continue;
        };
        let Some(head) = items.first().and_then(expr_head_name) else {
            continue;
        };
        if !matches!(head.as_str(), "define-component" | "define") {
            continue;
        }
        let Some(name_expr) = items.get(1) else {
            continue;
        };
        let (Some(name), is_component) = (if head == "define-component" {
            (expr_identifier(name_expr), true)
        } else if let Some(signature) = expr_list_items(name_expr, "define signature").ok() {
            (signature.first().and_then(expr_identifier), false)
        } else {
            (expr_identifier(name_expr), false)
        }) else {
            continue;
        };
        let mut params = Vec::new();
        let mut bound = BTreeSet::new();
        let mut free = BTreeSet::new();
        let body_start = if is_component {
            3
        } else if matches!(name_expr, ExprKind::List(_)) {
            2
        } else {
            2
        };
        if is_component {
            if let Some(signature) = items.get(2) {
                if let Ok(entries) = expr_list_items(signature, "component signature") {
                    for entry in entries {
                        let Ok(fields) = expr_list_items(&entry, "component parameter") else {
                            continue;
                        };
                        if fields.len() < 2 {
                            continue;
                        }
                        if let Some(key) = fields.get(1).and_then(expr_identifier) {
                            bound.insert(key);
                        }
                        if let Some(default) = fields.get(2) {
                            free.extend(collect_free_variables(default, &BTreeSet::new()));
                        }
                        params.push(header_param_from_entry_source(&entry.to_string())?);
                    }
                }
            }
        } else if let Ok(signature) = expr_list_items(name_expr, "define signature") {
            bound.extend(signature.iter().skip(1).filter_map(expr_identifier));
        }
        if is_component {
            for body in items.iter().skip(body_start) {
                let Ok(clause) = expr_list_items(body, "component clause") else {
                    continue;
                };
                match clause.first().and_then(expr_head_name).as_deref() {
                    Some("ports") => {
                        for port in clause.iter().skip(1) {
                            if let Ok(fields) = expr_list_items(port, "component port") {
                                if fields.first().and_then(expr_head_name).as_deref()
                                    == Some("port")
                                {
                                    if let Some(name) = fields.get(1).and_then(expr_identifier) {
                                        bound.insert(name);
                                    }
                                }
                            }
                        }
                    }
                    Some("verify") => {
                        for declaration in clause.iter().skip(1) {
                            if let Ok(fields) = expr_list_items(declaration, "verification clause")
                            {
                                if fields.first().and_then(expr_head_name).as_deref()
                                    == Some("metric")
                                {
                                    if let Some(name) = fields.get(1).and_then(expr_identifier) {
                                        bound.insert(name);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for body in items.iter().skip(body_start) {
            let clause_head = expr_list_items(body, "component clause")
                .ok()
                .and_then(|clause| clause.first().and_then(expr_head_name));
            if is_component && matches!(clause_head.as_deref(), Some("ports" | "verify")) {
                // These clauses declare local port/metric names and contain a
                // domain-specific vocabulary that the generic Scheme walker
                // cannot distinguish from source references. Keep only names
                // that resolve to authored top-level definitions.
                free.extend(
                    collect_free_variables(body, &bound)
                        .into_iter()
                        .filter(|symbol| top_level_names.contains(symbol)),
                );
            } else {
                free.extend(collect_free_variables(body, &bound));
            }
        }
        free.retain(|symbol| !bound.contains(symbol));
        let port_clauses = if is_component {
            items
                .iter()
                .skip(body_start)
                .filter(|item| {
                    expr_list_items(item, "component clause")
                        .ok()
                        .and_then(|clause| clause.first().and_then(expr_head_name))
                        .as_deref()
                        == Some("ports")
                })
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        definitions.insert(
            name,
            CapturedDefinition {
                form: form.to_string(),
                free,
                params,
                ports: component_header_ports(&port_clauses)?,
                is_component,
            },
        );
    }

    let mut output = Vec::new();
    for (name, definition) in &definitions {
        if !definition.is_component {
            continue;
        }
        let mut ordered = Vec::new();
        visit_dependencies(
            name,
            &definitions,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            &mut ordered,
        )?;
        let dependencies = ordered
            .iter()
            .filter(|dependency| *dependency != name)
            .cloned()
            .collect::<Vec<_>>();
        let component_source = ordered
            .iter()
            .filter_map(|dependency| definitions.get(dependency).map(|item| item.form.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        let revision_digest = format!("sha256:{:x}", Sha256::digest(component_source.as_bytes()));
        let component_id = format!(
            "local-{:x}",
            Sha256::digest(format!("{thread_id}\0{name}").as_bytes())
        );
        let params = definition.params.clone();
        let header = ComponentHeader {
            schema_version: 1,
            name: name.clone(),
            component_id: Some(component_id),
            revision_digest: Some(revision_digest.clone()),
            dependencies,
            description: None,
            params,
            tags: Vec::new(),
            provenance: ComponentProvenance {
                project_id: None,
                thread_id: Some(thread_id.to_string()),
                message_id: Some(message_id.to_string()),
                source_digest: revision_digest,
            },
            interfaces: Vec::new(),
            ports: definition.ports.clone(),
        };
        output.push(ExtractedComponent {
            name: name.clone(),
            component_source,
            header,
        });
    }
    Ok(output)
}

fn visit_dependencies(
    name: &str,
    definitions: &BTreeMap<String, CapturedDefinition>,
    visited: &mut BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
    ordered: &mut Vec<String>,
) -> AppResult<()> {
    if visited.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_string()) {
        return Err(AppError::validation(format!(
            "Automatic component capture found a recursive source dependency at `{name}`."
        )));
    }
    let definition = definitions.get(name).ok_or_else(|| {
        AppError::validation(format!(
            "Automatic component capture could not resolve source dependency `{name}`."
        ))
    })?;
    let unresolved = definition
        .free
        .iter()
        .filter(|symbol| !definitions.contains_key(*symbol))
        .cloned()
        .collect::<Vec<_>>();
    if !unresolved.is_empty() {
        return Err(AppError::validation(format!(
            "Automatic capture of component `{name}` has unresolved source dependencies: {}.",
            unresolved.join(", ")
        )));
    }
    for dependency in &definition.free {
        if definitions.contains_key(dependency) {
            visit_dependencies(dependency, definitions, visited, visiting, ordered)?;
        }
    }
    visiting.remove(name);
    visited.insert(name.to_string());
    ordered.push(name.to_string());
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct ComponentExtractRequest {
    pub source: String,
    pub part_key: String,
    /// Component name; defaults to the part key.
    pub component_name: Option<String>,
    /// One-line human description, surfaced by library search.
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHeaderParam {
    pub key: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHeaderPort {
    pub port_id: String,
    pub type_id: String,
    /// Complete authored `(port ...)` clause. Keeps frame and named fit
    /// metadata without freezing it to evaluated world coordinates.
    pub port_source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub source_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHeader {
    #[serde(default)]
    pub schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_digest: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Vec<ComponentHeaderParam>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub provenance: ComponentProvenance,
    /// Signature keys that participate in a `:relations` constraint of the
    /// source model — the fit-critical knobs of this component.
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub ports: Vec<ComponentHeaderPort>,
}

#[derive(Clone, Debug)]
pub struct ExtractedComponent {
    pub name: String,
    /// Self-contained `define-component` source, pasteable into any model.
    pub component_source: String,
    pub header: ComponentHeader,
}

struct ModelScan {
    /// param key -> full source text of its `(kind key default ...)` entry,
    /// in declaration order.
    param_entries: Vec<(String, String)>,
    /// Param keys referenced by any relation constraint.
    relation_keys: BTreeSet<String>,
    /// Lexical bindings in scope at the target part, in binding order
    /// (later entries shadow earlier ones).
    part_scope: Vec<(String, ExprKind)>,
    /// The matched part clause items.
    part_items: Vec<ExprKind>,
    /// All part/feature keys seen, for the unknown-key error.
    part_keys: Vec<String>,
}

pub fn extract_component(request: &ComponentExtractRequest) -> AppResult<ExtractedComponent> {
    let name = request
        .component_name
        .clone()
        .unwrap_or_else(|| request.part_key.clone());
    validate_component_name(&name)?;

    let forms = Parser::parse_without_lowering(&request.source)
        .map_err(|err| AppError::parse(format!("Extraction failed to parse source: {err}")))?;
    let scan = scan_model(&forms, &request.part_key)?;

    let body = scan.part_items.last().cloned().ok_or_else(|| {
        AppError::validation(format!(
            "Part `{}` has no geometry expression.",
            request.part_key
        ))
    })?;

    let port_clauses = scan
        .part_items
        .iter()
        .skip(2)
        .filter(|item| {
            expr_list_items(item, "part clause")
                .ok()
                .and_then(|items| items.first().and_then(expr_head_name))
                .as_deref()
                == Some("ports")
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut free = collect_free_variables(&body, &BTreeSet::new());
    for clause in &port_clauses {
        free.extend(port_clause_free_variables(clause)?);
    }
    let params_by_key: BTreeMap<&String, &String> =
        scan.param_entries.iter().map(|(k, v)| (k, v)).collect();

    let mut signature_entries = Vec::new();
    let mut header_params = Vec::new();
    let mut blockers = Vec::new();

    // Model params first, in their declaration order.
    for (key, entry_source) in &scan.param_entries {
        if free.contains(key) {
            signature_entries.push(entry_source.clone());
            header_params.push(header_param_from_entry_source(entry_source)?);
        }
    }
    // Then lexical bindings, sorted for determinism.
    for key in &free {
        if params_by_key.contains_key(key) {
            continue;
        }
        match resolve_scope_binding(&scan.part_scope, key) {
            Some(value) => match scalar_literal(value) {
                Some((kind, text, json)) => {
                    signature_entries.push(format!("({kind} {key} {text})"));
                    header_params.push(ComponentHeaderParam {
                        key: key.clone(),
                        kind: kind.to_string(),
                        default: Some(json),
                        label: None,
                    });
                }
                None => blockers.push(format!(
                    "binding `{key}` is not a scalar literal (current value: `{value}`)"
                )),
            },
            None => blockers.push(format!("unresolved free reference `{key}`")),
        }
    }

    if !blockers.is_empty() {
        return Err(AppError::validation(format!(
            "Extraction of part `{}` is blocked: {}.",
            request.part_key,
            blockers.join("; ")
        )));
    }

    let port_source = port_clauses
        .iter()
        .map(|clause| format!("  {clause}\n"))
        .collect::<String>();
    let component_source = format!(
        "(define-component {}\n  ({})\n{}  {})",
        name,
        signature_entries.join("\n   "),
        port_source,
        body
    );

    let interfaces = header_params
        .iter()
        .map(|param| param.key.clone())
        .filter(|key| scan.relation_keys.contains(key))
        .collect();

    let header = ComponentHeader {
        schema_version: 1,
        name: name.clone(),
        component_id: None,
        revision_digest: None,
        dependencies: Vec::new(),
        description: request.description.clone(),
        params: header_params,
        tags: request.tags.clone(),
        provenance: ComponentProvenance {
            project_id: None,
            thread_id: request.thread_id.clone(),
            message_id: request.message_id.clone(),
            source_digest: format!("sha256:{:x}", Sha256::digest(request.source.as_bytes())),
        },
        interfaces,
        ports: component_header_ports(&port_clauses)?,
    };

    Ok(ExtractedComponent {
        name,
        component_source,
        header,
    })
}

fn port_clause_free_variables(clause: &ExprKind) -> AppResult<BTreeSet<String>> {
    let mut free = BTreeSet::new();
    let items = expr_list_items(clause, "ports clause")
        .map_err(|error| AppError::parse(error.to_string()))?;
    for port_expr in items.iter().skip(1) {
        let port_items = expr_list_items(port_expr, "port clause")
            .map_err(|error| AppError::parse(error.to_string()))?;
        let mut index = 2usize;
        while index < port_items.len() {
            let Some(keyword) = extract_keyword_name(&port_items[index]) else {
                index += 1;
                continue;
            };
            let Some(value) = port_items.get(index + 1) else {
                break;
            };
            match keyword.as_str() {
                "frame" => {
                    let frame_items = expr_list_items(value, "port frame")
                        .map_err(|error| AppError::parse(error.to_string()))?;
                    for coordinate_value in frame_items.iter().skip(2).step_by(2) {
                        free.extend(collect_free_variables(coordinate_value, &BTreeSet::new()));
                    }
                }
                "params" => {
                    let params = expr_list_items(value, "port params")
                        .map_err(|error| AppError::parse(error.to_string()))?;
                    for param in params {
                        let pair = expr_list_items(&param, "port param")
                            .map_err(|error| AppError::parse(error.to_string()))?;
                        if let Some(param_value) = pair.get(1) {
                            free.extend(collect_free_variables(param_value, &BTreeSet::new()));
                        }
                    }
                }
                _ => {}
            }
            index += 2;
        }
    }
    Ok(free)
}

fn component_header_ports(clauses: &[ExprKind]) -> AppResult<Vec<ComponentHeaderPort>> {
    let mut ports = Vec::new();
    let mut seen = BTreeSet::new();
    for clause in clauses {
        let items = expr_list_items(clause, "ports clause")
            .map_err(|error| AppError::parse(error.to_string()))?;
        for port_expr in items.iter().skip(1) {
            let port_items = expr_list_items(port_expr, "port clause")
                .map_err(|error| AppError::parse(error.to_string()))?;
            if port_items.first().and_then(expr_head_name).as_deref() != Some("port")
                || port_items.len() < 2
            {
                return Err(AppError::parse(
                    "Extracted ports must use `(port id :type ... :frame ...)`.",
                ));
            }
            let port_id = expr_identifier(&port_items[1])
                .ok_or_else(|| AppError::parse("Extracted port id must be a literal symbol."))?;
            if !seen.insert(port_id.clone()) {
                return Err(AppError::validation(format!(
                    "Extracted part defines port `{port_id}` more than once."
                )));
            }
            let mut type_id = None;
            let mut index = 2usize;
            while index < port_items.len() {
                let keyword = extract_keyword_name(&port_items[index]).ok_or_else(|| {
                    AppError::parse(format!(
                        "Extracted port `{port_id}` expects keyword options."
                    ))
                })?;
                let value = port_items.get(index + 1).ok_or_else(|| {
                    AppError::parse(format!(
                        "Extracted port `{port_id}` option `:{keyword}` needs a value."
                    ))
                })?;
                if keyword == "type" {
                    type_id = Some(extract_symbol_or_text(value).ok_or_else(|| {
                        AppError::parse(format!(
                            "Extracted port `{port_id}` type must be literal text or symbol."
                        ))
                    })?);
                }
                index += 2;
            }
            ports.push(ComponentHeaderPort {
                port_id: port_id.clone(),
                type_id: type_id.ok_or_else(|| {
                    AppError::parse(format!("Extracted port `{port_id}` requires `:type`."))
                })?,
                port_source: port_expr.to_string(),
            });
        }
    }
    Ok(ports)
}

fn extract_keyword_name(expr: &ExprKind) -> Option<String> {
    let ExprKind::Atom(atom) = expr else {
        return None;
    };
    match &atom.syn.ty {
        TokenType::Keyword(name) | TokenType::Identifier(name) => {
            name.to_string().strip_prefix(':').map(str::to_string)
        }
        _ => None,
    }
}

fn extract_symbol_or_text(expr: &ExprKind) -> Option<String> {
    let ExprKind::Atom(atom) = expr else {
        return None;
    };
    match &atom.syn.ty {
        TokenType::Identifier(value) => Some(value.to_string()),
        TokenType::StringLiteral(value) => Some(value.to_string()),
        _ => None,
    }
}

fn validate_component_name(name: &str) -> AppResult<()> {
    let mut chars = name.chars();
    let head_ok = chars
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false);
    if head_ok
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Ok(());
    }
    Err(AppError::validation(format!(
        "Component name `{name}` must be a symbol: letters, digits, `_` or `-`, starting with a letter."
    )))
}

fn scan_model(forms: &[ExprKind], part_key: &str) -> AppResult<ModelScan> {
    let mut scan = ModelScan {
        param_entries: Vec::new(),
        relation_keys: BTreeSet::new(),
        part_scope: Vec::new(),
        part_items: Vec::new(),
        part_keys: Vec::new(),
    };
    let mut found = false;
    for form in forms {
        let Ok(items) = expr_list_items(form, "top-level form") else {
            continue;
        };
        if items.first().and_then(expr_head_name).as_deref() != Some("model") {
            continue;
        }
        let mut scope = Vec::new();
        scan_clauses(&items[1..], &mut scope, part_key, &mut scan, &mut found)?;
    }
    if !found {
        return Err(AppError::validation(format!(
            "No part or feature with key `{}`. Available keys: [{}].",
            part_key,
            scan.part_keys.join(", ")
        )));
    }
    Ok(scan)
}

fn scan_clauses(
    clauses: &[ExprKind],
    scope: &mut Vec<(String, ExprKind)>,
    part_key: &str,
    scan: &mut ModelScan,
    found: &mut bool,
) -> AppResult<()> {
    for clause in clauses {
        let Ok(items) = expr_list_items(clause, "model clause") else {
            continue;
        };
        let Some(head) = items.first().and_then(expr_head_name) else {
            continue;
        };
        match head.as_str() {
            "params" => scan_params_clause(&items[1..], scan),
            "begin" => scan_clauses(&items[1..], scope, part_key, scan, found)?,
            "let" | "let*" if items.len() >= 3 => {
                let mut nested = scope.clone();
                if let Ok(bindings) = expr_list_items(&items[1], "let bindings") {
                    for binding in &bindings {
                        if let Ok(pair) = expr_list_items(binding, "let binding") {
                            if pair.len() == 2 {
                                if let Some(name) = expr_identifier(&pair[0]) {
                                    nested.push((name, pair[1].clone()));
                                }
                            }
                        }
                    }
                }
                scan_clauses(&items[2..], &mut nested, part_key, scan, found)?;
            }
            "part" | "feature" => {
                if let Some(key) = items.get(1).and_then(expr_stringish_key) {
                    if key == part_key && !*found {
                        scan.part_items = items.clone();
                        scan.part_scope = scope.clone();
                        *found = true;
                    }
                    scan.part_keys.push(key);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn expr_stringish_key(expr: &ExprKind) -> Option<String> {
    if let Some(identifier) = expr_identifier(expr) {
        return Some(identifier);
    }
    let ExprKind::Atom(atom) = expr else {
        return None;
    };
    match &atom.syn.ty {
        TokenType::StringLiteral(value) => Some(value.resolve().to_string()),
        _ => None,
    }
}

fn scan_params_clause(entries: &[ExprKind], scan: &mut ModelScan) {
    let mut index = 0usize;
    while index < entries.len() {
        let entry = &entries[index];
        if let Some(name) = expr_identifier(entry) {
            if name.trim_start_matches('#') == ":relations" {
                if let Some(relations) = entries.get(index + 1) {
                    collect_relation_keys(relations, &mut scan.relation_keys);
                }
                index += 2;
                continue;
            }
        }
        if let Ok(items) = expr_list_items(entry, "param entry") {
            if let Some(key) = items.get(1).and_then(expr_identifier) {
                scan.param_entries.push((key, entry.to_string()));
            }
        }
        index += 1;
    }
}

fn collect_relation_keys(relations: &ExprKind, keys: &mut BTreeSet<String>) {
    let Ok(items) = expr_list_items(relations, "relations") else {
        return;
    };
    for relation in &items {
        let Ok(operands) = expr_list_items(relation, "relation") else {
            continue;
        };
        for operand in operands.iter().skip(1) {
            if let Some(name) = expr_identifier(operand) {
                keys.insert(name);
            }
        }
    }
}

fn resolve_scope_binding<'a>(scope: &'a [(String, ExprKind)], key: &str) -> Option<&'a ExprKind> {
    scope
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// Returns `(signature kind, source text, json value)` for scalar literals.
fn scalar_literal(value: &ExprKind) -> Option<(&'static str, String, serde_json::Value)> {
    let ExprKind::Atom(atom) = value else {
        return None;
    };
    match &atom.syn.ty {
        TokenType::Number(_) => {
            let text = value.to_string();
            let number: f64 = text.parse().ok()?;
            Some((
                "number",
                text,
                serde_json::Number::from_f64(number).map(serde_json::Value::Number)?,
            ))
        }
        TokenType::BooleanLiteral(flag) => Some((
            "toggle",
            if *flag { "true" } else { "false" }.to_string(),
            serde_json::Value::Bool(*flag),
        )),
        _ => None,
    }
}

fn header_param_from_entry_source(entry_source: &str) -> AppResult<ComponentHeaderParam> {
    let parsed = Parser::parse_without_lowering(entry_source)
        .map_err(|err| AppError::parse(format!("Invalid param entry `{entry_source}`: {err}")))?;
    let entry = parsed
        .first()
        .ok_or_else(|| AppError::parse(format!("Empty param entry `{entry_source}`.")))?;
    let items = expr_list_items(entry, "param entry")
        .map_err(|err| AppError::parse(format!("Invalid param entry `{entry_source}`: {err}")))?;
    let kind = items
        .first()
        .and_then(expr_identifier)
        .unwrap_or_else(|| "number".to_string());
    let key = items
        .get(1)
        .and_then(expr_identifier)
        .ok_or_else(|| AppError::parse(format!("Param entry `{entry_source}` has no key.")))?;

    let mut default = None;
    let mut label = None;
    let mut index = 2usize;
    while index < items.len() {
        let item = &items[index];
        if let Some(keyword) = expr_identifier(item)
            .map(|name| name.trim_start_matches('#').to_string())
            .filter(|name| name.starts_with(':'))
        {
            if keyword == ":label" {
                if let Some(value) = items.get(index + 1) {
                    label = Some(value.to_string().trim_matches('"').to_string());
                }
            }
            index += 2;
            continue;
        }
        if default.is_none() {
            default = scalar_literal(item)
                .map(|(_, _, json)| json)
                .or_else(|| Some(serde_json::Value::String(item.to_string())));
        }
        index += 1;
    }

    Ok(ComponentHeaderParam {
        key,
        kind,
        default,
        label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_scheme::compile_to_core_program;

    fn request(source: &str, part_key: &str) -> ComponentExtractRequest {
        ComponentExtractRequest {
            source: source.to_string(),
            part_key: part_key.to_string(),
            component_name: None,
            description: None,
            tags: vec!["test".to_string()],
            thread_id: Some("thread-1".to_string()),
            message_id: Some("message-9".to_string()),
        }
    }

    const MODEL_WITH_PARAM: &str = r#"
        (model
          (params (number width 12 :label "Width" :min 4 :max 30)
                  (number depth 6))
          (part bracket (box width depth 3))
          (part lid (box 4 4 1)))
    "#;

    #[test]
    fn extracts_part_with_referenced_model_params_as_signature() {
        let extracted = extract_component(&request(MODEL_WITH_PARAM, "bracket")).expect("extract");

        assert_eq!(extracted.name, "bracket");
        assert!(
            extracted
                .component_source
                .contains("(define-component bracket"),
            "{}",
            extracted.component_source
        );
        assert!(
            extracted.component_source.contains("width"),
            "{}",
            extracted.component_source
        );
        let keys: Vec<&str> = extracted
            .header
            .params
            .iter()
            .map(|param| param.key.as_str())
            .collect();
        assert_eq!(keys, vec!["width", "depth"]);
        assert_eq!(extracted.header.params[0].label.as_deref(), Some("Width"));
    }

    #[test]
    fn unreferenced_params_stay_out_of_the_signature() {
        let extracted = extract_component(&request(MODEL_WITH_PARAM, "lid")).expect("extract");
        assert!(
            extracted.header.params.is_empty(),
            "{:?}",
            extracted.header.params
        );
    }

    #[test]
    fn extracts_legacy_part_with_string_key() {
        let source = r#"
            (model
              (params (number clearance 0.2))
              (part "BottleCage" (box 12 clearance 3)))
        "#;

        let extracted =
            extract_component(&request(source, "BottleCage")).expect("extract string-key part");

        assert_eq!(extracted.name, "BottleCage");
        assert_eq!(extracted.header.params[0].key, "clearance");
    }

    #[test]
    fn scalar_let_bindings_become_plain_defaults() {
        let source = r#"
            (model
              (let* ((wall 2.4)
                     (wall 3.2))
                (part shell (box wall wall 10))))
        "#;
        let extracted = extract_component(&request(source, "shell")).expect("extract");
        assert!(
            extracted.component_source.contains("(number wall 3.2)"),
            "shadowed binding must resolve to the innermost value: {}",
            extracted.component_source
        );
    }

    #[test]
    fn non_scalar_free_bindings_are_reported_as_blockers() {
        let source = r#"
            (model
              (let ((profile_pts (list (point 0 0) (point 1 0))))
                (part shell (extrude (polygon profile_pts) 4))))
        "#;
        let err = extract_component(&request(source, "shell")).expect_err("blocked");
        let message = err.message.clone();
        assert!(message.contains("blocked"), "{message}");
        assert!(message.contains("profile_pts"), "{message}");
    }

    #[test]
    fn unknown_part_key_errors_deterministically() {
        let err = extract_component(&request(MODEL_WITH_PARAM, "nope")).expect_err("unknown key");
        let message = err.message.clone();
        assert!(message.contains("nope"), "{message}");
        assert!(
            message.contains("bracket"),
            "available keys listed: {message}"
        );
        assert!(message.contains("lid"), "available keys listed: {message}");
    }

    #[test]
    fn extracted_source_recompiles_standalone_when_instantiated() {
        let extracted = extract_component(&request(MODEL_WITH_PARAM, "bracket")).expect("extract");
        let wrapped = format!(
            "{}\n(model (part demo ({})))",
            extracted.component_source, extracted.name
        );
        let program = compile_to_core_program(&wrapped)
            .unwrap_or_else(|err| panic!("standalone recompile failed: {err}\n{wrapped}"));
        assert_eq!(program.parts.len(), 1);
        assert_eq!(program.parts[0].key, "demo");
    }

    #[test]
    fn extracted_source_with_quoted_alignment_recompiles_standalone() {
        let source = r#"
            (model
              (part "MountBase"
                (box 24 6 85 :align '(center max center))))
        "#;
        let extracted = extract_component(&request(source, "MountBase")).expect("extract");
        let wrapped = format!(
            "{}\n(model (part demo ({})))",
            extracted.component_source, extracted.name
        );

        compile_to_core_program(&wrapped)
            .unwrap_or_else(|err| panic!("quoted alignment recompile failed: {err}\n{wrapped}"));
    }

    #[test]
    fn header_carries_provenance_tags_and_interfaces() {
        let source = r#"
            (model
              (params (number pin_d 8)
                      (number bore 8.3)
                      :relations ((< pin_d bore)))
              (part pin (cylinder pin_d 10 48))
              (part sleeve (cylinder bore 10 48)))
        "#;
        let extracted = extract_component(&request(source, "pin")).expect("extract");

        assert_eq!(extracted.header.tags, vec!["test".to_string()]);
        assert_eq!(
            extracted.header.provenance.thread_id.as_deref(),
            Some("thread-1")
        );
        assert_eq!(
            extracted.header.provenance.message_id.as_deref(),
            Some("message-9")
        );
        assert!(
            extracted
                .header
                .provenance
                .source_digest
                .starts_with("sha256:"),
            "{}",
            extracted.header.provenance.source_digest
        );
        assert_eq!(extracted.header.interfaces, vec!["pin_d".to_string()]);

        let json = serde_json::to_value(&extracted.header).expect("serialize");
        assert!(json["provenance"]["sourceDigest"].is_string());
        assert_eq!(json["name"], "pin");
    }

    #[test]
    fn extraction_preserves_local_port_source_and_compact_header() {
        let source = r#"
            (model
              (params (number clearance 0.3))
              (part bracket
                (ports
                  (port mount
                    :type "mechanical.mount.v1"
                    :params ((clearance clearance))
                    :frame (frame
                      :origin '(0 0 0)
                      :x-axis '(1 0 0)
                      :z-axis '(0 0 1))))
                (box 10 clearance 4)))
        "#;
        let extracted = extract_component(&request(source, "bracket")).expect("extract port");

        assert!(extracted.component_source.contains("(ports"));
        assert!(
            extracted
                .component_source
                .contains(":params ((clearance clearance))")
        );
        assert_eq!(extracted.header.ports.len(), 1);
        assert_eq!(extracted.header.ports[0].port_id, "mount");
        assert_eq!(extracted.header.ports[0].type_id, "mechanical.mount.v1");
        assert!(extracted.header.ports[0].port_source.contains(":frame"));

        let wrapped = format!(
            "{}\n(model (part demo ({})))",
            extracted.component_source, extracted.name
        );
        compile_to_core_program(&wrapped)
            .unwrap_or_else(|error| panic!("extracted port source failed: {error}\n{wrapped}"));
    }

    #[test]
    fn extraction_rejects_port_frame_bound_to_parent_world_value() {
        let source = r#"
            (model
              (let ((world-origin (list 10 20 30)))
                (part bracket
                  (ports
                    (port mount :type "mechanical.mount.v1"
                      :frame (frame
                        :origin world-origin
                        :x-axis '(1 0 0)
                        :z-axis '(0 0 1))))
                  (box 10 4 2))))
        "#;
        let error = extract_component(&request(source, "bracket"))
            .expect_err("parent/world port binding blocks extraction");
        assert!(error.message.contains("world-origin"), "{}", error.message);
        assert!(error.message.contains("blocked"), "{}", error.message);
    }

    #[test]
    fn automatic_capture_tracks_only_component_and_transitive_helper_closure() {
        let initial = r#"
            (define base-width 4)
            (define (wall width) (box width base-width 2))
            (define-component cage ((number diameter 70)) (wall diameter))
            (define unrelated 99)
            (model (part body (cage :diameter 74)))
        "#;
        let spaced = "(define base-width 4) ; comment\n(define (wall width) (box width base-width 2))\n(define-component cage ((number diameter 70)) (wall diameter))\n(define unrelated 99)\n(model (part body (cage :diameter 90)))";
        let changed = initial.replace("(define base-width 4)", "(define base-width 5)");
        let first = extract_defined_components(initial, "thread-a", "message-1").unwrap();
        let same = extract_defined_components(spaced, "thread-a", "message-2").unwrap();
        let next = extract_defined_components(&changed, "thread-a", "message-3").unwrap();
        let first_cage = first
            .iter()
            .find(|component| component.name == "cage")
            .unwrap();
        let same_cage = same
            .iter()
            .find(|component| component.name == "cage")
            .unwrap();
        let next_cage = next
            .iter()
            .find(|component| component.name == "cage")
            .unwrap();

        assert_eq!(
            first_cage.header.revision_digest,
            same_cage.header.revision_digest
        );
        assert_ne!(
            first_cage.header.revision_digest,
            next_cage.header.revision_digest
        );
        assert_eq!(
            first_cage.header.component_id,
            next_cage.header.component_id
        );
        assert_eq!(first_cage.header.dependencies, vec!["base-width", "wall"]);
        assert!(
            first_cage
                .component_source
                .contains("(define base-width 4)")
        );
        assert_eq!(
            first_cage.header.params[0].default,
            Some(serde_json::json!(70.0))
        );
        assert!(
            extract_defined_components(
                "(model (part ordinary (box 1 2 3)))",
                "thread-a",
                "message-4"
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn automatic_capture_does_not_treat_quoted_or_shadowed_symbols_as_dependencies() {
        let source = r#"
            (define shadowed 10)
            (define-component plug ((number shadowed 2))
              (let ((local 1)) (box shadowed local 3))
              (verify (tag "shadowed")))
        "#;
        let component = extract_defined_components(source, "thread-a", "message-1")
            .unwrap()
            .into_iter()
            .find(|component| component.name == "plug")
            .unwrap();
        assert!(component.header.dependencies.is_empty());
    }

    #[test]
    fn automatic_capture_tracks_top_level_dependencies_used_in_parameter_defaults() {
        let source = "(define default-width 14) (define-component adapter ((number width default-width)) (box width 2 3))";
        let component = extract_defined_components(source, "thread-a", "message-1")
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(component.header.dependencies, vec!["default-width"]);
        assert!(
            component
                .component_source
                .starts_with("(define default-width 14)")
        );
    }

    #[test]
    fn automatic_capture_accepts_component_port_verify_and_build_declarations() {
        let source = r#"
          (define-component clip ((number width 4))
            (ports (port mount :type "mount.v1"
              :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1))))
            (verify (tag stable)
              (metric bad-edges (stl non-manifold-edge-count))
              (expect bad-edges (= 0)))
            (build (shape body (box width 2 1)) (result body))
            (box width 2 1))
        "#;
        let captured = extract_defined_components(source, "thread-a", "message-a").unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].header.ports[0].port_id, "mount");
        assert!(captured[0].component_source.contains("(metric bad-edges"));
    }
}
