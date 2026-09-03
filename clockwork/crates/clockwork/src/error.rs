use std::fmt;

#[derive(Debug)]
pub(crate) struct Error {
    code: &'static str,
    message: String,
}

impl Error {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;

pub(crate) trait Context<T> {
    fn context(self, code: &'static str, message: impl Into<String>) -> Result<T>;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: std::error::Error,
{
    fn context(self, code: &'static str, message: impl Into<String>) -> Result<T> {
        self.map_err(|error| Error::new(code, format!("{}: {error}", message.into())))
    }
}
