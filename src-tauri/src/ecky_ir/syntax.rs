use lexpr::parse::{KeywordSyntax, Options};
use lexpr::Value;

use crate::models::AppResult;

use super::shared::validation;

pub(super) fn list_items(value: &Value, context: &str) -> AppResult<Vec<Value>> {
    value
        .to_vec()
        .ok_or_else(|| validation(format!("Expected a proper list for {}.", context)))
}

pub(super) fn head_symbol<'a>(items: &'a [Value], context: &str) -> AppResult<&'a str> {
    items
        .first()
        .and_then(Value::as_symbol)
        .ok_or_else(|| validation(format!("Expected a symbolic head for {}.", context)))
}

pub(super) fn keyword_name(value: &Value) -> Option<&str> {
    value
        .as_keyword()
        .or_else(|| {
            value.as_symbol().and_then(|symbol| {
                symbol
                    .strip_prefix("#:")
                    .or_else(|| symbol.strip_prefix(':'))
            })
        })
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.strip_prefix("#:").or_else(|| text.strip_prefix(':')))
        })
}

pub(super) fn parse_number_value(value: &Value, context: &str) -> AppResult<f64> {
    value
        .as_f64()
        .ok_or_else(|| validation(format!("Expected a number for {}.", context)))
}

pub(super) fn parse_stringish(value: &Value, context: &str) -> AppResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(symbol) = value.as_symbol() {
        return Ok(symbol.to_string());
    }
    Err(validation(format!("Expected text for {}.", context)))
}

/// Strip `;` line comments, respecting string literals.
pub(super) fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let mut in_string = false;
            let mut cut = line.len();
            let mut chars = line.char_indices().peekable();
            while let Some((i, ch)) = chars.next() {
                match ch {
                    '\\' if in_string => {
                        chars.next();
                    }
                    '"' => in_string = !in_string,
                    ';' if !in_string => {
                        cut = i;
                        break;
                    }
                    _ => {}
                }
            }
            line[..cut].trim_end()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn ir_options() -> Options {
    Options::new().with_keyword_syntax(KeywordSyntax::ColonPrefix)
}

pub(super) fn ir_parse(source: &str) -> AppResult<Value> {
    lexpr::parse::Parser::from_str_custom(&strip_comments(source), ir_options())
        .expect_value()
        .map_err(|err| validation(format!("Failed to parse `.ecky`: {}", err)))
}

pub(super) fn canonicalize(source: &str) -> AppResult<String> {
    let value = ir_parse(source)?;
    lexpr::to_string(&value)
        .map_err(|err| validation(format!("Failed to canonicalize IR: {}", err)))
}
