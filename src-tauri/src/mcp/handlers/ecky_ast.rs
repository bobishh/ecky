mod operations;

pub use operations::{
    handle_ecky_ast_get, handle_ecky_ast_patch_validate, handle_ecky_ast_replace_and_render,
    handle_ecky_constraints_validate, handle_ecky_dependency_get, handle_ecky_selector_resolve,
};

mod shape_graph;
pub(super) use shape_graph::build_shape_graph_packet;

use super::{
    artifact_bundle_digest, handle_macro_preview_render, map_target_resolved_from,
    persist_agent_session, push_unique_strings, selection_target_match_ids,
    session_render_preview_for_request, try_record_agent_error, AgentContext,
};
use crate::mcp::contracts::*;
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, DesignOutput, DesignParams, MacroDialect,
    ModelManifest, ParamValue, PathResolver,
};
use std::collections::{HashMap, HashSet};

fn ecky_ast_authoring_enabled(state: &AppState) -> bool {
    state.config.lock().unwrap().mcp.ecky_ast_authoring
}
const DEFAULT_ECKY_AST_DEPTH: usize = 3;
const DEFAULT_ECKY_AST_MAX_NODES: usize = 120;
const MAX_ECKY_AST_DEPTH: usize = 12;
const MAX_ECKY_AST_NODES: usize = 500;
pub(super) const ECKY_AST_SOURCE_MAX_BYTES: usize = 4096;

fn path_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn core_node_kind_label(kind: &crate::ecky_core_ir::CoreNodeKind) -> &'static str {
    match kind {
        crate::ecky_core_ir::CoreNodeKind::Literal(_) => "Literal",
        crate::ecky_core_ir::CoreNodeKind::Reference(_) => "Reference",
        crate::ecky_core_ir::CoreNodeKind::Build { .. } => "Build",
        crate::ecky_core_ir::CoreNodeKind::Let { .. } => "Let",
        crate::ecky_core_ir::CoreNodeKind::If { .. } => "If",
        crate::ecky_core_ir::CoreNodeKind::Call { .. } => "Call",
        crate::ecky_core_ir::CoreNodeKind::Range { .. } => "Range",
        crate::ecky_core_ir::CoreNodeKind::Map { .. } => "Map",
        crate::ecky_core_ir::CoreNodeKind::Apply { .. } => "Apply",
        crate::ecky_core_ir::CoreNodeKind::List(_) => "List",
        crate::ecky_core_ir::CoreNodeKind::Group(_) => "Group",
    }
}

pub(super) fn core_node_child_paths<'a>(
    node: &'a crate::ecky_core_ir::CoreNode,
    path: &str,
) -> Vec<(String, &'a crate::ecky_core_ir::CoreNode)> {
    match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Literal(_)
        | crate::ecky_core_ir::CoreNodeKind::Reference(_) => Vec::new(),
        crate::ecky_core_ir::CoreNodeKind::Build { bindings, result } => bindings
            .iter()
            .map(|binding| {
                (
                    format!("{}/build/bindings/{}", path, path_segment(&binding.name)),
                    &binding.value,
                )
            })
            .chain(std::iter::once((
                format!("{path}/build/result"),
                result.as_ref(),
            )))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Let { bindings, body } => bindings
            .iter()
            .map(|binding| {
                (
                    format!("{}/let/bindings/{}", path, path_segment(&binding.name)),
                    &binding.value,
                )
            })
            .chain(std::iter::once((format!("{path}/let/body"), body.as_ref())))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => vec![
            (format!("{path}/if/condition"), condition.as_ref()),
            (format!("{path}/if/then"), then_branch.as_ref()),
            (format!("{path}/if/else"), else_branch.as_ref()),
        ],
        crate::ecky_core_ir::CoreNodeKind::Call { args, keywords, .. } => args
            .iter()
            .enumerate()
            .map(|(idx, arg)| (format!("{path}/call/args/{idx}"), arg))
            .chain(keywords.iter().map(|keyword| {
                (
                    format!("{}/call/keywords/{}", path, path_segment(&keyword.name)),
                    keyword.source_node(),
                )
            }))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Range { start, end } => vec![
            (format!("{path}/range/start"), start.as_ref()),
            (format!("{path}/range/end"), end.as_ref()),
        ],
        crate::ecky_core_ir::CoreNodeKind::Map { sources, body, .. } => sources
            .iter()
            .enumerate()
            .map(|(idx, source)| (format!("{path}/map/sources/{idx}"), source))
            .chain(std::iter::once((format!("{path}/map/body"), body.as_ref())))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Apply { args, list, .. } => args
            .iter()
            .enumerate()
            .map(|(idx, arg)| (format!("{path}/apply/args/{idx}"), arg))
            .chain(std::iter::once((
                format!("{path}/apply/list"),
                list.as_ref(),
            )))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::List(items) => items
            .iter()
            .enumerate()
            .map(|(idx, item)| (format!("{path}/list/{idx}"), item))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Group(items) => items
            .iter()
            .enumerate()
            .map(|(idx, item)| (format!("{path}/group/{idx}"), item))
            .collect(),
    }
}

fn core_node_op_label(node: &crate::ecky_core_ir::CoreNode) -> Option<String> {
    match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Call { op, .. }
        | crate::ecky_core_ir::CoreNodeKind::Apply { op, .. } => Some(format!("{op:?}")),
        _ => None,
    }
}

pub(super) fn core_node_digest(node: &crate::ecky_core_ir::CoreNode) -> String {
    let mut parts = vec![
        core_node_kind_label(&node.kind).to_string(),
        format!("{:?}", node.value_kind),
    ];
    match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Literal(value) => parts.push(format!("{value:?}")),
        crate::ecky_core_ir::CoreNodeKind::Reference(value) => parts.push(format!("{value:?}")),
        crate::ecky_core_ir::CoreNodeKind::Call { op, .. }
        | crate::ecky_core_ir::CoreNodeKind::Apply { op, .. } => parts.push(format!("{op:?}")),
        crate::ecky_core_ir::CoreNodeKind::Map { params, .. } => parts.push(format!("{params:?}")),
        crate::ecky_core_ir::CoreNodeKind::Build { bindings, .. } => parts.push(format!(
            "bindings:{:?}",
            bindings
                .iter()
                .map(|binding| &binding.name)
                .collect::<Vec<_>>()
        )),
        crate::ecky_core_ir::CoreNodeKind::Let { bindings, .. } => parts.push(format!(
            "bindings:{:?}",
            bindings
                .iter()
                .map(|binding| &binding.name)
                .collect::<Vec<_>>()
        )),
        crate::ecky_core_ir::CoreNodeKind::If { .. }
        | crate::ecky_core_ir::CoreNodeKind::Range { .. }
        | crate::ecky_core_ir::CoreNodeKind::List(_)
        | crate::ecky_core_ir::CoreNodeKind::Group(_) => {}
    }
    if let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } = &node.kind {
        parts.push(format!(
            "keywords:{:?}",
            keywords
                .iter()
                .map(|keyword| (&keyword.name, keyword.selector_payload()))
                .collect::<Vec<_>>()
        ));
    }
    for (_, child) in core_node_child_paths(node, "") {
        parts.push(core_node_digest(child));
    }
    crate::mcp::macro_buffer::source_digest(&parts.join("|"))
}

#[derive(Debug, Clone)]
struct EckyAstNodeAddressability {
    stable_node_key: String,
    source_addressable: bool,
    editable_ops: Vec<EckyAstEditOperation>,
    non_editable_reason: Option<String>,
}

fn binding_label_for_ast_path(path: &str) -> Option<String> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    if segments.len() == 2 && matches!(segments[0].as_str(), "params" | "parts") {
        return Some(segments[1].clone());
    }
    segments.windows(2).find_map(|window| {
        if matches!(window[0].as_str(), "bindings" | "keywords") {
            Some(window[1].clone())
        } else {
            None
        }
    })
}

fn source_slice_digest(source: &str, span: Option<(usize, usize)>) -> Option<String> {
    let (start, end) = span?;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    Some(crate::mcp::macro_buffer::source_digest(&source[start..end]))
}

pub(super) fn bounded_ecky_ast_source_slice(
    source: &str,
    span: (usize, usize),
) -> Option<EckyAstSourceSlice> {
    let (start, end) = span;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }

    let byte_len = end - start;
    let mut text_end = end.min(start + ECKY_AST_SOURCE_MAX_BYTES);
    while text_end > start && !source.is_char_boundary(text_end) {
        text_end -= 1;
    }
    if text_end == start {
        return None;
    }

    Some(EckyAstSourceSlice {
        span: EckyAstSpan {
            start: start as u32,
            end: end as u32,
        },
        text: source[start..text_end].to_string(),
        truncated: text_end < end,
        max_bytes: ECKY_AST_SOURCE_MAX_BYTES,
        byte_len,
    })
}

fn attach_ecky_ast_source_slices(source: &str, nodes: &mut [EckyAstNode]) {
    for node in nodes.iter_mut() {
        if !node.source_addressable {
            continue;
        }
        let Ok(span) = source_span_for_ecky_path(source, &node.path) else {
            continue;
        };
        node.source = bounded_ecky_ast_source_slice(source, span);
    }
}

fn stable_ast_node_key(
    source: &str,
    path: &str,
    kind: &str,
    value_kind: &str,
    op: Option<&str>,
    span: Option<(usize, usize)>,
) -> String {
    let mut parts = vec![
        format!("path={path}"),
        format!("kind={kind}"),
        format!("valueKind={value_kind}"),
    ];
    if let Some(op) = op {
        parts.push(format!("op={op}"));
    }
    if let Some(binding) = binding_label_for_ast_path(path) {
        parts.push(format!("binding={binding}"));
    }
    if let Some(digest) = source_slice_digest(source, span) {
        parts.push(format!("source={digest}"));
    }
    crate::mcp::macro_buffer::source_digest(&parts.join("|"))
}

fn editable_ops_for_source_target_kind(kind: &SourcePathTargetKind) -> Vec<EckyAstEditOperation> {
    match kind {
        SourcePathTargetKind::Root
        | SourcePathTargetKind::BuildResult
        | SourcePathTargetKind::LetBody => vec![EckyAstEditOperation::Replace],
        SourcePathTargetKind::PositionalArg | SourcePathTargetKind::KeywordValue { .. } => vec![
            EckyAstEditOperation::Replace,
            EckyAstEditOperation::InsertBefore,
            EckyAstEditOperation::InsertAfter,
            EckyAstEditOperation::Delete,
        ],
        SourcePathTargetKind::PartClause { .. }
        | SourcePathTargetKind::ParamDecl { .. }
        | SourcePathTargetKind::BuildBinding { .. }
        | SourcePathTargetKind::LetBinding { .. } => vec![
            EckyAstEditOperation::Replace,
            EckyAstEditOperation::InsertBefore,
            EckyAstEditOperation::InsertAfter,
            EckyAstEditOperation::Delete,
            EckyAstEditOperation::Rename,
        ],
    }
}

fn ecky_ast_node_addressability(
    source: &str,
    path: &str,
    kind: &str,
    value_kind: &str,
    op: Option<&str>,
    fallback_span: Option<(usize, usize)>,
) -> EckyAstNodeAddressability {
    let source_target = SourceExprParser::new(source).parse_all().and_then(|exprs| {
        let target = source_target_for_ecky_path(&exprs, source, path)?;
        Ok((
            (target.expr.start, target.expr.end),
            editable_ops_for_source_target_kind(&target.kind),
        ))
    });

    match source_target {
        Ok((source_span, editable_ops)) => EckyAstNodeAddressability {
            stable_node_key: stable_ast_node_key(
                source,
                path,
                kind,
                value_kind,
                op,
                Some(source_span),
            ),
            source_addressable: true,
            editable_ops,
            non_editable_reason: None,
        },
        Err(err) => EckyAstNodeAddressability {
            stable_node_key: stable_ast_node_key(source, path, kind, value_kind, op, fallback_span),
            source_addressable: false,
            editable_ops: Vec::new(),
            non_editable_reason: Some(err.message),
        },
    }
}

fn core_param_digest(param: &crate::ecky_core_ir::CoreParameter) -> String {
    crate::mcp::macro_buffer::source_digest(&format!(
        "param|{}|{}|{:?}|{:?}|{}|{:?}",
        param.key, param.label, param.kind, param.default_value, param.frozen, param.constraints
    ))
}

fn core_part_digest(part: &crate::ecky_core_ir::CorePart) -> String {
    crate::mcp::macro_buffer::source_digest(&format!(
        "part|{}|{}|{}",
        part.key,
        part.label,
        core_node_digest(&part.root)
    ))
}

fn collect_core_part_clause_ast_nodes(
    program: &crate::ecky_core_ir::CoreProgram,
    source: &str,
    requested_path: Option<&str>,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> AppResult<bool> {
    let mut truncated = false;
    for part in &program.parts {
        if nodes.len() >= max_nodes {
            return Ok(true);
        }
        let path = format!("/parts/{}", path_segment(&part.key));
        if let Some(requested_path) = requested_path {
            if requested_path != "/" && requested_path != path {
                continue;
            }
        }
        let span = source_span_for_ecky_path(source, &path)
            .ok()
            .map(|(start, end)| EckyAstSpan {
                start: start as u32,
                end: end as u32,
            });
        let addressability = ecky_ast_node_addressability(
            source,
            &path,
            "Part",
            "Part",
            None,
            span.as_ref()
                .map(|span| (span.start as usize, span.end as usize)),
        );
        nodes.push(EckyAstNode {
            path,
            stable_node_key: addressability.stable_node_key,
            digest: core_part_digest(part),
            node_id: 0,
            kind: "Part".to_string(),
            value_kind: "Part".to_string(),
            op: None,
            part_key: Some(part.key.clone()),
            span,
            source_addressable: addressability.source_addressable,
            editable_ops: addressability.editable_ops,
            non_editable_reason: addressability.non_editable_reason,
            source: None,
            child_paths: vec![format!("/parts/{}/root", path_segment(&part.key))],
        });
    }
    if nodes.len() >= max_nodes {
        truncated = true;
    }
    Ok(truncated)
}

fn collect_core_param_ast_nodes(
    program: &crate::ecky_core_ir::CoreProgram,
    source: &str,
    requested_path: Option<&str>,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> AppResult<bool> {
    let mut truncated = false;
    for param in &program.parameters {
        if nodes.len() >= max_nodes {
            return Ok(true);
        }
        let path = format!("/params/{}", path_segment(&param.key));
        if let Some(requested_path) = requested_path {
            if requested_path != "/" && requested_path != path {
                continue;
            }
        }
        let span = source_span_for_ecky_path(source, &path)
            .ok()
            .map(|(start, end)| EckyAstSpan {
                start: start as u32,
                end: end as u32,
            });
        let value_kind = format!("{:?}", param.kind);
        let addressability = ecky_ast_node_addressability(
            source,
            &path,
            "Param",
            &value_kind,
            None,
            span.as_ref()
                .map(|span| (span.start as usize, span.end as usize)),
        );
        nodes.push(EckyAstNode {
            path,
            stable_node_key: addressability.stable_node_key,
            digest: core_param_digest(param),
            node_id: 0,
            kind: "Param".to_string(),
            value_kind,
            op: None,
            part_key: None,
            span,
            source_addressable: addressability.source_addressable,
            editable_ops: addressability.editable_ops,
            non_editable_reason: addressability.non_editable_reason,
            source: None,
            child_paths: Vec::new(),
        });
    }
    if nodes.len() >= max_nodes {
        truncated = true;
    }
    Ok(truncated)
}

fn collect_core_ast_nodes(
    source: &str,
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    part_key: Option<&str>,
    depth: usize,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> bool {
    if nodes.len() >= max_nodes {
        return true;
    }
    let children = core_node_child_paths(node, path);
    let child_paths = children
        .iter()
        .map(|(child_path, _)| child_path.clone())
        .collect::<Vec<_>>();
    let kind = core_node_kind_label(&node.kind).to_string();
    let value_kind = format!("{:?}", node.value_kind);
    let op = core_node_op_label(node);
    let span = node.span.map(|span| EckyAstSpan {
        start: span.start,
        end: span.end,
    });
    let addressability = ecky_ast_node_addressability(
        source,
        path,
        &kind,
        &value_kind,
        op.as_deref(),
        span.as_ref()
            .map(|span| (span.start as usize, span.end as usize)),
    );
    nodes.push(EckyAstNode {
        path: path.to_string(),
        stable_node_key: addressability.stable_node_key,
        digest: core_node_digest(node),
        node_id: node.id.raw(),
        kind,
        value_kind,
        op,
        part_key: part_key.map(str::to_string),
        span,
        source_addressable: addressability.source_addressable,
        editable_ops: addressability.editable_ops,
        non_editable_reason: addressability.non_editable_reason,
        source: None,
        child_paths,
    });
    if depth == 0 {
        return false;
    }
    for (child_path, child) in children {
        if collect_core_ast_nodes(
            source,
            child,
            &child_path,
            part_key,
            depth - 1,
            max_nodes,
            nodes,
        ) {
            return true;
        }
    }
    false
}

fn collect_matching_core_ast_nodes(
    source: &str,
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    part_key: Option<&str>,
    requested_path: &str,
    depth: usize,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> bool {
    if path == requested_path {
        return collect_core_ast_nodes(source, node, path, part_key, depth, max_nodes, nodes);
    }
    for (child_path, child) in core_node_child_paths(node, path) {
        if requested_path.starts_with(&child_path)
            && collect_matching_core_ast_nodes(
                source,
                child,
                &child_path,
                part_key,
                requested_path,
                depth,
                max_nodes,
                nodes,
            )
        {
            return true;
        }
    }
    false
}

fn find_core_ast_node<'a>(
    node: &'a crate::ecky_core_ir::CoreNode,
    path: &str,
    requested_path: &str,
) -> Option<&'a crate::ecky_core_ir::CoreNode> {
    if path == requested_path {
        return Some(node);
    }
    for (child_path, child) in core_node_child_paths(node, path) {
        if requested_path.starts_with(&child_path) {
            if let Some(found) = find_core_ast_node(child, &child_path, requested_path) {
                return Some(found);
            }
        }
    }
    None
}

pub(super) fn find_core_ast_node_in_program<'a>(
    program: &'a crate::ecky_core_ir::CoreProgram,
    requested_path: &str,
) -> Option<&'a crate::ecky_core_ir::CoreNode> {
    for part in &program.parts {
        let root_path = format!("/parts/{}/root", path_segment(&part.key));
        if requested_path.starts_with(&root_path) {
            if let Some(found) = find_core_ast_node(&part.root, &root_path, requested_path) {
                return Some(found);
            }
        }
    }
    None
}

fn ast_path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>()
}

fn ecky_ast_operation_name(operation: &EckyAstEditOperation) -> &'static str {
    match operation {
        EckyAstEditOperation::Replace => "replace",
        EckyAstEditOperation::InsertBefore => "insertBefore",
        EckyAstEditOperation::InsertAfter => "insertAfter",
        EckyAstEditOperation::Delete => "delete",
        EckyAstEditOperation::Rename => "rename",
    }
}

fn source_line_for_offset(source: &str, offset: usize) -> Option<usize> {
    if offset > source.len() {
        return None;
    }
    Some(
        source.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1,
    )
}

fn parse_byte_offset_from_message(message: &str) -> Option<usize> {
    let marker = "byte ";
    let idx = message.find(marker)?;
    let digits = message[idx + marker.len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<usize>().ok())
        .flatten()
}

fn source_line_range_for_span(
    source: &str,
    span: crate::ecky_core_ir::SourceSpan,
) -> Option<(usize, usize)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    let start_line = source_line_for_offset(source, start)?;
    let inclusive_end = end.saturating_sub(1);
    let end_line = source_line_for_offset(source, inclusive_end)?;
    Some((start_line, end_line.max(start_line)))
}

fn compile_error_with_diagnostics(
    message: String,
    source: &str,
    compile_error: crate::ecky_core_ir::CompilerError,
    operation: Option<&str>,
    stable_node_key: Option<&str>,
) -> AppError {
    let mut error = AppError::validation(message);
    if let Some(operation) = operation {
        error = error.with_operation(operation.to_string());
    }
    if let Some(stable_node_key) = stable_node_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        error = error.with_stable_node_key(stable_node_key.to_string());
    }
    if let Some(span) = compile_error.primary_span {
        if let Some((start_line, end_line)) = source_line_range_for_span(source, span) {
            error = error.with_line_range(start_line, end_line);
        }
    } else if let Some(byte_offset) = parse_byte_offset_from_message(&compile_error.message) {
        if let Some(line) = source_line_for_offset(source, byte_offset.min(source.len())) {
            error = error.with_line_range(line, line);
        }
    }
    error
}

fn stable_node_key_for_program_path(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    path: &str,
) -> Option<String> {
    let segments = ast_path_segments(path);
    if segments.len() == 2 && segments[0] == "params" {
        let param = program
            .parameters
            .iter()
            .find(|item| item.key == segments[1])?;
        let span = source_span_for_ecky_path(source, path).ok();
        return Some(stable_ast_node_key(
            source,
            path,
            "Param",
            &format!("{:?}", param.kind),
            None,
            span,
        ));
    }
    if segments.len() == 2 && segments[0] == "parts" {
        let _part = program.parts.iter().find(|item| item.key == segments[1])?;
        let span = source_span_for_ecky_path(source, path).ok();
        return Some(stable_ast_node_key(
            source, path, "Part", "Part", None, span,
        ));
    }
    let node = find_core_ast_node_in_program(program, path)?;
    let fallback_span = node
        .span
        .map(|span| (span.start as usize, span.end as usize));
    let span = source_span_for_ecky_path(source, path)
        .ok()
        .or(fallback_span);
    Some(stable_ast_node_key(
        source,
        path,
        core_node_kind_label(&node.kind),
        &format!("{:?}", node.value_kind),
        core_node_op_label(node).as_deref(),
        span,
    ))
}

fn collect_program_node_paths(
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    paths: &mut Vec<String>,
) {
    paths.push(path.to_string());
    for (child_path, child) in core_node_child_paths(node, path) {
        collect_program_node_paths(child, &child_path, paths);
    }
}

fn all_program_ast_paths(program: &crate::ecky_core_ir::CoreProgram) -> Vec<String> {
    let mut paths = Vec::new();
    for param in &program.parameters {
        paths.push(format!("/params/{}", path_segment(&param.key)));
    }
    for part in &program.parts {
        let part_path = format!("/parts/{}", path_segment(&part.key));
        paths.push(part_path.clone());
        let root_path = format!("{part_path}/root");
        collect_program_node_paths(&part.root, &root_path, &mut paths);
    }
    paths
}

fn resolve_path_from_stable_node_key(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    stable_node_key: &str,
    tool_name: &str,
) -> AppResult<String> {
    let trimmed_key = stable_node_key.trim();
    if trimmed_key.is_empty() {
        return Err(AppError::validation(format!(
            "{tool_name} stableNodeKey must not be empty."
        )));
    }
    for path in all_program_ast_paths(program) {
        let Some(candidate_key) = stable_node_key_for_program_path(source, program, &path) else {
            continue;
        };
        if candidate_key == trimmed_key {
            return Ok(path);
        }
    }
    Err(AppError::validation(format!(
        "{tool_name} stableNodeKey not found in AST: {trimmed_key}."
    )))
}

fn resolve_ecky_ast_patch_path(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    path: Option<&str>,
    stable_node_key: Option<&str>,
    tool_name: &str,
) -> AppResult<String> {
    let explicit_path = path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let stable_node_key = stable_node_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let resolved_from_key = stable_node_key
        .as_deref()
        .map(|key| resolve_path_from_stable_node_key(source, program, key, tool_name))
        .transpose()?;

    match (explicit_path, resolved_from_key) {
        (None, None) => Err(AppError::validation(format!(
            "{tool_name} requires stableNodeKey or path."
        ))),
        (Some(path), None) => Ok(path),
        (None, Some(path)) => Ok(path),
        (Some(path), Some(resolved)) => {
            if path == resolved {
                Ok(path)
            } else {
                Err(AppError::validation(format!(
                    "{tool_name} stableNodeKey/path mismatch: stableNodeKey resolves to {resolved}, path is {path}."
                )))
            }
        }
    }
}

fn affected_node_keys_for_patch(
    old_source: &str,
    old_program: &crate::ecky_core_ir::CoreProgram,
    old_path: &str,
    new_source: &str,
    new_program: &crate::ecky_core_ir::CoreProgram,
    new_path: &str,
) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(key) = stable_node_key_for_program_path(old_source, old_program, old_path) {
        keys.push(key);
    }
    if !new_path.trim().is_empty() {
        if let Some(key) = stable_node_key_for_program_path(new_source, new_program, new_path) {
            if !keys.iter().any(|existing| existing == &key) {
                keys.push(key);
            }
        }
    }
    keys
}

fn dependency_impact_for_patch(
    program: &crate::ecky_core_ir::CoreProgram,
    edited_path: &str,
    affected_paths: &[String],
) -> EckyAstPatchDependencyImpactSummary {
    let summary_path = edited_path.trim();
    let segments = ast_path_segments(summary_path);
    if segments.len() == 2 && segments[0] == "params" {
        if let Ok(param_id) = param_id_for_dependency_key(program, &segments[1]) {
            let dependent_source_paths = dependent_source_paths_for_param(program, param_id);
            let reference_count = dependent_source_paths.len();
            let impacted_part_ids = impacted_part_ids_for_dependency_paths(&dependent_source_paths);
            let impact_labels = impact_labels_for_dependency(&impacted_part_ids, reference_count);
            return EckyAstPatchDependencyImpactSummary {
                path: format!("/params/{}", path_segment(&segments[1])),
                dependency_kind: "parameterReference".to_string(),
                dependent_source_paths,
                impacted_part_ids,
                impact_labels,
                reference_count,
            };
        }
    }

    let mut dependent_source_paths = Vec::new();
    for path in affected_paths {
        if path.trim().is_empty() {
            continue;
        }
        if !dependent_source_paths
            .iter()
            .any(|existing| existing == path)
        {
            dependent_source_paths.push(path.clone());
        }
    }
    let reference_count = dependent_source_paths.len();
    let impacted_part_ids = impacted_part_ids_for_dependency_paths(&dependent_source_paths);
    let impact_labels = impact_labels_for_dependency(&impacted_part_ids, reference_count);
    EckyAstPatchDependencyImpactSummary {
        path: summary_path.to_string(),
        dependency_kind: "pathLocal".to_string(),
        dependent_source_paths,
        impacted_part_ids,
        impact_labels,
        reference_count,
    }
}

enum EckyDependencyQuery {
    ParameterKey(String),
    SelectionTargetId(String),
}

fn parse_ecky_dependency_path(path: &str) -> AppResult<EckyDependencyQuery> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    if segments.len() != 2 || segments[1].is_empty() {
        return Err(AppError::validation(format!(
            "ecky_dependency_get supports /params/{{key}} and /targets/{{targetId}} paths. Unsupported path: {path}."
        )));
    }

    match segments[0].as_str() {
        "params" => Ok(EckyDependencyQuery::ParameterKey(segments[1].clone())),
        "targets" => Ok(EckyDependencyQuery::SelectionTargetId(segments[1].clone())),
        _ => Err(AppError::validation(format!(
            "ecky_dependency_get supports /params/{{key}} and /targets/{{targetId}} paths. Unsupported path: {path}."
        ))),
    }
}

fn param_id_for_dependency_key(
    program: &crate::ecky_core_ir::CoreProgram,
    key: &str,
) -> AppResult<crate::ecky_core_ir::ParamId> {
    program
        .parameters
        .iter()
        .find(|param| param.key == key)
        .map(|param| param.id)
        .ok_or_else(|| {
            AppError::validation(format!(
                "Ecky dependency source path not found: /params/{}.",
                key
            ))
        })
}

fn selection_targets_by_id<'a>(
    manifest: &'a ModelManifest,
    requested_id: &str,
) -> Vec<&'a crate::models::SelectionTarget> {
    manifest
        .selection_targets
        .iter()
        .filter(|target| {
            selection_target_match_ids(target)
                .iter()
                .any(|id| id == requested_id)
        })
        .collect()
}

fn selection_target_by_id<'a>(
    manifest: &'a ModelManifest,
    requested_id: &str,
) -> Option<&'a crate::models::SelectionTarget> {
    selection_targets_by_id(manifest, requested_id)
        .into_iter()
        .next()
}

fn feature_bindings_for_target_ids(
    manifest: &ModelManifest,
    target_ids: &[String],
) -> (Vec<String>, Vec<String>) {
    let Some(graph) = manifest.feature_graph.as_ref() else {
        return (Vec::new(), Vec::new());
    };

    let mut feature_ids = Vec::new();
    let mut source_paths = Vec::new();
    for node in &graph.nodes {
        let output_match = node.output_refs.iter().any(|output| {
            output
                .target_ids
                .iter()
                .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
        });
        let port_match = node.ports.iter().any(|port| {
            port.target_ids
                .iter()
                .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
        });
        if !output_match && !port_match {
            continue;
        }

        if !feature_ids
            .iter()
            .any(|existing| existing == &node.feature_id)
        {
            feature_ids.push(node.feature_id.clone());
        }
        if let Some(path) = node
            .source_ref
            .as_ref()
            .and_then(|source_ref| source_ref.path.clone())
        {
            if !path.trim().is_empty() && !source_paths.iter().any(|existing| existing == &path) {
                source_paths.push(path);
            }
        }
        for port in &node.ports {
            let port_hit = port
                .target_ids
                .iter()
                .any(|target_id| target_ids.iter().any(|requested| requested == target_id));
            if !port_hit {
                continue;
            }
            if let Some(path) = port
                .source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.clone())
            {
                if !path.trim().is_empty() && !source_paths.iter().any(|existing| existing == &path)
                {
                    source_paths.push(path);
                }
            }
        }
    }

    (feature_ids, source_paths)
}

fn selection_target_kind_role(kind: &crate::models::SelectionTargetKind) -> String {
    match kind {
        crate::models::SelectionTargetKind::Part => "part".to_string(),
        crate::models::SelectionTargetKind::Object => "object".to_string(),
        crate::models::SelectionTargetKind::Group => "group".to_string(),
        crate::models::SelectionTargetKind::Edge => "edge".to_string(),
        crate::models::SelectionTargetKind::Face => "face".to_string(),
    }
}

fn collect_selector_provenance_candidates(
    manifest: &ModelManifest,
    selected_targets: &[&crate::models::SelectionTarget],
    source: Option<&str>,
) -> EckySelectorResolveProvenanceCandidates {
    let mut source_paths = Vec::new();
    let mut operation_kinds = Vec::new();
    let mut primitive_ids = Vec::new();
    let mut feature_roles = Vec::new();

    for target in selected_targets {
        push_unique_strings(&mut primitive_ids, &target.primitive_ids);

        let feature_role = selection_target_kind_role(&target.kind);
        if !feature_roles
            .iter()
            .any(|existing| existing == &feature_role)
        {
            feature_roles.push(feature_role);
        }

        let target_ids = selection_target_match_ids(target);
        let Some(graph) = manifest.feature_graph.as_ref() else {
            continue;
        };
        for node in &graph.nodes {
            let output_match = node.output_refs.iter().any(|output| {
                output
                    .target_ids
                    .iter()
                    .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
            });
            let port_match = node.ports.iter().any(|port| {
                port.target_ids
                    .iter()
                    .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
            });
            if !output_match && !port_match {
                continue;
            }

            if !node.kind.trim().is_empty()
                && !operation_kinds
                    .iter()
                    .any(|existing| existing == &node.kind)
            {
                operation_kinds.push(node.kind.clone());
            }

            if let Some(path) = node
                .source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.clone())
                .filter(|path| !path.trim().is_empty())
            {
                if !source_paths.iter().any(|existing| existing == &path) {
                    source_paths.push(path);
                }
            }

            for port in &node.ports {
                let port_hit = port
                    .target_ids
                    .iter()
                    .any(|target_id| target_ids.iter().any(|requested| requested == target_id));
                if !port_hit {
                    continue;
                }
                if let Some(path) = port
                    .source_ref
                    .as_ref()
                    .and_then(|source_ref| source_ref.path.clone())
                    .filter(|path| !path.trim().is_empty())
                {
                    if !source_paths.iter().any(|existing| existing == &path) {
                        source_paths.push(path);
                    }
                }
            }
        }
    }

    let mut source_stable_node_keys = Vec::new();
    if let Some(source_text) = source {
        if let Ok(program) = crate::ecky_scheme::compile_to_core_program(source_text) {
            let mut seen = HashSet::new();
            for path in source_paths {
                if let Some(stable_key) =
                    stable_node_key_for_program_path(source_text, &program, &path)
                {
                    let trimmed = stable_key.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                        source_stable_node_keys.push(trimmed.to_string());
                    }
                }
            }
        }
    }

    EckySelectorResolveProvenanceCandidates {
        feature_role: if feature_roles.len() == 1 {
            feature_roles.into_iter().next()
        } else {
            None
        },
        source_stable_node_keys,
        operation_kinds,
        primitive_ids,
    }
}

fn collect_param_reference_paths(
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    param_id: crate::ecky_core_ir::ParamId,
    paths: &mut Vec<String>,
) {
    if matches!(
        &node.kind,
        crate::ecky_core_ir::CoreNodeKind::Reference(
            crate::ecky_core_ir::CoreReference::Parameter(id)
        ) if *id == param_id
    ) {
        paths.push(path.to_string());
    }
    for (child_path, child) in core_node_child_paths(node, path) {
        collect_param_reference_paths(child, &child_path, param_id, paths);
    }
}

fn dependent_source_paths_for_param(
    program: &crate::ecky_core_ir::CoreProgram,
    param_id: crate::ecky_core_ir::ParamId,
) -> Vec<String> {
    let mut paths = Vec::new();
    for part in &program.parts {
        let root_path = format!("/parts/{}/root", path_segment(&part.key));
        collect_param_reference_paths(&part.root, &root_path, param_id, &mut paths);
    }
    paths
}

fn impacted_part_ids_for_dependency_paths(paths: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for path in paths {
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() >= 2 && segments[0] == "parts" {
            let part_id = path_segment_decode(segments[1]);
            if !ids.iter().any(|existing| existing == &part_id) {
                ids.push(part_id);
            }
        }
    }
    ids
}

fn impact_labels_for_dependency(
    impacted_part_ids: &[String],
    reference_count: usize,
) -> Vec<String> {
    if reference_count == 0 {
        return vec!["local".to_string()];
    }
    if impacted_part_ids.is_empty() {
        return vec!["local".to_string()];
    }
    if impacted_part_ids.len() == 1 {
        return vec!["part-local".to_string(), "export-affecting".to_string()];
    }
    vec!["assembly-wide".to_string(), "export-affecting".to_string()]
}

fn param_value_from_core(value: &crate::ecky_core_ir::CoreParameterValue) -> ParamValue {
    match value {
        crate::ecky_core_ir::CoreParameterValue::Number(value) => ParamValue::Number(*value),
        crate::ecky_core_ir::CoreParameterValue::Boolean(value) => ParamValue::Boolean(*value),
        crate::ecky_core_ir::CoreParameterValue::Text(value)
        | crate::ecky_core_ir::CoreParameterValue::Choice(value)
        | crate::ecky_core_ir::CoreParameterValue::Image(value) => {
            ParamValue::String(value.clone())
        }
    }
}

fn param_value_matches_core_choice(
    value: &ParamValue,
    choice: &crate::ecky_core_ir::CoreParameterValue,
) -> bool {
    match (value, choice) {
        (ParamValue::Number(left), crate::ecky_core_ir::CoreParameterValue::Number(right)) => {
            left == right
        }
        (ParamValue::String(left), crate::ecky_core_ir::CoreParameterValue::Choice(right))
        | (ParamValue::String(left), crate::ecky_core_ir::CoreParameterValue::Text(right))
        | (ParamValue::String(left), crate::ecky_core_ir::CoreParameterValue::Image(right)) => {
            left == right
        }
        _ => false,
    }
}

fn effective_ecky_constraint_params(
    program: &crate::ecky_core_ir::CoreProgram,
    design_params: &DesignParams,
    provided_params: Option<DesignParams>,
) -> (DesignParams, String) {
    let mut params = DesignParams::new();
    for param in &program.parameters {
        params.insert(
            param.key.clone(),
            param_value_from_core(&param.default_value),
        );
    }

    match provided_params {
        Some(provided) => {
            for (key, value) in provided {
                params.insert(key, value);
            }
            (params, "provided".to_string())
        }
        None => {
            for (key, value) in design_params {
                params.insert(key.clone(), value.clone());
            }
            (params, "initialOrDefault".to_string())
        }
    }
}

fn validate_ecky_constraint_row(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    param: &crate::ecky_core_ir::CoreParameter,
    value: &ParamValue,
) -> EckyConstraintValidationRow {
    let mut failures = Vec::new();
    let number_value = match (&param.kind, value) {
        (crate::ecky_core_ir::CoreParameterKind::Number, ParamValue::Number(value)) => Some(*value),
        (crate::ecky_core_ir::CoreParameterKind::Number, other) => {
            failures.push(format!("Expected number, got {}.", other.kind()));
            None
        }
        (crate::ecky_core_ir::CoreParameterKind::Boolean, ParamValue::Boolean(_)) => None,
        (crate::ecky_core_ir::CoreParameterKind::Boolean, other) => {
            failures.push(format!("Expected boolean, got {}.", other.kind()));
            None
        }
        (crate::ecky_core_ir::CoreParameterKind::Choice, ParamValue::String(_))
        | (crate::ecky_core_ir::CoreParameterKind::Choice, ParamValue::Number(_))
        | (crate::ecky_core_ir::CoreParameterKind::Text, ParamValue::String(_))
        | (crate::ecky_core_ir::CoreParameterKind::Image, ParamValue::String(_)) => None,
        (
            crate::ecky_core_ir::CoreParameterKind::Choice,
            other @ (ParamValue::Boolean(_) | ParamValue::Null),
        ) => {
            failures.push(format!("Expected choice value, got {}.", other.kind()));
            None
        }
        (
            crate::ecky_core_ir::CoreParameterKind::Text
            | crate::ecky_core_ir::CoreParameterKind::Image,
            other,
        ) => {
            failures.push(format!("Expected string, got {}.", other.kind()));
            None
        }
    };

    if let Some(value) = number_value {
        if let Some(min) = param.constraints.min {
            if value < min {
                failures.push(format!("Value {value} is below min {min}."));
            }
        }
        if let Some(max) = param.constraints.max {
            if value > max {
                failures.push(format!("Value {value} is above max {max}."));
            }
        }
        if let Some(step) = param.constraints.step {
            if !step.is_finite() || step <= 0.0 {
                failures.push(format!("Step constraint {step} is not positive."));
            } else {
                let base = param.constraints.min.unwrap_or(0.0);
                let units = (value - base) / step;
                let nearest = units.round();
                let tolerance = 1e-9_f64.max(units.abs() * 1e-9);
                if (units - nearest).abs() > tolerance {
                    failures.push(format!(
                        "Value {value} does not align to step {step} from base {base}."
                    ));
                }
            }
        }
    }

    if !param.constraints.choices.is_empty()
        && !param
            .constraints
            .choices
            .iter()
            .any(|choice| param_value_matches_core_choice(value, &choice.value))
    {
        let choices = param
            .constraints
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        failures.push(format!("Value is not one of allowed choices: {choices}."));
    }

    let path = format!("/params/{}", path_segment(&param.key));
    let involved_param_keys = vec![param.key.clone()];
    let source_stable_node_keys = stable_node_key_for_program_path(source, program, &path)
        .into_iter()
        .collect::<Vec<_>>();
    let source_stable_node_key = source_stable_node_keys.first().cloned();
    let mut affects_stable_node_keys = source_stable_node_keys.clone();
    for dependency_path in dependent_source_paths_for_param(program, param.id) {
        if let Some(stable_key) =
            stable_node_key_for_program_path(source, program, &dependency_path)
        {
            if !affects_stable_node_keys
                .iter()
                .any(|existing| existing == &stable_key)
            {
                affects_stable_node_keys.push(stable_key);
            }
        }
    }
    let raw_value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if failures.is_empty() {
        EckyConstraintValidationRow {
            path,
            status: "pass".to_string(),
            severity: "info".to_string(),
            raw_value,
            message: "OK.".to_string(),
            constraint_id: Some(format!("param_{}", param.key)),
            label: Some(format!("Parameter `{}` constraints", param.label)),
            kind: Some("parameter".to_string()),
            source_stable_node_key,
            depends_on_param_keys: involved_param_keys.clone(),
            affects_stable_node_keys,
            involved_param_keys,
            source_stable_node_keys,
        }
    } else {
        EckyConstraintValidationRow {
            path,
            status: "fail".to_string(),
            severity: "error".to_string(),
            raw_value,
            message: failures.join(" "),
            constraint_id: Some(format!("param_{}", param.key)),
            label: Some(format!("Parameter `{}` constraints", param.label)),
            kind: Some("parameter".to_string()),
            source_stable_node_key,
            depends_on_param_keys: involved_param_keys.clone(),
            affects_stable_node_keys,
            involved_param_keys,
            source_stable_node_keys,
        }
    }
}

fn validate_ecky_constraints(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    params: &DesignParams,
) -> Vec<EckyConstraintValidationRow> {
    let mut rows = program
        .parameters
        .iter()
        .map(|param| {
            let value = params
                .get(&param.key)
                .cloned()
                .unwrap_or_else(|| param_value_from_core(&param.default_value));
            validate_ecky_constraint_row(source, program, param, &value)
        })
        .collect::<Vec<_>>();
    rows.extend(
        program
            .constraints
            .relations
            .iter()
            .enumerate()
            .map(|(index, relation)| {
                validate_ecky_relation_constraint_row(source, program, params, relation, index)
            }),
    );
    rows
}

fn evaluate_relation_operand(
    program: &crate::ecky_core_ir::CoreProgram,
    params: &DesignParams,
    operand: &crate::ecky_core_ir::CoreRelationOperand,
) -> Result<(f64, Option<String>), String> {
    match operand {
        crate::ecky_core_ir::CoreRelationOperand::Number(value) => Ok((*value, None)),
        crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) => {
            let param = program
                .parameters
                .iter()
                .find(|candidate| candidate.id == *param_id)
                .ok_or_else(|| {
                    format!(
                        "Relation references unknown parameter id {}.",
                        param_id.raw()
                    )
                })?;
            let value = params
                .get(&param.key)
                .cloned()
                .unwrap_or_else(|| param_value_from_core(&param.default_value));
            match value {
                ParamValue::Number(number) => Ok((number, Some(param.key.clone()))),
                other => Err(format!(
                    "Relation operand `{}` expected number, got {}.",
                    param.key,
                    other.kind()
                )),
            }
        }
    }
}

fn validate_ecky_relation_constraint_row(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    params: &DesignParams,
    relation: &crate::ecky_core_ir::CoreRelationConstraint,
    index: usize,
) -> EckyConstraintValidationRow {
    let mut failures = Vec::new();
    let mut involved_param_keys = Vec::new();
    let mut depends_on_param_keys = Vec::new();
    for operand in [&relation.left, &relation.right] {
        let crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) = operand else {
            continue;
        };
        let Some(param_key) = program
            .parameters
            .iter()
            .find(|param| param.id == *param_id)
            .map(|param| param.key.clone())
        else {
            continue;
        };
        if !depends_on_param_keys
            .iter()
            .any(|candidate| candidate == &param_key)
        {
            depends_on_param_keys.push(param_key);
        }
    }

    let left = evaluate_relation_operand(program, params, &relation.left).map_err(|err| {
        failures.push(err);
    });
    let right = evaluate_relation_operand(program, params, &relation.right).map_err(|err| {
        failures.push(err);
    });

    let left_value = left.ok().map(|(value, key)| {
        if let Some(key) = key {
            if !involved_param_keys
                .iter()
                .any(|candidate| candidate == &key)
            {
                involved_param_keys.push(key);
            }
        }
        value
    });
    let right_value = right.ok().map(|(value, key)| {
        if let Some(key) = key {
            if !involved_param_keys
                .iter()
                .any(|candidate| candidate == &key)
            {
                involved_param_keys.push(key);
            }
        }
        value
    });

    if let (Some(left), Some(right)) = (left_value, right_value) {
        let relation_ok = match relation.operator {
            crate::ecky_core_ir::CoreRelationOperator::LessThan => left < right,
            crate::ecky_core_ir::CoreRelationOperator::LessThanOrEqual => left <= right,
            crate::ecky_core_ir::CoreRelationOperator::GreaterThan => left > right,
            crate::ecky_core_ir::CoreRelationOperator::GreaterThanOrEqual => left >= right,
        };
        if !relation_ok {
            failures.push(format!(
                "Relation {} failed: {} !{} {}.",
                relation.operator.as_str(),
                left,
                relation.operator.as_str(),
                right
            ));
        }
    }

    let path = format!("/params/:relations/{index}");
    let mut source_stable_node_keys = stable_node_key_for_program_path(source, program, &path)
        .into_iter()
        .collect::<Vec<_>>();
    let source_stable_node_key = source_stable_node_keys.first().cloned().or_else(|| {
        Some(stable_ast_node_key(
            source,
            &path,
            "RelationConstraint",
            "RelationConstraint",
            Some(relation.operator.as_str()),
            None,
        ))
    });
    if let Some(stable_key) = source_stable_node_key.clone() {
        if !source_stable_node_keys
            .iter()
            .any(|existing| existing == &stable_key)
        {
            source_stable_node_keys.push(stable_key);
        }
    }
    let mut affects_stable_node_keys = source_stable_node_keys.clone();
    for key in &depends_on_param_keys {
        let param_path = format!("/params/{}", path_segment(key));
        if let Some(stable_key) = stable_node_key_for_program_path(source, program, &param_path) {
            if !affects_stable_node_keys
                .iter()
                .any(|existing| existing == &stable_key)
            {
                affects_stable_node_keys.push(stable_key);
            }
        }
    }
    let raw_value = serde_json::json!({
        "operator": relation.operator.as_str(),
        "left": left_value,
        "right": right_value,
    });

    if failures.is_empty() {
        EckyConstraintValidationRow {
            path,
            status: "pass".to_string(),
            severity: "info".to_string(),
            raw_value,
            message: "OK.".to_string(),
            constraint_id: Some(format!("relation_{index}")),
            label: Some(format!("Relation #{}", index + 1)),
            kind: Some("relation".to_string()),
            source_stable_node_key,
            depends_on_param_keys: depends_on_param_keys.clone(),
            affects_stable_node_keys: affects_stable_node_keys.clone(),
            involved_param_keys,
            source_stable_node_keys,
        }
    } else {
        EckyConstraintValidationRow {
            path,
            status: "fail".to_string(),
            severity: "error".to_string(),
            raw_value,
            message: failures.join(" "),
            constraint_id: Some(format!("relation_{index}")),
            label: Some(format!("Relation #{}", index + 1)),
            kind: Some("relation".to_string()),
            source_stable_node_key,
            depends_on_param_keys,
            affects_stable_node_keys,
            involved_param_keys,
            source_stable_node_keys,
        }
    }
}

#[derive(Debug, Clone)]
struct AnonymousDeltaUse {
    part_key: String,
    param_key: String,
    delta: f64,
    path: String,
}

fn collect_anonymous_delta_uses(
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    part_key: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    out: &mut Vec<AnonymousDeltaUse>,
) {
    let mut maybe_record_use = |param_id: crate::ecky_core_ir::ParamId, delta: f64| {
        let Some(param_key) = program
            .parameters
            .iter()
            .find(|param| param.id == param_id)
            .map(|param| param.key.clone())
        else {
            return;
        };
        out.push(AnonymousDeltaUse {
            part_key: part_key.to_string(),
            param_key,
            delta,
            path: path.to_string(),
        });
    };

    if let crate::ecky_core_ir::CoreNodeKind::Call {
        op: crate::ecky_core_ir::CoreOperation::Custom(op_name),
        args,
        ..
    } = &node.kind
    {
        let param_id_from_node = |candidate: &crate::ecky_core_ir::CoreNode| match &candidate.kind {
            crate::ecky_core_ir::CoreNodeKind::Reference(
                crate::ecky_core_ir::CoreReference::Parameter(param_id),
            ) => Some(*param_id),
            _ => None,
        };
        let number_from_node = |candidate: &crate::ecky_core_ir::CoreNode| match &candidate.kind {
            crate::ecky_core_ir::CoreNodeKind::Literal(
                crate::ecky_core_ir::CoreLiteral::Number(value),
            ) => Some(*value),
            _ => None,
        };

        if op_name == "+" && args.len() == 2 {
            match (
                param_id_from_node(&args[0]),
                number_from_node(&args[1]),
                number_from_node(&args[0]),
                param_id_from_node(&args[1]),
            ) {
                (Some(param_id), Some(delta), _, _) => maybe_record_use(param_id, delta),
                (_, _, Some(delta), Some(param_id)) => maybe_record_use(param_id, delta),
                _ => {}
            }
        }
        if op_name == "-" && args.len() == 2 {
            if let (Some(param_id), Some(delta)) =
                (param_id_from_node(&args[0]), number_from_node(&args[1]))
            {
                maybe_record_use(param_id, -delta);
            }
        }
    }

    for (child_path, child) in core_node_child_paths(node, path) {
        collect_anonymous_delta_uses(child, &child_path, part_key, program, out);
    }
}

fn anonymous_delta_suffix_for_param_key(param_key: &str) -> Option<(&'static str, String)> {
    let trimmed = param_key.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(stem) = trimmed.strip_suffix("_w") {
        return Some(("_margin_x", stem.trim_end_matches('_').to_string()));
    }
    if let Some(stem) = trimmed.strip_suffix("_h") {
        return Some(("_margin_y", stem.trim_end_matches('_').to_string()));
    }
    if let Some(stem) = trimmed.strip_suffix("width") {
        return Some(("_margin_x", stem.trim_end_matches('_').to_string()));
    }
    if let Some(stem) = trimmed.strip_suffix("height") {
        return Some(("_margin_y", stem.trim_end_matches('_').to_string()));
    }
    None
}

fn anonymous_delta_numeric_token(value: f64) -> String {
    if value.is_finite() && (value.fract().abs() < 1e-9) {
        if value < 0.0 {
            return format!("neg_{:.0}", value.abs());
        }
        return format!("{:.0}", value);
    }
    let base = value.to_string().replace('.', "_");
    if value < 0.0 {
        format!("neg_{}", base.trim_start_matches('-'))
    } else {
        base
    }
}

fn anonymous_delta_suggested_param_key(param_key: &str, delta: f64) -> String {
    if let Some((suffix, stem)) = anonymous_delta_suffix_for_param_key(param_key) {
        if stem.is_empty() {
            return format!("{param_key}{suffix}");
        }
        return format!("{stem}{suffix}");
    }
    let token = anonymous_delta_numeric_token(delta);
    format!("delta_{param_key}_{token}")
}

pub(super) fn collect_ecky_constraint_authoring_lints(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
) -> Vec<EckyConstraintAuthoringLint> {
    let mut uses = Vec::new();
    for part in &program.parts {
        let root_path = format!("/parts/{}/root", path_segment(&part.key));
        collect_anonymous_delta_uses(&part.root, &root_path, &part.key, program, &mut uses);
    }

    let mut grouped: HashMap<(String, String, u64), Vec<&AnonymousDeltaUse>> = HashMap::new();
    for usage in &uses {
        grouped
            .entry((
                usage.part_key.clone(),
                usage.param_key.clone(),
                usage.delta.to_bits(),
            ))
            .or_default()
            .push(usage);
    }

    let mut lints = Vec::new();
    for ((part_key, param_key, delta_bits), group_uses) in grouped {
        if group_uses.len() < 2 {
            continue;
        }
        let delta = f64::from_bits(delta_bits);
        let occurrence_count = group_uses.len();
        let suggested_param_key = anonymous_delta_suggested_param_key(&param_key, delta);
        let mut source_stable_node_keys = Vec::new();
        for usage in group_uses {
            if let Some(stable_key) = stable_node_key_for_program_path(source, program, &usage.path)
            {
                if !source_stable_node_keys
                    .iter()
                    .any(|existing| existing == &stable_key)
                {
                    source_stable_node_keys.push(stable_key);
                }
            }
        }
        lints.push(EckyConstraintAuthoringLint {
            kind: "anonymousDelta".to_string(),
            part_key: part_key.clone(),
            param_key: param_key.clone(),
            delta,
            occurrence_count,
            suggested_param_key: suggested_param_key.clone(),
            message: format!(
                "Repeated anonymous delta on `{param_key}` in part `{part_key}`. Extract `{suggested_param_key}` parameter and reuse."
            ),
            source_stable_node_keys,
        });
    }

    lints.sort_by(|left, right| {
        left.part_key
            .cmp(&right.part_key)
            .then(left.param_key.cmp(&right.param_key))
            .then(left.suggested_param_key.cmp(&right.suggested_param_key))
    });
    lints
}

fn source_addressable_digest_for_path(
    program: &crate::ecky_core_ir::CoreProgram,
    requested_path: &str,
) -> Option<String> {
    let segments = requested_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    if segments.len() == 2 && segments[0] == "params" {
        return program
            .parameters
            .iter()
            .find(|param| param.key == segments[1])
            .map(core_param_digest);
    }
    if segments.len() == 2 && segments[0] == "parts" {
        return program
            .parts
            .iter()
            .find(|part| part.key == segments[1])
            .map(core_part_digest);
    }
    find_core_ast_node_in_program(program, requested_path).map(core_node_digest)
}

fn raw_source_target_digest_for_path(source: &str, requested_path: &str) -> AppResult<String> {
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, requested_path)?;
    Ok(crate::mcp::macro_buffer::source_digest(&format!(
        "{:?}|{}",
        target.kind,
        &source[target.expr.start..target.expr.end]
    )))
}

pub(super) fn edit_digest_for_ecky_path(
    program: &crate::ecky_core_ir::CoreProgram,
    source: &str,
    requested_path: &str,
) -> AppResult<String> {
    Ok(source_addressable_digest_for_path(program, requested_path)
        .unwrap_or(raw_source_target_digest_for_path(source, requested_path)?))
}

#[derive(Debug, Clone)]
struct SourceExprSpan {
    start: usize,
    end: usize,
    children: Vec<SourceExprSpan>,
}

impl SourceExprSpan {
    fn atom_text<'a>(&self, source: &'a str) -> Option<&'a str> {
        if self.children.is_empty() {
            Some(&source[self.start..self.end])
        } else {
            None
        }
    }
}

struct SourceExprParser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> SourceExprParser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn parse_all(mut self) -> AppResult<Vec<SourceExprSpan>> {
        let mut exprs = Vec::new();
        while self.skip_ws_and_comments() < self.source.len() {
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    fn skip_ws_and_comments(&mut self) -> usize {
        while self.cursor < self.source.len() {
            let rest = &self.source[self.cursor..];
            if rest.starts_with(';') {
                while self.cursor < self.source.len()
                    && !self.source[self.cursor..].starts_with('\n')
                {
                    self.cursor += 1;
                }
            } else if let Some(ch) = rest.chars().next() {
                if ch.is_whitespace() {
                    self.cursor += ch.len_utf8();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        self.cursor
    }

    fn parse_expr(&mut self) -> AppResult<SourceExprSpan> {
        self.skip_ws_and_comments();
        if self.cursor >= self.source.len() {
            return Err(AppError::validation("Unexpected end of Ecky source."));
        }
        if self.source[self.cursor..].starts_with('(') {
            return self.parse_list();
        }
        if self.source[self.cursor..].starts_with('"') {
            return self.parse_string();
        }
        Ok(self.parse_atom())
    }

    fn parse_list(&mut self) -> AppResult<SourceExprSpan> {
        let start = self.cursor;
        self.cursor += 1;
        let mut children = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.cursor >= self.source.len() {
                return Err(AppError::validation("Unclosed list in Ecky source."));
            }
            if self.source[self.cursor..].starts_with(')') {
                self.cursor += 1;
                return Ok(SourceExprSpan {
                    start,
                    end: self.cursor,
                    children,
                });
            }
            children.push(self.parse_expr()?);
        }
    }

    fn parse_string(&mut self) -> AppResult<SourceExprSpan> {
        let start = self.cursor;
        self.cursor += 1;
        let mut escaped = false;
        while self.cursor < self.source.len() {
            let ch = self.source[self.cursor..]
                .chars()
                .next()
                .ok_or_else(|| AppError::validation("Unclosed string in Ecky source."))?;
            self.cursor += ch.len_utf8();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Ok(SourceExprSpan {
                    start,
                    end: self.cursor,
                    children: Vec::new(),
                });
            }
        }
        Err(AppError::validation("Unclosed string in Ecky source."))
    }

    fn parse_atom(&mut self) -> SourceExprSpan {
        let start = self.cursor;
        while self.cursor < self.source.len() {
            let ch = self.source[self.cursor..].chars().next().unwrap();
            if ch.is_whitespace() || ch == '(' || ch == ')' || ch == ';' {
                break;
            }
            self.cursor += ch.len_utf8();
        }
        SourceExprSpan {
            start,
            end: self.cursor,
            children: Vec::new(),
        }
    }
}

fn path_segment_decode(value: &str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}

fn list_head<'a>(expr: &'a SourceExprSpan, source: &'a str) -> Option<&'a str> {
    expr.children
        .first()
        .and_then(|head| head.atom_text(source))
}

fn source_positional_arg<'a>(
    expr: &'a SourceExprSpan,
    source: &str,
    index: usize,
) -> Option<&'a SourceExprSpan> {
    let mut positional = 0usize;
    let mut idx = 1usize;
    while idx < expr.children.len() {
        let child = &expr.children[idx];
        if child
            .atom_text(source)
            .is_some_and(|text| text.starts_with(':'))
        {
            idx += 2;
            continue;
        }
        if positional == index {
            return Some(child);
        }
        positional += 1;
        idx += 1;
    }
    None
}

fn source_keyword_value<'a>(
    expr: &'a SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<&'a SourceExprSpan> {
    let expected = format!(":{name}");
    expr.children.windows(2).find_map(|pair| {
        if pair[0].atom_text(source) == Some(expected.as_str()) {
            Some(&pair[1])
        } else {
            None
        }
    })
}

fn source_keyword_pair_span(
    expr: &SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<(usize, usize)> {
    let expected = format!(":{name}");
    expr.children.windows(2).find_map(|pair| {
        if pair[0].atom_text(source) == Some(expected.as_str()) {
            Some((pair[0].start, pair[1].end))
        } else {
            None
        }
    })
}

fn model_form<'a>(exprs: &'a [SourceExprSpan], source: &str) -> AppResult<&'a SourceExprSpan> {
    exprs
        .iter()
        .find(|expr| list_head(expr, source) == Some("model"))
        .ok_or_else(|| AppError::validation("Ecky source has no model form."))
}

fn model_part_clause<'a>(
    model: &'a SourceExprSpan,
    source: &str,
    part_key: &str,
) -> Option<&'a SourceExprSpan> {
    model.children.iter().find(|expr| {
        list_head(expr, source) == Some("part")
            && expr.children.get(1).and_then(|item| item.atom_text(source)) == Some(part_key)
    })
}

fn model_params_form<'a>(model: &'a SourceExprSpan, source: &str) -> Option<&'a SourceExprSpan> {
    model
        .children
        .iter()
        .find(|expr| list_head(expr, source) == Some("params"))
}

fn model_param_decl<'a>(
    params: &'a SourceExprSpan,
    source: &str,
    key: &str,
) -> Option<&'a SourceExprSpan> {
    params
        .children
        .iter()
        .skip(1)
        .find(|expr| expr.children.get(1).and_then(|item| item.atom_text(source)) == Some(key))
}

fn build_shape_clause<'a>(
    build: &'a SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<&'a SourceExprSpan> {
    build.children.iter().skip(1).find(|expr| {
        list_head(expr, source) == Some("shape")
            && expr.children.get(1).and_then(|item| item.atom_text(source)) == Some(name)
    })
}

fn build_result_clause<'a>(build: &'a SourceExprSpan, source: &str) -> Option<&'a SourceExprSpan> {
    build
        .children
        .iter()
        .skip(1)
        .find(|expr| list_head(expr, source) == Some("result"))
}

fn let_binding_pair<'a>(
    let_expr: &'a SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<&'a SourceExprSpan> {
    let_expr.children.get(1).and_then(|bindings| {
        bindings.children.iter().find(|pair| {
            let Some(raw_name) = pair
                .children
                .first()
                .and_then(|item| item.atom_text(source))
            else {
                return false;
            };
            raw_name == name || name.contains(raw_name)
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourcePathTargetKind {
    Root,
    PositionalArg,
    KeywordValue { name: String },
    PartClause { name: String },
    ParamDecl { name: String },
    BuildBinding { name: String },
    BuildResult,
    LetBinding { name: String },
    LetBody,
}

struct SourcePathTarget<'a> {
    expr: &'a SourceExprSpan,
    parent: Option<&'a SourceExprSpan>,
    scope: Option<&'a SourceExprSpan>,
    kind: SourcePathTargetKind,
}

fn source_target_for_ecky_path<'a>(
    exprs: &'a [SourceExprSpan],
    source: &str,
    path: &str,
) -> AppResult<SourcePathTarget<'a>> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    let model = model_form(exprs, source)?;
    if segments.len() == 2 && segments[0] == "params" {
        let params = model_params_form(model, source)
            .ok_or_else(|| AppError::validation("Ecky source has no params form."))?;
        let param = model_param_decl(params, source, &segments[1]).ok_or_else(|| {
            AppError::validation(format!("Ecky source has no param {}.", segments[1]))
        })?;
        return Ok(SourcePathTarget {
            expr: param,
            parent: Some(params),
            scope: Some(model),
            kind: SourcePathTargetKind::ParamDecl {
                name: segments[1].clone(),
            },
        });
    }
    if segments.len() == 2 && segments[0] == "parts" {
        let part = model_part_clause(model, source, &segments[1]).ok_or_else(|| {
            AppError::validation(format!("Ecky source has no part {}.", segments[1]))
        })?;
        return Ok(SourcePathTarget {
            expr: part,
            parent: Some(model),
            scope: Some(model),
            kind: SourcePathTargetKind::PartClause {
                name: segments[1].clone(),
            },
        });
    }
    if segments.len() < 3 || segments[0] != "parts" || segments[2] != "root" {
        return Err(AppError::validation(format!(
            "Ecky AST path is not source-span addressable in v1: {path}."
        )));
    }
    let part_key = &segments[1];
    let part = model_part_clause(model, source, part_key)
        .ok_or_else(|| AppError::validation(format!("Ecky source has no part {part_key}.")))?;
    let mut current = part
        .children
        .get(2)
        .ok_or_else(|| AppError::validation(format!("Ecky part {part_key} has no root node.")))?;
    let mut parent = None;
    let mut scope = Some(part);
    let mut kind = SourcePathTargetKind::Root;
    let mut idx = 3usize;
    while idx < segments.len() {
        match segments.get(idx).map(String::as_str) {
            Some("build") if list_head(current, source) == Some("build") => {
                match segments.get(idx + 1).map(String::as_str) {
                    Some("bindings") => {
                        let name = segments.get(idx + 2).ok_or_else(|| {
                            AppError::validation(format!("Invalid Ecky AST path: {path}."))
                        })?;
                        let shape = build_shape_clause(current, source, name).ok_or_else(|| {
                            AppError::validation(format!(
                                "Ecky AST source build binding not found: {path}."
                            ))
                        })?;
                        parent = Some(shape);
                        scope = Some(current);
                        current = shape.children.get(2).ok_or_else(|| {
                            AppError::validation(format!("Ecky build binding {name} has no value."))
                        })?;
                        kind = SourcePathTargetKind::BuildBinding {
                            name: name.to_string(),
                        };
                        idx += 3;
                    }
                    Some("result") => {
                        let result = build_result_clause(current, source).ok_or_else(|| {
                            AppError::validation(format!(
                                "Ecky AST source build result not found: {path}."
                            ))
                        })?;
                        parent = Some(result);
                        scope = Some(current);
                        current = result.children.get(1).ok_or_else(|| {
                            AppError::validation("Ecky build result has no value.")
                        })?;
                        kind = SourcePathTargetKind::BuildResult;
                        idx += 2;
                    }
                    _ => {
                        return Err(AppError::validation(format!(
                            "Ecky AST path is not source-span addressable in v1: {path}."
                        )));
                    }
                }
            }
            Some("let")
                if list_head(current, source) == Some("let")
                    || list_head(current, source) == Some("let*") =>
            {
                match segments.get(idx + 1).map(String::as_str) {
                    Some("bindings") => {
                        let name = segments.get(idx + 2).ok_or_else(|| {
                            AppError::validation(format!("Invalid Ecky AST path: {path}."))
                        })?;
                        let binding = let_binding_pair(current, source, name).ok_or_else(|| {
                            AppError::validation(format!(
                                "Ecky AST source let binding not found: {path}."
                            ))
                        })?;
                        parent = Some(binding);
                        scope = Some(current);
                        current = binding.children.get(1).ok_or_else(|| {
                            AppError::validation(format!("Ecky let binding {name} has no value."))
                        })?;
                        let raw_name = binding
                            .children
                            .first()
                            .and_then(|item| item.atom_text(source))
                            .unwrap_or(name);
                        kind = SourcePathTargetKind::LetBinding {
                            name: raw_name.to_string(),
                        };
                        idx += 3;
                    }
                    Some("body") => {
                        parent = Some(current);
                        scope = Some(current);
                        current = current
                            .children
                            .get(2)
                            .ok_or_else(|| AppError::validation("Ecky let form has no body."))?;
                        kind = SourcePathTargetKind::LetBody;
                        idx += 2;
                    }
                    _ => {
                        return Err(AppError::validation(format!(
                            "Ecky AST path is not source-span addressable in v1: {path}."
                        )));
                    }
                }
            }
            Some("call") if segments.get(idx + 1).map(String::as_str) == Some("args") => {
                let arg_index = segments
                    .get(idx + 2)
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| {
                        AppError::validation(format!("Invalid Ecky AST path: {path}."))
                    })?;
                parent = Some(current);
                scope = Some(current);
                current = source_positional_arg(current, source, arg_index).ok_or_else(|| {
                    AppError::validation(format!("Ecky AST source arg path not found: {path}."))
                })?;
                kind = SourcePathTargetKind::PositionalArg;
                idx += 3;
            }
            Some("call") if segments.get(idx + 1).map(String::as_str) == Some("keywords") => {
                let keyword = segments.get(idx + 2).ok_or_else(|| {
                    AppError::validation(format!("Invalid Ecky AST path: {path}."))
                })?;
                parent = Some(current);
                scope = Some(current);
                current = source_keyword_value(current, source, keyword).ok_or_else(|| {
                    AppError::validation(format!("Ecky AST source keyword path not found: {path}."))
                })?;
                kind = SourcePathTargetKind::KeywordValue {
                    name: keyword.to_string(),
                };
                idx += 3;
            }
            _ => {
                return Err(AppError::validation(format!(
                    "Ecky AST path is not source-span addressable in v1: {path}."
                )));
            }
        }
    }
    Ok(SourcePathTarget {
        expr: current,
        parent,
        scope,
        kind,
    })
}

fn source_span_for_ecky_path(source: &str, path: &str) -> AppResult<(usize, usize)> {
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    Ok((target.expr.start, target.expr.end))
}

fn source_anchor_span_for_edit(source: &str, path: &str) -> AppResult<(usize, usize)> {
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    match (&target.kind, target.parent) {
        (SourcePathTargetKind::KeywordValue { name }, Some(parent)) => {
            source_keyword_pair_span(parent, source, name).ok_or_else(|| {
                AppError::validation(format!("Ecky AST source keyword pair not found: {path}."))
            })
        }
        (
            SourcePathTargetKind::BuildBinding { .. }
            | SourcePathTargetKind::BuildResult
            | SourcePathTargetKind::LetBinding { .. },
            Some(parent),
        ) => Ok((parent.start, parent.end)),
        _ => Ok((target.expr.start, target.expr.end)),
    }
}

fn expand_delete_span(source: &str, start: usize, end: usize) -> (usize, usize) {
    if end < source.len() {
        if let Some(ch) = source[end..].chars().next() {
            if ch.is_whitespace() {
                return (start, end + ch.len_utf8());
            }
        }
    }
    if start > 0 {
        if let Some((prev_start, ch)) = source[..start].char_indices().last() {
            if ch.is_whitespace() {
                return (prev_start, end);
            }
        }
    }
    (start, end)
}

fn validate_ecky_identifier(name: &str) -> AppResult<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.starts_with(':')
        || trimmed
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' || ch == ';')
    {
        return Err(AppError::validation(format!(
            "Invalid Ecky identifier for rename: {name}."
        )));
    }
    Ok(trimmed)
}

fn collect_identifier_spans(
    expr: &SourceExprSpan,
    source: &str,
    name: &str,
    spans: &mut Vec<(usize, usize)>,
) {
    if let Some(text) = expr.atom_text(source) {
        if text == name {
            spans.push((expr.start, expr.end));
        }
        return;
    }
    for child in &expr.children {
        collect_identifier_spans(child, source, name, spans);
    }
}

fn rewrite_ranges(source: &str, ranges: &[(usize, usize)], replacement: &str) -> AppResult<String> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|(start, _)| *start);
    for pair in sorted.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(AppError::validation("Overlapping Ecky AST rename ranges."));
        }
    }
    let mut next = source.to_string();
    for (start, end) in sorted.into_iter().rev() {
        if start >= end
            || end > next.len()
            || !next.is_char_boundary(start)
            || !next.is_char_boundary(end)
        {
            return Err(AppError::validation(format!(
                "Invalid Ecky AST rename range {start}..{end}."
            )));
        }
        next.replace_range(start..end, replacement);
    }
    Ok(next)
}

fn collect_identifier_spans_excluding_shadowed_lets(
    expr: &SourceExprSpan,
    source: &str,
    name: &str,
    spans: &mut Vec<(usize, usize)>,
) {
    if let Some(text) = expr.atom_text(source) {
        if text == name {
            spans.push((expr.start, expr.end));
        }
        return;
    }

    let head = list_head(expr, source);
    if matches!(head, Some("let") | Some("let*")) {
        let mut shadows_name = false;
        if let Some(bindings) = expr.children.get(1) {
            for binding in &bindings.children {
                if binding
                    .children
                    .first()
                    .and_then(|item| item.atom_text(source))
                    == Some(name)
                {
                    shadows_name = true;
                }
                for child in binding.children.iter().skip(1) {
                    collect_identifier_spans_excluding_shadowed_lets(child, source, name, spans);
                }
            }
        }
        if !shadows_name {
            for child in expr.children.iter().skip(2) {
                collect_identifier_spans_excluding_shadowed_lets(child, source, name, spans);
            }
        }
        return;
    }

    for child in &expr.children {
        collect_identifier_spans_excluding_shadowed_lets(child, source, name, spans);
    }
}

fn rename_ecky_source_target(source: &str, path: &str, new_name: &str) -> AppResult<String> {
    let new_name = validate_ecky_identifier(new_name)?;
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    let mut ranges = Vec::new();
    match (&target.kind, target.parent, target.scope) {
        (SourcePathTargetKind::BuildBinding { name }, Some(shape), Some(build)) => {
            if build_shape_clause(build, source, new_name).is_some() {
                return Err(AppError::validation(format!(
                    "Ecky build binding {new_name} already exists."
                )));
            }
            let name_atom = shape.children.get(1).ok_or_else(|| {
                AppError::validation(format!("Ecky build binding {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
            let shape_index = build
                .children
                .iter()
                .position(|child| child.start == shape.start && child.end == shape.end)
                .ok_or_else(|| AppError::validation("Ecky build binding parent not found."))?;
            for child in build.children.iter().skip(shape_index + 1) {
                collect_identifier_spans(child, source, name, &mut ranges);
            }
        }
        (SourcePathTargetKind::LetBinding { name }, Some(binding), Some(let_expr)) => {
            let duplicate = let_expr
                .children
                .get(1)
                .map(|bindings| {
                    bindings.children.iter().any(|pair| {
                        pair.children
                            .first()
                            .and_then(|item| item.atom_text(source))
                            == Some(new_name)
                    })
                })
                .unwrap_or(false);
            if duplicate {
                return Err(AppError::validation(format!(
                    "Ecky let binding {new_name} already exists."
                )));
            }
            let name_atom = binding.children.first().ok_or_else(|| {
                AppError::validation(format!("Ecky let binding {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
            let body = let_expr
                .children
                .get(2)
                .ok_or_else(|| AppError::validation("Ecky let form has no body."))?;
            collect_identifier_spans(body, source, name, &mut ranges);
        }
        (SourcePathTargetKind::PartClause { name }, Some(_), Some(model)) => {
            if model_part_clause(model, source, new_name).is_some() {
                return Err(AppError::validation(format!(
                    "Ecky part {new_name} already exists."
                )));
            }
            let part = target.expr;
            let name_atom = part.children.get(1).ok_or_else(|| {
                AppError::validation(format!("Ecky part {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
        }
        (SourcePathTargetKind::ParamDecl { name }, Some(_), Some(model)) => {
            let params = model_params_form(model, source)
                .ok_or_else(|| AppError::validation("Ecky source has no params form."))?;
            if model_param_decl(params, source, new_name).is_some() {
                return Err(AppError::validation(format!(
                    "Ecky param {new_name} already exists."
                )));
            }
            let param = target.expr;
            let name_atom = param.children.get(1).ok_or_else(|| {
                AppError::validation(format!("Ecky param {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
            for child in &model.children {
                if child.start == params.start && child.end == params.end {
                    continue;
                }
                collect_identifier_spans_excluding_shadowed_lets(child, source, name, &mut ranges);
            }
        }
        _ => {
            return Err(AppError::validation(format!(
                "Ecky AST rename is not supported for path: {path}."
            )));
        }
    }
    let next_source = rewrite_ranges(source, &ranges, new_name)?;
    crate::ecky_scheme::compile_to_core_program(&next_source).map_err(|err| {
        compile_error_with_diagnostics(
            format!("Rename produced invalid Ecky source at {path}: {err}"),
            &next_source,
            err,
            Some("rename"),
            None,
        )
    })?;
    Ok(next_source)
}

pub(super) fn replace_ecky_ast_source(
    source: &str,
    expected_source_digest: &str,
    path: &str,
    expected_node_digest: &str,
    operation: &EckyAstEditOperation,
    replacement_source: Option<&str>,
    new_name: Option<&str>,
) -> AppResult<String> {
    crate::mcp::macro_buffer::assert_expected_digest(source, expected_source_digest)?;
    let operation_name = ecky_ast_operation_name(operation);
    let program = crate::ecky_scheme::compile_to_core_program(source).map_err(|err| {
        compile_error_with_diagnostics(
            format!("Failed to compile Ecky source: {err}"),
            source,
            err,
            Some(operation_name),
            None,
        )
    })?;
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    let target_kind = target.kind.clone();
    let node = find_core_ast_node_in_program(&program, path);
    let diagnostic_stable_node_key = stable_node_key_for_program_path(source, &program, path);
    let actual_node_digest = edit_digest_for_ecky_path(&program, source, path)?;
    if actual_node_digest != expected_node_digest {
        return Err(AppError::validation(format!(
            "Ecky AST node digest mismatch at {path}: expected {expected_node_digest}, actual {actual_node_digest}."
        )));
    }
    if matches!(operation, EckyAstEditOperation::Rename) {
        let new_name = new_name
            .or(replacement_source)
            .ok_or_else(|| AppError::validation("newName is required for Ecky AST rename."))?;
        return rename_ecky_source_target(source, path, new_name);
    }

    let replacement = match operation {
        EckyAstEditOperation::Replace
        | EckyAstEditOperation::InsertBefore
        | EckyAstEditOperation::InsertAfter => replacement_source
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::validation("replacementSource is required for Ecky AST replace/insert.")
            })?,
        EckyAstEditOperation::Delete => "",
        EckyAstEditOperation::Rename => unreachable!("rename returned above"),
    };

    let (start, end) = match operation {
        EckyAstEditOperation::Replace => {
            let core_span = if matches!(
                target_kind,
                SourcePathTargetKind::Root
                    | SourcePathTargetKind::PositionalArg
                    | SourcePathTargetKind::KeywordValue { .. }
            ) {
                node.and_then(|node| node.span)
                    .map(|span| (span.start as usize, span.end as usize))
            } else {
                None
            };
            match core_span {
                Some((start, end))
                    if start < end
                        && end <= source.len()
                        && source.is_char_boundary(start)
                        && source.is_char_boundary(end) =>
                {
                    (start, end)
                }
                _ => source_span_for_ecky_path(source, path)?,
            }
        }
        EckyAstEditOperation::InsertBefore
        | EckyAstEditOperation::InsertAfter
        | EckyAstEditOperation::Delete => source_anchor_span_for_edit(source, path)?,
        EckyAstEditOperation::Rename => unreachable!("rename returned above"),
    };
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return Err(AppError::validation(format!(
            "Ecky AST node at {path} has invalid source span {start}..{end}."
        )));
    }

    let next_source = match operation {
        EckyAstEditOperation::Replace => {
            let mut next_source = String::with_capacity(
                source
                    .len()
                    .saturating_sub(end - start)
                    .saturating_add(replacement.len()),
            );
            next_source.push_str(&source[..start]);
            next_source.push_str(replacement);
            next_source.push_str(&source[end..]);
            next_source
        }
        EckyAstEditOperation::InsertBefore => {
            let inserted = format!("{replacement} ");
            let mut next_source = String::with_capacity(source.len() + inserted.len());
            next_source.push_str(&source[..start]);
            next_source.push_str(&inserted);
            next_source.push_str(&source[start..]);
            next_source
        }
        EckyAstEditOperation::InsertAfter => {
            let inserted = format!(" {replacement}");
            let mut next_source = String::with_capacity(source.len() + inserted.len());
            next_source.push_str(&source[..end]);
            next_source.push_str(&inserted);
            next_source.push_str(&source[end..]);
            next_source
        }
        EckyAstEditOperation::Delete => {
            let exprs = SourceExprParser::new(source).parse_all()?;
            let target = source_target_for_ecky_path(&exprs, source, path)?;
            if matches!(target.kind, SourcePathTargetKind::Root) {
                return Err(AppError::validation(
                    "Deleting a part root is not supported by Ecky AST v1.",
                ));
            }
            let (delete_start, delete_end) = expand_delete_span(source, start, end);
            let mut next_source =
                String::with_capacity(source.len().saturating_sub(delete_end - delete_start));
            next_source.push_str(&source[..delete_start]);
            next_source.push_str(&source[delete_end..]);
            next_source
        }
        EckyAstEditOperation::Rename => unreachable!("rename returned above"),
    };
    crate::ecky_scheme::compile_to_core_program(&next_source).map_err(|err| {
        compile_error_with_diagnostics(
            format!("Replacement produced invalid Ecky source at {path}: {err}"),
            &next_source,
            err,
            Some(operation_name),
            diagnostic_stable_node_key.as_deref(),
        )
    })?;
    Ok(next_source)
}

fn text_line_len(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count().max(1)
    }
}

fn ecky_ast_patch_diff_side(source: &str, start: usize, end: usize) -> EckyAstPatchTextDiffSide {
    EckyAstPatchTextDiffSide {
        digest: crate::mcp::macro_buffer::source_digest(&source[start..end]),
        byte_len: end - start,
        line_len: text_line_len(&source[start..end]),
        span: EckyAstSpan {
            start: start as u32,
            end: end as u32,
        },
    }
}

fn patch_diff_spans(before: &str, after: &str) -> ((usize, usize), (usize, usize)) {
    let before_bytes = before.as_bytes();
    let after_bytes = after.as_bytes();

    let mut prefix = 0usize;
    let min_len = before_bytes.len().min(after_bytes.len());
    while prefix < min_len && before_bytes[prefix] == after_bytes[prefix] {
        prefix += 1;
    }

    let mut before_suffix = before_bytes.len();
    let mut after_suffix = after_bytes.len();
    while before_suffix > prefix
        && after_suffix > prefix
        && before_bytes[before_suffix - 1] == after_bytes[after_suffix - 1]
    {
        before_suffix -= 1;
        after_suffix -= 1;
    }

    ((prefix, before_suffix), (prefix, after_suffix))
}

fn renamed_path(path: &str, new_name: &str) -> String {
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(last) = segments.last_mut() {
        *last = path_segment(new_name);
    }
    format!("/{}", segments.join("/"))
}

fn validate_ecky_ast_patch(
    source: &str,
    expected_source_digest: &str,
    path: &str,
    expected_node_digest: &str,
    operation: &EckyAstEditOperation,
    replacement_source: Option<&str>,
    new_name: Option<&str>,
) -> AppResult<(String, String, String, EckyAstPatchDiff)> {
    let next_source = replace_ecky_ast_source(
        source,
        expected_source_digest,
        path,
        expected_node_digest,
        operation,
        replacement_source,
        new_name,
    )?;
    let next_program =
        crate::ecky_scheme::compile_to_core_program(&next_source).map_err(|err| {
            compile_error_with_diagnostics(
                format!("Failed to compile Ecky source: {err}"),
                &next_source,
                err,
                Some(ecky_ast_operation_name(operation)),
                None,
            )
        })?;
    let new_path = match operation {
        EckyAstEditOperation::Rename => new_name.map(|name| renamed_path(path, name)),
        EckyAstEditOperation::Delete => None,
        _ => Some(path.to_string()),
    };
    let new_node_digest = new_path
        .as_deref()
        .and_then(|next_path| {
            edit_digest_for_ecky_path(&next_program, &next_source, next_path).ok()
        })
        .unwrap_or_else(|| "deleted".to_string());
    let ((old_start, old_end), (new_start, new_end)) = patch_diff_spans(source, &next_source);
    let diff = EckyAstPatchDiff {
        old: ecky_ast_patch_diff_side(source, old_start, old_end),
        new: ecky_ast_patch_diff_side(&next_source, new_start, new_end),
    };
    Ok((
        next_source,
        new_node_digest,
        new_path.unwrap_or_default(),
        diff,
    ))
}
