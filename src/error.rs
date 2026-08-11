use std::io;

use thiserror::Error;

/// Result type used throughout the application.
pub type AppResult<T> = Result<T, AppError>;

/// A user-facing failure with a stable machine code and process exit category.
#[derive(Debug, Error)]
pub enum AppError {
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
    /// Construct an invalid syntax or input error (exit code 2).
    #[must_use]
    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            code,
            message: message.into(),
        }
    }

    /// Construct a missing library, tree, or node error (exit code 3).
    #[must_use]
    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::NotFound {
            code,
            message: message.into(),
        }
    }

    /// Construct an invariant or confirmation conflict (exit code 4).
    #[must_use]
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict {
            code,
            message: message.into(),
        }
    }

    /// Construct a database, migration, integrity, or index error (exit code 5).
    #[must_use]
    pub fn database(code: &'static str, message: impl Into<String>) -> Self {
        Self::Database {
            code,
            message: message.into(),
        }
    }

    /// Construct an unexpected runtime error (exit code 1).
    #[must_use]
    pub fn unexpected(code: &'static str, message: impl Into<String>) -> Self {
        Self::Unexpected {
            code,
            message: message.into(),
        }
    }

    /// Return the stable code emitted in a JSON error envelope.
    #[must_use]
    pub const fn code(&self) -> &'static str {
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

    /// Return the documented process exit code for this error category.
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
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
    fn constructors_keep_stable_codes_and_exit_categories() {
        let invalid = AppError::invalid("empty_title", "a title is required");
        let missing = AppError::not_found("node_not_found", "node 7 was not found");
        let conflict = AppError::conflict("would_create_cycle", "move rejected");
        let database = AppError::database("index_stale", "run annals reindex");
        let unexpected = AppError::unexpected("internal_error", "unexpected failure");

        assert_eq!((invalid.code(), invalid.exit_code()), ("empty_title", 2));
        assert_eq!((missing.code(), missing.exit_code()), ("node_not_found", 3));
        assert_eq!(
            (conflict.code(), conflict.exit_code()),
            ("would_create_cycle", 4)
        );
        assert_eq!((database.code(), database.exit_code()), ("index_stale", 5));
        assert_eq!(
            (unexpected.code(), unexpected.exit_code()),
            ("internal_error", 1)
        );
    }
}
