use netcore_common::ErrorCode;

/// Internal error carrying the stable wire error category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerError {
    pub code: ErrorCode,
    pub message: String,
}

impl ManagerError {
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidArgument,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::NotAuthorized,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::NotFound,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::BackendUnavailable,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn busy(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Busy,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn operation(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::OperationFailed,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Unsupported,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ManagerError {}

impl From<anyhow::Error> for ManagerError {
    fn from(error: anyhow::Error) -> Self {
        Self::operation(format!("{error:#}"))
    }
}

impl From<std::io::Error> for ManagerError {
    fn from(error: std::io::Error) -> Self {
        Self::operation(error.to_string())
    }
}
