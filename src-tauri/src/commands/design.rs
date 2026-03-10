use rustpython_parser::ast::{self, Constant, Expr, Stmt};
use rustpython_parser::{parse, Mode};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::session::{build_runtime_snapshot, write_last_snapshot};
use crate::models::{
    validate_design_output, validate_design_params, validate_model_manifest, validate_ui_spec,
    AppError, AppResult, AppState, ArtifactBundle, DesignOutput, DesignParams, InteractionMode,
    Message, MessageRole, MessageStatus, ModelManifest, ParamValue, ParsedParamsResult,
    SelectOption, SelectValue, UiField, UiSpec,
};
use crate::{db, persist_thread_summary};

fn field_label(key: &str) -> String {
    key.replace(['_', '-'], " ")
}

fn create_field(key: &str, value: &ParamValue) -> UiField {
    let label = field_label(key);
    match value {
        ParamValue::String(text) => UiField::Select {
            key: key.to_string(),
            label,
            options: vec![SelectOption {
                label: text.clone(),
                value: SelectValue::String(text.clone()),
            }],
            frozen: false,
        },
        ParamValue::Number(_) | ParamValue::Null => UiField::Number {
            key: key.to_string(),
            label,
            min: None,
            max: None,
            step: None,
            min_from: None,
            max_from: None,
            frozen: false,
        },
        ParamValue::Boolean(_) => UiField::Checkbox {
            key: key.to_string(),
            label,
            frozen: false,
        },
    }
}

fn extract_value(expr: &Expr) -> ParamValue {
    match expr {
        Expr::Constant(expr_const) => match &expr_const.value {
            Constant::Str(text) => ParamValue::String(text.to_string()),
            Constant::Int(value) => {
                let numeric: i64 = value.try_into().unwrap_or(0);
                ParamValue::Number(numeric as f64)
            }
            Constant::Float(value) => ParamValue::Number(*value),
            Constant::Bool(value) => ParamValue::Boolean(*value),
            Constant::None => ParamValue::Null,
            _ => ParamValue::Number(0.0),
        },
        _ => ParamValue::Number(0.0),
    }
}

fn process_params_value(value: &Expr, fields: &mut Vec<UiField>, params: &mut DesignParams) {
    if let Expr::Dict(dict) = value {
        for (index, key_opt) in dict.keys.iter().enumerate() {
            if let Some(Expr::Constant(const_key)) = key_opt {
                if let Constant::Str(key) = &const_key.value {
                    if let Some(val_expr) = dict.values.get(index) {
                        let inferred = extract_value(val_expr);
                        params.insert(key.to_string(), inferred.clone());
                        fields.push(create_field(key, &inferred));
                    }
                }
            }
        }
    } else if let Expr::Call(call) = value {
        if let Expr::Name(func_name) = &*call.func {
            if func_name.id.as_str() == "dict" {
                for keyword in &call.keywords {
                    if let Some(arg_id) = &keyword.arg {
                        let key = arg_id.as_str().to_string();
                        let inferred = extract_value(&keyword.value);
                        params.insert(key.clone(), inferred.clone());
                        fields.push(create_field(&key, &inferred));
                    }
                }
            }
        }
    }
}

fn scan_expr_for_params_get(expr: &Expr, fields: &mut Vec<UiField>, params: &mut DesignParams) {
    match expr {
        Expr::Call(call) => {
            if let Expr::Attribute(attr) = &*call.func {
                if let Expr::Name(obj_name) = &*attr.value {
                    if obj_name.id.as_str() == "params" && attr.attr.as_str() == "get" {
                        if call.args.len() >= 2 {
                            if let Expr::Constant(const_key) = &call.args[0] {
                                if let Constant::Str(key) = &const_key.value {
                                    if !params.contains_key(key.as_str()) {
                                        let inferred = extract_value(&call.args[1]);
                                        params.insert(key.to_string(), inferred.clone());
                                        fields.push(create_field(key, &inferred));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for arg in &call.args {
                scan_expr_for_params_get(arg, fields, params);
            }
            for keyword in &call.keywords {
                scan_expr_for_params_get(&keyword.value, fields, params);
            }
        }
        Expr::BinOp(bin_op) => {
            scan_expr_for_params_get(&bin_op.left, fields, params);
            scan_expr_for_params_get(&bin_op.right, fields, params);
        }
        Expr::Dict(dict) => {
            for value in &dict.values {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        Expr::List(list) => {
            for value in &list.elts {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        Expr::Tuple(tuple) => {
            for value in &tuple.elts {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        _ => {}
    }
}

fn scan_stmt_for_params_get(stmt: &Stmt, fields: &mut Vec<UiField>, params: &mut DesignParams) {
    match stmt {
        Stmt::Assign(assign) => scan_expr_for_params_get(&assign.value, fields, params),
        Stmt::AnnAssign(assign) => {
            if let Some(value) = &assign.value {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        Stmt::Expr(expr) => scan_expr_for_params_get(&expr.value, fields, params),
        Stmt::For(for_stmt) => {
            for stmt in &for_stmt.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
            scan_expr_for_params_get(&for_stmt.iter, fields, params);
        }
        Stmt::If(if_stmt) => {
            for stmt in &if_stmt.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
            for stmt in &if_stmt.orelse {
                scan_stmt_for_params_get(stmt, fields, params);
            }
            scan_expr_for_params_get(&if_stmt.test, fields, params);
        }
        Stmt::With(with_stmt) => {
            for stmt in &with_stmt.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
        }
        Stmt::FunctionDef(function) => {
            for stmt in &function.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
        }
        _ => {}
    }
}

#[tauri::command]
#[specta::specta]
pub fn parse_macro_params(macro_code: String) -> ParsedParamsResult {
    let mut fields = Vec::new();
    let mut params = DesignParams::new();

    let ast = match parse(&macro_code, Mode::Module, "<embedded>") {
        Ok(parsed) => parsed,
        Err(_) => return ParsedParamsResult { fields, params },
    };

    if let ast::Mod::Module(module) = ast {
        for stmt in &module.body {
            match stmt {
                Stmt::Assign(assign) => {
                    let is_params = assign.targets.iter().any(
                        |target| matches!(target, Expr::Name(name) if name.id.as_str() == "params"),
                    );
                    if is_params {
                        process_params_value(&assign.value, &mut fields, &mut params);
                    }
                }
                Stmt::AnnAssign(assign) => {
                    if let Expr::Name(name) = &*assign.target {
                        if name.id.as_str() == "params" {
                            if let Some(value) = &assign.value {
                                process_params_value(value, &mut fields, &mut params);
                            }
                        }
                    }
                }
                _ => {}
            }
            scan_stmt_for_params_get(stmt, &mut fields, &mut params);
        }
    }

    let mut unique_fields = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    for field in fields {
        if seen_keys.insert(field.key().to_string()) {
            unique_fields.push(field);
        }
    }

    ParsedParamsResult {
        fields: unique_fields,
        params,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn add_manual_version(
    thread_id: String,
    title: String,
    version_name: String,
    macro_code: String,
    parameters: DesignParams,
    ui_spec: UiSpec,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<String> {
    validate_ui_spec(&ui_spec)?;
    validate_design_params(&parameters, &ui_spec)?;
    if let Some(manifest) = model_manifest.as_ref() {
        validate_model_manifest(manifest)?;
        if let Some(bundle) = artifact_bundle.as_ref() {
            if manifest.model_id != bundle.model_id {
                return Err(AppError::validation(
                    "Model manifest does not match artifact bundle model id.",
                ));
            }
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let db = state.db.lock().await;

    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: "Manual edit committed as new version.".to_string(),
        interaction_mode: InteractionMode::Design,
        macro_code,
        ui_spec,
        initial_params: parameters,
    };
    validate_design_output(&output)?;

    let thread_traits = if db::get_thread_title(&db, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .is_none()
    {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let msg_id = Uuid::new_v4().to_string();
    let msg = Message {
        id: msg_id.clone(),
        role: MessageRole::Assistant,
        content: "Manual edit committed as new version.".to_string(),
        status: MessageStatus::Success,
        output: Some(output),
        usage: None,
        artifact_bundle: artifact_bundle.clone(),
        model_manifest: model_manifest.clone(),
        image_data: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|err| AppError::persistence(err.to_string()))?;
    let _ = persist_thread_summary(&db, &thread_id, &title);
    let snapshot = build_runtime_snapshot(
        msg.output.clone(),
        Some(thread_id.clone()),
        Some(msg_id.clone()),
        artifact_bundle,
        model_manifest,
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    write_last_snapshot(&app, Some(&snapshot));

    Ok(msg_id)
}

#[tauri::command]
#[specta::specta]
pub async fn add_imported_model_version(
    thread_id: String,
    title: String,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<String> {
    validate_model_manifest(&model_manifest)?;
    if artifact_bundle.model_id != model_manifest.model_id {
        return Err(AppError::validation(
            "Imported model manifest does not match artifact bundle model id.",
        ));
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let db = state.db.lock().await;

    let thread_traits = if db::get_thread_title(&db, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .is_none()
    {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let msg_id = Uuid::new_v4().to_string();
    let label = model_manifest.document.document_label.trim();
    let content = if label.is_empty() {
        "Imported FreeCAD model.".to_string()
    } else {
        format!("Imported FreeCAD model: {}.", label)
    };
    let msg = Message {
        id: msg_id.clone(),
        role: MessageRole::Assistant,
        content,
        status: MessageStatus::Success,
        output: None,
        usage: None,
        artifact_bundle: Some(artifact_bundle.clone()),
        model_manifest: Some(model_manifest.clone()),
        image_data: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|err| AppError::persistence(err.to_string()))?;
    let _ = persist_thread_summary(&db, &thread_id, &title);
    let snapshot = build_runtime_snapshot(
        None,
        Some(thread_id.clone()),
        Some(msg_id.clone()),
        Some(artifact_bundle),
        Some(model_manifest),
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    write_last_snapshot(&app, Some(&snapshot));

    Ok(msg_id)
}

#[tauri::command]
#[specta::specta]
pub async fn update_ui_spec(
    message_id: String,
    ui_spec: UiSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    validate_ui_spec(&ui_spec)?;

    let (updated_output, updated_thread_id, artifact_bundle, model_manifest) = {
        let db = state.db.lock().await;
        let (mut current_output, current_thread_id) =
            db::get_message_output_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .ok_or_else(|| {
                    AppError::not_found("Message output not found for uiSpec update.")
                })?;
        current_output.ui_spec = ui_spec;
        validate_design_output(&current_output)?;
        db::update_message_ui_spec(&db, &message_id, &current_output.ui_spec)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        let (artifact_bundle, model_manifest, _) =
            db::get_message_runtime_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .unwrap_or((None, None, current_thread_id.clone()));
        (
            current_output,
            current_thread_id,
            artifact_bundle,
            model_manifest,
        )
    };

    {
        let snapshot = build_runtime_snapshot(
            Some(updated_output.clone()),
            Some(updated_thread_id.clone()),
            Some(message_id.clone()),
            artifact_bundle,
            model_manifest,
            None,
        );
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
        write_last_snapshot(&app, Some(&snapshot));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_parameters(
    message_id: String,
    parameters: DesignParams,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let (updated_output, updated_thread_id, artifact_bundle, model_manifest) = {
        let db = state.db.lock().await;
        let (mut current_output, current_thread_id) =
            db::get_message_output_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .ok_or_else(|| {
                    AppError::not_found("Message output not found for parameter update.")
                })?;
        validate_design_params(&parameters, &current_output.ui_spec)?;
        current_output.initial_params = parameters;
        validate_design_output(&current_output)?;
        db::update_message_parameters(&db, &message_id, &current_output.initial_params)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        let (artifact_bundle, model_manifest, _) =
            db::get_message_runtime_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .unwrap_or((None, None, current_thread_id.clone()));
        (
            current_output,
            current_thread_id,
            artifact_bundle,
            model_manifest,
        )
    };

    {
        let snapshot = build_runtime_snapshot(
            Some(updated_output.clone()),
            Some(updated_thread_id.clone()),
            Some(message_id.clone()),
            artifact_bundle,
            model_manifest,
            None,
        );
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
        write_last_snapshot(&app, Some(&snapshot));
    }
    Ok(())
}
