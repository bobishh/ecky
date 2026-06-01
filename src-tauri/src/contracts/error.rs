use super::ParamValue;
use serde::{Deserialize, Serialize};
use specta::Type;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppErrorCode {
    Validation,
    NotFound,
    Conflict,
    Provider,
    Persistence,
    Render,
    Parse,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticParamValue {
    pub key: String,
    pub value: ParamValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_params: Vec<DiagnosticParamValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_node_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_context: Option<DiagnosticContext>,
}

fn sanitize_diagnostic_tail_token(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| match ch {
            '|' | '\n' | '\r' | '\t' | ' ' => '_',
            _ => ch,
        })
        .collect()
}

fn diagnostic_tail_param_value(value: &ParamValue) -> String {
    match value {
        ParamValue::String(value) => sanitize_diagnostic_tail_token(value),
        ParamValue::Number(value) => value.to_string(),
        ParamValue::Boolean(value) => value.to_string(),
        ParamValue::Null => "null".to_string(),
    }
}

fn diagnostic_context_tail(context: &DiagnosticContext) -> Option<String> {
    let mut tokens = Vec::new();
    if let Some(part_key) = context
        .part_key
        .as_deref()
        .map(sanitize_diagnostic_tail_token)
        .filter(|value| !value.is_empty())
    {
        tokens.push(format!("part={part_key}"));
    }
    if let Some(op_name) = context
        .op_name
        .as_deref()
        .map(sanitize_diagnostic_tail_token)
        .filter(|value| !value.is_empty())
    {
        tokens.push(format!("op={op_name}"));
    }
    if let Some(start_line) = context.start_line {
        let end_line = context.end_line.unwrap_or(start_line).max(start_line);
        if end_line == start_line {
            tokens.push(format!("lines={start_line}"));
        } else {
            tokens.push(format!("lines={start_line}-{end_line}"));
        }
    }
    for param in &context.resolved_params {
        let key = sanitize_diagnostic_tail_token(&param.key);
        if key.is_empty() {
            continue;
        }
        tokens.push(format!(
            "{key}={}",
            diagnostic_tail_param_value(&param.value)
        ));
    }
    (!tokens.is_empty()).then(|| tokens.join(" "))
}

fn append_diagnostic_tail(details: Option<String>, tail: &str) -> Option<String> {
    match details {
        Some(details) => {
            let trimmed = details.trim_end();
            if trimmed
                .lines()
                .last()
                .is_some_and(|line| line.trim() == tail)
            {
                Some(trimmed.to_string())
            } else if trimmed.is_empty() {
                Some(tail.to_string())
            } else {
                Some(format!("{trimmed}\n{tail}"))
            }
        }
        None => Some(tail.to_string()),
    }
}

impl AppError {
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            stable_node_key: None,
            start_line: None,
            end_line: None,
            operation: None,
            diagnostic_context: None,
        }
    }

    pub fn with_details(
        code: AppErrorCode,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
            stable_node_key: None,
            start_line: None,
            end_line: None,
            operation: None,
            diagnostic_context: None,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Validation, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::NotFound, message)
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Provider, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Conflict, message)
    }

    pub fn persistence(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Persistence, message)
    }

    pub fn render(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Render, message)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Parse, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, message)
    }

    pub fn with_stable_node_key(mut self, stable_node_key: impl Into<String>) -> Self {
        self.stable_node_key = Some(stable_node_key.into());
        self
    }

    pub fn with_line_range(mut self, start_line: usize, end_line: usize) -> Self {
        self.start_line = Some(start_line);
        self.end_line = Some(end_line);
        self
    }

    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    pub fn with_diagnostic_context(mut self, diagnostic_context: DiagnosticContext) -> Self {
        if let Some(tail) = diagnostic_context_tail(&diagnostic_context) {
            self.details = append_diagnostic_tail(self.details, &tail);
        }
        self.diagnostic_context = Some(diagnostic_context);
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.details.as_deref() {
            Some(details) if !details.trim().is_empty() => {
                write!(f, "{}: {}", self.message, details)
            }
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for AppError {}
