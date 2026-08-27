//! Typed asynchronous client for a per-user `nucleusd` instance.
//!
//! The HTTP origin is intentionally synthetic: every connection is forced over
//! the configured Unix-domain socket by reqwest.

use std::env;
use std::path::{Path, PathBuf};

use nucleus_core::{
    CancelJobResponseV1, ErrorResponseV1, HealthResponseV1, JobAcceptedV1, JobId, JobRequestV1,
    JobV1, ListJobsQueryV1, ListJobsResponseV1, LogSchemaV1, LogsQueryV1, LogsResponseV1,
    PendingToolCallV1, RegisteredToolsetV1, SchemaId, ToolCallId, ToolCallsQueryV1,
    ToolCallsResponseV1, ToolResultV1, ToolsetRegistrationV1,
};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

const ORIGIN: &str = "http://nucleus.local";

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("HOME is unavailable or is not an absolute path; pass an explicit socket path")]
    MissingHome,
    #[error("Nucleus socket path must be absolute: {0}")]
    RelativeSocket(PathBuf),
    #[error("request validation failed: {0}")]
    Validation(#[from] nucleus_core::ValidationError),
    #[error("unable to build the local HTTP client: {0}")]
    Build(#[source] reqwest::Error),
    #[error("unable to communicate with nucleusd at {socket}: {source}")]
    Transport {
        socket: PathBuf,
        #[source]
        source: reqwest::Error,
    },
    #[error("Nucleus API returned HTTP {status}: {message} ({code})")]
    Api {
        status: u16,
        code: String,
        message: String,
        response: Box<ErrorResponseV1>,
    },
    #[error("Nucleus API returned HTTP {status} with an undecodable response: {body}")]
    UndecodableError { status: u16, body: String },
    #[error("unable to decode a successful Nucleus API response (HTTP {status}): {source}")]
    Decode {
        status: u16,
        #[source]
        source: serde_json::Error,
    },
}

/// A client whose connections are pinned to one Unix-domain socket.
#[derive(Clone, Debug)]
pub struct NucleusClient {
    socket: PathBuf,
    http: reqwest::Client,
}

impl NucleusClient {
    /// Construct a client for an explicit absolute Unix socket path.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is relative or the HTTP client cannot be
    /// constructed.
    pub fn new(socket: impl Into<PathBuf>) -> Result<Self, ClientError> {
        let socket = socket.into();
        if !socket.is_absolute() {
            return Err(ClientError::RelativeSocket(socket));
        }
        let http = reqwest::Client::builder()
            .http1_only()
            .unix_socket(socket.clone())
            .build()
            .map_err(ClientError::Build)?;
        Ok(Self { socket, http })
    }

    /// Connect using `NUCLEUS_SOCKET`, or the standard per-user path when it is
    /// not set.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket cannot be resolved or the HTTP client
    /// cannot be constructed.
    pub fn for_current_user() -> Result<Self, ClientError> {
        Self::new(default_socket_path()?)
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    /// Read daemon health.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn health(&self) -> Result<HealthResponseV1, ClientError> {
        self.get("/v1/health").await
    }

    /// Validate and submit one job.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if validation or the API call fails.
    pub async fn submit_job(&self, request: &JobRequestV1) -> Result<JobAcceptedV1, ClientError> {
        request.validate()?;
        self.send_json(Method::POST, "/v1/jobs", request).await
    }

    /// Retrieve one job and all of its attempts.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn get_job(&self, job_id: &JobId) -> Result<JobV1, ClientError> {
        self.get(&format!("/v1/jobs/{}", path_segment(job_id.as_str())))
            .await
    }

    /// List jobs matching a validated query.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if validation or the API call fails.
    pub async fn list_jobs(
        &self,
        query: &ListJobsQueryV1,
    ) -> Result<ListJobsResponseV1, ClientError> {
        query.validate()?;
        let response = self
            .http
            .get(Self::url("/v1/jobs"))
            .query(query)
            .send()
            .await
            .map_err(|source| self.transport(source))?;
        self.decode(response).await
    }

    /// Fetch one bounded page of schema-bound raw records. A daemon may hold a
    /// request with `follow=true` until at least one record becomes available or
    /// the job becomes terminal.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if validation or the API call fails.
    pub async fn logs(
        &self,
        job_id: &JobId,
        query: &LogsQueryV1,
    ) -> Result<LogsResponseV1, ClientError> {
        query.validate()?;
        let response = self
            .http
            .get(Self::url(&format!(
                "/v1/jobs/{}/logs",
                path_segment(job_id.as_str())
            )))
            .query(query)
            .send()
            .await
            .map_err(|source| self.transport(source))?;
        self.decode(response).await
    }

    /// Request cancellation of one job.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn cancel_job(&self, job_id: &JobId) -> Result<CancelJobResponseV1, ClientError> {
        self.send_empty(
            Method::POST,
            &format!("/v1/jobs/{}/cancel", path_segment(job_id.as_str())),
        )
        .await
    }

    /// Retrieve one exact registered schema document.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn get_schema(&self, schema_id: &SchemaId) -> Result<LogSchemaV1, ClientError> {
        self.get(&format!("/v1/schemas/{}", path_segment(schema_id.as_str())))
            .await
    }

    /// Register an exact schema document idempotently.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn register_schema(&self, schema: &LogSchemaV1) -> Result<LogSchemaV1, ClientError> {
        self.send_json(Method::POST, "/v1/schemas", schema).await
    }

    /// Validate and register a requester-owned toolset.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if validation or the API call fails.
    pub async fn register_toolset(
        &self,
        registration: &ToolsetRegistrationV1,
    ) -> Result<RegisteredToolsetV1, ClientError> {
        registration.validate()?;
        self.send_json(Method::POST, "/v1/toolsets", registration)
            .await
    }

    /// Retrieve a registered toolset identity and digest.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn get_toolset(
        &self,
        provider: &str,
        name: &str,
        version: u32,
    ) -> Result<RegisteredToolsetV1, ClientError> {
        self.get(&format!(
            "/v1/toolsets/{}/{}/{version}",
            path_segment(provider),
            path_segment(name)
        ))
        .await
    }

    /// Long-poll for requester-owned managed-tool calls. `wait_seconds=0` is a
    /// non-blocking mailbox read.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if validation or the API call fails.
    pub async fn pending_tool_calls(
        &self,
        job_id: &JobId,
        query: &ToolCallsQueryV1,
    ) -> Result<ToolCallsResponseV1, ClientError> {
        query.validate()?;
        let response = self
            .http
            .get(Self::url(&format!(
                "/v1/jobs/{}/tool-calls",
                path_segment(job_id.as_str())
            )))
            .query(query)
            .send()
            .await
            .map_err(|source| self.transport(source))?;
        self.decode(response).await
    }

    /// Answer one pending managed-tool call.
    ///
    /// # Errors
    ///
    /// Returns a [`ClientError`] if the API call fails.
    pub async fn post_tool_result(
        &self,
        job_id: &JobId,
        call_id: &ToolCallId,
        result: &ToolResultV1,
    ) -> Result<PendingToolCallV1, ClientError> {
        self.send_json(
            Method::POST,
            &format!(
                "/v1/jobs/{}/tool-calls/{}/result",
                path_segment(job_id.as_str()),
                path_segment(call_id.as_str())
            ),
            result,
        )
        .await
    }

    async fn get<T>(&self, path: &str) -> Result<T, ClientError>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .get(Self::url(path))
            .send()
            .await
            .map_err(|source| self.transport(source))?;
        self.decode(response).await
    }

    async fn send_empty<T>(&self, method: Method, path: &str) -> Result<T, ClientError>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .request(method, Self::url(path))
            .send()
            .await
            .map_err(|source| self.transport(source))?;
        self.decode(response).await
    }

    async fn send_json<B, T>(&self, method: Method, path: &str, body: &B) -> Result<T, ClientError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let response = self
            .http
            .request(method, Self::url(path))
            .json(body)
            .send()
            .await
            .map_err(|source| self.transport(source))?;
        self.decode(response).await
    }

    async fn decode<T>(&self, response: reqwest::Response) -> Result<T, ClientError>
    where
        T: DeserializeOwned,
    {
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|source| self.transport(source))?;
        if status.is_success() {
            return serde_json::from_slice(&body).map_err(|source| ClientError::Decode {
                status: status.as_u16(),
                source,
            });
        }

        match serde_json::from_slice::<ErrorResponseV1>(&body) {
            Ok(api_error) => Err(ClientError::Api {
                status: status.as_u16(),
                code: api_error.code.clone(),
                message: api_error.message.clone(),
                response: Box::new(api_error),
            }),
            Err(_) => Err(ClientError::UndecodableError {
                status: status.as_u16(),
                body: printable_error_body(status, &body),
            }),
        }
    }

    fn transport(&self, source: reqwest::Error) -> ClientError {
        ClientError::Transport {
            socket: self.socket.clone(),
            source,
        }
    }

    fn url(path: &str) -> String {
        debug_assert!(path.starts_with('/'));
        format!("{ORIGIN}{path}")
    }
}

/// Resolve the standard per-user socket without opening it.
///
/// # Errors
///
/// Returns an error when neither an absolute override nor an absolute home
/// directory is available.
pub fn default_socket_path() -> Result<PathBuf, ClientError> {
    if let Some(path) = env::var_os("NUCLEUS_SOCKET") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(ClientError::RelativeSocket(path));
        }
        return Ok(path);
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(ClientError::MissingHome)?;
    Ok(home.join("Library/Application Support/Nucleus/nucleus.sock"))
}

fn printable_error_body(status: StatusCode, body: &[u8]) -> String {
    const MAX: usize = 4_096;
    if body.is_empty() {
        return status
            .canonical_reason()
            .unwrap_or("empty response body")
            .to_owned();
    }
    let prefix = &body[..body.len().min(MAX)];
    let mut text = String::from_utf8_lossy(prefix).into_owned();
    if body.len() > MAX {
        text.push('…');
    }
    text
}

fn path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            // Writing to String is infallible.
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn explicit_socket_must_be_absolute() {
        let Err(error) = NucleusClient::new("relative.sock") else {
            panic!("relative socket path must fail");
        };
        assert!(matches!(error, ClientError::RelativeSocket(_)));
    }

    #[test]
    fn printable_error_body_is_bounded() {
        let body = vec![b'x'; 5_000];
        let rendered = printable_error_body(StatusCode::BAD_REQUEST, &body);
        assert!(rendered.len() < body.len());
        assert!(rendered.ends_with('…'));
    }

    #[test]
    fn path_components_cannot_change_routes() {
        assert_eq!(path_segment("job:1"), "job%3A1");
        assert_eq!(path_segment("../health"), "..%2Fhealth");
    }

    #[tokio::test]
    async fn health_uses_the_configured_unix_socket() {
        let temporary = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
        let socket = temporary.path().join("nucleus.sock");
        let listener = tokio::net::UnixListener::bind(&socket)
            .unwrap_or_else(|error| panic!("bind Unix socket: {error}"));
        let server = tokio::spawn(async move {
            let (mut connection, _) = listener
                .accept()
                .await
                .unwrap_or_else(|error| panic!("accept client: {error}"));
            let mut request = vec![0_u8; 4_096];
            let read = connection
                .read(&mut request)
                .await
                .unwrap_or_else(|error| panic!("read request: {error}"));
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /v1/health HTTP/1.1"));

            let body = r#"{"version":1,"status":"ok","daemonVersion":"0.1.0"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            connection
                .write_all(response.as_bytes())
                .await
                .unwrap_or_else(|error| panic!("write response: {error}"));
        });

        let health = NucleusClient::new(&socket)
            .unwrap_or_else(|error| panic!("construct client: {error}"))
            .health()
            .await
            .unwrap_or_else(|error| panic!("read health: {error}"));
        assert_eq!(health.status, "ok");
        assert_eq!(health.daemon_version, "0.1.0");
        server
            .await
            .unwrap_or_else(|error| panic!("join server: {error}"));
    }
}
