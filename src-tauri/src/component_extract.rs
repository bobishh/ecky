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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub source_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHeader {
    pub name: String,
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

    let free = collect_free_variables(&body, &BTreeSet::new());
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

    let component_source = format!(
        "(define-component {}\n  ({})\n  {})",
        name,
        signature_entries.join("\n   "),
        body
    );

    let interfaces = header_params
        .iter()
        .map(|param| param.key.clone())
        .filter(|key| scan.relation_keys.contains(key))
        .collect();

    let header = ComponentHeader {
        name: name.clone(),
        description: request.description.clone(),
        params: header_params,
        tags: request.tags.clone(),
        provenance: ComponentProvenance {
            thread_id: request.thread_id.clone(),
            message_id: request.message_id.clone(),
            source_digest: format!("sha256:{:x}", Sha256::digest(request.source.as_bytes())),
        },
        interfaces,
    };

    Ok(ExtractedComponent {
        name,
        component_source,
        header,
    })
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
                if let Some(key) = items.get(1).and_then(expr_identifier) {
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
}
