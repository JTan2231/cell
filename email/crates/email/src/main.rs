use std::fmt;
use std::io;
use std::time::Duration;

use clap::Parser;
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
    /// Email subject.
    subject: String,
    /// Plain-text body, or - to read UTF-8 text from stdin.
    body: String,
}

#[derive(Serialize)]
struct ResendRequest<'a> {
    from: &'static str,
    to: [&'static str; 1],
    subject: &'a str,
    text: &'a str,
}

#[derive(Deserialize)]
struct ResendResponse {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppError(String);

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

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("email: {error}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    let cli = Cli::parse();
    let body = read_body(&cli.body, io::stdin().lock())?;
    let api_key = resend_api_key()?;
    let idempotency_key = new_idempotency_key();
    let email_id = send_to(
        RESEND_ENDPOINT,
        &api_key,
        &idempotency_key,
        &cli.subject,
        &body,
    )
    .await?;
    println!("Sent {email_id}");
    Ok(())
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

fn resend_api_key() -> AppResult<String> {
    let api_key = std::env::var("RESEND_API_KEY")
        .map_err(|_| AppError::new("RESEND_API_KEY must be set to send email"))?;
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

async fn send_to(
    endpoint: &str,
    api_key: &str,
    idempotency_key: &str,
    subject: &str,
    body: &str,
) -> AppResult<String> {
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

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| AppError::new("unable to initialize the Resend client"))?;
    let request = ResendRequest {
        from: FROM,
        to: [TO],
        subject,
        text: body,
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

    use super::{Cli, FROM, TO, new_idempotency_key, read_body, send_to, validate_api_key};

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

        let email_id = send_to(&endpoint, "test-secret", "email/frozen", "Subject", "Body").await?;
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
        let error = send_to(&endpoint, "test-secret", "email/fixture", "Subject", "Body")
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
