use std::fmt::Write as _;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokio::runtime::Builder;
use uuid::Uuid;

use crate::config::EmailConfig;
use crate::digest::{DailyDigest, DigestItem};
use crate::error::{AppError, AppResult};
use crate::render::terminal_text;

const RESEND_ENDPOINT: &str = "https://api.resend.com/emails";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(100), Duration::from_millis(250)];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EmailPreview {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) attention_count: usize,
    pub(crate) pending_concern_count: usize,
    pub(crate) todo_count: usize,
    pub(crate) subject: String,
    pub(crate) text: String,
    pub(crate) html: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SendResult {
    pub(crate) email_id: String,
    pub(crate) idempotency_key: String,
}

#[derive(Serialize)]
struct ResendRequest<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
    html: &'a str,
}

#[derive(Deserialize)]
struct ResendResponse {
    id: String,
}

impl EmailPreview {
    #[must_use]
    pub(crate) fn new(config: &EmailConfig, digest: &DailyDigest) -> Self {
        let attention_count = digest.attention_count();
        let pending_concern_count = digest.pending_concern_count;
        let todo_count = digest.open_todo_count;
        let subject = render_subject(digest);
        let (text, html) = render_bodies(digest);
        Self {
            from: config.from.clone(),
            to: config.to.clone(),
            attention_count,
            pending_concern_count,
            todo_count,
            subject,
            text,
            html,
        }
    }
}

pub(crate) fn send(preview: &EmailPreview, scheduled: bool) -> AppResult<SendResult> {
    let api_key = resend_api_key()?;
    let idempotency_key = if scheduled {
        let now = OffsetDateTime::now_local().map_err(|error| {
            AppError::unexpected(
                "local_time_unavailable",
                format!("unable to determine the local email schedule date: {error}"),
            )
        })?;
        scheduled_idempotency_key(scheduled_occurrence_date(now)?)
    } else {
        ad_hoc_idempotency_key()
    };
    let email_id = send_to(RESEND_ENDPOINT, &api_key, &idempotency_key, preview)?;
    Ok(SendResult {
        email_id,
        idempotency_key,
    })
}

fn resend_api_key() -> AppResult<String> {
    let api_key = std::env::var("RESEND_API_KEY").map_err(|_| {
        AppError::invalid(
            "resend_api_key_not_configured",
            "RESEND_API_KEY must be set to send Todo email",
        )
    })?;
    if api_key.trim().is_empty() || api_key.trim() != api_key {
        return Err(AppError::invalid(
            "resend_api_key_not_configured",
            "RESEND_API_KEY must be nonblank and contain no surrounding whitespace",
        ));
    }
    Ok(api_key)
}

fn scheduled_idempotency_key(date: Date) -> String {
    format!("todo-daily-email/{date}")
}

fn scheduled_occurrence_date(now: OffsetDateTime) -> AppResult<Date> {
    if now.hour() >= 9 {
        return Ok(now.date());
    }
    now.date().previous_day().ok_or_else(|| {
        AppError::unexpected(
            "local_time_unavailable",
            "unable to determine the previous local email schedule date",
        )
    })
}

fn ad_hoc_idempotency_key() -> String {
    format!("todo-email/{}", Uuid::now_v7())
}

fn send_to(
    endpoint: &str,
    api_key: &str,
    idempotency_key: &str,
    preview: &EmailPreview,
) -> AppResult<String> {
    let mut headers = HeaderMap::new();
    let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
        AppError::invalid(
            "invalid_resend_api_key",
            "RESEND_API_KEY contains characters that cannot be sent in an HTTP header",
        )
    })?;
    authorization.set_sensitive(true);
    headers.insert(AUTHORIZATION, authorization);
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("todo/", env!("CARGO_PKG_VERSION"))),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| {
            AppError::unexpected(
                "email_client_failed",
                format!("unable to initialize the Resend client: {error}"),
            )
        })?;
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            AppError::unexpected(
                "email_client_failed",
                format!("unable to initialize the Resend runtime: {error}"),
            )
        })?;
    let request = ResendRequest {
        from: &preview.from,
        to: [&preview.to],
        subject: &preview.subject,
        text: &preview.text,
        html: &preview.html,
    };
    runtime.block_on(async {
        for attempt in 0..=RETRY_DELAYS.len() {
            let response = client
                .post(endpoint)
                .header("Idempotency-Key", idempotency_key)
                .json(&request)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    if let Some(delay) = RETRY_DELAYS.get(attempt) {
                        tokio::time::sleep(*delay).await;
                        continue;
                    }
                    return Err(AppError::unexpected(
                        "email_send_failed",
                        format!("unable to send Todo email through Resend: {error}"),
                    ));
                }
            };
            let status = response.status();
            if (status.as_u16() == 429 || status.is_server_error())
                && let Some(delay) = RETRY_DELAYS.get(attempt)
            {
                tokio::time::sleep(*delay).await;
                continue;
            }
            if !status.is_success() {
                return Err(AppError::unexpected(
                    "email_send_failed",
                    format!("Resend rejected Todo email with HTTP {status}"),
                ));
            }
            let response = response.json::<ResendResponse>().await.map_err(|error| {
                AppError::unexpected(
                    "invalid_email_response",
                    format!("Resend returned an invalid success response: {error}"),
                )
            })?;
            if response.id.trim().is_empty() {
                return Err(AppError::unexpected(
                    "invalid_email_response",
                    "Resend returned a blank email ID",
                ));
            }
            return Ok(response.id);
        }
        Err(AppError::unexpected(
            "email_send_failed",
            "Todo email exhausted its Resend attempts",
        ))
    })
}

fn render_subject(digest: &DailyDigest) -> String {
    if digest.open_todo_count == 0 && digest.pending_concern_count == 0 {
        return "Todo daily: all clear".to_owned();
    }
    format!(
        "Todo daily: {} · {}",
        attention_phrase(digest.attention_count()),
        open_todo_phrase(digest.open_todo_count),
    )
}

fn render_bodies(digest: &DailyDigest) -> (String, String) {
    if digest.open_todo_count == 0 && digest.pending_concern_count == 0 {
        return (
            "Todo daily\n\nNothing needs attention. There are no open todos or unresolved concerns."
                .to_owned(),
            concat!(
                "<h1>Todo daily</h1>",
                "<p>Nothing needs attention. There are no open todos or unresolved concerns.</p>"
            )
            .to_owned(),
        );
    }

    let summary = format!(
        "{} · {} · {}",
        attention_phrase(digest.attention_count()),
        open_todo_phrase(digest.open_todo_count),
        unresolved_concern_phrase(digest.pending_concern_count),
    );
    let mut text = format!("Todo daily\n{summary}");
    let mut html = format!("<h1>Todo daily</h1><p>{}</p>", html_text(&summary));
    render_section(
        &mut text,
        &mut html,
        "Needs your decision",
        &digest.decisions,
    );
    render_section(&mut text, &mut html, "Needs follow-up", &digest.followups);
    render_section(&mut text, &mut html, "Other open todos", &digest.other_open);
    (text, html)
}

fn render_section(text: &mut String, html: &mut String, heading: &str, items: &[DigestItem]) {
    if items.is_empty() {
        return;
    }
    let _ = write!(text, "\n\n{heading}");
    let _ = write!(html, "<h2>{}</h2><ul>", html_text(heading));
    for item in items {
        let title = terminal_text(&item.title, false);
        let message = terminal_text(&item.message, false);
        let references = item
            .references
            .iter()
            .map(|reference| terminal_text(reference, false))
            .collect::<Vec<_>>()
            .join(" · ");
        let _ = write!(text, "\n\n{title}\n{message}");
        let _ = write!(
            html,
            "<li><strong>{}</strong><br>{}",
            html_text(&title),
            html_text(&message)
        );
        if !references.is_empty() {
            let _ = write!(text, "\nReference: {references}");
            let _ = write!(
                html,
                "<br><small>Reference: {}</small>",
                html_text(&references)
            );
        }
        for command in &item.inspect_commands {
            let command = terminal_text(command, false);
            let _ = write!(text, "\nInspect: {command}");
            let _ = write!(
                html,
                "<br><small>Inspect: <code>{}</code></small>",
                html_text(&command)
            );
        }
        html.push_str("</li>");
    }
    html.push_str("</ul>");
}

fn attention_phrase(count: usize) -> String {
    match count {
        0 => "none need attention".to_owned(),
        1 => "1 needs attention".to_owned(),
        _ => format!("{count} need attention"),
    }
}

fn open_todo_phrase(count: usize) -> String {
    if count == 1 {
        "1 open todo".to_owned()
    } else {
        format!("{count} open todos")
    }
}

fn unresolved_concern_phrase(count: usize) -> String {
    if count == 1 {
        "1 unresolved concern".to_owned()
    } else {
        format!("{count} unresolved concerns")
    }
}

fn html_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use serde_json::Value;
    use time::{Date, Month};

    use super::{
        EmailPreview, ad_hoc_idempotency_key, scheduled_idempotency_key, scheduled_occurrence_date,
        send_to,
    };
    use crate::config::EmailConfig;
    use crate::digest::{DailyDigest, DigestItem};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
    type TestServer = (
        String,
        mpsc::Receiver<String>,
        thread::JoinHandle<std::io::Result<()>>,
    );

    struct TestResponse {
        status: u16,
        reason: String,
        body: String,
    }

    #[test]
    fn renders_plain_language_sections_with_secondary_references() {
        let config = EmailConfig {
            from: "Todo <todo@example.com>".to_owned(),
            to: "person@example.com".to_owned(),
        };
        let digest = DailyDigest {
            decisions: vec![item(
                "Review <this> & \"that's\"\u{1b}",
                "A proposed desired state is ready for your review.",
                &["Todo t2", "Desired-state design d3"],
                &["todo design show d3"],
            )],
            followups: vec![item(
                "Captured concern",
                "This captured concern remains unresolved.",
                &["Concern c4"],
                &["todo concern show c4"],
            )],
            other_open: vec![item(
                "First",
                "The desired state is accepted; this todo remains open.",
                &["Todo t1", "Desired-state design d1"],
                &["todo show t1"],
            )],
            open_todo_count: 2,
            pending_concern_count: 1,
        };
        let preview = EmailPreview::new(&config, &digest);

        assert_eq!(preview.todo_count, 2);
        assert_eq!(preview.pending_concern_count, 1);
        assert_eq!(preview.attention_count, 2);
        assert_eq!(
            preview.subject,
            "Todo daily: 2 need attention · 2 open todos"
        );
        assert!(preview.text.starts_with(concat!(
            "Todo daily\n",
            "2 need attention · 2 open todos · 1 unresolved concern\n\n",
            "Needs your decision\n\n",
            "Review <this> & \"that's\"\\u{1b}\n",
        )));
        assert!(
            preview
                .text
                .contains("Reference: Todo t2 · Desired-state design d3")
        );
        assert!(preview.text.contains("Needs follow-up\n\nCaptured concern"));
        assert!(preview.text.contains("Other open todos\n\nFirst"));
        assert!(
            preview.html.contains(
                "<strong>Review &lt;this&gt; &amp; &quot;that&#39;s&quot;\\u{1b}</strong>"
            )
        );
        assert!(
            preview
                .html
                .contains("<small>Reference: Todo t2 · Desired-state design d3</small>")
        );
        assert!(!preview.html.contains("<strong>t2</strong>"));
    }

    #[test]
    fn empty_digest_is_an_explicit_all_clear() {
        let digest = empty_digest();
        let preview = EmailPreview::new(
            &EmailConfig {
                from: "sender@example.com".to_owned(),
                to: "person@example.com".to_owned(),
            },
            &digest,
        );
        assert_eq!(preview.subject, "Todo daily: all clear");
        assert!(
            preview
                .text
                .contains("no open todos or unresolved concerns")
        );
        assert!(
            preview
                .html
                .contains("no open todos or unresolved concerns")
        );
    }

    #[test]
    fn subject_distinguishes_attention_from_open_state() {
        let config = EmailConfig {
            from: "sender@example.com".to_owned(),
            to: "person@example.com".to_owned(),
        };
        let concern_only = DailyDigest {
            decisions: Vec::new(),
            followups: vec![item(
                "Captured concern",
                "This captured concern remains unresolved.",
                &["Concern c1"],
                &["todo concern show c1"],
            )],
            other_open: Vec::new(),
            open_todo_count: 0,
            pending_concern_count: 1,
        };
        assert_eq!(
            EmailPreview::new(&config, &concern_only).subject,
            "Todo daily: 1 needs attention · 0 open todos"
        );

        let open_without_attention = DailyDigest {
            decisions: Vec::new(),
            followups: Vec::new(),
            other_open: vec![item(
                "Open todo",
                "The desired state is accepted; this todo remains open.",
                &["Todo t1"],
                &["todo show t1"],
            )],
            open_todo_count: 1,
            pending_concern_count: 0,
        };
        assert_eq!(
            EmailPreview::new(&config, &open_without_attention).subject,
            "Todo daily: none need attention · 1 open todo"
        );
    }

    #[test]
    fn scheduled_keys_are_daily_and_manual_keys_are_ad_hoc() -> TestResult {
        let date = Date::from_calendar_date(2026, Month::August, 27)?;
        assert_eq!(
            scheduled_idempotency_key(date),
            "todo-daily-email/2026-08-27"
        );
        let first = ad_hoc_idempotency_key();
        let second = ad_hoc_idempotency_key();
        assert!(first.starts_with("todo-email/"));
        assert_ne!(first, second);
        Ok(())
    }

    #[test]
    fn scheduled_date_tracks_the_most_recent_local_nine_am() -> TestResult {
        let previous = Date::from_calendar_date(2026, Month::August, 27)?;
        let today = Date::from_calendar_date(2026, Month::August, 28)?;
        assert_eq!(
            scheduled_occurrence_date(today.with_hms(8, 59, 59)?.assume_utc())?,
            previous
        );
        assert_eq!(
            scheduled_occurrence_date(today.with_hms(9, 0, 0)?.assume_utc())?,
            today
        );
        Ok(())
    }

    #[test]
    fn resend_request_has_exact_payload_and_headers() -> TestResult {
        let body = r#"{"id":"email_123"}"#;
        let (endpoint, requests, server) = serve_once(200, "OK", body)?;
        let digest = one_open_digest("First");
        let preview = EmailPreview::new(
            &EmailConfig {
                from: "Todo <todo@example.com>".to_owned(),
                to: "person@example.com".to_owned(),
            },
            &digest,
        );
        let email_id = send_to(&endpoint, "test-secret", "todo-email/fixture", &preview)?;
        assert_eq!(email_id, "email_123");

        let request = requests.recv_timeout(Duration::from_secs(2))?;
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .ok_or("request did not contain an HTTP header boundary")?;
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /emails http/1.1"));
        assert!(headers.contains("authorization: bearer test-secret"));
        assert!(headers.contains("idempotency-key: todo-email/fixture"));
        assert!(headers.contains(concat!("user-agent: todo/", env!("CARGO_PKG_VERSION"))));
        let body: Value = serde_json::from_str(body)?;
        assert_eq!(body["from"], "Todo <todo@example.com>");
        assert_eq!(body["to"][0], "person@example.com");
        assert_eq!(
            body["subject"],
            "Todo daily: 1 needs attention · 1 open todo"
        );
        assert!(body.get("text").is_some());
        assert!(body.get("html").is_some());
        assert!(body.get("scheduled_at").is_none());
        finish_server(server)?;
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn transient_resend_rejection_retries_the_frozen_request() -> TestResult {
        let (endpoint, requests, server) = serve(vec![
            TestResponse {
                status: 429,
                reason: "Too Many Requests".to_owned(),
                body: r#"{"message":"retry"}"#.to_owned(),
            },
            TestResponse {
                status: 200,
                reason: "OK".to_owned(),
                body: r#"{"id":"email_after_retry"}"#.to_owned(),
            },
        ])?;
        let digest = one_open_digest("First");
        let preview = EmailPreview::new(
            &EmailConfig {
                from: "sender@example.com".to_owned(),
                to: "person@example.com".to_owned(),
            },
            &digest,
        );
        let idempotency_key = "todo-daily-email/2026-08-27";
        let email_id = send_to(&endpoint, "test-secret", idempotency_key, &preview)?;
        assert_eq!(email_id, "email_after_retry");
        let first = requests.recv_timeout(Duration::from_secs(2))?;
        let second = requests.recv_timeout(Duration::from_secs(2))?;
        let first_body = first
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .ok_or("first request did not contain a body")?;
        let second_body = second
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .ok_or("second request did not contain a body")?;
        assert_eq!(first_body, second_body);
        assert!(
            first
                .to_ascii_lowercase()
                .contains("idempotency-key: todo-daily-email/2026-08-27")
        );
        assert!(
            second
                .to_ascii_lowercase()
                .contains("idempotency-key: todo-daily-email/2026-08-27")
        );
        finish_server(server)?;
        Ok(())
    }

    #[test]
    fn resend_errors_are_stable_and_do_not_expose_the_key() -> TestResult {
        let (endpoint, _request, server) =
            serve_once(422, "Unprocessable Entity", r#"{"message":"test-secret"}"#)?;
        let digest = empty_digest();
        let preview = EmailPreview::new(
            &EmailConfig {
                from: "sender@example.com".to_owned(),
                to: "person@example.com".to_owned(),
            },
            &digest,
        );
        let error = send_to(&endpoint, "test-secret", "manual-key", &preview)
            .err()
            .ok_or("Resend rejection unexpectedly succeeded")?;
        assert_eq!(error.code(), "email_send_failed");
        assert!(!error.to_string().contains("test-secret"));
        finish_server(server)?;
        Ok(())
    }

    fn empty_digest() -> DailyDigest {
        DailyDigest {
            decisions: Vec::new(),
            followups: Vec::new(),
            other_open: Vec::new(),
            open_todo_count: 0,
            pending_concern_count: 0,
        }
    }

    fn one_open_digest(title: &str) -> DailyDigest {
        DailyDigest {
            decisions: Vec::new(),
            followups: vec![item(
                title,
                "The current situation has not been assessed.",
                &["Todo t1"],
                &["todo show t1"],
            )],
            other_open: Vec::new(),
            open_todo_count: 1,
            pending_concern_count: 0,
        }
    }

    fn item(title: &str, message: &str, references: &[&str], commands: &[&str]) -> DigestItem {
        DigestItem {
            title: title.to_owned(),
            message: message.to_owned(),
            references: references.iter().map(|value| (*value).to_owned()).collect(),
            inspect_commands: commands.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    fn serve_once(status: u16, reason: &str, response_body: &str) -> TestResult<TestServer> {
        serve(vec![TestResponse {
            status,
            reason: reason.to_owned(),
            body: response_body.to_owned(),
        }])
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

    fn finish_server(server: thread::JoinHandle<std::io::Result<()>>) -> TestResult {
        match server.join() {
            Ok(result) => result.map_err(Into::into),
            Err(_) => Err("test Resend server panicked".into()),
        }
    }
}
