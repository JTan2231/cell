use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{code}: {message}")]
    Domain { code: &'static str, message: String },
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Nucleus error: {0}")]
    Nucleus(#[from] nucleus_client::ClientError),
}

impl Error {
    pub fn domain(code: &'static str, message: impl Into<String>) -> Self {
        Self::Domain {
            code,
            message: message.into(),
        }
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::Domain { code, .. } => code,
            Self::Io { .. } => "io_failed",
            Self::Sql(_) => "database_failed",
            Self::Json(_) => "json_failed",
            Self::Nucleus(_) => "nucleus_failed",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}
