use super::*;

const SHAPE_GRAPH_SECTION_MAX_ITEMS: usize = 64;

fn shape_graph_section_enabled(
    filters: &[ShapeGraphFilterSection],
    section: ShapeGraphFilterSection,
) -> bool {
    filters.is_empty() || filters.contains(&section)
}
fn shape_graph_payload<T>(items: Vec<T>) -> ShapeGraphSectionPayload<T> {
    let truncated = items.len() > SHAPE_GRAPH_SECTION_MAX_ITEMS;
    ShapeGraphSectionPayload {
        truncated,
        items: items
            .into_iter()
            .take(SHAPE_GRAPH_SECTION_MAX_ITEMS)
            .collect(),
    }
}

fn relation_operand_label(
    program: &crate::ecky_core_ir::CoreProgram,
    operand: &crate::ecky_core_ir::CoreRelationOperand,
) -> String {
    match operand {
        crate::ecky_core_ir::CoreRelationOperand::Number(value) => {
            if value.fract() == 0.0 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            }
        }
        crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) => program
            .parameters
            .iter()
            .find(|param| param.id == *param_id)
            .map(|param| param.key.clone())
            .unwrap_or_else(|| format!("param#{}", param_id.raw())),
    }
}

fn collect_relation_param_keys(
    program: &crate::ecky_core_ir::CoreProgram,
    relation: &crate::ecky_core_ir::CoreRelationConstraint,
) -> Vec<String> {
    let mut keys = Vec::new();
    for operand in [&relation.left, &relation.right] {
        if let crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) = operand {
            if let Some(param_key) = program
                .parameters
                .iter()
                .find(|param| param.id == *param_id)
                .map(|param| param.key.clone())
            {
                if !keys.iter().any(|existing| existing == &param_key) {
                    keys.push(param_key);
                }
            }
        }
    }
    keys
}

pub(in crate::mcp::handlers) fn build_shape_graph_packet(
    design_output: &DesignOutput,
    model_manifest: Option<&ModelManifest>,
    artifact_bundle: Option<&ArtifactBundle>,
    filters: &[ShapeGraphFilterSection],
) -> AppResult<ShapeGraphPacket> {
    let source = design_output.macro_code.as_str();
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let program = if design_output.source_language == crate::models::SourceLanguage::EckyIrV0 {
        Some(
            crate::ecky_scheme::compile_to_core_program(source).map_err(|err| {
                compile_error_with_diagnostics(
                    format!("Failed to compile Ecky source for shapeGraph: {err}"),
                    source,
                    err,
                    None,
                    None,
                )
            })?,
        )
    } else {
        None
    };

    let mut core_fingerprint = Vec::new();
    let mut editable_stable_node_keys = Vec::new();
    if let Some(program) = program.as_ref() {
        for param in &program.parameters {
            core_fingerprint.push(format!("param:{}", param.key));
            let path = format!("/params/{}", path_segment(&param.key));
            if let Some(stable_key) = stable_node_key_for_program_path(source, program, &path) {
                if !stable_key.trim().is_empty()
                    && !editable_stable_node_keys
                        .iter()
                        .any(|existing| existing == &stable_key)
                {
                    editable_stable_node_keys.push(stable_key);
                }
            }
        }
        for part in &program.parts {
            core_fingerprint.push(format!("part:{}", part.key));
            let path = format!("/parts/{}", path_segment(&part.key));
            if let Some(stable_key) = stable_node_key_for_program_path(source, program, &path) {
                if !stable_key.trim().is_empty()
                    && !editable_stable_node_keys
                        .iter()
                        .any(|existing| existing == &stable_key)
                {
                    editable_stable_node_keys.push(stable_key);
                }
            }
        }
        for relation in &program.constraints.relations {
            core_fingerprint.push(format!(
                "relation:{}:{}:{}",
                relation_operand_label(program, &relation.left),
                relation.operator.as_str(),
                relation_operand_label(program, &relation.right)
            ));
        }
    }
    let core_digest = crate::mcp::macro_buffer::source_digest(&core_fingerprint.join("|"));

    let parts = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Parts).then(|| {
        let mut section_items = Vec::new();
        if let Some(manifest) = model_manifest {
            for part in &manifest.parts {
                let stable_node_key = program.as_ref().and_then(|program| {
                    let path = format!("/parts/{}", path_segment(&part.part_id));
                    stable_node_key_for_program_path(source, program, &path)
                });
                section_items.push(ShapeGraphPart {
                    part_id: part.part_id.clone(),
                    label: part.label.clone(),
                    kind: part.kind.clone(),
                    editable: part.editable,
                    stable_node_key,
                });
            }
        } else if let Some(program) = program.as_ref() {
            for part in &program.parts {
                let path = format!("/parts/{}", path_segment(&part.key));
                section_items.push(ShapeGraphPart {
                    part_id: part.key.clone(),
                    label: part.label.clone(),
                    kind: "solid".to_string(),
                    editable: true,
                    stable_node_key: stable_node_key_for_program_path(source, program, &path),
                });
            }
        }
        shape_graph_payload(section_items)
    });

    let instances =
        shape_graph_section_enabled(filters, ShapeGraphFilterSection::Instances).then(|| {
            let mut section_items = Vec::new();
            if let Some(graph) = model_manifest.and_then(|manifest| manifest.feature_graph.as_ref())
            {
                for node in &graph.nodes {
                    let node_kind = node.kind.to_ascii_lowercase();
                    if !(node_kind.contains("repeat") || node_kind.contains("instance")) {
                        continue;
                    }
                    let target_ids = node
                        .output_refs
                        .iter()
                        .flat_map(|output| output.target_ids.iter().cloned())
                        .collect::<Vec<_>>();
                    section_items.push(ShapeGraphInstance {
                        instance_id: node.feature_id.clone(),
                        prototype_feature_id: node.dependency_ids.first().cloned(),
                        dependency_ids: node.dependency_ids.clone(),
                        target_ids,
                    });
                }
            }
            shape_graph_payload(section_items)
        });

    let constraints = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Constraints)
        .then(|| {
            let mut section_items = Vec::new();
            if let Some(program) = program.as_ref() {
                for (index, relation) in program.constraints.relations.iter().enumerate() {
                    let path = format!("/params/:relations/{index}");
                    let depends_on_param_keys = collect_relation_param_keys(program, relation);
                    let mut affects_stable_node_keys = Vec::new();
                    for param_key in &depends_on_param_keys {
                        let Some(param_id) = program
                            .parameters
                            .iter()
                            .find(|param| param.key == *param_key)
                            .map(|param| param.id)
                        else {
                            continue;
                        };
                        for source_path in dependent_source_paths_for_param(program, param_id) {
                            let Some(stable_key) =
                                stable_node_key_for_program_path(source, program, &source_path)
                            else {
                                continue;
                            };
                            if stable_key.trim().is_empty()
                                || affects_stable_node_keys
                                    .iter()
                                    .any(|existing| existing == &stable_key)
                            {
                                continue;
                            }
                            affects_stable_node_keys.push(stable_key);
                        }
                    }
                    section_items.push(ShapeGraphConstraint {
                        constraint_id: format!("relation:{index}"),
                        label: format!(
                            "{} {} {}",
                            relation_operand_label(program, &relation.left),
                            relation.operator.as_str(),
                            relation_operand_label(program, &relation.right)
                        ),
                        kind: "relation".to_string(),
                        depends_on_param_keys,
                        affects_stable_node_keys,
                        source_stable_node_key: stable_node_key_for_program_path(
                            source, program, &path,
                        ),
                    });
                }
            }
            shape_graph_payload(section_items)
        });

    let debug_overlays = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Debug)
        .then(|| shape_graph_payload(Vec::<ShapeGraphDebugOverlay>::new()));

    let dependencies = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Dependencies)
        .then(|| {
            let mut section_items = Vec::new();
            if let Some(program) = program.as_ref() {
                for param in &program.parameters {
                    let dependent_source_paths =
                        dependent_source_paths_for_param(program, param.id);
                    section_items.push(ShapeGraphDependency {
                        parameter_key: param.key.clone(),
                        impacted_part_ids: impacted_part_ids_for_dependency_paths(
                            &dependent_source_paths,
                        ),
                        dependent_source_paths,
                    });
                }
            }
            shape_graph_payload(section_items)
        });

    let topology_target_counts = if let Some(bundle) = artifact_bundle {
        ShapeGraphTopologyCounts {
            edge_target_count: bundle.edge_targets.len(),
            face_target_count: bundle.face_targets.len(),
        }
    } else {
        ShapeGraphTopologyCounts {
            edge_target_count: 0,
            face_target_count: 0,
        }
    };

    Ok(ShapeGraphPacket {
        source_digest,
        core_digest,
        artifact_digest: artifact_bundle.map(artifact_bundle_digest),
        editable_stable_node_keys,
        topology_target_counts,
        parts,
        instances,
        constraints,
        debug_overlays,
        dependencies,
    })
}
