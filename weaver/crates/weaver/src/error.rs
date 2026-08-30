use std::process::ExitCode;

use thiserror::Error;

pub(crate) type AppResult<T> = Result<T, WeaverError>;

#[derive(Debug, Error)]
pub(crate) enum WeaverError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Runtime(String),
    #[error("{0}")]
    Retryable(String),
}

impl WeaverError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub(crate) fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }

    pub(crate) fn retryable(message: impl Into<String>) -> Self {
        Self::Retryable(message.into())
    }

    pub(crate) fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            Self::Runtime(_) | Self::Retryable(_) => ExitCode::FAILURE,
        }
    }

    pub(crate) const fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}
