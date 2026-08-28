use std::io;

use thiserror::Error;

pub(crate) type AppResult<T> = Result<T, AppError>;

/// A user-facing failure with a stable machine code and process exit category.
#[derive(Debug, Error)]
pub(crate) enum AppError {
    #[error("{message}")]
    Invalid { code: &'static str, message: String },
    #[error("{message}")]
    NotFound { code: &'static str, message: String },
    #[error("{message}")]
    Conflict { code: &'static str, message: String },
    #[error("{message}")]
    Database { code: &'static str, message: String },
    #[error("{message}")]
    Unexpected { code: &'static str, message: String },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl AppError {
    #[must_use]
    pub(crate) fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::NotFound {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn database(code: &'static str, message: impl Into<String>) -> Self {
        Self::Database {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn unexpected(code: &'static str, message: impl Into<String>) -> Self {
        Self::Unexpected {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Invalid { code, .. }
            | Self::NotFound { code, .. }
            | Self::Conflict { code, .. }
            | Self::Database { code, .. }
            | Self::Unexpected { code, .. } => code,
            Self::Sqlite(_) => "database_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "json_error",
        }
    }

    #[must_use]
    pub(crate) const fn exit_code(&self) -> i32 {
        match self {
            Self::Unexpected { .. } | Self::Io(_) | Self::Json(_) => 1,
            Self::Invalid { .. } => 2,
            Self::NotFound { .. } => 3,
            Self::Conflict { .. } => 4,
            Self::Database { .. } | Self::Sqlite(_) => 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn errors_keep_stable_exit_categories() {
        let values = [
            (AppError::unexpected("runtime", "runtime"), "runtime", 1),
            (AppError::invalid("input", "input"), "input", 2),
            (AppError::not_found("missing", "missing"), "missing", 3),
            (AppError::conflict("conflict", "conflict"), "conflict", 4),
            (AppError::database("database", "database"), "database", 5),
        ];
        for (error, code, exit_code) in values {
            assert_eq!(error.code(), code);
            assert_eq!(error.exit_code(), exit_code);
        }
    }
}
