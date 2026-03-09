use serde_json::json;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::models::{AppState, DesignOutput, Message};
use crate::{db, persist_thread_summary};

use rustpython_parser::ast::{self, Stmt, Expr, Constant};
use rustpython_parser::{parse, Mode};

#[derive(serde::Serialize)]
pub struct ParsedParamsResult {
    pub fields: Vec<serde_json::Value>,
    pub params: serde_json::Map<String, serde_json::Value>,
}

#[tauri::command]
pub fn parse_macro_params(macro_code: String) -> ParsedParamsResult {
    println!("Rust: parse_macro_params called with {} chars", macro_code.len());
    let mut fields = Vec::new();
    let mut params = serde_json::Map::new();

    let ast = match parse(&macro_code, Mode::Module, "<embedded>") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Rust: parse error: {:?}", e);
            return ParsedParamsResult { fields, params };
        }
    };

    if let ast::Mod::Module(module) = ast {
        for stmt in &module.body {
            // 1. Support 'params = { ... }' or 'params = dict(...)'
            match stmt {
                Stmt::Assign(assign) => {
                    let mut is_params = false;
                    for target in &assign.targets {
                        if let Expr::Name(name) = target {
                            if name.id.as_str() == "params" {
                                is_params = true;
                                break;
                            }
                        }
                    }
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
            // 2. Scan for 'params.get("key", default)' pattern
            scan_stmt_for_params_get(stmt, &mut fields, &mut params);
        }
    }

    // Deduplicate fields by key
    let mut unique_fields = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    for field in fields {
        if let Some(key) = field.get("key").and_then(|k| k.as_str()) {
            if seen_keys.insert(key.to_string()) {
                unique_fields.push(field);
            }
        }
    }

    println!("Rust: returning {} fields", unique_fields.len());
    ParsedParamsResult { fields: unique_fields, params }
}

fn scan_stmt_for_params_get(stmt: &Stmt, fields: &mut Vec<serde_json::Value>, params: &mut serde_json::Map<String, serde_json::Value>) {
    match stmt {
        Stmt::Assign(a) => scan_expr_for_params_get(&a.value, fields, params),
        Stmt::AnnAssign(a) => {
            if let Some(v) = &a.value {
                scan_expr_for_params_get(v, fields, params);
            }
        }
        Stmt::Expr(e) => scan_expr_for_params_get(&e.value, fields, params),
        Stmt::For(f) => {
            for s in &f.body { scan_stmt_for_params_get(s, fields, params); }
            scan_expr_for_params_get(&f.iter, fields, params);
        }
        Stmt::If(i) => {
            for s in &i.body { scan_stmt_for_params_get(s, fields, params); }
            for s in &i.orelse { scan_stmt_for_params_get(s, fields, params); }
            scan_expr_for_params_get(&i.test, fields, params);
        }
        Stmt::With(w) => {
            for s in &w.body { scan_stmt_for_params_get(s, fields, params); }
        }
        Stmt::FunctionDef(f) => {
            for s in &f.body { scan_stmt_for_params_get(s, fields, params); }
        }
        _ => {}
    }
}

fn scan_expr_for_params_get(expr: &Expr, fields: &mut Vec<serde_json::Value>, params: &mut serde_json::Map<String, serde_json::Value>) {
    if let Expr::Call(call) = expr {
        if let Expr::Attribute(attr) = &*call.func {
            if let Expr::Name(obj_name) = &*attr.value {
                if obj_name.id.as_str() == "params" && attr.attr.as_str() == "get" {
                    if call.args.len() >= 2 {
                        if let Expr::Constant(const_key) = &call.args[0] {
                            if let Constant::Str(key_str) = &const_key.value {
                                let (val, val_type) = extract_value(&call.args[1]);
                                if !params.contains_key(key_str.as_str()) {
                                    params.insert(key_str.to_string(), val);
                                    fields.push(create_field(key_str.as_str(), &val_type));
                                }
                            }
                        }
                    }
                }
            }
        }
        for arg in &call.args { scan_expr_for_params_get(arg, fields, params); }
        for kw in &call.keywords { scan_expr_for_params_get(&kw.value, fields, params); }
    } else if let Expr::BinOp(b) = expr {
        scan_expr_for_params_get(&b.left, fields, params);
        scan_expr_for_params_get(&b.right, fields, params);
    } else if let Expr::Dict(d) = expr {
        for v in &d.values { scan_expr_for_params_get(v, fields, params); }
    } else if let Expr::List(l) = expr {
        for e in &l.elts { scan_expr_for_params_get(e, fields, params); }
    } else if let Expr::Tuple(t) = expr {
        for e in &t.elts { scan_expr_for_params_get(e, fields, params); }
    }
}

fn process_params_value(value: &Expr, fields: &mut Vec<serde_json::Value>, params: &mut serde_json::Map<String, serde_json::Value>) {
    if let Expr::Dict(dict) = value {
        println!("Rust: params is a dict literal with {} keys", dict.keys.len());
        for (i, key_opt) in dict.keys.iter().enumerate() {
            if let Some(Expr::Constant(const_key)) = key_opt {
                if let Constant::Str(s) = &const_key.value {
                    let key_str = s.to_string();
                    if let Some(val_expr) = dict.values.get(i) {
                        let (val, val_type) = extract_value(val_expr);
                        params.insert(key_str.clone(), val);
                        fields.push(create_field(&key_str, &val_type));
                    }
                }
            }
        }
    } else if let Expr::Call(call) = value {
        if let Expr::Name(func_name) = &*call.func {
            if func_name.id.as_str() == "dict" {
                println!("Rust: params is a dict() call with {} kwargs", call.keywords.len());
                for kw in &call.keywords {
                    if let Some(arg_id) = &kw.arg {
                        let key_str = arg_id.as_str().to_string();
                        let (val, val_type) = extract_value(&kw.value);
                        params.insert(key_str.clone(), val);
                        fields.push(create_field(&key_str, &val_type));
                    }
                }
            }
        }
    }
}

fn extract_value(expr: &Expr) -> (serde_json::Value, String) {
    if let Expr::Constant(expr_const) = expr {
        match &expr_const.value {
            Constant::Str(s) => (json!(s.to_string()), "select".to_string()),
            Constant::Int(i) => {
                // Try converting BigInt to i64
                let val: i64 = i.try_into().unwrap_or(0);
                (json!(val), "number".to_string())
            },
            Constant::Float(f) => (json!(f), "number".to_string()),
            Constant::Bool(b) => (json!(b), "checkbox".to_string()),
            _ => (json!(0), "number".to_string()),
        }
    } else {
        (json!(0), "number".to_string())
    }
}

fn create_field(key: &str, field_type: &str) -> serde_json::Value {
    json!({
        "key": key,
        "label": key.replace(['_', '-'], " "),
        "type": field_type,
        "min": serde_json::Value::Null,
        "max": serde_json::Value::Null,
        "step": serde_json::Value::Null,
        "min_from": "",
        "max_from": "",
        "freezed": false
    })
}

#[tauri::command]
pub async fn add_manual_version(
    thread_id: String,
    title: String,
    version_name: String,
    macro_code: String,
    parameters: serde_json::Value,
    ui_spec: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let db = state.db.lock().await;

    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: "Manual edit committed as new version.".to_string(),
        interaction_mode: "design".to_string(),
        macro_code,
        ui_spec,
        initial_params: parameters,
    };

    let msg_id = Uuid::new_v4().to_string();
    let msg = Message {
        id: msg_id.clone(),
        role: "assistant".to_string(),
        content: "Manual edit committed as new version.".to_string(),
        status: "success".to_string(),
        output: Some(output),
        image_data: None,
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|e: rusqlite::Error| e.to_string())?;

    let thread_traits = if db::get_thread_title(&db, &thread_id)
        .unwrap_or(None)
        .is_none()
    {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref())
        .map_err(|e: rusqlite::Error| e.to_string())?;
    let _ = persist_thread_summary(&db, &thread_id, &title);

    Ok(msg_id)
}

#[tauri::command]
pub async fn update_ui_spec(
    message_id: String,
    ui_spec: serde_json::Value,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let (updated_output, updated_thread_id) = {
        let db = state.db.lock().await;
        db::update_message_ui_spec(&db, &message_id, &ui_spec).map_err(|e| e.to_string())?;
        db::get_message_output_and_thread(&db, &message_id).map_err(|e| e.to_string())?
    }
    .ok_or("Message output not found for ui_spec update")?;

    {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(updated_output.clone());
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(updated_thread_id.clone());
    }

    let cache_path = app
        .path()
        .app_config_dir()
        .unwrap()
        .join("last_design.json");
    let session_data = json!({
        "design": updated_output,
        "thread_id": Some(updated_thread_id)
    });
    if let Ok(json) = serde_json::to_string_pretty(&session_data) {
        let _ = fs::write(cache_path, json);
    }

    Ok(())
}

#[tauri::command]
pub async fn update_parameters(
    message_id: String,
    parameters: serde_json::Value,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let (updated_output, updated_thread_id) = {
        let db = state.db.lock().await;
        db::update_message_parameters(&db, &message_id, &parameters).map_err(|e| e.to_string())?;
        db::get_message_output_and_thread(&db, &message_id).map_err(|e| e.to_string())?
    }
    .ok_or("Message output not found for parameter update")?;

    {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(updated_output.clone());
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(updated_thread_id.clone());
    }

    let cache_path = app
        .path()
        .app_config_dir()
        .unwrap()
        .join("last_design.json");
    let session_data = json!({
        "design": updated_output,
        "thread_id": Some(updated_thread_id)
    });
    if let Ok(json) = serde_json::to_string_pretty(&session_data) {
        let _ = fs::write(cache_path, json);
    }

    Ok(())
}
