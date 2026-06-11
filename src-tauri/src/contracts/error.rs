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

/// Which authoring layer owns a failure. Orthogonal to `AppErrorCode`: it
/// answers "which wall did I hit", not "what kind of error". Only populated for
/// authoring failures (`None` on `AppError` means "not an authoring error").
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorLayer {
    /// The parenthesized `.ecky` surface (syntax, references, bad inputs).
    Surface,
    /// The finite Core IR op vocabulary (op/selector not in the set, arity, types).
    CoreIr,
    /// The active geometry backend cannot execute a lowered op.
    Backend,
}

/// A structured next-action for an error: a one-line hint plus concrete valid
/// alternatives (e.g. nearest-op "did you mean" suggestions).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ErrorFix {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<ErrorLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<ErrorFix>,
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
            layer: None,
            fix: None,
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
            layer: None,
            fix: None,
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

/// What kind of authoring failure occurred, within a layer. Internal-only — it
/// never crosses the Tauri boundary, so it carries no serde/`Type` derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoringReason {
    ParseSyntax,
    UnknownOp,
    Arity,
    Type,
    Unsupported,
    ConstrainedValue,
}

/// An authoring-path failure with a mandatory `layer` and `reason`. The lowering
/// crate returns `Result<_, AuthoringError>`, so an unlayered authoring error is
/// impossible to construct. Converts one-way into `AppError` at the command
/// boundary via `From`; there is deliberately no reverse conversion.
#[derive(Debug, Clone)]
pub struct AuthoringError {
    pub layer: ErrorLayer,
    pub reason: AuthoringReason,
    pub message: String,
    pub op: Option<String>,
    pub span: Option<(usize, usize)>,
    pub fix: Option<ErrorFix>,
}

impl AuthoringError {
    pub fn new(layer: ErrorLayer, reason: AuthoringReason, message: impl Into<String>) -> Self {
        Self {
            layer,
            reason,
            message: message.into(),
            op: None,
            span: None,
            fix: None,
        }
    }

    pub fn surface(reason: AuthoringReason, message: impl Into<String>) -> Self {
        Self::new(ErrorLayer::Surface, reason, message)
    }

    pub fn core_ir(reason: AuthoringReason, message: impl Into<String>) -> Self {
        Self::new(ErrorLayer::CoreIr, reason, message)
    }

    pub fn backend(reason: AuthoringReason, message: impl Into<String>) -> Self {
        Self::new(ErrorLayer::Backend, reason, message)
    }

    pub fn with_op(mut self, op: impl Into<String>) -> Self {
        self.op = Some(op.into());
        self
    }

    pub fn with_span(mut self, start_line: usize, end_line: usize) -> Self {
        self.span = Some((start_line, end_line));
        self
    }

    pub fn with_fix(mut self, fix: ErrorFix) -> Self {
        self.fix = Some(fix);
        self
    }
}

impl std::fmt::Display for AuthoringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AuthoringError {}

impl From<AuthoringError> for AppError {
    fn from(err: AuthoringError) -> Self {
        let code = match err.layer {
            ErrorLayer::Surface => AppErrorCode::Parse,
            ErrorLayer::CoreIr => AppErrorCode::Validation,
            ErrorLayer::Backend => AppErrorCode::Render,
        };
        let mut app = AppError::new(code, err.message);
        app.layer = Some(err.layer);
        app.operation = err.op;
        if let Some((start_line, end_line)) = err.span {
            app.start_line = Some(start_line);
            app.end_line = Some(end_line);
        }
        app.fix = err.fix;
        app
    }
}

#[cfg(test)]
mod authoring_error_tests {
    use super::*;

    #[test]
    fn core_ir_layer_maps_to_validation_and_carries_fields() {
        let err = AuthoringError::core_ir(AuthoringReason::UnknownOp, "unknown op `bx`")
            .with_op("bx")
            .with_span(3, 3)
            .with_fix(ErrorFix {
                hint: Some("did you mean `box`?".into()),
                suggestions: vec!["box".into()],
            });

        let app: AppError = err.into();
        assert_eq!(app.code, AppErrorCode::Validation);
        assert_eq!(app.layer, Some(ErrorLayer::CoreIr));
        assert_eq!(app.operation.as_deref(), Some("bx"));
        assert_eq!(app.start_line, Some(3));
        assert_eq!(app.end_line, Some(3));
        let fix = app.fix.expect("fix present");
        assert_eq!(fix.suggestions, vec!["box".to_string()]);
    }

    #[test]
    fn surface_and_backend_layers_map_to_parse_and_render() {
        let surface: AppError =
            AuthoringError::surface(AuthoringReason::ParseSyntax, "unexpected token").into();
        assert_eq!(surface.code, AppErrorCode::Parse);
        assert_eq!(surface.layer, Some(ErrorLayer::Surface));

        let backend: AppError =
            AuthoringError::backend(AuthoringReason::Unsupported, "native cannot execute").into();
        assert_eq!(backend.code, AppErrorCode::Render);
        assert_eq!(backend.layer, Some(ErrorLayer::Backend));
    }

    #[test]
    fn app_error_payload_without_layer_or_fix_deserializes() {
        let json = r#"{"code":"validation","message":"boom"}"#;
        let parsed: AppError = serde_json::from_str(json).expect("deserialize legacy payload");
        assert_eq!(parsed.layer, None);
        assert_eq!(parsed.fix, None);
    }

    #[test]
    fn app_error_with_layer_and_fix_serializes_camel_case_and_round_trips() {
        let app: AppError = AuthoringError::backend(AuthoringReason::Unsupported, "no")
            .with_fix(ErrorFix {
                hint: Some("use the mesh backend".into()),
                suggestions: vec![],
            })
            .into();
        let json = serde_json::to_string(&app).expect("serialize");
        assert!(json.contains("\"layer\":\"backend\""), "camelCase layer: {json}");
        assert!(json.contains("\"fix\""), "fix present: {json}");
        let round: AppError = serde_json::from_str(&json).expect("round trip");
        assert_eq!(round, app);
    }
}
