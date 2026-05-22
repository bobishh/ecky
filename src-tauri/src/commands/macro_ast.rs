//! Source-range projection for the Macro AST map ("New Params" view).
//!
//! Parses the ORIGINAL `.ecky` text (no wrapping, no lowering) and returns
//! top-level model clause nodes with exact byte ranges, so the map can offer
//! true edit-in-place: the frontend slices the range, lets the author edit
//! that node's source, splices it back, and applies through the normal
//! validate -> render -> stage pipeline.

use serde::Serialize;
use specta::Type;
use steel_core::parser::ast::ExprKind;
use steel_core::parser::parser::Parser;

use crate::ecky_scheme::compiler::{expr_head_name, expr_identifier, expr_list_items};
use crate::models::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MacroAstSourceNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    /// Byte offsets into the exact source string that was passed in.
    pub start_byte: u32,
    pub end_byte: u32,
}

#[tauri::command]
#[specta::specta]
pub fn macro_ast_source_map(macro_code: String) -> AppResult<Vec<MacroAstSourceNode>> {
    macro_ast_source_map_impl(&macro_code)
}

fn macro_ast_source_map_impl(source: &str) -> AppResult<Vec<MacroAstSourceNode>> {
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| AppError::parse(format!("Macro parse failed: {err}")))?;
    let mut nodes = Vec::new();
    for form in &forms {
        let Ok(items) = expr_list_items(form, "top-level form") else {
            continue;
        };
        if items.first().and_then(expr_head_name).as_deref() != Some("model") {
            continue;
        }
        if let Some(range) = node_range(source, form) {
            nodes.push(MacroAstSourceNode {
                id: "model".to_string(),
                kind: "model".to_string(),
                label: "model".to_string(),
                start_byte: range.0,
                end_byte: range.1,
            });
        }
        collect_clause_nodes(source, &items[1..], &mut nodes);
    }
    Ok(nodes)
}

fn collect_clause_nodes(source: &str, clauses: &[ExprKind], nodes: &mut Vec<MacroAstSourceNode>) {
    for clause in clauses {
        let Ok(items) = expr_list_items(clause, "model clause") else {
            continue;
        };
        let Some(head) = items.first().and_then(expr_head_name) else {
            continue;
        };
        match head.as_str() {
            "begin" => collect_clause_nodes(source, &items[1..], nodes),
            "let" | "let*" if items.len() >= 3 => collect_clause_nodes(source, &items[2..], nodes),
            "part" | "feature" => {
                let key = items
                    .get(1)
                    .and_then(expr_identifier)
                    .unwrap_or_else(|| format!("{}-{}", head, nodes.len()));
                push_node(source, clause, &format!("{head}:{key}"), &head, &key, nodes);
            }
            "params" => {
                let index = nodes.iter().filter(|node| node.kind == "params").count();
                push_node(
                    source,
                    clause,
                    &format!("params:{index}"),
                    "params",
                    "params",
                    nodes,
                );
            }
            "verify" => {
                let label = items
                    .get(1)
                    .and_then(|section| expr_list_items(section, "verify section").ok())
                    .and_then(|section| section.get(1).and_then(expr_identifier))
                    .unwrap_or_else(|| "verify".to_string());
                let index = nodes.iter().filter(|node| node.kind == "verify").count();
                push_node(
                    source,
                    clause,
                    &format!("verify:{index}"),
                    "verify",
                    &label,
                    nodes,
                );
            }
            "define-component" => {
                let name = items
                    .get(1)
                    .and_then(expr_identifier)
                    .unwrap_or_else(|| "component".to_string());
                push_node(
                    source,
                    clause,
                    &format!("component:{name}"),
                    "component",
                    &name,
                    nodes,
                );
            }
            _ => {}
        }
    }
}

fn push_node(
    source: &str,
    clause: &ExprKind,
    id: &str,
    kind: &str,
    label: &str,
    nodes: &mut Vec<MacroAstSourceNode>,
) {
    if let Some((start, end)) = node_range(source, clause) {
        nodes.push(MacroAstSourceNode {
            id: id.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
            start_byte: start,
            end_byte: end,
        });
    }
}

/// Authoritative range: the parser's span start anchored on `(`, with the
/// end found by string/comment-aware balanced paren scanning, so the range
/// always covers the complete form regardless of span end semantics.
fn node_range(source: &str, expr: &ExprKind) -> Option<(u32, u32)> {
    let span = expr.span();
    let mut start = span.start as usize;
    let bytes = source.as_bytes();
    // The span may anchor on the head atom; back up to the opening paren.
    while start > 0 && bytes.get(start) != Some(&b'(') {
        start -= 1;
        if !bytes[start].is_ascii_whitespace() && bytes[start] != b'(' {
            // walked into other content without finding '('
            if bytes[start] == b')' {
                return None;
            }
        }
        if bytes.get(start) == Some(&b'(') {
            break;
        }
    }
    if bytes.get(start) != Some(&b'(') {
        return None;
    }
    let end = balanced_end(source, start)?;
    Some((start as u32, end as u32))
}

fn balanced_end(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;
    for (offset, ch) in source[start..].char_indices() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            ';' => in_comment = true,
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"(model
  (params (number width 12 :label "Width") (number depth 6))
  (verify (tag wall_ok) (metric min_wall_thickness "body") (expect (>= value 1)))
  (let* ((wall 2.4))
    (part body (box width depth wall)))
  (part lid "Lid ; not a comment" (box 4 4 1)))"#;

    fn slice(node: &MacroAstSourceNode) -> &'static str {
        &SOURCE[node.start_byte as usize..node.end_byte as usize]
    }

    #[test]
    fn maps_model_clauses_to_exact_source_ranges() {
        let nodes = macro_ast_source_map_impl(SOURCE).expect("map");
        let by_id: std::collections::BTreeMap<&str, &MacroAstSourceNode> =
            nodes.iter().map(|node| (node.id.as_str(), node)).collect();

        let model = by_id.get("model").expect("model node");
        assert_eq!(slice(model), SOURCE);

        let body = by_id.get("part:body").expect("body node");
        assert_eq!(slice(body), "(part body (box width depth wall))");

        let lid = by_id.get("part:lid").expect("lid node");
        assert_eq!(slice(lid), "(part lid \"Lid ; not a comment\" (box 4 4 1))");

        let params = by_id.get("params:0").expect("params node");
        assert!(slice(params).starts_with("(params"));
        assert!(slice(params).ends_with("(number depth 6))"));

        let verify = by_id.get("verify:0").expect("verify node");
        assert_eq!(verify.label, "wall_ok");
        assert!(slice(verify).starts_with("(verify"));
        assert!(slice(verify).ends_with(")"));
    }

    #[test]
    fn splicing_a_part_range_yields_a_compilable_model() {
        let nodes = macro_ast_source_map_impl(SOURCE).expect("map");
        let body = nodes
            .iter()
            .find(|node| node.id == "part:body")
            .expect("body");
        let edited = format!(
            "{}{}{}",
            &SOURCE[..body.start_byte as usize],
            "(part body (box width depth (* wall 2)))",
            &SOURCE[body.end_byte as usize..]
        );
        crate::ecky_scheme::compile_to_core_program(&edited).expect("edited model compiles");
    }

    #[test]
    fn parse_errors_surface_as_validation() {
        let err = macro_ast_source_map_impl("(model (part body").expect_err("unclosed");
        assert!(err.message.contains("parse"), "{}", err.message);
    }
}
