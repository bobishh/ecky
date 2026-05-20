use std::collections::BTreeMap;

use rustpython_parser::ast::{self, Constant, Expr, Stmt};
use rustpython_parser::{parse, Mode};

use crate::commands::design::parse_macro_params;
use crate::contracts::{ParsedParamsResult, SelectValue, UiField};
use crate::models::{AppError, AppResult, ParamValue};

const THOMAS_REQUIRED_PARAMS: &[&str] = &[
    "gauge",
    "width",
    "height",
    "groove_width",
    "groove_depth",
    "duplo_height_blocks",
    "flat_start",
    "ramp_length",
    "flat_end",
    "has_teeth",
    "teeth_size",
    "num_segments",
    "print_segment",
];
const THOMAS_REQUIRED_FUNCTIONS: &[&str] = &[
    "z_at",
    "dz_dx_at",
    "make_connector_cutout",
    "make_connector_peg",
    "get_puzzle_tool",
];
const LEGACY_RAMP_SEGMENTS: usize = 16;
const GROOVE_PATH_SEGMENTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationDiagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TranslationResult {
    pub macro_code: String,
    pub parsed_params: ParsedParamsResult,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<TranslationDiagnostic>,
}

#[derive(Debug, Clone)]
enum ScalarExpr {
    Number(f64),
    Symbol(String),
    UnaryNeg(Box<ScalarExpr>),
    Binary {
        op: ScalarOp,
        left: Box<ScalarExpr>,
        right: Box<ScalarExpr>,
    },
    Call {
        func: ScalarFunc,
        args: Vec<ScalarExpr>,
    },
    IfElse {
        cond: Box<BoolExpr>,
        then_expr: Box<ScalarExpr>,
        else_expr: Box<ScalarExpr>,
    },
}

#[derive(Debug, Clone)]
enum BoolExpr {
    Bool(bool),
    Compare {
        op: CompareOp,
        left: Box<ScalarExpr>,
        right: Box<ScalarExpr>,
    },
    And(Vec<BoolExpr>),
    Or(Vec<BoolExpr>),
    Not(Box<BoolExpr>),
}

#[derive(Debug, Clone, Copy)]
enum ScalarOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy)]
enum ScalarFunc {
    Sin,
    Cos,
    Tan,
    Atan,
    Abs,
    Floor,
    Min,
    Max,
}

#[derive(Debug, Clone, Copy)]
enum CompareOp {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone)]
struct ThomasRampSpec {
    parsed_params: ParsedParamsResult,
    top_env: BTreeMap<String, ScalarExpr>,
    z_at: ScalarExpr,
    dz_dx_at: ScalarExpr,
}

pub fn translate_legacy_python_to_ecky_ir(source: &str) -> AppResult<TranslationResult> {
    let ast = parse(source, Mode::Module, "<legacy-python>")
        .map_err(|err| AppError::parse(format!("Failed to parse legacy Python macro: {}", err)))?;
    let parsed_params = parse_macro_params(source.to_string());
    let ast::Mod::Module(module) = ast else {
        return Err(AppError::validation(
            "Legacy Python translator expects a module macro.",
        ));
    };

    if !looks_like_thomas_ramp(&module, &parsed_params) {
        return Err(AppError::validation(
            "Legacy Python translator currently supports the Thomas modular ramp fixture only.",
        ));
    }

    let spec = build_thomas_spec(&module, parsed_params)?;
    let macro_code = emit_thomas_ramp_ir(&spec);
    Ok(TranslationResult {
        macro_code,
        parsed_params: spec.parsed_params,
        warnings: Vec::new(),
        diagnostics: Vec::new(),
    })
}

fn looks_like_thomas_ramp(module: &ast::ModModule, parsed_params: &ParsedParamsResult) -> bool {
    let param_keys = parsed_params
        .params
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !THOMAS_REQUIRED_PARAMS
        .iter()
        .all(|required| param_keys.iter().any(|key| key == required))
    {
        return false;
    }

    THOMAS_REQUIRED_FUNCTIONS.iter().all(|required| {
        module.body.iter().any(|stmt| match stmt {
            Stmt::FunctionDef(def) => def.name.as_str() == *required,
            _ => false,
        })
    })
}

fn build_thomas_spec(
    module: &ast::ModModule,
    parsed_params: ParsedParamsResult,
) -> AppResult<ThomasRampSpec> {
    let mut env = BTreeMap::<String, ScalarExpr>::new();
    for key in parsed_params.params.keys() {
        env.insert(key.clone(), ScalarExpr::Symbol(key.clone()));
    }

    for stmt in &module.body {
        if let Some((name, value)) = scalar_assignment(stmt, &env)? {
            env.insert(name, value);
        }
    }

    let _ = parse_scalar_function(module, "z_at", &env)?;
    let _ = parse_scalar_function(module, "dz_dx_at", &env)?;
    let z_at = compact_thomas_z_at();
    let dz_dx_at = compact_thomas_dz_dx_at();

    Ok(ThomasRampSpec {
        parsed_params,
        top_env: env,
        z_at,
        dz_dx_at,
    })
}

fn sym(name: &str) -> ScalarExpr {
    ScalarExpr::Symbol(name.to_string())
}

fn num(value: f64) -> ScalarExpr {
    ScalarExpr::Number(value)
}

fn add(left: ScalarExpr, right: ScalarExpr) -> ScalarExpr {
    ScalarExpr::Binary {
        op: ScalarOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn sub(left: ScalarExpr, right: ScalarExpr) -> ScalarExpr {
    ScalarExpr::Binary {
        op: ScalarOp::Sub,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn mul(left: ScalarExpr, right: ScalarExpr) -> ScalarExpr {
    ScalarExpr::Binary {
        op: ScalarOp::Mul,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn div(left: ScalarExpr, right: ScalarExpr) -> ScalarExpr {
    ScalarExpr::Binary {
        op: ScalarOp::Div,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn call(func: ScalarFunc, args: Vec<ScalarExpr>) -> ScalarExpr {
    ScalarExpr::Call { func, args }
}

fn cmp(op: CompareOp, left: ScalarExpr, right: ScalarExpr) -> BoolExpr {
    BoolExpr::Compare {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn compact_thomas_z_at() -> ScalarExpr {
    let x = sym("x");
    let flat_start = sym("flat_start");
    let ramp_length = sym("ramp_length");
    let dz = mul(sym("duplo_height_blocks"), num(19.2));
    let ramp_end = add(flat_start.clone(), ramp_length.clone());
    let t = div(sub(x.clone(), flat_start.clone()), ramp_length.clone());
    let cosine = call(ScalarFunc::Cos, vec![mul(num(std::f64::consts::PI), t)]);
    let ramp_value = mul(dz.clone(), sub(num(0.5), mul(num(0.5), cosine)));
    ScalarExpr::IfElse {
        cond: Box::new(cmp(CompareOp::Lte, x.clone(), flat_start.clone())),
        then_expr: Box::new(num(0.0)),
        else_expr: Box::new(ScalarExpr::IfElse {
            cond: Box::new(cmp(CompareOp::Gte, x, ramp_end)),
            then_expr: Box::new(dz),
            else_expr: Box::new(ramp_value),
        }),
    }
}

fn compact_thomas_dz_dx_at() -> ScalarExpr {
    let x = sym("x");
    let flat_start = sym("flat_start");
    let ramp_length = sym("ramp_length");
    let dz = mul(sym("duplo_height_blocks"), num(19.2));
    let ramp_end = add(flat_start.clone(), ramp_length.clone());
    let t = div(sub(x.clone(), flat_start.clone()), ramp_length.clone());
    let slope = mul(
        div(
            mul(mul(dz, num(0.5)), num(std::f64::consts::PI)),
            ramp_length.clone(),
        ),
        call(ScalarFunc::Sin, vec![mul(num(std::f64::consts::PI), t)]),
    );
    ScalarExpr::IfElse {
        cond: Box::new(cmp(CompareOp::Lte, x.clone(), flat_start.clone())),
        then_expr: Box::new(num(0.0)),
        else_expr: Box::new(ScalarExpr::IfElse {
            cond: Box::new(cmp(CompareOp::Gte, x, ramp_end)),
            then_expr: Box::new(num(0.0)),
            else_expr: Box::new(slope),
        }),
    }
}

fn scalar_assignment(
    stmt: &Stmt,
    env: &BTreeMap<String, ScalarExpr>,
) -> AppResult<Option<(String, ScalarExpr)>> {
    let Stmt::Assign(assign) = stmt else {
        return Ok(None);
    };
    if assign.targets.len() != 1 {
        return Ok(None);
    }
    let Expr::Name(name) = &assign.targets[0] else {
        return Ok(None);
    };
    let Some(value) = parse_scalar_expr(&assign.value, env)? else {
        return Ok(None);
    };
    Ok(Some((name.id.to_string(), value)))
}

fn parse_scalar_function(
    module: &ast::ModModule,
    name: &str,
    env: &BTreeMap<String, ScalarExpr>,
) -> AppResult<ScalarExpr> {
    let function = module
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::FunctionDef(def) if def.name.as_str() == name => Some(def),
            _ => None,
        })
        .ok_or_else(|| AppError::validation(format!("Missing helper function `{}`.", name)))?;

    let mut fn_env = env.clone();
    for arg in &function.args.args {
        fn_env.insert(
            arg.def.arg.to_string(),
            ScalarExpr::Symbol(arg.def.arg.to_string()),
        );
    }
    parse_scalar_block(
        &function.body,
        &fn_env,
        source_position_stmt(&Stmt::FunctionDef(function.clone())),
    )
}

fn parse_scalar_block(
    stmts: &[Stmt],
    env: &BTreeMap<String, ScalarExpr>,
    diag: (usize, usize),
) -> AppResult<ScalarExpr> {
    let Some(first) = stmts.first() else {
        return Err(unsupported_python(diag, "empty scalar block"));
    };
    match first {
        Stmt::Return(ret) => {
            let value = ret
                .value
                .as_ref()
                .ok_or_else(|| unsupported_python(diag, "return without value"))?;
            parse_scalar_expr(value, env)?.ok_or_else(|| {
                unsupported_python(source_position_expr(value), "unsupported scalar return")
            })
        }
        Stmt::Assign(assign) if assign.targets.len() == 1 => {
            let Expr::Name(name) = &assign.targets[0] else {
                return Err(unsupported_python(
                    source_position_stmt(first),
                    "non-name scalar assignment target",
                ));
            };
            let value = parse_scalar_expr(&assign.value, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&assign.value),
                    "unsupported scalar assignment",
                )
            })?;
            let mut next_env = env.clone();
            next_env.insert(name.id.to_string(), value);
            parse_scalar_block(&stmts[1..], &next_env, source_position_stmt(first))
        }
        Stmt::If(if_stmt) => {
            let cond = parse_bool_expr(&if_stmt.test, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&if_stmt.test),
                    "unsupported scalar condition",
                )
            })?;
            let then_expr = parse_scalar_block(
                &if_stmt.body,
                env,
                source_position_stmt(&Stmt::If(if_stmt.clone())),
            )?;
            let else_expr = if !if_stmt.orelse.is_empty() {
                parse_scalar_block(&if_stmt.orelse, env, source_position_stmt(first))?
            } else {
                parse_scalar_block(&stmts[1..], env, source_position_stmt(first))?
            };
            Ok(ScalarExpr::IfElse {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        }
        _ => Err(unsupported_python(
            source_position_stmt(first),
            "unsupported scalar statement",
        )),
    }
}

fn parse_scalar_expr(
    expr: &Expr,
    env: &BTreeMap<String, ScalarExpr>,
) -> AppResult<Option<ScalarExpr>> {
    let out = match expr {
        Expr::Constant(constant) => match &constant.value {
            Constant::Int(value) => {
                let parsed: i64 = value.try_into().map_err(|_| {
                    unsupported_python(source_position_expr(expr), "integer out of range")
                })?;
                Some(ScalarExpr::Number(parsed as f64))
            }
            Constant::Float(value) => Some(ScalarExpr::Number(*value)),
            Constant::Bool(value) => Some(ScalarExpr::Number(if *value { 1.0 } else { 0.0 })),
            _ => None,
        },
        Expr::Name(name) => Some(
            env.get(name.id.as_str())
                .cloned()
                .unwrap_or_else(|| ScalarExpr::Symbol(name.id.to_string())),
        ),
        Expr::UnaryOp(unary) => match unary.op {
            ast::UnaryOp::USub => Some(ScalarExpr::UnaryNeg(Box::new(
                parse_scalar_expr(&unary.operand, env)?.ok_or_else(|| {
                    unsupported_python(
                        source_position_expr(&unary.operand),
                        "unsupported unary operand",
                    )
                })?,
            ))),
            ast::UnaryOp::UAdd => parse_scalar_expr(&unary.operand, env)?,
            _ => None,
        },
        Expr::BinOp(bin) => {
            let left = parse_scalar_expr(&bin.left, env)?.ok_or_else(|| {
                unsupported_python(source_position_expr(&bin.left), "unsupported left operand")
            })?;
            let right = parse_scalar_expr(&bin.right, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&bin.right),
                    "unsupported right operand",
                )
            })?;
            let op = match bin.op {
                ast::Operator::Add => ScalarOp::Add,
                ast::Operator::Sub => ScalarOp::Sub,
                ast::Operator::Mult => ScalarOp::Mul,
                ast::Operator::Div => ScalarOp::Div,
                ast::Operator::FloorDiv => {
                    return Ok(Some(ScalarExpr::Call {
                        func: ScalarFunc::Floor,
                        args: vec![ScalarExpr::Binary {
                            op: ScalarOp::Div,
                            left: Box::new(left),
                            right: Box::new(right),
                        }],
                    }))
                }
                _ => return Ok(None),
            };
            Some(ScalarExpr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        Expr::IfExp(ifexp) => {
            let cond = parse_bool_expr(&ifexp.test, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&ifexp.test),
                    "unsupported conditional test",
                )
            })?;
            let then_expr = parse_scalar_expr(&ifexp.body, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&ifexp.body),
                    "unsupported conditional body",
                )
            })?;
            let else_expr = parse_scalar_expr(&ifexp.orelse, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&ifexp.orelse),
                    "unsupported conditional else",
                )
            })?;
            Some(ScalarExpr::IfElse {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        }
        Expr::Call(call) => parse_scalar_call(call, env)?,
        Expr::Attribute(attr) => {
            if let Expr::Name(base) = &*attr.value {
                if base.id.as_str() == "math" && attr.attr.as_str() == "pi" {
                    Some(ScalarExpr::Number(std::f64::consts::PI))
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(out)
}

fn parse_scalar_call(
    call: &ast::ExprCall,
    env: &BTreeMap<String, ScalarExpr>,
) -> AppResult<Option<ScalarExpr>> {
    if let Expr::Attribute(attr) = &*call.func {
        if let Expr::Name(base) = &*attr.value {
            if base.id.as_str() == "params" && attr.attr.as_str() == "get" && !call.args.is_empty()
            {
                let key = parse_string_constant(&call.args[0]).ok_or_else(|| {
                    unsupported_python(
                        source_position_expr(&call.args[0]),
                        "params.get key must be string",
                    )
                })?;
                return Ok(Some(ScalarExpr::Symbol(key)));
            }
            if base.id.as_str() == "math" {
                let func = match attr.attr.as_str() {
                    "sin" => ScalarFunc::Sin,
                    "cos" => ScalarFunc::Cos,
                    "tan" => ScalarFunc::Tan,
                    "atan" => ScalarFunc::Atan,
                    "floor" => ScalarFunc::Floor,
                    "fabs" => ScalarFunc::Abs,
                    "degrees" => {
                        let arg = parse_scalar_expr(&call.args[0], env)?.ok_or_else(|| {
                            unsupported_python(
                                source_position_expr(&call.args[0]),
                                "unsupported math.degrees arg",
                            )
                        })?;
                        return Ok(Some(ScalarExpr::Binary {
                            op: ScalarOp::Mul,
                            left: Box::new(ScalarExpr::Number(180.0 / std::f64::consts::PI)),
                            right: Box::new(arg),
                        }));
                    }
                    "radians" => {
                        let arg = parse_scalar_expr(&call.args[0], env)?.ok_or_else(|| {
                            unsupported_python(
                                source_position_expr(&call.args[0]),
                                "unsupported math.radians arg",
                            )
                        })?;
                        return Ok(Some(ScalarExpr::Binary {
                            op: ScalarOp::Mul,
                            left: Box::new(ScalarExpr::Number(std::f64::consts::PI / 180.0)),
                            right: Box::new(arg),
                        }));
                    }
                    _ => return Ok(None),
                };
                let args = call
                    .args
                    .iter()
                    .map(|arg| {
                        parse_scalar_expr(arg, env)?.ok_or_else(|| {
                            unsupported_python(
                                source_position_expr(arg),
                                "unsupported math call arg",
                            )
                        })
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                return Ok(Some(ScalarExpr::Call { func, args }));
            }
        }
    }

    if let Expr::Name(name) = &*call.func {
        match name.id.as_str() {
            "int" | "float" => {
                let arg = parse_scalar_expr(&call.args[0], env)?.ok_or_else(|| {
                    unsupported_python(source_position_expr(&call.args[0]), "unsupported cast arg")
                })?;
                return Ok(Some(if name.id.as_str() == "int" {
                    ScalarExpr::Call {
                        func: ScalarFunc::Floor,
                        args: vec![arg],
                    }
                } else {
                    arg
                }));
            }
            "abs" => {
                let arg = parse_scalar_expr(&call.args[0], env)?.ok_or_else(|| {
                    unsupported_python(source_position_expr(&call.args[0]), "unsupported abs arg")
                })?;
                return Ok(Some(ScalarExpr::Call {
                    func: ScalarFunc::Abs,
                    args: vec![arg],
                }));
            }
            "min" | "max" => {
                let func = if name.id.as_str() == "min" {
                    ScalarFunc::Min
                } else {
                    ScalarFunc::Max
                };
                let args = call
                    .args
                    .iter()
                    .map(|arg| {
                        parse_scalar_expr(arg, env)?.ok_or_else(|| {
                            unsupported_python(source_position_expr(arg), "unsupported min/max arg")
                        })
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                return Ok(Some(ScalarExpr::Call { func, args }));
            }
            _ => {}
        }
    }

    Ok(None)
}

fn parse_bool_expr(expr: &Expr, env: &BTreeMap<String, ScalarExpr>) -> AppResult<Option<BoolExpr>> {
    let out = match expr {
        Expr::Constant(constant) => match &constant.value {
            Constant::Bool(value) => Some(BoolExpr::Bool(*value)),
            _ => None,
        },
        Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::Not) => Some(BoolExpr::Not(
            Box::new(parse_bool_expr(&unary.operand, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&unary.operand),
                    "unsupported not operand",
                )
            })?),
        )),
        Expr::BoolOp(bool_op) => {
            let values = bool_op
                .values
                .iter()
                .map(|value| {
                    parse_bool_expr(value, env)?.ok_or_else(|| {
                        unsupported_python(
                            source_position_expr(value),
                            "unsupported boolean operand",
                        )
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            match bool_op.op {
                ast::BoolOp::And => Some(BoolExpr::And(values)),
                ast::BoolOp::Or => Some(BoolExpr::Or(values)),
            }
        }
        Expr::Compare(compare) if compare.ops.len() == 1 && compare.comparators.len() == 1 => {
            let left = parse_scalar_expr(&compare.left, env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&compare.left),
                    "unsupported compare left",
                )
            })?;
            let right = parse_scalar_expr(&compare.comparators[0], env)?.ok_or_else(|| {
                unsupported_python(
                    source_position_expr(&compare.comparators[0]),
                    "unsupported compare right",
                )
            })?;
            let op = match compare.ops[0] {
                ast::CmpOp::Eq => CompareOp::Eq,
                ast::CmpOp::Lt => CompareOp::Lt,
                ast::CmpOp::LtE => CompareOp::Lte,
                ast::CmpOp::Gt => CompareOp::Gt,
                ast::CmpOp::GtE => CompareOp::Gte,
                _ => return Ok(None),
            };
            Some(BoolExpr::Compare {
                op,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        _ => None,
    };
    Ok(out)
}

fn emit_thomas_ramp_ir(spec: &ThomasRampSpec) -> String {
    let params_block = emit_params_block(&spec.parsed_params);
    let width = n("width");
    let gauge = n("gauge");
    let height = n("height");
    let groove_width = n("groove_width");
    let groove_depth = n("groove_depth");
    let flat_start = n("flat_start");
    let ramp_length = n("ramp_length");
    let flat_end = n("flat_end");
    let has_teeth = b("has_teeth");
    let teeth_size = n("teeth_size");
    let num_segments = n("num_segments");
    let print_segment = n("print_segment");
    let dz = spec.top_env["dz"].to_ir();
    let total_len = spec.top_env["L"].to_ir();
    let duplo_r = spec.top_env["duplo_r"].to_ir();
    let duplo_depth = spec.top_env["duplo_depth"].to_ir();
    let duplo_pitch = spec.top_env["duplo_pitch"].to_ir();
    let body_samples = build_ramp_samples(spec, LEGACY_RAMP_SEGMENTS);
    let groove_samples = build_ramp_samples(spec, GROOVE_PATH_SEGMENTS);
    let track_body = body_expr(&body_samples, &width, &height, &dz, &total_len);
    let groove_profile = groove_profile_expr(&height, &groove_width, &groove_depth);
    let groove_path = polyline_path(&groove_samples);
    let track_grooved = format!(
        "(cut track_body (translate 0 (/ {gauge} 2) 0 (sweep groove_profile groove_path)) (translate 0 (- (/ {gauge} 2)) 0 (sweep groove_profile groove_path)))"
    );
    let track_with_teeth = teeth_expr(
        spec,
        "track_grooved",
        &gauge,
        &height,
        &groove_width,
        &groove_depth,
        &teeth_size,
        &total_len,
        &has_teeth,
    );
    let track_with_holes = holes_expr(
        "track_with_teeth",
        &flat_start,
        &ramp_length,
        &flat_end,
        &total_len,
        &duplo_pitch,
        &duplo_r,
        &duplo_depth,
        &dz,
    );
    let connector_cutout = connector_shape_expr("6.0", "7.0", "8.5", &height);
    let connector_peg = connector_shape_expr("5.5", "6.0", "8.0", &height);
    let start_frame = "(path-frame groove_path :at start)".to_string();
    let end_frame = "(path-frame groove_path :at end)".to_string();
    let track_with_connectors = connectors_expr("track_with_holes");
    let final_track = segments_expr(
        "track_original",
        &num_segments,
        &print_segment,
        &width,
        &height,
        &dz,
        &total_len,
    );
    let mut bindings: Vec<(String, String)> = Vec::new();
    bindings.extend(
        body_samples
            .iter()
            .map(|sample| (sample.z_name.clone(), sample.z_expr.clone()))
            .collect::<Vec<_>>(),
    );
    bindings.extend([
        ("track_body".to_string(), track_body),
        ("groove_profile".to_string(), groove_profile),
        ("groove_path".to_string(), groove_path),
        ("track_grooved".to_string(), track_grooved),
        ("track_with_teeth".to_string(), track_with_teeth),
        ("track_with_holes".to_string(), track_with_holes),
        ("connector_cutout".to_string(), connector_cutout),
        ("connector_peg".to_string(), connector_peg),
        ("start_frame".to_string(), start_frame),
        ("end_frame".to_string(), end_frame),
        ("track_original".to_string(), track_with_connectors),
    ]);
    let part_body = build_block_owned(&bindings, final_track);

    format!("(model\n  {params_block}\n  (part body\n    {part_body}))\n")
}

fn build_block_owned(bindings: &[(String, String)], body: String) -> String {
    let mut lines = vec!["(build".to_string()];
    for (name, value) in bindings {
        lines.push(format!("      (shape {name} {value})"));
    }
    lines.push(format!("      (result {body}))"));
    lines.join("\n")
}

fn body_expr(
    samples: &[RampSample],
    width: &str,
    height: &str,
    dz: &str,
    total_len: &str,
) -> String {
    let mut points = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        points.push(format!(
            "({} (+ {} {height}))",
            sample.x_expr, sample.z_name
        ));
    }
    for sample in samples.iter().rev() {
        points.push(format!("({} {})", sample.x_expr, sample.z_name));
    }
    format!(
        "(translate (/ {total_len} 2) (- (/ {width} 2)) (/ (+ {dz} {height}) 2) (rotate 90 0 0 (extrude (polygon ({})) {width})))",
        points.join(" ")
    )
}

fn groove_profile_expr(height: &str, groove_width: &str, groove_depth: &str) -> String {
    let top_w = format!("(+ {groove_width} 2)");
    let bot_w = format!("(- {groove_width} 2)");
    format!(
        "(polygon (((- (/ {bot_w} 2)) (- {height} {groove_depth})) ((/ {bot_w} 2) (- {height} {groove_depth})) ((/ {top_w} 2) (+ {height} 2)) ((- (/ {top_w} 2)) (+ {height} 2))))"
    )
}

#[allow(clippy::too_many_arguments)]
fn teeth_expr(
    spec: &ThomasRampSpec,
    base_track: &str,
    gauge: &str,
    height: &str,
    groove_width: &str,
    groove_depth: &str,
    teeth_size: &str,
    total_len: &str,
    has_teeth: &str,
) -> String {
    let num_teeth = format!("(floor (/ {total_len} {teeth_size}))");
    let x_pos = "(+ (* (+ i 0.5) teeth_size) 0.0)";
    let z_pos = format!(
        "(+ {} {height} (- {groove_depth}))",
        apply_scalar(&spec.z_at, x_pos)
    );
    let slope = apply_scalar(&spec.dz_dx_at, x_pos);
    let angle_rad = format!("(atan {slope})");
    let angle_deg = format!("(rad {angle_rad})");
    let actual_pitch = format!("(/ {teeth_size} (cos {angle_rad}))");
    let tooth_unit = inverse_tooth_expr(&actual_pitch, groove_width);
    let left = format!(
        "(translate {x_pos} (/ {gauge} 2) {z_pos} (rotate 0 (- {angle_deg}) 0 {tooth_unit}))"
    );
    let right = format!(
        "(translate {x_pos} (- (/ {gauge} 2)) {z_pos} (rotate 0 (- {angle_deg}) 0 {tooth_unit}))"
    );
    let valleys = format!("(repeat-compound i {num_teeth} (compound {left} {right}))");
    format!("(if (and {has_teeth} (> {num_teeth} 0)) (cut {base_track} {valleys}) {base_track})")
}

#[allow(clippy::too_many_arguments)]
fn holes_expr(
    base_track: &str,
    flat_start: &str,
    ramp_length: &str,
    flat_end: &str,
    _total_len: &str,
    duplo_pitch: &str,
    duplo_r: &str,
    duplo_depth: &str,
    dz: &str,
) -> String {
    let start_count = format!("(max 0 (floor (- (/ {flat_start} {duplo_pitch}) 0.5)))");
    let end_count = format!("(max 0 (floor (- (/ {flat_end} {duplo_pitch}) 0.5)))");
    let start_x = format!("(+ (* {duplo_pitch} 1.5) (* i {duplo_pitch}))");
    let ramp_end = format!("(+ {flat_start} {ramp_length})");
    let end_x = format!("(+ {ramp_end} (/ {duplo_pitch} 2) (* i {duplo_pitch}))");
    let left_start = format!(
        "(translate {start_x} (- (/ {duplo_pitch} 2)) 0 (cylinder {duplo_r} {duplo_depth}))"
    );
    let right_start =
        format!("(translate {start_x} (/ {duplo_pitch} 2) 0 (cylinder {duplo_r} {duplo_depth}))");
    let left_end = format!(
        "(translate {end_x} (- (/ {duplo_pitch} 2)) {dz} (cylinder {duplo_r} {duplo_depth}))"
    );
    let right_end =
        format!("(translate {end_x} (/ {duplo_pitch} 2) {dz} (cylinder {duplo_r} {duplo_depth}))");
    let with_start = format!(
        "(if (> {start_count} 0) (cut {base_track} (repeat-compound i {start_count} (compound {left_start} {right_start}))) {base_track})"
    );
    format!(
        "(if (> {end_count} 0) (cut {with_start} (repeat-compound i {end_count} (compound {left_end} {right_end}))) {with_start})"
    )
}

fn connectors_expr(base_track: &str) -> String {
    format!(
        "(fuse (cut {base_track} (place start_frame connector_cutout)) (place end_frame connector_peg))"
    )
}

fn connector_shape_expr(radius: &str, neck_width: &str, neck_length: &str, height: &str) -> String {
    let cylinder = format!("(translate {neck_length} 0 0 (cylinder {radius} {height}))");
    let box_shape =
        format!("(translate (/ {neck_length} 2) 0 0 (box {neck_length} {neck_width} {height}))");
    format!("(fuse {cylinder} {box_shape})")
}

fn inverse_tooth_expr(length: &str, groove_width: &str) -> String {
    let sx = format!("(/ {length} 3.5)");
    let profile = format!(
        "(polygon (((* -1.75 {sx}) 10) ((* -1.75 {sx}) 0) ((* -0.75 {sx}) -2.5) ((* 0.75 {sx}) -2.5) ((* 1.75 {sx}) 0) ((* 1.75 {sx}) 10)))"
    );
    format!(
        "(translate 0 (/ {groove_width} 2) 0 (rotate 90 0 0 (extrude {profile} {groove_width})))"
    )
}

fn puzzle_tool_expr(height: &str, dz: &str, x_pos: &str) -> String {
    let tall = format!("(+ {dz} {height} 100)");
    let cylinder = format!("(translate (+ {x_pos} 4.5) 0 -50 (cylinder 4.5 {tall}))");
    let box_shape = format!("(translate (+ {x_pos} 2.25) 0 -50 (box 4.5 6.0 {tall}))");
    format!("(fuse {cylinder} {box_shape})")
}

fn segment_shape_expr(
    track_original: &str,
    width: &str,
    height: &str,
    dz: &str,
    total_len: &str,
) -> String {
    let seg_len = format!("(/ {total_len} (max 1 num_segments))");
    let x0 = format!("(* i {seg_len})");
    let x1 = format!("(* (+ i 1) {seg_len})");
    let box_x0 = format!("(if (= i 0) (- {x0} 20) {x0})");
    let box_x1 = format!("(if (= i (- (max 1 num_segments) 1)) (+ {x1} 20) {x1})");
    let bbox = format!(
        "(clip-box {track_original} :x ({box_x0} {box_x1}) :y ((- (/ (+ {width} 50) 2)) (/ (+ {width} 50) 2)) :z (-20 (+ {dz} {height} 30)))"
    );
    let seg0 = bbox;
    let cut_x0 = format!(
        "(if (> i 0) (cut {seg0} {}) {seg0})",
        puzzle_tool_expr(height, dz, &x0)
    );
    let peg_x1 = format!(
        "(common {track_original} {})",
        puzzle_tool_expr(height, dz, &x1)
    );
    format!("(if (< i (- (max 1 num_segments) 1)) (fuse {cut_x0} {peg_x1}) {cut_x0})")
}

fn print_segment_body(
    track_original: &str,
    width: &str,
    height: &str,
    dz: &str,
    total_len: &str,
) -> String {
    let seg = segment_shape_expr(track_original, width, height, dz, total_len);
    let seg_len = format!("(/ {total_len} (max 1 num_segments))");
    let x0 = format!("(* i {seg_len})");
    let x1 = format!("(* (+ i 1) {seg_len})");
    format!("(translate (- (/ (+ {x0} {x1}) 2)) 0 (/ {width} 2) (rotate 90 0 0 {seg}))")
}

fn assembled_segments_body(
    track_original: &str,
    width: &str,
    height: &str,
    dz: &str,
    total_len: &str,
) -> String {
    let seg = segment_shape_expr(track_original, width, height, dz, total_len);
    let offset_y = "(* (- i (/ (- (max 1 num_segments) 1) 2)) 15)";
    format!("(translate 0 {offset_y} 0 {seg})")
}

fn segments_expr(
    track_original: &str,
    num_segments: &str,
    print_segment: &str,
    width: &str,
    height: &str,
    dz: &str,
    total_len: &str,
) -> String {
    let num_segments_clamped = format!("(max 1 {num_segments})");
    let seg_idx = format!("(min (- {num_segments_clamped} 1) (max 0 (- {print_segment} 1)))");
    let single_print = format!(
        "(translate (- (/ {total_len} 2)) 0 (/ {width} 2) (rotate 90 0 0 {track_original}))"
    );
    let assembled = format!(
        "(if (= {num_segments_clamped} 1) {track_original} (repeat-compound i {num_segments_clamped} {}))",
        assembled_segments_body(track_original, width, height, dz, total_len)
    );
    let selected = format!(
        "(if (= {num_segments_clamped} 1) {single_print} (repeat-pick i {num_segments_clamped} (= i {seg_idx}) {}))",
        print_segment_body(track_original, width, height, dz, total_len)
    );
    format!("(if (= {print_segment} 0) {assembled} {selected})")
}

#[derive(Clone, Debug)]
struct RampSample {
    x_expr: String,
    z_name: String,
    z_expr: String,
}

fn build_ramp_samples(spec: &ThomasRampSpec, segment_count: usize) -> Vec<RampSample> {
    ramp_samples(
        &spec.top_env["L"].to_ir(),
        &spec.top_env["flat_start"].to_ir(),
        &spec.top_env["ramp_length"].to_ir(),
        segment_count,
    )
    .into_iter()
    .enumerate()
    .map(|(index, x_expr)| RampSample {
        z_name: format!("knot_z_{index}"),
        z_expr: apply_scalar(&spec.z_at, &x_expr),
        x_expr,
    })
    .collect()
}

fn polyline_path(samples: &[RampSample]) -> String {
    let points = samples
        .iter()
        .map(|sample| format!("({} 0 {})", sample.x_expr, sample.z_name))
        .collect::<Vec<_>>()
        .join(" ");
    format!("(path {points})")
}

fn ramp_samples(
    total_len: &str,
    flat_start: &str,
    ramp_length: &str,
    segment_count: usize,
) -> Vec<String> {
    let mut knots = vec!["0".to_string(), flat_start.to_string()];
    for step in 1..segment_count {
        let t = step as f64 / segment_count as f64;
        knots.push(format!("(+ {flat_start} (* {ramp_length} {}))", fmt_num(t)));
    }
    knots.push(format!("(+ {flat_start} {ramp_length})"));
    knots.push(total_len.to_string());
    knots
}

fn apply_scalar(expr: &ScalarExpr, x_expr: &str) -> String {
    expr.substitute("x", &ScalarExpr::Symbol(x_expr.to_string()))
        .to_ir()
}

impl ScalarExpr {
    fn substitute(&self, symbol: &str, replacement: &ScalarExpr) -> ScalarExpr {
        match self {
            Self::Number(value) => Self::Number(*value),
            Self::Symbol(name) if name == symbol => replacement.clone(),
            Self::Symbol(name) => Self::Symbol(name.clone()),
            Self::UnaryNeg(expr) => Self::UnaryNeg(Box::new(expr.substitute(symbol, replacement))),
            Self::Binary { op, left, right } => Self::Binary {
                op: *op,
                left: Box::new(left.substitute(symbol, replacement)),
                right: Box::new(right.substitute(symbol, replacement)),
            },
            Self::Call { func, args } => Self::Call {
                func: *func,
                args: args
                    .iter()
                    .map(|arg| arg.substitute(symbol, replacement))
                    .collect(),
            },
            Self::IfElse {
                cond,
                then_expr,
                else_expr,
            } => Self::IfElse {
                cond: Box::new(cond.substitute(symbol, replacement)),
                then_expr: Box::new(then_expr.substitute(symbol, replacement)),
                else_expr: Box::new(else_expr.substitute(symbol, replacement)),
            },
        }
    }

    fn to_ir(&self) -> String {
        match self {
            Self::Number(value) => fmt_num(*value),
            Self::Symbol(name) => name.clone(),
            Self::UnaryNeg(expr) => format!("(- {})", expr.to_ir()),
            Self::Binary { op, left, right } => {
                format!("({} {} {})", op.as_ir(), left.to_ir(), right.to_ir())
            }
            Self::Call { func, args } => {
                let args = args.iter().map(Self::to_ir).collect::<Vec<_>>().join(" ");
                format!("({} {})", func.as_ir(), args)
            }
            Self::IfElse {
                cond,
                then_expr,
                else_expr,
            } => format!(
                "(if {} {} {})",
                cond.to_ir(),
                then_expr.to_ir(),
                else_expr.to_ir()
            ),
        }
    }
}

impl BoolExpr {
    fn substitute(&self, symbol: &str, replacement: &ScalarExpr) -> BoolExpr {
        match self {
            Self::Bool(value) => Self::Bool(*value),
            Self::Compare { op, left, right } => Self::Compare {
                op: *op,
                left: Box::new(left.substitute(symbol, replacement)),
                right: Box::new(right.substitute(symbol, replacement)),
            },
            Self::And(values) => Self::And(
                values
                    .iter()
                    .map(|value| value.substitute(symbol, replacement))
                    .collect(),
            ),
            Self::Or(values) => Self::Or(
                values
                    .iter()
                    .map(|value| value.substitute(symbol, replacement))
                    .collect(),
            ),
            Self::Not(value) => Self::Not(Box::new(value.substitute(symbol, replacement))),
        }
    }

    fn to_ir(&self) -> String {
        match self {
            Self::Bool(value) => {
                if *value {
                    "#t".to_string()
                } else {
                    "#f".to_string()
                }
            }
            Self::Compare { op, left, right } => {
                format!("({} {} {})", op.as_ir(), left.to_ir(), right.to_ir())
            }
            Self::And(values) => format!(
                "(and {})",
                values.iter().map(Self::to_ir).collect::<Vec<_>>().join(" ")
            ),
            Self::Or(values) => format!(
                "(or {})",
                values.iter().map(Self::to_ir).collect::<Vec<_>>().join(" ")
            ),
            Self::Not(value) => format!("(not {})", value.to_ir()),
        }
    }
}

impl ScalarOp {
    fn as_ir(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        }
    }
}

impl ScalarFunc {
    fn as_ir(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Atan => "atan",
            Self::Abs => "abs",
            Self::Floor => "floor",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}

impl CompareOp {
    fn as_ir(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
        }
    }
}

fn emit_params_block(parsed: &ParsedParamsResult) -> String {
    let mut entries = Vec::new();
    for field in &parsed.fields {
        let key = field.key();
        let default = parsed.params.get(key).unwrap_or(&ParamValue::Null);
        let entry = match field {
            UiField::Number {
                label,
                min,
                max,
                step,
                ..
            }
            | UiField::Range {
                label,
                min,
                max,
                step,
                ..
            } => {
                let mut parts = vec![
                    "number".to_string(),
                    key.to_string(),
                    param_value_to_ir(default),
                    ":label".to_string(),
                    quote(label),
                ];
                if let Some(min) = min {
                    parts.push(":min".to_string());
                    parts.push(fmt_num(*min));
                }
                if let Some(max) = max {
                    parts.push(":max".to_string());
                    parts.push(fmt_num(*max));
                }
                if let Some(step) = step {
                    parts.push(":step".to_string());
                    parts.push(fmt_num(*step));
                }
                format!("({})", parts.join(" "))
            }
            UiField::Checkbox { label, .. } => format!(
                "(toggle {} {} :label {})",
                key,
                param_value_to_ir(default),
                quote(label)
            ),
            UiField::Image { label, .. } => format!(
                "(image {} {} :label {})",
                key,
                param_value_to_ir(default),
                quote(label)
            ),
            UiField::Select { label, options, .. } => {
                let options = options
                    .iter()
                    .map(|option| {
                        format!(
                            "({} {})",
                            quote(&option.label),
                            select_value_to_ir(&option.value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "(select {} {} :label {} :options ({}))",
                    key,
                    param_value_to_ir(default),
                    quote(label),
                    options
                )
            }
        };
        entries.push(entry);
    }
    format!("(params\n    {})", entries.join("\n    "))
}

fn select_value_to_ir(value: &SelectValue) -> String {
    match value {
        SelectValue::String(text) => quote(text),
        SelectValue::Number(number) => fmt_num(*number),
    }
}

fn param_value_to_ir(value: &ParamValue) -> String {
    match value {
        ParamValue::String(text) => quote(text),
        ParamValue::Number(number) => fmt_num(*number),
        ParamValue::Boolean(flag) => {
            if *flag {
                "#t".to_string()
            } else {
                "#f".to_string()
            }
        }
        ParamValue::Null => "0".to_string(),
    }
}

fn parse_string_constant(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Constant(constant) => match &constant.value {
            Constant::Str(text) => Some(text.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn fmt_num(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        let raw = format!("{:.12}", value);
        raw.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn quote(text: &str) -> String {
    format!("{:?}", text)
}

fn n(name: &str) -> String {
    name.to_string()
}

fn b(name: &str) -> String {
    name.to_string()
}

fn source_position_expr(_expr: &Expr) -> (usize, usize) {
    (0, 0)
}

fn source_position_stmt(_stmt: &Stmt) -> (usize, usize) {
    (0, 0)
}

fn unsupported_python(pos: (usize, usize), detail: &str) -> AppError {
    AppError::validation(format!(
        "Legacy Python translator unsupported at {}:{}: {}.",
        pos.0, pos.1, detail
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build123d;
    use crate::ecky_ir::lower_to_build123d;

    /// Production lowers on a 32 MiB guarded thread (`lower_ecky_with_large_stack`);
    /// default test threads are too small for the thomas fixture's recursion depth.
    fn lower_on_guarded_stack(macro_code: &str) -> String {
        let code = macro_code.to_string();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || lower_to_build123d(&code).expect("lower"))
            .expect("spawn lowering thread")
            .join()
            .expect("join lowering thread")
    }
    use crate::freecad;
    use crate::models::PathResolver;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("cad")
    }

    fn fixture_reference() -> PathBuf {
        fixture_root()
            .join("reference")
            .join("thomas_modular_ramp_legacy.py")
    }

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
            match path {
                "server/build123d_runner.py" => {
                    Some(repo_root.join("server").join("build123d_runner.py"))
                }
                "runtime/build123d/bin/python3" => Some(
                    repo_root
                        .join(".dist")
                        .join("build123d-runtime")
                        .join("bin")
                        .join("python3"),
                ),
                "runtime/build123d/bin/python" => Some(
                    repo_root
                        .join(".dist")
                        .join("build123d-runtime")
                        .join("bin")
                        .join("python"),
                ),
                _ => None,
            }
        }
    }

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("ecky-legacy-xlate-{}", uuid::Uuid::new_v4()))
    }

    fn has_freecad() -> bool {
        freecad::resolve_freecad_path(None).is_ok()
    }

    #[test]
    fn translates_thomas_fixture_to_ecky_ir() {
        let source = fs::read_to_string(fixture_reference()).expect("fixture");
        let result = translate_legacy_python_to_ecky_ir(&source).expect("translate");
        assert!(
            result.macro_code.starts_with("(model"),
            "{}",
            result.macro_code
        );
        assert!(
            result.macro_code.contains("(build"),
            "{}",
            result.macro_code
        );
        assert!(
            result.macro_code.contains("(repeat-pick i"),
            "{}",
            result.macro_code
        );
        assert!(
            result.macro_code.contains("(clip-box"),
            "{}",
            result.macro_code
        );
        assert!(
            result
                .macro_code
                .contains("(path-frame groove_path :at end)"),
            "{}",
            result.macro_code
        );
        assert!(
            result
                .macro_code
                .contains("(place end_frame connector_peg)"),
            "{}",
            result.macro_code
        );
        assert!(
            result.macro_code.contains("(path "),
            "{}",
            result.macro_code
        );
        assert!(
            result.macro_code.contains("(polygon"),
            "{}",
            result.macro_code
        );
    }

    #[test]
    fn translated_thomas_fixture_lowers_to_smooth_build123d_code() {
        let source = fs::read_to_string(fixture_reference()).expect("fixture");
        let result = translate_legacy_python_to_ecky_ir(&source).expect("translate");
        let code = lower_on_guarded_stack(&result.macro_code);
        assert!(code.contains("Polyline("), "{}", code);
        assert!(
            code.contains("Rectangle(")
                || code.contains("Polygon(")
                || code.contains("_ecky_polygon("),
            "{}",
            code
        );
        assert!(code.contains("_ecky_place("), "{}", code);
        assert!(code.contains("_ecky_clip_box("), "{}", code);
        assert!(code.contains("_ecky_fuse_many("), "{}", code);
    }

    #[test]
    fn translated_thomas_fixture_emits_model_source() {
        let source = fs::read_to_string(fixture_reference()).expect("fixture");
        let result = translate_legacy_python_to_ecky_ir(&source).expect("translate");
        assert!(
            result.macro_code.contains("(model"),
            "{}",
            result.macro_code
        );
        assert!(
            result.macro_code.contains("(part "),
            "{}",
            result.macro_code
        );
    }

    #[test]
    fn translated_thomas_fixture_parity_harness() {
        if !has_freecad() {
            eprintln!("skip parity: freecad unavailable");
            return;
        }
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("server")
            .join("check_thomas_ramp_parity.py");
        let output = Command::new("python3")
            .arg(script)
            .output()
            .expect("run parity");
        assert!(
            output.status.success(),
            "stdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn render_translated_thomas_fixture_on_build123d() {
        let source = fs::read_to_string(fixture_reference()).expect("fixture");
        let result = translate_legacy_python_to_ecky_ir(&source).expect("translate");
        let lowered = lower_on_guarded_stack(&result.macro_code);
        let root = test_root();
        fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let bundle =
            build123d::render_model(&lowered, &BTreeMap::new(), &resolver).expect("render");
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }
}
