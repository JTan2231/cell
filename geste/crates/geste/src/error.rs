use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    code: &'static str,
    message: String,
    exit_code: i32,
}

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code: 1,
        }
    }

    pub fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code: 2,
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

pub trait Context<T> {
    fn context(self, code: &'static str, message: impl Into<String>) -> AppResult<T>;
}

impl<T, E: std::error::Error> Context<T> for Result<T, E> {
    fn context(self, code: &'static str, message: impl Into<String>) -> AppResult<T> {
        self.map_err(|error| AppError::new(code, format!("{}: {error}", message.into())))
    }
}
