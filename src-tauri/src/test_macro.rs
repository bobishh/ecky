use rustpython_parser::{parse, Mode};
use rustpython_parser::ast::{self, Stmt, Expr, Constant};
use serde_json::json;

pub struct ParsedParamsResult {
    pub fields: Vec<serde_json::Value>,
    pub params: serde_json::Map<String, serde_json::Value>,
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

fn extract_value(expr: &Expr) -> (serde_json::Value, String) {
    if let Expr::Constant(expr_const) = expr {
        match &expr_const.value {
            Constant::Str(s) => (json!(s.to_string()), "select".to_string()),
            Constant::Int(i) => {
                let val: i64 = i.clone().try_into().unwrap_or(0);
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

fn main() {
    let macro_code = r#"
import FreeCAD
import Part
import math

# --- PARAMETERS ---
# Z-Mount Parameters
throat_dia = params.get("throat_diameter", 54.4)
ring_od = params.get("ring_od", 52.0)
z_lug_thickness = params.get("z_lug_thickness", 1.35)
lug_angle = params.get("lug_angle", 30.0)
z_gap = params.get("z_gap", 1.75)
z_flange_od = params.get("z_flange_od", 62.0)
z_flange_thickness = params.get("z_flange_thickness", 2.0)
inner_dia = params.get("inner_diameter", 46.0)

# Altix & Optical Parameters
ffd_offset = params.get("ffd_offset", 0.0)
altix_od = params.get("altix_od", 50.0)
altix_bore_dia = params.get("altix_bore_dia", 42.5)
altix_clear_aperture = params.get("altix_clear_aperture", 38.0)
altix_tab_id = params.get("altix_tab_id", 39.0)
lug_clearance = params.get("altix_lug_clearance", 1.4)
altix_tab_thickness = params.get("altix_tab_thickness", 1.5)
altix_gap_angle = params.get("altix_gap_angle", 50.0)
"#;

    let ast = parse(macro_code, Mode::Module, "<test>").unwrap();
    let mut fields = Vec::new();
    let mut params = serde_json::Map::new();

    if let ast::Mod::Module(module) = ast {
        for stmt in &module.body {
            scan_stmt_for_params_get(stmt, &mut fields, &mut params);
        }
    }

    println!("{}", serde_json::to_string_pretty(&json!({
        "fields": fields,
        "initial_params": params
    })).unwrap());
}
