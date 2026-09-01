use std::fmt;

#[derive(Debug)]
pub(crate) struct AppError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl AppError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

pub(crate) type AppResult<T> = Result<T, AppError>;

pub(crate) trait Context<T> {
    fn context(self, code: &'static str, message: impl Into<String>) -> AppResult<T>;
}

impl<T, E: std::error::Error> Context<T> for Result<T, E> {
    fn context(self, code: &'static str, message: impl Into<String>) -> AppResult<T> {
        self.map_err(|error| AppError::new(code, format!("{}: {error}", message.into())))
    }
}
