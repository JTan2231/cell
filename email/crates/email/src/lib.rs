//! Provider-owned plain-text Email submission.

pub mod api;
mod client;
mod receiving;

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use clap::{Parser, Subcommand};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const RESEND_ENDPOINT: &str = "https://api.resend.com/emails";
const FROM: &str = "Codex <codex@joeytan.dev>";
const TO: &str = "j.tan2231@gmail.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(100), Duration::from_millis(250)];

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "email",
    version,
    about = "Send a plain-text email to j.tan2231@gmail.com"
)]
struct Cli {
    /// Stable caller-owned key used to deduplicate this exact request.
    #[arg(long, value_name = "KEY", value_parser = parse_idempotency_key)]
    idempotency_key: Option<String>,
    /// Local file to attach; repeat for multiple files. Reads exact bytes before sending.
    #[arg(long = "attach", value_name = "PATH")]
    attachments: Vec<PathBuf>,
    /// Read a JSON body and base64 attachment payload from stdin; body must be -.
    #[arg(long, conflicts_with = "attachments")]
    payload_stdin: bool,
    /// Email subject.
    subject: String,
    /// Plain-text body, or - to read UTF-8 text from stdin.
    body: String,
}

#[derive(Debug, Parser)]
#[command(name = "email", version, about = "Send personal email or read received account email")]
struct SendCli {
    #[command(flatten)]
    message: Cli,
    /// One caller-authorized reply mailbox. Does not change the fixed recipient.
    #[arg(long, value_name = "ADDRESS")]
    reply_to: Option<String>,
    /// RFC Message-ID of the message being answered, including angle brackets.
    #[arg(long, value_name = "MESSAGE_ID")]
    in_reply_to: Option<String>,
    /// One RFC Message-ID in the thread's References chain; repeat in order.
    #[arg(long = "reference", value_name = "MESSAGE_ID")]
    references: Vec<String>,
}

#[derive(Debug, Parser)]
#[command(name = "email receive", version, about = "Read received Resend account email without retaining it")]
struct ReceiveCli {
    #[command(subcommand)]
    command: ReceiveCommand,
}

#[derive(Debug, Subcommand)]
enum ReceiveCommand {
    /// Read one metadata page, ordered from newer to older records.
    List {
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: u16,
        /// Last provider email ID from the preceding page; excluded from this page.
        #[arg(long, value_name = "ID")]
        after: Option<String>,
    },
    /// Read one full received email, without fetching remote content or attachments.
    Get { id: String },
}

#[derive(Serialize)]
struct ResendRequest<'a> {
    from: &'static str,
    to: [&'static str; 1],
    subject: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<ResendAttachment<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    headers: BTreeMap<&'static str, String>,
}

#[derive(Serialize)]
struct ResendAttachment<'a> {
    filename: &'a str,
    content: String,
}

#[derive(Deserialize)]
struct ResendResponse {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError(String);

impl AppError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AppError {}

type AppResult<T> = Result<T, AppError>;

/// Run Email's command-line entry point.
///
/// # Panics
/// Panics if the Tokio runtime cannot be created or this entry point is called
/// from another asynchronous runtime. Library callers should use [`api::send`].
#[tokio::main(flavor = "current_thread")]
pub async fn main_entry() {
    if let Err(error) = run().await {
        eprintln!("email: {error}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments.get(1).is_some_and(|value| value == "receive")
        && arguments.get(2).is_some_and(|value| {
            matches!(value.to_str(), Some("list" | "get" | "--help" | "-h" | "--version" | "-V"))
        })
    {
        let receive = ReceiveCli::parse_from(
            std::iter::once(arguments[0].clone()).chain(arguments.into_iter().skip(2)),
        );
        let output = match receive.command {
            ReceiveCommand::List { limit, after } => {
                let page = api::list_received(&api::ReceivedPageRequest { limit, after }).await?;
                serde_json::to_string(&page)
            }
            ReceiveCommand::Get { id } => {
                let message = api::get_received(&id).await?;
                serde_json::to_string(&message)
            }
        }
        .map_err(|_| AppError::new("unable to encode received email output"))?;
        println!("{output}");
        return Ok(());
    }
    let send = SendCli::parse_from(arguments);
    let options = api::ReplyOptions {
        reply_to: send.reply_to,
        in_reply_to: send.in_reply_to,
        references: send.references,
    };
    validate_reply_options(&options)?;
    let cli = send.message;
    let (body, attachments) = if cli.payload_stdin {
        if cli.body != "-" {
            return Err(AppError::new("--payload-stdin requires body -"));
        }
        read_payload(io::stdin().lock())?
    } else {
        (
            read_body(&cli.body, io::stdin().lock())?,
            cli.attachments
                .iter()
                .map(|path| read_attachment(path))
                .collect::<AppResult<Vec<_>>>()?,
        )
    };
    let receipt = api::send_with_options(
        &api::Message {
            subject: cli.subject,
            body,
            idempotency_key: cli.idempotency_key,
        },
        &attachments,
        &options,
    )
    .await?;
    println!("{receipt}");
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputPayload {
    body: String,
    #[serde(default)]
    attachments: Vec<InputAttachment>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputAttachment {
    filename: String,
    content: String,
}

fn read_payload(input: impl io::Read) -> AppResult<(String, Vec<api::Attachment>)> {
    let payload: InputPayload = serde_json::from_reader(input)
        .map_err(|_| AppError::new("invalid JSON email payload on stdin"))?;
    let attachments = payload
        .attachments
        .into_iter()
        .map(|attachment| {
            validate_attachment_filename(&attachment.filename)?;
            let content = base64::engine::general_purpose::STANDARD
                .decode(attachment.content)
                .map_err(|_| AppError::new("invalid base64 attachment content"))?;
            Ok(api::Attachment {
                filename: attachment.filename,
                content,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((payload.body, attachments))
}

fn read_body(argument: &str, mut input: impl io::Read) -> AppResult<String> {
    if argument != "-" {
        return Ok(argument.to_owned());
    }

    let mut body = String::new();
    input
        .read_to_string(&mut body)
        .map_err(|_| AppError::new("unable to read a UTF-8 email body from stdin"))?;
    Ok(body)
}

fn validate_attachment_filename(filename: &str) -> AppResult<()> {
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename
            .chars()
            .any(|character| character.is_control() || character == '/' || character == '\\')
    {
        return Err(AppError::new(
            "attachment filename must be a nonempty UTF-8 basename without control characters",
        ));
    }
    Ok(())
}

fn read_attachment(path: &Path) -> AppResult<api::Attachment> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::new("attachment must have a UTF-8 filename"))?;
    validate_attachment_filename(filename)?;
    let metadata = std::fs::metadata(path)
        .map_err(|_| AppError::new("unable to read local attachment file"))?;
    if !metadata.is_file() {
        return Err(AppError::new("attachment must be a regular local file"));
    }
    let content =
        std::fs::read(path).map_err(|_| AppError::new("unable to read local attachment file"))?;
    Ok(api::Attachment {
        filename: filename.to_owned(),
        content,
    })
}

fn resend_api_key() -> AppResult<String> {
    let api_key = std::env::var("RESEND_API_KEY")
        .map_err(|_| AppError::new("RESEND_API_KEY must be set to use Email transport"))?;
    validate_api_key(api_key)
}

fn validate_api_key(api_key: String) -> AppResult<String> {
    if api_key.trim().is_empty() || api_key.trim() != api_key {
        return Err(AppError::new(
            "RESEND_API_KEY must be nonblank and contain no surrounding whitespace",
        ));
    }
    Ok(api_key)
}

fn new_idempotency_key() -> String {
    format!("email/{}", Uuid::now_v7())
}

fn parse_idempotency_key(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > 256 {
        return Err("must contain between 1 and 256 ASCII characters".to_owned());
    }
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("must contain only visible ASCII characters without whitespace".to_owned());
    }
    Ok(value.to_owned())
}

fn validate_reply_options(options: &api::ReplyOptions) -> AppResult<()> {
    if let Some(address) = &options.reply_to {
        let valid = address.len() <= 254
            && address.split_once('@').is_some_and(|(local, domain)| {
                !local.is_empty()
                    && local.len() <= 64
                    && !local.starts_with('.')
                    && !local.ends_with('.')
                    && !local.contains("..")
                    && local.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&byte)
                    })
                    && !domain.is_empty()
                    && domain.split('.').all(|label| {
                        !label.is_empty()
                            && label.len() <= 63
                            && !label.starts_with('-')
                            && !label.ends_with('-')
                            && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    })
            });
        if !valid {
            return Err(AppError::new("reply-to must be one ASCII mailbox without a display name"));
        }
    }
    if let Some(id) = &options.in_reply_to {
        validate_message_id(id)?;
    }
    if options.references.len() > 64
        || options.references.iter().map(|id| id.len().saturating_add(1)).sum::<usize>() > 8192
    {
        return Err(AppError::new("references must contain at most 64 message IDs and 8192 bytes"));
    }
    for id in &options.references {
        validate_message_id(id)?;
    }
    Ok(())
}

fn validate_message_id(id: &str) -> AppResult<()> {
    let valid = id.len() <= 998
        && id.strip_prefix('<').and_then(|value| value.strip_suffix('>')).is_some_and(|value| {
            value.split_once('@').is_some_and(|(left, right)| !left.is_empty() && !right.is_empty() && !right.contains('@'))
                && value.bytes().all(|byte| byte.is_ascii_graphic() && !b"<>\\\"".contains(&byte))
        });
    if !valid {
        return Err(AppError::new("reply headers require one bracketed ASCII Message-ID without whitespace"));
    }
    Ok(())
}

#[cfg(test)]
async fn send_to(
    endpoint: &str,
    api_key: &str,
    idempotency_key: &str,
    subject: &str,
    body: &str,
    attachments: &[api::Attachment],
) -> AppResult<String> {
    send_to_with_options(endpoint, api_key, idempotency_key, subject, body, attachments, &api::ReplyOptions::default()).await
}

fn resend_client(api_key: &str) -> AppResult<reqwest::Client> {
    let mut headers = HeaderMap::new();
    let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
        AppError::new("RESEND_API_KEY contains characters that cannot be sent in an HTTP header")
    })?;
    authorization.set_sensitive(true);
    headers.insert(AUTHORIZATION, authorization);
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("email/", env!("CARGO_PKG_VERSION"))),
    );

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| AppError::new("unable to initialize the Resend client"))
}

async fn send_to_with_options(
    endpoint: &str,
    api_key: &str,
    idempotency_key: &str,
    subject: &str,
    body: &str,
    attachments: &[api::Attachment],
    options: &api::ReplyOptions,
) -> AppResult<String> {
    let client = resend_client(api_key)?;
    let mut headers = BTreeMap::new();
    if let Some(id) = &options.in_reply_to {
        headers.insert("In-Reply-To", id.clone());
    }
    if !options.references.is_empty() {
        headers.insert("References", options.references.join(" "));
    }
    let request = ResendRequest {
        from: FROM,
        to: [TO],
        subject,
        text: body,
        attachments: attachments
            .iter()
            .map(|attachment| ResendAttachment {
                filename: &attachment.filename,
                content: base64::engine::general_purpose::STANDARD.encode(&attachment.content),
            })
            .collect(),
        reply_to: options.reply_to.as_deref(),
        headers,
    };

    for attempt in 0..=RETRY_DELAYS.len() {
        let response = client
            .post(endpoint)
            .header("Idempotency-Key", idempotency_key)
            .json(&request)
            .send()
            .await;

        let Ok(response) = response else {
            if let Some(delay) = RETRY_DELAYS.get(attempt) {
                tokio::time::sleep(*delay).await;
                continue;
            }
            return Err(AppError::new("unable to send email through Resend"));
        };

        let status = response.status();
        if (status.as_u16() == 429 || status.is_server_error())
            && let Some(delay) = RETRY_DELAYS.get(attempt)
        {
            tokio::time::sleep(*delay).await;
            continue;
        }
        if !status.is_success() {
            return Err(AppError::new(format!(
                "Resend rejected email with HTTP {status}"
            )));
        }

        let response = response
            .json::<ResendResponse>()
            .await
            .map_err(|_| AppError::new("Resend returned an invalid success response"))?;
        if response.id.trim().is_empty()
            || response.id.trim() != response.id
            || response.id.chars().any(char::is_control)
        {
            return Err(AppError::new("Resend returned an invalid email ID"));
        }
        return Ok(response.id);
    }

    Err(AppError::new("email exhausted its Resend attempts"))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use clap::Parser as _;
    use serde_json::Value;
    use uuid::{Uuid, Version};

    use super::{
        Cli, FROM, TO, new_idempotency_key, parse_idempotency_key, read_attachment, read_body,
        send_to, validate_api_key, validate_attachment_filename,
    };

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
    type TestServer = (
        String,
        mpsc::Receiver<String>,
        thread::JoinHandle<std::io::Result<()>>,
    );

    struct TestResponse {
        status: u16,
        reason: &'static str,
        body: &'static str,
    }

    #[test]
    fn cli_accepts_exactly_subject_and_body() -> TestResult {
        let cli = Cli::try_parse_from(["email", "A subject", "A body"])?;
        assert_eq!(
            cli,
            Cli {
                idempotency_key: None,
                attachments: Vec::new(),
                payload_stdin: false,
                subject: "A subject".to_owned(),
                body: "A body".to_owned(),
            }
        );
        assert!(Cli::try_parse_from(["email", "subject"]).is_err());
        assert!(Cli::try_parse_from(["email", "subject", "body", "extra"]).is_err());
        assert!(
            Cli::try_parse_from(["email", "subject", "body", "--to", "other@example.com"]).is_err()
        );
        Ok(())
    }

    #[test]
    fn cli_accepts_a_caller_owned_idempotency_key() -> TestResult {
        let cli = Cli::try_parse_from([
            "email",
            "--idempotency-key",
            "decisions/daily/2026-09-01",
            "A subject",
            "A body",
        ])?;
        assert_eq!(
            cli,
            Cli {
                idempotency_key: Some("decisions/daily/2026-09-01".to_owned()),
                attachments: Vec::new(),
                payload_stdin: false,
                subject: "A subject".to_owned(),
                body: "A body".to_owned(),
            }
        );
        assert!(Cli::try_parse_from(["email", "--idempotency-key", "subject",]).is_err());
        assert!(
            Cli::try_parse_from(["email", "--idempotency-key", "", "A subject", "A body",])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn cli_accepts_repeated_local_attachments() -> TestResult {
        let cli = Cli::try_parse_from([
            "email",
            "--attach",
            "/private/a.pdf",
            "--attach",
            "/private/b.pdf",
            "Subject",
            "-",
        ])?;
        assert_eq!(
            cli.attachments,
            [
                std::path::PathBuf::from("/private/a.pdf"),
                std::path::PathBuf::from("/private/b.pdf")
            ]
        );
        Ok(())
    }

    #[test]
    fn attachments_capture_bytes_without_transmitting_paths() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("resume.pdf");
        std::fs::write(&path, b"%PDF-fixture\0\xff")?;
        let attachment = read_attachment(&path)?;
        std::fs::write(&path, b"changed after capture")?;
        assert_eq!(attachment.filename, "resume.pdf");
        assert_eq!(attachment.content, b"%PDF-fixture\0\xff");
        assert!(read_attachment(directory.path()).is_err());
        let missing = directory.path().join("secret-not-found.pdf");
        let error = read_attachment(&missing)
            .err()
            .ok_or("missing file was accepted")?;
        assert!(!error.to_string().contains("secret-not-found"));
        for invalid in [
            "",
            ".",
            "..",
            "/private/resume.pdf",
            "..\\resume.pdf",
            "bad\nname.pdf",
        ] {
            assert!(validate_attachment_filename(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn body_dash_reads_utf8_stdin() -> TestResult {
        assert_eq!(
            read_body("-", Cursor::new("first line\nsecond line"))?,
            "first line\nsecond line"
        );
        assert_eq!(read_body("literal", Cursor::new("ignored"))?, "literal");
        let error = read_body("-", Cursor::new([0xff]))
            .err()
            .ok_or("invalid UTF-8 unexpectedly succeeded")?;
        assert_eq!(
            error.to_string(),
            "unable to read a UTF-8 email body from stdin"
        );
        Ok(())
    }

    #[test]
    fn api_key_must_be_nonblank_without_surrounding_whitespace() {
        assert_eq!(
            validate_api_key("re_test".to_owned()),
            Ok("re_test".to_owned())
        );
        assert!(validate_api_key(String::new()).is_err());
        assert!(validate_api_key(" re_test".to_owned()).is_err());
        assert!(validate_api_key("re_test\n".to_owned()).is_err());
    }

    #[test]
    fn idempotency_keys_are_unique_uuid_v7_values() -> TestResult {
        let first = new_idempotency_key();
        let second = new_idempotency_key();
        assert_ne!(first, second);
        let uuid = first
            .strip_prefix("email/")
            .ok_or("idempotency key had the wrong prefix")?;
        assert_eq!(
            Uuid::parse_str(uuid)?.get_version(),
            Some(Version::SortRand)
        );
        Ok(())
    }

    #[test]
    fn caller_idempotency_keys_are_strict_header_values() {
        let longest = "a".repeat(256);
        assert_eq!(
            parse_idempotency_key("decisions/daily/2026-09-01"),
            Ok("decisions/daily/2026-09-01".to_owned())
        );
        assert_eq!(parse_idempotency_key(&longest), Ok(longest));

        for invalid in [
            "",
            "has space",
            " leading",
            "trailing\t",
            "line\nbreak",
            "café",
        ] {
            assert!(
                parse_idempotency_key(invalid).is_err(),
                "accepted invalid key: {invalid:?}"
            );
        }
        assert!(parse_idempotency_key(&"a".repeat(257)).is_err());
    }

    #[tokio::test]
    async fn request_has_exact_fixed_addresses_payload_and_headers() -> TestResult {
        let (endpoint, requests, server) = serve(vec![TestResponse {
            status: 200,
            reason: "OK",
            body: r#"{"id":"email_123"}"#,
        }])?;
        let email_id = send_to(
            &endpoint,
            "test-secret",
            "email/fixture",
            "Hello ✓",
            "First line\nSecond line",
            &[],
        )
        .await?;
        assert_eq!(email_id, "email_123");

        let request = requests.recv_timeout(Duration::from_secs(2))?;
        let (headers, body) = split_request(&request)?;
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /emails http/1.1"));
        assert!(headers.contains("authorization: bearer test-secret"));
        assert!(headers.contains("idempotency-key: email/fixture"));
        assert!(headers.contains(concat!("user-agent: email/", env!("CARGO_PKG_VERSION"))));

        let body: Value = serde_json::from_str(body)?;
        assert_eq!(body.as_object().map(serde_json::Map::len), Some(4));
        assert_eq!(body["from"], FROM);
        assert_eq!(body["to"], serde_json::json!([TO]));
        assert_eq!(body["subject"], "Hello ✓");
        assert_eq!(body["text"], "First line\nSecond line");
        assert!(body.get("html").is_none());

        finish_server(server)?;
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn transient_failures_retry_three_times_with_one_frozen_request() -> TestResult {
        let (endpoint, requests, server) = serve(vec![
            TestResponse {
                status: 429,
                reason: "Too Many Requests",
                body: r#"{"message":"retry"}"#,
            },
            TestResponse {
                status: 503,
                reason: "Service Unavailable",
                body: r#"{"message":"retry again"}"#,
            },
            TestResponse {
                status: 200,
                reason: "OK",
                body: r#"{"id":"email_after_retry"}"#,
            },
        ])?;

        let attachment = super::api::Attachment {
            filename: "resume.pdf".to_owned(),
            content: b"%PDF-1.7\n".to_vec(),
        };
        let email_id = send_to(
            &endpoint,
            "test-secret",
            "email/frozen",
            "Subject",
            "Body",
            &[attachment],
        )
        .await?;
        assert_eq!(email_id, "email_after_retry");

        let first = requests.recv_timeout(Duration::from_secs(2))?;
        let second = requests.recv_timeout(Duration::from_secs(2))?;
        let third = requests.recv_timeout(Duration::from_secs(2))?;
        for request in [&first, &second, &third] {
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("idempotency-key: email/frozen")
            );
        }
        assert_eq!(split_request(&first)?.1, split_request(&second)?.1);
        assert_eq!(split_request(&second)?.1, split_request(&third)?.1);
        let payload: Value = serde_json::from_str(split_request(&first)?.1)?;
        assert_eq!(
            payload["attachments"],
            serde_json::json!([{
                "filename": "resume.pdf",
                "content": "JVBERi0xLjcK"
            }])
        );
        finish_server(server)?;
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn permanent_rejection_is_not_retried_and_hides_remote_body() -> TestResult {
        let (endpoint, requests, server) = serve(vec![TestResponse {
            status: 422,
            reason: "Unprocessable Entity",
            body: r#"{"message":"test-secret and private body"}"#,
        }])?;
        let error = send_to(
            &endpoint,
            "test-secret",
            "email/fixture",
            "Subject",
            "private body",
            &[],
        )
        .await
        .err()
        .ok_or("Resend rejection unexpectedly succeeded")?;
        assert_eq!(
            error.to_string(),
            "Resend rejected email with HTTP 422 Unprocessable Entity"
        );
        assert!(!error.to_string().contains("test-secret"));
        assert!(!error.to_string().contains("private body"));
        let _ = requests.recv_timeout(Duration::from_secs(2))?;
        finish_server(server)?;
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn invalid_success_response_is_secret_safe() -> TestResult {
        let (endpoint, _requests, server) = serve(vec![TestResponse {
            status: 200,
            reason: "OK",
            body: r#"{"id":"  ","message":"test-secret"}"#,
        }])?;
        let error = send_to(
            &endpoint,
            "test-secret",
            "email/fixture",
            "Subject",
            "Body",
            &[],
        )
        .await
        .err()
        .ok_or("invalid response unexpectedly succeeded")?;
        assert_eq!(error.to_string(), "Resend returned an invalid email ID");
        assert!(!error.to_string().contains("test-secret"));
        finish_server(server)?;
        Ok(())
    }

    fn serve(responses: Vec<TestResponse>) -> TestResult<TestServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || -> std::io::Result<()> {
            for response in responses {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request_is_complete(&request) {
                        break;
                    }
                }
                let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
                let reply = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.reason,
                    response.body.len(),
                    response.body,
                );
                stream.write_all(reply.as_bytes())?;
            }
            Ok(())
        });
        Ok((format!("http://{address}/emails"), receiver, server))
    }

    fn request_is_complete(request: &[u8]) -> bool {
        let text = String::from_utf8_lossy(request);
        let Some((headers, body)) = text.split_once("\r\n\r\n") else {
            return false;
        };
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        });
        content_length.is_some_and(|length| body.len() >= length)
    }

    fn split_request(request: &str) -> TestResult<(&str, &str)> {
        request
            .split_once("\r\n\r\n")
            .ok_or_else(|| "request did not contain an HTTP header boundary".into())
    }

    fn finish_server(server: thread::JoinHandle<std::io::Result<()>>) -> TestResult {
        match server.join() {
            Ok(result) => result.map_err(Into::into),
            Err(_) => Err("test Resend server panicked".into()),
        }
    }
}

pub mod installation;
