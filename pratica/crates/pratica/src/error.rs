use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    code: String,
    message: String,
    exit_code: i32,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            exit_code: 1,
        }
    }

    pub fn usage(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            exit_code: 2,
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<crate::store::StoreError> for AppError {
    fn from(error: crate::store::StoreError) -> Self {
        use crate::store::StoreErrorKind;

        let code = match error.kind() {
            StoreErrorKind::InvalidInput => "invalid_input",
            StoreErrorKind::NotFound => "not_found",
            StoreErrorKind::Conflict => "conflict",
            StoreErrorKind::Stale => "stale_state",
            StoreErrorKind::Database => "database_error",
            StoreErrorKind::Filesystem => "filesystem_error",
            StoreErrorKind::CorruptState => "corrupt_state",
        };
        let message = error.message();
        if error.kind() == StoreErrorKind::InvalidInput {
            Self::usage(code, message)
        } else {
            Self::new(code, message)
        }
    }
}

impl From<crate::model::MarkdownError> for AppError {
    fn from(error: crate::model::MarkdownError) -> Self {
        Self::usage("invalid_markdown", error.to_string())
    }
}

impl From<crate::source_catalog::CatalogError> for AppError {
    fn from(error: crate::source_catalog::CatalogError) -> Self {
        Self::usage(error.code(), error.message())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub trait Context<T> {
    fn context(self, code: impl Into<String>, message: impl Into<String>) -> AppResult<T>;
}

impl<T, E: std::error::Error> Context<T> for Result<T, E> {
    fn context(self, code: impl Into<String>, message: impl Into<String>) -> AppResult<T> {
        self.map_err(|error| AppError::new(code, format!("{}: {error}", message.into())))
    }
}
