use std::collections::BTreeMap;

use crate::ecky_deterministic;
use crate::models::{AppResult, ParamValue};

use super::model::{expr_head_symbol, expr_list_items, inline_let_expr, IrExpr};
use super::shared::{unsupported, validation};

pub(super) fn eval_number(value: &IrExpr, env: &BTreeMap<String, ParamValue>) -> AppResult<f64> {
    let value = inline_let_expr(value)?;
    if let Some(number) = value.as_f64() {
        return Ok(number);
    }
    if let Some(symbol) = value.as_symbol() {
        return env
            .get(symbol)
            .and_then(|param| match param {
                ParamValue::Number(number) => Some(*number),
                _ => None,
            })
            .ok_or_else(|| validation(format!("Unknown numeric symbol `{}`.", symbol)));
    }

    let items = expr_list_items(&value, "numeric expression")?;
    let op = expr_head_symbol(items, "numeric expression")?;
    let args = &items[1..];
    match op {
        "+" => args
            .iter()
            .try_fold(0.0, |acc, arg| Ok(acc + eval_number(arg, env)?)),
        "-" => {
            if args.is_empty() {
                return Err(validation("`-` expects at least one numeric argument."));
            }
            if args.len() == 1 {
                Ok(-eval_number(&args[0], env)?)
            } else {
                let first = eval_number(&args[0], env)?;
                args[1..]
                    .iter()
                    .try_fold(first, |acc, arg| Ok(acc - eval_number(arg, env)?))
            }
        }
        "*" => args
            .iter()
            .try_fold(1.0, |acc, arg| Ok(acc * eval_number(arg, env)?)),
        "/" => {
            if args.len() != 2 {
                return Err(validation("`/` expects exactly two numeric arguments."));
            }
            Ok(eval_number(&args[0], env)? / eval_number(&args[1], env)?)
        }
        "min" => args.iter().try_fold(
            f64::INFINITY,
            |acc, arg| Ok(acc.min(eval_number(arg, env)?)),
        ),
        "max" => args.iter().try_fold(f64::NEG_INFINITY, |acc, arg| {
            Ok(acc.max(eval_number(arg, env)?))
        }),
        "clamp" => {
            if args.len() != 3 {
                return Err(validation("`clamp` expects value, min, and max."));
            }
            Ok(eval_number(&args[0], env)?
                .max(eval_number(&args[1], env)?)
                .min(eval_number(&args[2], env)?))
        }
        "lerp" => {
            if args.len() != 3 {
                return Err(validation("`lerp` expects start, end, and t."));
            }
            let start = eval_number(&args[0], env)?;
            let end = eval_number(&args[1], env)?;
            let t = eval_number(&args[2], env)?;
            Ok(start + (end - start) * t)
        }
        "smoothstep" => {
            if args.len() != 3 {
                return Err(validation("`smoothstep` expects edge0, edge1, and x."));
            }
            let edge0 = eval_number(&args[0], env)?;
            let edge1 = eval_number(&args[1], env)?;
            if approx_eq(edge0, edge1) {
                return Err(validation("`smoothstep` needs distinct edge values."));
            }
            let x = eval_number(&args[2], env)?;
            let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
            Ok(t * t * (3.0 - 2.0 * t))
        }
        "if" => {
            if args.len() != 3 {
                return Err(validation("`if` expects condition, then, else."));
            }
            if eval_bool(&args[0], env)? {
                eval_number(&args[1], env)
            } else {
                eval_number(&args[2], env)
            }
        }
        "sin" => unary_op(args, env, f64::sin),
        "cos" => unary_op(args, env, f64::cos),
        "tan" => unary_op(args, env, f64::tan),
        "atan" => unary_op(args, env, f64::atan),
        "atan2" => {
            if args.len() != 2 {
                return Err(validation("`atan2` expects y and x."));
            }
            Ok(eval_number(&args[0], env)?.atan2(eval_number(&args[1], env)?))
        }
        "deg" | "deg->rad" => unary_op(args, env, |value| value.to_radians()),
        "rad" | "rad->deg" => unary_op(args, env, |value| value.to_degrees()),
        "abs" => unary_op(args, env, f64::abs),
        "floor" => unary_op(args, env, f64::floor),
        "signed-pow" => binary_op(args, env, |value, exponent| {
            value.signum() * value.abs().powf(exponent)
        }),
        "hash01" => ternary_op(args, env, ecky_deterministic::hash01),
        "hash-signed" => ternary_op(args, env, ecky_deterministic::hash_signed),
        "noise2" => ternary_op(args, env, ecky_deterministic::noise2),
        "voronoi2" => ternary_op(args, env, ecky_deterministic::voronoi2),
        "cell-distance2" => ternary_op(args, env, ecky_deterministic::cell_distance2),
        "fbm2" => {
            if args.len() != 6 {
                return Err(validation(
                    "`fbm2` expects x, y, seed, octaves, lacunarity, and gain.",
                ));
            }
            Ok(ecky_deterministic::fbm2(
                eval_number(&args[0], env)?,
                eval_number(&args[1], env)?,
                eval_number(&args[2], env)?,
                eval_number(&args[3], env)?,
                eval_number(&args[4], env)?,
                eval_number(&args[5], env)?,
            ))
        }
        other => Err(unsupported(format!(
            "Numeric operator `{}` is not supported by current `.ecky` runtime.",
            other
        ))),
    }
}

pub(super) fn unary_op(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    op: impl Fn(f64) -> f64,
) -> AppResult<f64> {
    if args.len() != 1 {
        return Err(validation(
            "Unary numeric operator expects exactly one argument.",
        ));
    }
    Ok(op(eval_number(&args[0], env)?))
}

fn binary_op(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    op: impl Fn(f64, f64) -> f64,
) -> AppResult<f64> {
    if args.len() != 2 {
        return Err(validation(
            "Binary numeric operator expects exactly two arguments.",
        ));
    }
    Ok(op(eval_number(&args[0], env)?, eval_number(&args[1], env)?))
}

fn ternary_op(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    op: impl Fn(f64, f64, f64) -> f64,
) -> AppResult<f64> {
    if args.len() != 3 {
        return Err(validation(
            "Ternary numeric operator expects exactly three arguments.",
        ));
    }
    Ok(op(
        eval_number(&args[0], env)?,
        eval_number(&args[1], env)?,
        eval_number(&args[2], env)?,
    ))
}

pub(super) fn eval_bool(value: &IrExpr, env: &BTreeMap<String, ParamValue>) -> AppResult<bool> {
    let value = inline_let_expr(value)?;
    if let Some(flag) = value.as_bool() {
        return Ok(flag);
    }
    if let Some(symbol) = value.as_symbol() {
        return env
            .get(symbol)
            .and_then(|param| match param {
                ParamValue::Boolean(flag) => Some(*flag),
                _ => None,
            })
            .ok_or_else(|| validation(format!("Unknown boolean symbol `{}`.", symbol)));
    }

    let items = expr_list_items(&value, "boolean expression")?;
    let op = expr_head_symbol(items, "boolean expression")?;
    let args = &items[1..];
    match op {
        "not" => {
            if args.len() != 1 {
                return Err(validation("`not` expects one argument."));
            }
            Ok(!eval_bool(&args[0], env)?)
        }
        "and" => args
            .iter()
            .try_fold(true, |acc, arg| Ok(acc && eval_bool(arg, env)?)),
        "or" => args
            .iter()
            .try_fold(false, |acc, arg| Ok(acc || eval_bool(arg, env)?)),
        "=" => {
            if args.len() != 2 {
                return Err(validation("`=` expects exactly two arguments."));
            }
            // Support both numeric and string equality.
            if let (Ok(a), Ok(b)) = (eval_stringish(&args[0], env), eval_stringish(&args[1], env)) {
                return Ok(a == b);
            }
            compare_numbers(args, env, |a, b| (a - b).abs() <= f64::EPSILON)
        }
        ">" => compare_numbers(args, env, |a, b| a > b),
        ">=" => compare_numbers(args, env, |a, b| a >= b),
        "<" => compare_numbers(args, env, |a, b| a < b),
        "<=" => compare_numbers(args, env, |a, b| a <= b),
        "if" => {
            if args.len() != 3 {
                return Err(validation("`if` expects condition, then, else."));
            }
            if eval_bool(&args[0], env)? {
                eval_bool(&args[1], env)
            } else {
                eval_bool(&args[2], env)
            }
        }
        other => Err(unsupported(format!(
            "Boolean operator `{}` is not supported by current `.ecky` runtime.",
            other
        ))),
    }
}

pub(super) fn eval_stringish(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<String> {
    let value = inline_let_expr(value)?;
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(kw) = value.as_keyword() {
        return Ok(kw.to_string());
    }
    if let Some(symbol) = value.as_symbol() {
        if let Some(param) = env.get(symbol) {
            return match param {
                ParamValue::String(s) => Ok(s.clone()),
                ParamValue::Number(n) => Ok(n.to_string()),
                ParamValue::Boolean(b) => Ok(b.to_string()),
                ParamValue::Null => Ok("null".to_string()),
            };
        }
        return Ok(symbol.to_string());
    }
    if let Some(items) = value.as_list() {
        if items.first().and_then(IrExpr::as_symbol) == Some("if") && items.len() == 4 {
            if eval_bool(&items[1], env)? {
                return eval_stringish(&items[2], env);
            }
            return eval_stringish(&items[3], env);
        }
    }
    Err(validation("Expected a string value."))
}

pub(super) fn compare_numbers(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    predicate: impl Fn(f64, f64) -> bool,
) -> AppResult<bool> {
    if args.len() != 2 {
        return Err(validation(
            "Comparison expects exactly two numeric arguments.",
        ));
    }
    Ok(predicate(
        eval_number(&args[0], env)?,
        eval_number(&args[1], env)?,
    ))
}

pub(super) fn eval_points(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Vec<[f64; 2]>> {
    let points = expr_list_items(value, "polygon points")?;
    points
        .iter()
        .map(|point| {
            let pair = expr_list_items(point, "polygon point")?;
            if pair.len() != 2 {
                return Err(validation("Polygon points must be `(x y)` pairs."));
            }
            Ok([eval_number(&pair[0], env)?, eval_number(&pair[1], env)?])
        })
        .collect()
}

pub(super) fn eval_points_3d(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Vec<[f64; 3]>> {
    let points = expr_list_items(value, "3D point list")?;
    points
        .iter()
        .map(|point| {
            let triple = expr_list_items(point, "3D point")?;
            if triple.len() != 3 {
                return Err(validation("3D points must be `(x y z)` triples."));
            }
            Ok([
                eval_number(&triple[0], env)?,
                eval_number(&triple[1], env)?,
                eval_number(&triple[2], env)?,
            ])
        })
        .collect()
}

pub(super) fn approx_eq(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON
}

pub(super) fn parse_count(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
    minimum: usize,
) -> AppResult<usize> {
    let parsed = eval_number(value, env)?;
    if !parsed.is_finite() {
        return Err(validation(format!(
            "`{}` must be a finite number.",
            context
        )));
    }
    Ok(parsed.round().max(minimum as f64) as usize)
}
