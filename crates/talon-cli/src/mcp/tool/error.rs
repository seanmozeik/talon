use serde_json::Value;
use talon_core::{ErrorCode, ErrorEnvelope, TalonEnvelope, TalonError};

#[derive(Debug)]
pub(super) struct ToolError {
    action: &'static str,
    code: ErrorCode,
    message: String,
    detail: Option<Value>,
}

impl ToolError {
    pub(super) fn new(action: &'static str, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            action,
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub(super) fn with_detail(
        action: &'static str,
        code: ErrorCode,
        message: impl Into<String>,
        detail: Value,
    ) -> Self {
        Self {
            action,
            code,
            message: message.into(),
            detail: Some(detail),
        }
    }

    pub(super) fn envelope(self) -> TalonEnvelope {
        TalonEnvelope::err(
            self.action,
            ErrorEnvelope {
                code: self.code,
                message: self.message,
                detail: self.detail,
            },
        )
    }
}

impl From<TalonError> for ToolError {
    fn from(error: TalonError) -> Self {
        Self::new("talon", error.code(), error.to_string())
    }
}
