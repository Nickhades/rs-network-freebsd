use serde::{Deserialize, Serialize};

/// Stable machine-readable error categories exposed by the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    NotAuthorized,
    NotFound,
    BackendUnavailable,
    Busy,
    OperationFailed,
    Unsupported,
}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid_argument",
            Self::NotAuthorized => "not_authorized",
            Self::NotFound => "not_found",
            Self::BackendUnavailable => "backend_unavailable",
            Self::Busy => "busy",
            Self::OperationFailed => "operation_failed",
            Self::Unsupported => "unsupported",
        }
    }
}

/// JSON-serializable error payload used by the CLI and desktop integrations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorPayload {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ErrorPayload {}
