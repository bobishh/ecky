use std::collections::BTreeMap;
use std::fs;

use crate::contracts::{AppError, AppResult, DesignParams, ParamValue};
use crate::ecky_cad_host::svg_profile::{parse_svg_profile, SvgFitMode};
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBinding, CoreKeywordArg, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation,
    CorePart, CorePrimitive, CoreProgram, CoreShapeBinding, CoreSymbol, CoreValueKind, NodeId,
    SourceSpan,
};

pub fn normalize_core_program_for_direct_occt(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<CoreProgram> {
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let env = crate::ecky_ir::build_core_program_param_env_for_eval(program, parameters)?;
    let mut next_node_id = next_program_node_id(program);

    let parts = program
        .parts
        .iter()
        .map(|part| {
            Ok(CorePart {
                id: part.id,
                key: part.key.clone(),
                label: part.label.clone(),
                root: normalize_node_for_direct_occt(
                    &part.root,
                    &param_names,
                    &env,
                    &mut next_node_id,
                )?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(CoreProgram {
        id: program.id,
        parameters: program.parameters.clone(),
        parts,
        feature_decls: program.feature_decls.clone(),
        constraints: program.constraints.clone(),
    })
}

fn normalize_node_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    match &node.kind {
        CoreNodeKind::Literal(_) => Ok(node.clone()),
        CoreNodeKind::Reference(_) => Ok(node.clone()),
        CoreNodeKind::Build { bindings, result } => {
            let mut nested_env = env.clone();
            let normalized_bindings = bindings
                .iter()
                .map(|binding| {
                    let value = normalize_node_for_direct_occt(
                        &binding.value,
                        param_names,
                        &nested_env,
                        next_node_id,
                    )?;
                    if let Some(value) = eval_scalar_binding(&value, param_names, &nested_env)? {
                        nested_env.insert(binding.name.clone(), value);
                    }
                    Ok(CoreShapeBinding {
                        name: binding.name.clone(),
                        value,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok(rebuild_node(
                node,
                CoreNodeKind::Build {
                    bindings: normalized_bindings,
                    result: Box::new(normalize_node_for_direct_occt(
                        result,
                        param_names,
                        &nested_env,
                        next_node_id,
                    )?),
                },
            ))
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested_env = env.clone();
            let normalized_bindings = bindings
                .iter()
                .map(|binding| {
                    let value = normalize_node_for_direct_occt(
                        &binding.value,
                        param_names,
                        &nested_env,
                        next_node_id,
                    )?;
                    let literal_value = if let Some(param_value) =
                        eval_scalar_binding(&value, param_names, &nested_env)?
                    {
                        nested_env.insert(binding.name.clone(), param_value.clone());
                        param_value_literal_node(next_node_id, &param_value, value.span)
                    } else {
                        value
                    };
                    Ok(CoreBinding {
                        name: binding.name.clone(),
                        value: literal_value,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok(rebuild_node(
                node,
                CoreNodeKind::Let {
                    bindings: normalized_bindings,
                    body: Box::new(normalize_node_for_direct_occt(
                        body,
                        param_names,
                        &nested_env,
                        next_node_id,
                    )?),
                },
            ))
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let normalized_condition =
                normalize_node_for_direct_occt(condition, param_names, env, next_node_id)?;
            match crate::ecky_ir::eval_core_bool_with_locals(
                &normalized_condition,
                param_names,
                env,
            ) {
                Ok(true) => {
                    normalize_node_for_direct_occt(then_branch, param_names, env, next_node_id)
                }
                Ok(false) => {
                    normalize_node_for_direct_occt(else_branch, param_names, env, next_node_id)
                }
                Err(_) => Ok(rebuild_node(
                    node,
                    CoreNodeKind::If {
                        condition: Box::new(normalized_condition),
                        then_branch: Box::new(normalize_node_for_direct_occt(
                            then_branch,
                            param_names,
                            env,
                            next_node_id,
                        )?),
                        else_branch: Box::new(normalize_node_for_direct_occt(
                            else_branch,
                            param_names,
                            env,
                            next_node_id,
                        )?),
                    },
                )),
            }
        }
        CoreNodeKind::Call { op, args, keywords } => match op {
            CoreOperation::Array(CoreArrayOp::Repeat) => {
                if args.len() != 3 {
                    return Err(AppError::validation(
                        "`repeat` expects an index symbol, a count, and a body.".to_string(),
                    ));
                }
                let index = extract_repeat_index(&args[0])?;
                let count = finite_repeat_count(&args[1], param_names, env, "`repeat` count")?;
                let body = &args[2];
                let mut items = Vec::new();
                for iteration in 0..count {
                    let mut loop_env = env.clone();
                    loop_env.insert(index.clone(), ParamValue::Number(iteration as f64));
                    let normalized_body =
                        normalize_node_for_direct_occt(body, param_names, &loop_env, next_node_id)?;
                    items.push(rewrap_with_index(
                        node,
                        &index,
                        iteration as f64,
                        normalized_body,
                        next_node_id,
                    ));
                }
                Ok(rebuild_node(node, CoreNodeKind::List(items)))
            }
            CoreOperation::Array(CoreArrayOp::RepeatUnion) => {
                if args.len() != 3 {
                    return Err(AppError::validation(
                        "`repeat-union` expects an index symbol, a count, and a body.".to_string(),
                    ));
                }
                let index = extract_repeat_index(&args[0])?;
                let count =
                    finite_repeat_count(&args[1], param_names, env, "`repeat-union` count")?;
                let body = &args[2];
                let mut items = Vec::new();
                for iteration in 0..count {
                    let mut loop_env = env.clone();
                    loop_env.insert(index.clone(), ParamValue::Number(iteration as f64));
                    let normalized_body =
                        normalize_node_for_direct_occt(body, param_names, &loop_env, next_node_id)?;
                    items.push(rewrap_with_index(
                        node,
                        &index,
                        iteration as f64,
                        normalized_body,
                        next_node_id,
                    ));
                }
                if items.is_empty() {
                    return Err(AppError::validation(
                        "Direct OCCT normalizer could not expand `repeat-union`: produced no geometry.",
                    ));
                }
                Ok(rebuild_node(
                    node,
                    CoreNodeKind::Call {
                        op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Union),
                        args: items,
                        keywords: Vec::new(),
                    },
                ))
            }
            CoreOperation::Array(CoreArrayOp::RepeatCompound) => {
                if args.len() != 3 {
                    return Err(AppError::validation(
                        "`repeat-compound` expects an index symbol, a count, and a body."
                            .to_string(),
                    ));
                }
                let index = extract_repeat_index(&args[0])?;
                let count =
                    finite_repeat_count(&args[1], param_names, env, "`repeat-compound` count")?;
                let body = &args[2];
                let mut items = Vec::new();
                for iteration in 0..count {
                    let mut loop_env = env.clone();
                    loop_env.insert(index.clone(), ParamValue::Number(iteration as f64));
                    let normalized_body =
                        normalize_node_for_direct_occt(body, param_names, &loop_env, next_node_id)?;
                    items.push(rewrap_with_index(
                        node,
                        &index,
                        iteration as f64,
                        normalized_body,
                        next_node_id,
                    ));
                }
                Ok(rebuild_node(
                    node,
                    CoreNodeKind::Call {
                        op: CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Group),
                        args: items,
                        keywords: Vec::new(),
                    },
                ))
            }
            CoreOperation::Array(CoreArrayOp::RepeatPick) => {
                if args.len() != 4 {
                    return Err(AppError::validation(
                        "`repeat-pick` expects an index symbol, a count, a predicate, and a body."
                            .to_string(),
                    ));
                }
                let index = extract_repeat_index(&args[0])?;
                let count = finite_repeat_count(&args[1], param_names, env, "`repeat-pick` count")?;
                let predicate =
                    normalize_node_for_direct_occt(&args[2], param_names, env, next_node_id)?;
                let body = &args[3];
                let mut last_selected = None;
                for iteration in 0..count {
                    let mut loop_env = env.clone();
                    loop_env.insert(index.clone(), ParamValue::Number(iteration as f64));
                    let selected = match crate::ecky_ir::eval_core_bool_with_locals(
                        &predicate,
                        param_names,
                        &loop_env,
                    ) {
                        Ok(true) => Some(()),
                        Ok(false) => None,
                        Err(err) => {
                            return Err(AppError::validation(format!(
                            "Direct OCCT normalizer could not evaluate `repeat-pick` predicate: {}",
                            err
                        )))
                        }
                    };
                    if selected.is_some() {
                        let normalized_body = normalize_node_for_direct_occt(
                            body,
                            param_names,
                            &loop_env,
                            next_node_id,
                        )?;
                        last_selected = Some(rewrap_with_index(
                            node,
                            &index,
                            iteration as f64,
                            normalized_body,
                            next_node_id,
                        ));
                    }
                }
                last_selected.ok_or_else(|| {
                    AppError::validation(
                        "Direct OCCT normalizer could not expand `repeat-pick`: no matching iteration.",
                    )
                })
            }
            _ => {
                let normalized_args = match op {
                    CoreOperation::Custom(name) if name == "sampled-radial-loft" => args
                        .iter()
                        .map(|arg| Ok(clone_node_with_fresh_ids(arg, next_node_id)))
                        .collect::<AppResult<Vec<_>>>()?,
                    _ => args
                        .iter()
                        .map(|arg| {
                            normalize_node_for_direct_occt(arg, param_names, env, next_node_id)
                        })
                        .collect::<AppResult<Vec<_>>>()?,
                };
                let normalized_keywords = keywords
                    .iter()
                    .map(|keyword| {
                        normalize_keyword_source_for_direct_occt(
                            keyword.name.as_str(),
                            keyword.source_node(),
                            param_names,
                            env,
                            next_node_id,
                        )
                        .map(|source| match keyword.selector_payload() {
                            Some(selector) => CoreKeywordArg::selector(
                                keyword.name.clone(),
                                source,
                                selector.clone(),
                            ),
                            None => CoreKeywordArg::expr(keyword.name.clone(), source),
                        })
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                match op {
                    CoreOperation::Custom(name) if name == "sampled-radial-loft" => {
                        Ok(rebuild_node(
                            node,
                            CoreNodeKind::Call {
                                op: op.clone(),
                                args: normalized_args,
                                keywords: normalized_keywords,
                            },
                        ))
                    }
                    CoreOperation::Custom(name) if name == "hole" => {
                        Err(AppError::validation(typed_hole_error(&normalized_keywords)))
                    }
                    CoreOperation::Custom(name) if name == "helical-ridge" => Ok(rebuild_node(
                        node,
                        CoreNodeKind::Call {
                            op: op.clone(),
                            args: normalized_args,
                            keywords: normalized_keywords,
                        },
                    )),
                    CoreOperation::Custom(name) if is_scalar_eval_custom_op(name) => {
                        Ok(rebuild_node(
                            node,
                            CoreNodeKind::Call {
                                op: op.clone(),
                                args: normalized_args,
                                keywords: normalized_keywords,
                            },
                        ))
                    }
                    CoreOperation::Custom(name) => Err(AppError::validation(format!(
                        "Direct OCCT normalizer does not support custom operation `{name}`."
                    ))),
                    CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Xor) => {
                        normalize_xor_node(node, normalized_args, next_node_id)
                    }
                    CoreOperation::Primitive(CorePrimitive::Svg) => normalize_svg_node(
                        node,
                        args,
                        &normalized_keywords,
                        param_names,
                        env,
                        next_node_id,
                    ),
                    _ => Ok(rebuild_node(
                        node,
                        CoreNodeKind::Call {
                            op: op.clone(),
                            args: normalized_args,
                            keywords: normalized_keywords,
                        },
                    )),
                }
            }
        },
        CoreNodeKind::Range { start, end } => {
            let normalized_start =
                normalize_node_for_direct_occt(start, param_names, env, next_node_id)?;
            let normalized_end =
                normalize_node_for_direct_occt(end, param_names, env, next_node_id)?;
            let finite_range = expand_finite_range(
                &normalized_start,
                &normalized_end,
                param_names,
                env,
                next_node_id,
                "`range`",
            )?;
            Ok(rebuild_node(node, CoreNodeKind::List(finite_range)))
        }
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            let normalized_sources = sources
                .iter()
                .map(|source| {
                    normalize_node_for_direct_occt(source, param_names, env, next_node_id)
                })
                .collect::<AppResult<Vec<_>>>()?;
            let source_items = normalized_sources
                .iter()
                .map(|source| list_items(source, next_node_id, "`map` source"))
                .collect::<AppResult<Vec<_>>>()?;
            if source_items.is_empty() {
                return Ok(rebuild_node(node, CoreNodeKind::List(Vec::new())));
            }

            for (index, items) in source_items.iter().enumerate() {
                let shortest = source_items[0].len();
                if items.len() != shortest {
                    return Err(AppError::validation(format!(
                        "`map` source length mismatch at source {index}: expected {shortest}, got {}",
                        items.len()
                    )));
                }
            }

            let mut mapped = Vec::new();
            if params.len() != source_items.len() {
                return Err(AppError::validation(format!(
                    "`map` expected {} parameters, got {}",
                    source_items.len(),
                    params.len()
                )));
            }

            for index in 0..source_items[0].len() {
                let mut iteration_env = env.clone();
                let mut bindings = Vec::with_capacity(params.len());
                for (param_name, source_values) in params.iter().zip(&source_items) {
                    let value = source_values[index].clone();
                    if let Some(value) = eval_scalar_binding(&value, param_names, &iteration_env)? {
                        iteration_env.insert(param_name.clone(), value);
                    }
                    bindings.push(CoreBinding {
                        name: param_name.clone(),
                        value,
                    });
                }
                let mapped_body = normalize_node_for_direct_occt(
                    body,
                    param_names,
                    &iteration_env,
                    next_node_id,
                )?;
                let mapped_value_kind = mapped_body.value_kind;
                mapped.push(CoreNode {
                    id: next_id(next_node_id),
                    kind: CoreNodeKind::Let {
                        bindings,
                        body: Box::new(mapped_body),
                    },
                    value_kind: mapped_value_kind,
                    span: node.span,
                });
            }

            Ok(rebuild_node(node, CoreNodeKind::List(mapped)))
        }
        CoreNodeKind::Apply { op, args, list } => {
            let normalized_args = args
                .iter()
                .map(|arg| normalize_node_for_direct_occt(arg, param_names, env, next_node_id))
                .collect::<AppResult<Vec<_>>>()?;
            let normalized_list =
                normalize_node_for_direct_occt(list, param_names, env, next_node_id)?;
            let items = list_items(&normalized_list, next_node_id, "`apply` list")?;
            let mut expanded = normalized_args;
            expanded.extend(items);
            Ok(rebuild_node(
                node,
                CoreNodeKind::Call {
                    op: op.clone(),
                    args: expanded,
                    keywords: Vec::new(),
                },
            ))
        }
        CoreNodeKind::List(items) => Ok(rebuild_node(
            node,
            CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| {
                        normalize_node_for_direct_occt(item, param_names, env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            ),
        )),
        CoreNodeKind::Group(items) => Ok(rebuild_node(
            node,
            CoreNodeKind::Group(
                items
                    .iter()
                    .map(|item| {
                        normalize_node_for_direct_occt(item, param_names, env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            ),
        )),
    }
}

fn normalize_keyword_source_for_direct_occt(
    keyword_name: &str,
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if keyword_name == "align" {
        return normalize_align_keyword_tuple(node, next_node_id);
    }
    normalize_node_for_direct_occt(node, param_names, env, next_node_id)
}

fn normalize_align_keyword_tuple(node: &CoreNode, next_node_id: &mut u64) -> AppResult<CoreNode> {
    match &node.kind {
        CoreNodeKind::List(_) | CoreNodeKind::Group(_) => Ok(node.clone()),
        CoreNodeKind::Call {
            op: CoreOperation::Custom(head),
            args,
            keywords,
        } if keywords.is_empty() => {
            let head_symbol = align_symbol_from_name(head).ok_or_else(|| {
                AppError::validation("Direct OCCT `:align` expects `(min|center|max)^3`.")
            })?;
            let mut items = Vec::with_capacity(args.len() + 1);
            items.push(CoreNode {
                id: next_id(next_node_id),
                kind: CoreNodeKind::Literal(CoreLiteral::Symbol(head_symbol)),
                value_kind: CoreValueKind::Any,
                span: node.span,
            });
            for arg in args {
                let CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) = arg.kind else {
                    return Err(AppError::validation(
                        "Direct OCCT `:align` expects `(min|center|max)^3`.",
                    ));
                };
                if !matches!(
                    symbol,
                    CoreSymbol::Min | CoreSymbol::Center | CoreSymbol::Max
                ) {
                    return Err(AppError::validation(
                        "Direct OCCT `:align` expects `(min|center|max)^3`.",
                    ));
                }
                items.push(CoreNode {
                    id: next_id(next_node_id),
                    kind: CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)),
                    value_kind: CoreValueKind::Any,
                    span: arg.span,
                });
            }
            Ok(CoreNode {
                id: node.id,
                kind: CoreNodeKind::List(items),
                value_kind: node.value_kind,
                span: node.span,
            })
        }
        _ => Err(AppError::validation(
            "Direct OCCT `:align` expects `(min|center|max)^3`.",
        )),
    }
}

fn align_symbol_from_name(name: &str) -> Option<CoreSymbol> {
    match name {
        "min" => Some(CoreSymbol::Min),
        "center" => Some(CoreSymbol::Center),
        "max" => Some(CoreSymbol::Max),
        _ => None,
    }
}

fn normalize_xor_node(
    node: &CoreNode,
    normalized_args: Vec<CoreNode>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if normalized_args.len() < 2 {
        return Err(AppError::validation("`xor` expects at least two operands."));
    }

    let union_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Union),
            args: normalized_args.clone(),
            keywords: Vec::new(),
        },
        node.value_kind,
    );
    let intersection_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Intersection),
            args: normalized_args,
            keywords: Vec::new(),
        },
        node.value_kind,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Difference),
            args: vec![union_node, intersection_node],
            keywords: Vec::new(),
        },
    ))
}

fn rewrap_with_index(
    template: &CoreNode,
    index_name: &str,
    index_value: f64,
    body: CoreNode,
    next_node_id: &mut u64,
) -> CoreNode {
    CoreNode {
        id: next_id(next_node_id),
        kind: CoreNodeKind::Let {
            bindings: vec![CoreBinding {
                name: index_name.to_string(),
                value: CoreNode {
                    id: next_id(next_node_id),
                    kind: CoreNodeKind::Literal(CoreLiteral::Number(index_value)),
                    value_kind: CoreValueKind::Number,
                    span: None,
                },
            }],
            body: Box::new(body),
        },
        value_kind: template.value_kind,
        span: template.span,
    }
}

fn extract_repeat_index(index_node: &CoreNode) -> AppResult<String> {
    match &index_node.kind {
        CoreNodeKind::Reference(crate::ecky_core_ir::CoreReference::Local(name)) => {
            Ok(name.clone())
        }
        _ => Err(AppError::validation(
            "`repeat` loop index must be a local binding symbol.",
        )),
    }
}

fn finite_repeat_count(
    count: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
) -> AppResult<usize> {
    let count =
        crate::ecky_ir::eval_core_number_with_locals(count, param_names, env).map_err(|err| {
            AppError::validation(format!(
                "Direct OCCT normalizer could not evaluate {context} as a number: {err}",
            ))
        })?;
    if !count.is_finite() {
        return Err(AppError::validation(format!(
            "Direct OCCT normalizer requires {context} to be finite.",
        )));
    }
    let floored = count.floor();
    if floored > usize::MAX as f64 {
        return Err(AppError::validation(format!(
            "Direct OCCT normalizer cannot expand `{}`; iteration count is too large.",
            context,
        )));
    }
    if floored < 0.0 {
        return Ok(0);
    }
    Ok(floored as usize)
}

fn expand_finite_range(
    start: &CoreNode,
    end: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
    context: &str,
) -> AppResult<Vec<CoreNode>> {
    let start =
        crate::ecky_ir::eval_core_number_with_locals(start, param_names, env).map_err(|err| {
            AppError::validation(format!(
                "Direct OCCT normalizer could not evaluate {context} start as a number: {err}",
            ))
        })?;
    let end =
        crate::ecky_ir::eval_core_number_with_locals(end, param_names, env).map_err(|err| {
            AppError::validation(format!(
                "Direct OCCT normalizer could not evaluate {context} end as a number: {err}",
            ))
        })?;
    if !start.is_finite() {
        return Err(AppError::validation(format!(
            "Direct OCCT normalizer requires {context} start to be finite."
        )));
    }
    if !end.is_finite() {
        return Err(AppError::validation(format!(
            "Direct OCCT normalizer requires {context} end to be finite."
        )));
    }
    let mut start = start.floor() as isize;
    let end = end.floor() as isize;
    if start > end {
        start = end;
    }
    Ok((start..end)
        .map(|index| CoreNode {
            id: next_id(next_node_id),
            kind: CoreNodeKind::Literal(CoreLiteral::Number(index as f64)),
            value_kind: CoreValueKind::Number,
            span: None,
        })
        .collect())
}

fn list_items(node: &CoreNode, next_node_id: &mut u64, context: &str) -> AppResult<Vec<CoreNode>> {
    match &node.kind {
        CoreNodeKind::List(items) => Ok(items
            .iter()
            .map(|item| clone_node_with_fresh_ids(item, next_node_id))
            .collect()),
        CoreNodeKind::Let { bindings, body } => {
            let body_items = list_items(body, next_node_id, context)?;
            let mut wrapped = Vec::with_capacity(body_items.len());
            for item in body_items {
                wrapped.push(wrap_bindings_around_item(bindings, item, next_node_id));
            }
            Ok(wrapped)
        }
        other => Err(AppError::validation(format!(
            "Direct OCCT normalizer {context} must be a list, got {:?}",
            other
        ))),
    }
}

fn eval_scalar_binding(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Option<ParamValue>> {
    let node = rewrite_local_aliases_for_eval(node, env);
    Ok(match node.value_kind {
        CoreValueKind::Number => Some(ParamValue::Number(
            crate::ecky_ir::eval_core_number_with_locals(&node, param_names, env)?,
        )),
        CoreValueKind::Boolean => Some(ParamValue::Boolean(
            crate::ecky_ir::eval_core_bool_with_locals(&node, param_names, env)?,
        )),
        CoreValueKind::Text => Some(ParamValue::String(
            crate::ecky_ir::eval_core_stringish_with_locals(&node, param_names, env)?,
        )),
        _ => None,
    })
}

fn param_value_literal_node(
    next_node_id: &mut u64,
    value: &ParamValue,
    span: Option<SourceSpan>,
) -> CoreNode {
    match value {
        ParamValue::Number(number) => CoreNode {
            id: next_id(next_node_id),
            kind: CoreNodeKind::Literal(CoreLiteral::Number(*number)),
            value_kind: CoreValueKind::Number,
            span,
        },
        ParamValue::Boolean(flag) => CoreNode {
            id: next_id(next_node_id),
            kind: CoreNodeKind::Literal(CoreLiteral::Boolean(*flag)),
            value_kind: CoreValueKind::Boolean,
            span,
        },
        ParamValue::String(text) => CoreNode {
            id: next_id(next_node_id),
            kind: CoreNodeKind::Literal(CoreLiteral::Text(text.clone())),
            value_kind: CoreValueKind::Text,
            span,
        },
        ParamValue::Null => unreachable!("scalar folding only emits number/bool/text"),
    }
}

fn rewrite_local_aliases_for_eval(node: &CoreNode, env: &BTreeMap<String, ParamValue>) -> CoreNode {
    match &node.kind {
        CoreNodeKind::Reference(crate::ecky_core_ir::CoreReference::Local(name)) => {
            let resolved = resolve_eval_local_alias(name, env).unwrap_or_else(|| name.clone());
            if resolved == *name {
                return node.clone();
            }
            let mut rewritten = node.clone();
            rewritten.kind =
                CoreNodeKind::Reference(crate::ecky_core_ir::CoreReference::Local(resolved));
            rewritten
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => node.clone(),
        CoreNodeKind::Build { bindings, result } => rebuild_node(
            node,
            CoreNodeKind::Build {
                bindings: bindings
                    .iter()
                    .map(|binding| CoreShapeBinding {
                        name: binding.name.clone(),
                        value: rewrite_local_aliases_for_eval(&binding.value, env),
                    })
                    .collect(),
                result: Box::new(rewrite_local_aliases_for_eval(result, env)),
            },
        ),
        CoreNodeKind::Let { bindings, body } => rebuild_node(
            node,
            CoreNodeKind::Let {
                bindings: bindings
                    .iter()
                    .map(|binding| CoreBinding {
                        name: binding.name.clone(),
                        value: rewrite_local_aliases_for_eval(&binding.value, env),
                    })
                    .collect(),
                body: Box::new(rewrite_local_aliases_for_eval(body, env)),
            },
        ),
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => rebuild_node(
            node,
            CoreNodeKind::If {
                condition: Box::new(rewrite_local_aliases_for_eval(condition, env)),
                then_branch: Box::new(rewrite_local_aliases_for_eval(then_branch, env)),
                else_branch: Box::new(rewrite_local_aliases_for_eval(else_branch, env)),
            },
        ),
        CoreNodeKind::Call { op, args, keywords } => rebuild_node(
            node,
            CoreNodeKind::Call {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| rewrite_local_aliases_for_eval(arg, env))
                    .collect(),
                keywords: keywords
                    .iter()
                    .map(|keyword| match keyword.selector_payload() {
                        Some(selector) => CoreKeywordArg::selector(
                            keyword.name.clone(),
                            rewrite_local_aliases_for_eval(keyword.source_node(), env),
                            selector.clone(),
                        ),
                        None => CoreKeywordArg::expr(
                            keyword.name.clone(),
                            rewrite_local_aliases_for_eval(keyword.source_node(), env),
                        ),
                    })
                    .collect(),
            },
        ),
        CoreNodeKind::Range { start, end } => rebuild_node(
            node,
            CoreNodeKind::Range {
                start: Box::new(rewrite_local_aliases_for_eval(start, env)),
                end: Box::new(rewrite_local_aliases_for_eval(end, env)),
            },
        ),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => rebuild_node(
            node,
            CoreNodeKind::Map {
                params: params.clone(),
                sources: sources
                    .iter()
                    .map(|source| rewrite_local_aliases_for_eval(source, env))
                    .collect(),
                body: Box::new(rewrite_local_aliases_for_eval(body, env)),
            },
        ),
        CoreNodeKind::Apply { op, args, list } => rebuild_node(
            node,
            CoreNodeKind::Apply {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| rewrite_local_aliases_for_eval(arg, env))
                    .collect(),
                list: Box::new(rewrite_local_aliases_for_eval(list, env)),
            },
        ),
        CoreNodeKind::List(items) => rebuild_node(
            node,
            CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| rewrite_local_aliases_for_eval(item, env))
                    .collect(),
            ),
        ),
        CoreNodeKind::Group(items) => rebuild_node(
            node,
            CoreNodeKind::Group(
                items
                    .iter()
                    .map(|item| rewrite_local_aliases_for_eval(item, env))
                    .collect(),
            ),
        ),
    }
}

fn resolve_eval_local_alias(name: &str, env: &BTreeMap<String, ParamValue>) -> Option<String> {
    if env.contains_key(name) {
        return Some(name.to_string());
    }
    let trimmed = name.trim_start_matches('#');
    if trimmed != name && env.contains_key(trimmed) {
        return Some(trimmed.to_string());
    }
    if trimmed != name {
        let stripped = trimmed.trim_end_matches(|ch: char| ch.is_ascii_digit());
        if !stripped.is_empty() && env.contains_key(stripped) {
            return Some(stripped.to_string());
        }
    }
    None
}

fn is_scalar_eval_custom_op(name: &str) -> bool {
    matches!(
        name,
        "not"
            | "and"
            | "or"
            | "="
            | ">"
            | ">="
            | "<"
            | "<="
            | "+"
            | "-"
            | "*"
            | "/"
            | "min"
            | "max"
            | "clamp"
            | "abs"
            | "floor"
            | "sin"
            | "cos"
            | "tan"
            | "atan"
            | "atan2"
            | "deg"
            | "deg->rad"
            | "rad"
            | "rad->deg"
            | "smoothstep"
            | "signed-pow"
            | "lerp"
            | "if"
            | "hash01"
            | "hash-signed"
            | "noise2"
            | "voronoi2"
            | "cell-distance2"
            | "fbm2"
            | "even?"
            | "odd?"
            | "zero?"
            | "null?"
            | "empty?"
    )
}

fn rebuild_node(node: &CoreNode, kind: CoreNodeKind) -> CoreNode {
    let mut rebuilt = CoreNode::new(node.id, kind, node.value_kind);
    rebuilt.span = node.span;
    rebuilt
}

fn wrap_bindings_around_item(
    bindings: &[CoreBinding],
    item: CoreNode,
    next_node_id: &mut u64,
) -> CoreNode {
    let value_kind = item.value_kind;
    let span = item.span.clone();
    CoreNode {
        id: next_id(next_node_id),
        kind: CoreNodeKind::Let {
            bindings: bindings
                .iter()
                .map(|binding| CoreBinding {
                    name: binding.name.clone(),
                    value: clone_node_with_fresh_ids(&binding.value, next_node_id),
                })
                .collect(),
            body: Box::new(item),
        },
        value_kind,
        span,
    }
}

fn clone_node_with_fresh_ids(node: &CoreNode, next_node_id: &mut u64) -> CoreNode {
    let kind = match &node.kind {
        CoreNodeKind::Literal(literal) => CoreNodeKind::Literal(literal.clone()),
        CoreNodeKind::Reference(reference) => CoreNodeKind::Reference(reference.clone()),
        CoreNodeKind::Build { bindings, result } => CoreNodeKind::Build {
            bindings: bindings
                .iter()
                .map(|binding| CoreShapeBinding {
                    name: binding.name.clone(),
                    value: clone_node_with_fresh_ids(&binding.value, next_node_id),
                })
                .collect(),
            result: Box::new(clone_node_with_fresh_ids(result, next_node_id)),
        },
        CoreNodeKind::Let { bindings, body } => CoreNodeKind::Let {
            bindings: bindings
                .iter()
                .map(|binding| CoreBinding {
                    name: binding.name.clone(),
                    value: clone_node_with_fresh_ids(&binding.value, next_node_id),
                })
                .collect(),
            body: Box::new(clone_node_with_fresh_ids(body, next_node_id)),
        },
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => CoreNodeKind::If {
            condition: Box::new(clone_node_with_fresh_ids(condition, next_node_id)),
            then_branch: Box::new(clone_node_with_fresh_ids(then_branch, next_node_id)),
            else_branch: Box::new(clone_node_with_fresh_ids(else_branch, next_node_id)),
        },
        CoreNodeKind::Call { op, args, keywords } => CoreNodeKind::Call {
            op: op.clone(),
            args: args
                .iter()
                .map(|arg| clone_node_with_fresh_ids(arg, next_node_id))
                .collect(),
            keywords: keywords
                .iter()
                .map(|keyword| match keyword.selector_payload() {
                    Some(selector) => CoreKeywordArg::selector(
                        keyword.name.clone(),
                        clone_node_with_fresh_ids(keyword.source_node(), next_node_id),
                        selector.clone(),
                    ),
                    None => CoreKeywordArg::expr(
                        keyword.name.clone(),
                        clone_node_with_fresh_ids(keyword.source_node(), next_node_id),
                    ),
                })
                .collect(),
        },
        CoreNodeKind::Range { start, end } => CoreNodeKind::Range {
            start: Box::new(clone_node_with_fresh_ids(start, next_node_id)),
            end: Box::new(clone_node_with_fresh_ids(end, next_node_id)),
        },
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => CoreNodeKind::Map {
            params: params.clone(),
            sources: sources
                .iter()
                .map(|source| clone_node_with_fresh_ids(source, next_node_id))
                .collect(),
            body: Box::new(clone_node_with_fresh_ids(body, next_node_id)),
        },
        CoreNodeKind::Apply { op, args, list } => CoreNodeKind::Apply {
            op: op.clone(),
            args: args
                .iter()
                .map(|arg| clone_node_with_fresh_ids(arg, next_node_id))
                .collect(),
            list: Box::new(clone_node_with_fresh_ids(list, next_node_id)),
        },
        CoreNodeKind::List(items) => CoreNodeKind::List(
            items
                .iter()
                .map(|item| clone_node_with_fresh_ids(item, next_node_id))
                .collect(),
        ),
        CoreNodeKind::Group(items) => CoreNodeKind::Group(
            items
                .iter()
                .map(|item| clone_node_with_fresh_ids(item, next_node_id))
                .collect(),
        ),
    };
    let mut cloned = CoreNode::new(next_id(next_node_id), kind, node.value_kind);
    cloned.span = node.span;
    cloned
}

fn normalize_svg_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if !keywords.is_empty() {
        return Err(AppError::validation(
            "`svg` does not support keyword arguments in Direct OCCT normalizer.",
        ));
    }
    if args.is_empty() || args.len() > 4 {
        return Err(AppError::validation(
            "`svg` expects a file path, optional target width/height, and optional fit mode.",
        ));
    }

    let source = crate::ecky_ir::eval_core_stringish_with_locals(&args[0], param_names, env)?;
    let svg_text = if fs::metadata(&source).is_ok() {
        fs::read_to_string(&source).map_err(|err| {
            AppError::validation(format!(
                "Direct OCCT normalizer could not read SVG file `{source}`: {err}",
            ))
        })?
    } else if source.trim_start().starts_with('<') {
        source
    } else {
        return Err(AppError::validation(format!(
            "Direct OCCT normalizer could not read SVG source at `{source}`.",
        )));
    };

    let target_width = args
        .get(1)
        .map(|arg| {
            crate::ecky_ir::eval_core_number_with_locals(arg, param_names, env).map_err(|err| {
                AppError::validation(format!(
                    "Direct OCCT normalizer could not evaluate `svg` width: {err}",
                ))
            })
        })
        .transpose()?;

    let target_height = args
        .get(2)
        .map(|arg| {
            crate::ecky_ir::eval_core_number_with_locals(arg, param_names, env).map_err(|err| {
                AppError::validation(format!(
                    "Direct OCCT normalizer could not evaluate `svg` height: {err}",
                ))
            })
        })
        .transpose()?;

    let fit_mode = args
        .get(3)
        .map(|arg| {
            let value = crate::ecky_ir::eval_core_stringish_with_locals(arg, param_names, env)?;
            SvgFitMode::from_str(&value).ok_or_else(|| {
                AppError::validation(format!(
                    "`svg` fit mode must be `contain`, `cover`, or `stretch`, got {value}",
                ))
            })
        })
        .transpose()?;

    let profile = parse_svg_profile(
        &svg_text,
        target_width,
        target_height,
        fit_mode.unwrap_or(SvgFitMode::Contain),
        true,
    )?;

    let outer = normalized_svg_polygon_node(&profile.outer_loop, next_node_id);
    let holes = profile
        .hole_loops
        .iter()
        .map(|points| normalized_svg_polygon_node(points, next_node_id))
        .collect::<Vec<_>>();
    let keywords = if holes.is_empty() {
        Vec::new()
    } else {
        vec![CoreKeywordArg::expr(
            "holes".to_string(),
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::List(holes),
                CoreValueKind::List,
            ),
        )]
    };

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Profile),
            args: vec![outer],
            keywords,
        },
    ))
}

fn normalized_svg_polygon_node(points: &Vec<[f64; 2]>, next_node_id: &mut u64) -> CoreNode {
    let point_nodes = points
        .iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point2(*point)),
                CoreValueKind::Point2,
            )
        })
        .collect::<Vec<_>>();

    let list = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::List(point_nodes),
        CoreValueKind::List,
    );

    CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Polygon),
            args: vec![list],
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    )
}

fn next_id(next_node_id: &mut u64) -> NodeId {
    let id = *next_node_id;
    *next_node_id += 1;
    NodeId::new(id)
}

fn next_program_node_id(program: &CoreProgram) -> u64 {
    program
        .parts
        .iter()
        .map(|part| max_node_id(&part.root))
        .max()
        .unwrap_or(0)
        + 1
}

fn max_node_id(node: &CoreNode) -> u64 {
    let child_max = match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => 0,
        CoreNodeKind::Build { bindings, result } => bindings
            .iter()
            .map(|binding| max_node_id(&binding.value))
            .chain(std::iter::once(max_node_id(result)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::Let { bindings, body } => bindings
            .iter()
            .map(|binding| max_node_id(&binding.value))
            .chain(std::iter::once(max_node_id(body)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => [
            max_node_id(condition),
            max_node_id(then_branch),
            max_node_id(else_branch),
        ]
        .into_iter()
        .max()
        .unwrap_or(0),
        CoreNodeKind::Call { args, keywords, .. } => args
            .iter()
            .map(max_node_id)
            .chain(
                keywords
                    .iter()
                    .map(|keyword| max_node_id(keyword.source_node())),
            )
            .max()
            .unwrap_or(0),
        CoreNodeKind::Range { start, end } => [max_node_id(start), max_node_id(end)]
            .into_iter()
            .max()
            .unwrap_or(0),
        CoreNodeKind::Map { sources, body, .. } => sources
            .iter()
            .map(max_node_id)
            .chain(std::iter::once(max_node_id(body)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::Apply { args, list, .. } => args
            .iter()
            .map(max_node_id)
            .chain(std::iter::once(max_node_id(list)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().map(max_node_id).max().unwrap_or(0)
        }
    };
    node.id.raw().max(child_max)
}

fn typed_hole_error(keywords: &[CoreKeywordArg]) -> String {
    let requested_type = keyword_text(keywords, "type").unwrap_or_else(|| "unknown".to_string());
    let goal = keyword_text(keywords, "goal").unwrap_or_else(|| "unspecified".to_string());
    format!(
        "Typed hole requested type `{}` with goal `{}` must be filled before direct OCCT planning.",
        requested_type, goal
    )
}

fn keyword_text(keywords: &[CoreKeywordArg], name: &str) -> Option<String> {
    keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .and_then(|keyword| match &keyword.source_node().kind {
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => Some(text.clone()),
            _ => None,
        })
}

fn operation_name(op: &CoreOperation) -> String {
    match op {
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Box) => "box".to_string(),
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Sphere) => {
            "sphere".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Cylinder) => {
            "cylinder".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Cone) => "cone".to_string(),
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Circle) => {
            "circle".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Rectangle) => {
            "rectangle".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::RoundedRectangle) => {
            "rounded-rectangle".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::RoundedPolygon) => {
            "rounded-polygon".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Polygon) => {
            "polygon".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Profile) => {
            "profile".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::MakeFace) => {
            "make-face".to_string()
        }
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Text) => "text".to_string(),
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Svg) => "svg".to_string(),
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Stl) => {
            "import-stl".to_string()
        }
        CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Union) => "union".to_string(),
        CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Difference) => {
            "difference".to_string()
        }
        CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Intersection) => {
            "intersection".to_string()
        }
        CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Xor) => "xor".to_string(),
        CoreOperation::Transform(crate::ecky_core_ir::CoreTransformOp::Translate) => {
            "translate".to_string()
        }
        CoreOperation::Transform(crate::ecky_core_ir::CoreTransformOp::Rotate) => {
            "rotate".to_string()
        }
        CoreOperation::Transform(crate::ecky_core_ir::CoreTransformOp::Scale) => {
            "scale".to_string()
        }
        CoreOperation::Transform(crate::ecky_core_ir::CoreTransformOp::Mirror) => {
            "mirror".to_string()
        }
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Extrude) => {
            "extrude".to_string()
        }
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Revolve) => {
            "revolve".to_string()
        }
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Loft) => "loft".to_string(),
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Sweep) => "sweep".to_string(),
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Shell) => "shell".to_string(),
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Offset) => "offset".to_string(),
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::OffsetRounded) => {
            "offset-rounded".to_string()
        }
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Fillet) => "fillet".to_string(),
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Chamfer) => {
            "chamfer".to_string()
        }
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Taper) => "taper".to_string(),
        CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Twist) => "twist".to_string(),
        CoreOperation::Path(crate::ecky_core_ir::CorePathOp::Polyline) => "polyline".to_string(),
        CoreOperation::Path(crate::ecky_core_ir::CorePathOp::BezierPath) => {
            "bezier-path".to_string()
        }
        CoreOperation::Path(crate::ecky_core_ir::CorePathOp::Bspline) => "bspline".to_string(),
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::LinearArray) => {
            "linear-array".to_string()
        }
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RadialArray) => {
            "radial-array".to_string()
        }
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::GridArray) => {
            "grid-array".to_string()
        }
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::ArcArray) => "arc-array".to_string(),
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::Repeat) => "repeat".to_string(),
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RepeatUnion) => {
            "repeat-union".to_string()
        }
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RepeatCompound) => {
            "repeat-compound".to_string()
        }
        CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RepeatPick) => {
            "repeat-pick".to_string()
        }
        CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::Plane) => "plane".to_string(),
        CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::Location) => "location".to_string(),
        CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::PathFrame) => {
            "path-frame".to_string()
        }
        CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::Place) => "place".to_string(),
        CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::ClipBox) => "clip-box".to_string(),
        CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Group) => "compound".to_string(),
        CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Comment) => "comment".to_string(),
        CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Annotate) => "annotate".to_string(),
        CoreOperation::Custom(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_core_ir::{CoreLiteral, CoreNodeKind, CoreOperation, PartId, ProgramId};
    use crate::{
        ecky_cad_host::direct_occt_normalize::normalize_core_program_for_direct_occt,
        ecky_scheme::compile_to_core_program,
    };

    fn compile(source: &str) -> CoreProgram {
        compile_to_core_program(source).expect("compile")
    }

    #[test]
    fn resolves_scalar_if_branches() {
        let program = compile(
            r#"
            (model
              (params (toggle include-box #t))
              (part body
                (if include-box
                  (box 1 1 1)
                  (sphere 2))))
            "#,
        );

        let normalized = normalize_core_program_for_direct_occt(&program, &Default::default())
            .expect("normalize");

        let op = match &normalized.parts[0].root.kind {
            CoreNodeKind::Call { op, .. } => op,
            _ => panic!("expected call"),
        };

        assert!(matches!(
            op,
            CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Box)
        ));
    }

    #[test]
    fn expands_finite_range_into_literal_list() {
        let program = compile(
            r#"
            (model
              (part body
                (range 0 4)))
            "#,
        );

        let normalized = normalize_core_program_for_direct_occt(&program, &Default::default())
            .expect("normalize");
        let items = match &normalized.parts[0].root.kind {
            CoreNodeKind::List(items) => items,
            other => panic!("expected list, got {:?}", other),
        };

        let numbers = items
            .iter()
            .map(|item| match &item.kind {
                CoreNodeKind::Literal(CoreLiteral::Number(n)) => *n,
                _ => panic!("expected literal number"),
            })
            .collect::<Vec<_>>();

        assert_eq!(numbers, vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn expands_finite_map() {
        let program = compile(
            r#"
            (model
              (part body
                (map
                  (lambda (i)
                    (box 1 1 1))
                  (range 0 3))))
            "#,
        );

        let normalized = normalize_core_program_for_direct_occt(&program, &Default::default())
            .expect("normalize");

        let items = match &normalized.parts[0].root.kind {
            CoreNodeKind::List(items) => items,
            other => panic!("expected list, got {:?}", other),
        };

        assert_eq!(items.len(), 3);
        assert!(items
            .iter()
            .all(|item| matches!(item.kind, CoreNodeKind::Let { .. })));
    }

    #[test]
    fn expands_apply_over_finite_list() {
        let program = compile(
            r#"
            (model
              (part body
                (apply union
                  (box 1 1 1)
                  (list
                    (box 2 2 2)
                    (box 3 3 3)))))
            "#,
        );

        let normalized = normalize_core_program_for_direct_occt(&program, &Default::default())
            .expect("normalize");

        match &normalized.parts[0].root.kind {
            CoreNodeKind::Call { op, args, .. } => {
                assert!(matches!(
                    op,
                    CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Union)
                ));
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected call, got {:?}", other),
        }
    }

    #[test]
    fn expands_repeat_union_and_repeat_pick() {
        let repeat_program = compile(
            r#"
            (model
              (part body
                (repeat-union i 3 (box 1 1 1))))
            "#,
        );
        let normalized =
            normalize_core_program_for_direct_occt(&repeat_program, &Default::default())
                .expect("normalize");
        assert!(matches!(
            &normalized.parts[0].root.kind,
            CoreNodeKind::Call { op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Union), args, .. } if args.len() == 3
        ));

        let pick_program = compile(
            r#"
            (model
              (part body
                (repeat-pick i 4 (> i 2)
                  (box 1 1 1))))
            "#,
        );
        let normalized = normalize_core_program_for_direct_occt(&pick_program, &Default::default())
            .expect("normalize");

        match &normalized.parts[0].root.kind {
            CoreNodeKind::Let { bindings, body } => {
                assert_eq!(bindings.len(), 1);
                assert_eq!(bindings[0].name, "i");
                assert!(matches!(
                    &body.kind,
                    CoreNodeKind::Call {
                        op: CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Box),
                        ..
                    }
                ));
            }
            other => panic!("expected let, got {:?}", other),
        }
    }

    #[test]
    fn preserves_sampled_radial_loft_call_without_rewrite() {
        let program = compile(
            r#"
            (model
              (part body
                (sampled-radial-loft
                  (theta z fz)
                  :height 40
                  :z-steps 2
                  :theta-steps 4
                  :radius 8)))
            "#,
        );

        let normalized = normalize_core_program_for_direct_occt(&program, &Default::default())
            .expect("normalize");

        match &normalized.parts[0].root.kind {
            CoreNodeKind::Call {
                op: CoreOperation::Custom(name),
                ..
            } => {
                assert_eq!(name, "sampled-radial-loft");
            }
            other => panic!("expected sampled-radial-loft call, got {:?}", other),
        }
    }

    #[test]
    fn rewrites_xor_into_supported_boolean_ops() {
        let xor_program = compile(
            r#"
            (model
              (part body
                (xor (box 1 1 1) (sphere 1))))
            "#,
        );
        let normalized = normalize_core_program_for_direct_occt(&xor_program, &Default::default())
            .expect("xor normalized");

        match &normalized.parts[0].root.kind {
            CoreNodeKind::Call {
                op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Difference),
                args,
                ..
            } => {
                assert_eq!(args.len(), 2);
                assert!(matches!(
                    args[0].kind,
                    CoreNodeKind::Call {
                        op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Union),
                        ..
                    }
                ));
                assert!(matches!(
                    args[1].kind,
                    CoreNodeKind::Call {
                        op: CoreOperation::Boolean(
                            crate::ecky_core_ir::CoreBooleanOp::Intersection
                        ),
                        ..
                    }
                ));
            }
            other => panic!("expected xor rewrite into difference, got {:?}", other),
        }
    }

    #[test]
    fn rejects_typed_holes_but_preserves_native_supported_ops() {
        let hole_program = compile(
            r#"
            (model
              (part body
                (difference
                  (box 1 1 1)
                  (hole :type solid :goal "threaded insert cavity"))))
            "#,
        );
        let err = normalize_core_program_for_direct_occt(&hole_program, &Default::default())
            .expect_err("hole rejected");
        assert!(err.to_string().contains("Typed hole"));

        let native_program = CoreProgram {
            id: ProgramId::new(1),
            parameters: Vec::new(),
            parts: vec![
                CorePart {
                    id: PartId::new(1),
                    key: "text-part".into(),
                    label: "text-part".into(),
                    root: CoreNode::new(
                        NodeId::new(2),
                        CoreNodeKind::Call {
                            op: crate::ecky_core_ir::CoreOperation::Primitive(
                                crate::ecky_core_ir::CorePrimitive::Text,
                            ),
                            args: vec![
                                CoreNode::new(
                                    NodeId::new(3),
                                    CoreNodeKind::Literal(CoreLiteral::Text("ABC".into())),
                                    CoreValueKind::Text,
                                ),
                                CoreNode::new(
                                    NodeId::new(4),
                                    CoreNodeKind::Literal(CoreLiteral::Number(1.0)),
                                    CoreValueKind::Number,
                                ),
                            ],
                            keywords: Vec::new(),
                        },
                        CoreValueKind::Solid,
                    ),
                },
                CorePart {
                    id: PartId::new(2),
                    key: "stl-part".into(),
                    label: "stl-part".into(),
                    root: CoreNode::new(
                        NodeId::new(5),
                        CoreNodeKind::Call {
                            op: crate::ecky_core_ir::CoreOperation::Primitive(
                                crate::ecky_core_ir::CorePrimitive::Stl,
                            ),
                            args: vec![CoreNode::new(
                                NodeId::new(6),
                                CoreNodeKind::Literal(CoreLiteral::Text("/tmp/sample.stl".into())),
                                CoreValueKind::Text,
                            )],
                            keywords: Vec::new(),
                        },
                        CoreValueKind::Solid,
                    ),
                },
                CorePart {
                    id: PartId::new(3),
                    key: "ridge".into(),
                    label: "ridge".into(),
                    root: CoreNode::new(
                        NodeId::new(7),
                        CoreNodeKind::Call {
                            op: crate::ecky_core_ir::CoreOperation::Custom("helical-ridge".into()),
                            args: Vec::new(),
                            keywords: vec![
                                CoreKeywordArg::expr(
                                    "radius".into(),
                                    CoreNode::new(
                                        NodeId::new(8),
                                        CoreNodeKind::Literal(CoreLiteral::Number(18.0)),
                                        CoreValueKind::Number,
                                    ),
                                ),
                                CoreKeywordArg::expr(
                                    "pitch".into(),
                                    CoreNode::new(
                                        NodeId::new(9),
                                        CoreNodeKind::Literal(CoreLiteral::Number(3.0)),
                                        CoreValueKind::Number,
                                    ),
                                ),
                                CoreKeywordArg::expr(
                                    "height".into(),
                                    CoreNode::new(
                                        NodeId::new(10),
                                        CoreNodeKind::Literal(CoreLiteral::Number(24.0)),
                                        CoreValueKind::Number,
                                    ),
                                ),
                                CoreKeywordArg::expr(
                                    "base-width".into(),
                                    CoreNode::new(
                                        NodeId::new(11),
                                        CoreNodeKind::Literal(CoreLiteral::Number(1.2)),
                                        CoreValueKind::Number,
                                    ),
                                ),
                                CoreKeywordArg::expr(
                                    "crest-width".into(),
                                    CoreNode::new(
                                        NodeId::new(12),
                                        CoreNodeKind::Literal(CoreLiteral::Number(0.35)),
                                        CoreValueKind::Number,
                                    ),
                                ),
                                CoreKeywordArg::expr(
                                    "depth".into(),
                                    CoreNode::new(
                                        NodeId::new(13),
                                        CoreNodeKind::Literal(CoreLiteral::Number(0.6)),
                                        CoreValueKind::Number,
                                    ),
                                ),
                            ],
                        },
                        CoreValueKind::Solid,
                    ),
                },
            ],
            feature_decls: Default::default(),
            constraints: Default::default(),
        };

        let normalized =
            normalize_core_program_for_direct_occt(&native_program, &Default::default())
                .expect("native-supported ops preserved");
        assert_eq!(normalized.parts.len(), 3);
        assert!(matches!(
            normalized.parts[0].root.kind,
            CoreNodeKind::Call {
                op: crate::ecky_core_ir::CoreOperation::Primitive(
                    crate::ecky_core_ir::CorePrimitive::Text
                ),
                ..
            }
        ));
        assert!(matches!(
            normalized.parts[1].root.kind,
            CoreNodeKind::Call {
                op: crate::ecky_core_ir::CoreOperation::Primitive(
                    crate::ecky_core_ir::CorePrimitive::Stl
                ),
                ..
            }
        ));
        assert!(matches!(
            normalized.parts[2].root.kind,
            CoreNodeKind::Call {
                op: crate::ecky_core_ir::CoreOperation::Custom(ref name),
                ..
            } if name == "helical-ridge"
        ));
    }

    #[test]
    fn normalizes_svg_profile_for_direct_occt() {
        let svg_path = std::path::Path::new("/tmp/ecky-direct-occt-svg-normalize-profile.svg");
        {
            let mut file = std::fs::File::create(&svg_path).expect("create svg");
            use std::io::Write;
            file.write_all(
                b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">\n  <path d=\"M1 1h8v8h-8z\"/>\n</svg>\n",
            )
            .expect("write svg");
        }

        let program = compile(
            r#"(model (part body (extrude (svg "/tmp/ecky-direct-occt-svg-normalize-profile.svg" 10 10 "contain") 4)))"#,
        );

        let normalized = normalize_core_program_for_direct_occt(&program, &Default::default())
            .expect("normalize");

        let root = &normalized.parts[0].root;
        match &root.kind {
            CoreNodeKind::Call {
                op: CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Extrude),
                args: _,
                keywords,
            } => assert!(keywords.is_empty()),
            other => panic!("expected extrude, got {:?}", other),
        }
        match &normalized.parts[0].root.kind {
            CoreNodeKind::Call {
                op: CoreOperation::Surface(crate::ecky_core_ir::CoreSurfaceOp::Extrude),
                args,
                ..
            } => match &args[0].kind {
                CoreNodeKind::Call {
                    op: CoreOperation::Primitive(CorePrimitive::Profile),
                    args: profile_args,
                    ..
                } => {
                    assert_eq!(profile_args.len(), 1);
                    match &profile_args[0].kind {
                        CoreNodeKind::Call {
                            op: CoreOperation::Primitive(CorePrimitive::Polygon),
                            ..
                        } => {}
                        other => panic!("expected polygon profile outer loop, got {:?}", other),
                    }
                }
                other => panic!("expected profile, got {:?}", other),
            },
            other => panic!("expected extrude, got {:?}", other),
        }

        assert!(std::fs::remove_file(svg_path).is_ok());
    }
}
