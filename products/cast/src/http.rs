//! Bounded public HTTP. Credentials are confined to the two provider API hosts.
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use reqwest::{Client, Method, Url, header};
use serde_json::Value;

pub type Reserve = Arc<dyn Fn(&str, u64) -> Result<String, String> + Send + Sync>;
pub type Settle = Arc<dyn Fn(&str, Option<u64>) -> Result<(), String> + Send + Sync>;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

pub struct HttpClient {
    reserve: Reserve,
    settle: Settle,
    cooldown: Mutex<BTreeMap<String, Instant>>,
}

pub struct HttpText {
    pub url: String,
    pub body: String,
}

impl HttpClient {
    /// Creates a client whose requests use durable reservation and settlement callbacks.
    ///
    /// # Errors
    /// This constructor currently cannot fail; the result leaves initialization extensible.
    pub fn new(reserve: Reserve, settle: Settle) -> Result<Self, String> {
        Ok(Self {
            reserve,
            settle,
            cooldown: Mutex::new(BTreeMap::new()),
        })
    }

    /// Reads public JSON within network and budget limits.
    ///
    /// # Errors
    /// Returns policy, budget, transport, size, or JSON decoding errors.
    pub async fn get_json(&self, url: &str) -> Result<Value, String> {
        let response = self.request(url, None, "http", 0).await?;
        serde_json::from_str(&response.body).map_err(|_| "invalid JSON response".into())
    }

    /// Reads public UTF-8 content and returns its final URL.
    ///
    /// # Errors
    /// Returns policy, budget, transport, size, or text decoding errors.
    pub async fn get_text(&self, url: &str) -> Result<HttpText, String> {
        self.request(url, None, "http", 0).await
    }

    /// Posts unauthenticated JSON within network and budget limits.
    ///
    /// # Errors
    /// Returns policy, budget, transport, size, or JSON decoding errors.
    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value, String> {
        let response = self.request(url, Some(body), "http", 0).await?;
        serde_json::from_str(&response.body).map_err(|_| "invalid JSON response".into())
    }

    /// Calls a known provider with a host-confined credential and reserved native units.
    ///
    /// # Errors
    /// Returns credential, policy, budget, transport, size, or JSON decoding errors.
    pub async fn provider_json(
        &self,
        provider: &str,
        url: &str,
        body: Option<&Value>,
        max_units: u64,
    ) -> Result<Value, String> {
        let response = self.request(url, body, provider, max_units).await?;
        serde_json::from_str(&response.body).map_err(|_| "invalid provider JSON response".into())
    }

    #[allow(clippy::too_many_lines)]
    async fn request(
        &self,
        input: &str,
        body: Option<&Value>,
        provider: &str,
        max_units: u64,
    ) -> Result<HttpText, String> {
        let mut url = public_url(input)?;
        let credential = match provider {
            "brave"
                if url.scheme() == "https" && url.host_str() == Some("api.search.brave.com") =>
            {
                Some(("X-Subscription-Token", read_key("BRAVE_SEARCH_API_KEY")?))
            }
            "theirstack"
                if url.scheme() == "https" && url.host_str() == Some("api.theirstack.com") =>
            {
                Some((
                    "Authorization",
                    format!("Bearer {}", read_key("THEIRSTACK_API_KEY")?),
                ))
            }
            "http" => None,
            _ => return Err("provider credentials require their exact HTTPS API host".into()),
        };
        let mut redirects = 0;
        let mut retries = 0;
        loop {
            let delay = self
                .cooldown
                .lock()
                .map_err(|_| "HTTP pacing lock poisoned")?
                .get(provider)
                .and_then(|until| until.checked_duration_since(Instant::now()));
            if let Some(delay) = delay {
                if delay > TIMEOUT {
                    return Err(format!(
                        "provider_cooldown: {provider}; retry_after_seconds={}",
                        delay.as_secs() + 1
                    ));
                }
                tokio::time::sleep(delay).await;
            }
            let reservation = (self.reserve)(provider, max_units)?;
            let result =
                tokio::time::timeout(TIMEOUT, send_once(&url, body, credential.as_ref())).await;
            let response = match result {
                Ok(Ok(response)) => response,
                result => {
                    (self.settle)(&reservation, None)?;
                    if retries < 2 {
                        retries += 1;
                        tokio::time::sleep(Duration::from_millis(250 * retries)).await;
                        continue;
                    }
                    return Err(match result {
                        Err(_) => "HTTP request timed out".into(),
                        Ok(Err(error)) => error,
                        _ => unreachable!(),
                    });
                }
            };
            let delay = response_delay(response.headers(), provider).max(match provider {
                "brave" => Duration::from_secs(1),
                "theirstack" => Duration::from_secs(7),
                _ => Duration::ZERO,
            });
            if !delay.is_zero() {
                self.cooldown
                    .lock()
                    .map_err(|_| "HTTP pacing lock poisoned")?
                    .insert(provider.into(), Instant::now() + delay);
            }
            if response.status().is_redirection() {
                (self.settle)(&reservation, None)?;
                if credential.is_some() || body.is_some() {
                    return Err("authenticated or POST redirects are unsupported".into());
                }
                if redirects >= 5 {
                    return Err("HTTP redirect limit reached".into());
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or("redirect has no valid location")?;
                url = public_url(
                    url.join(location)
                        .map_err(|_| "invalid redirect location")?
                        .as_str(),
                )?;
                redirects += 1;
                continue;
            }
            if !response.status().is_success() {
                (self.settle)(&reservation, None)?;
                if (response.status().is_server_error() || response.status().as_u16() == 429)
                    && retries < 2
                    && delay <= TIMEOUT
                {
                    retries += 1;
                    tokio::time::sleep(Duration::from_secs(1 << retries)).await;
                    continue;
                }
                return Err(format!(
                    "HTTP status {} from {}; retry_after_seconds={}",
                    response.status().as_u16(),
                    url.host_str().unwrap_or("unknown"),
                    delay.as_secs()
                ));
            }
            let bytes = match tokio::time::timeout(TIMEOUT, read_body(response)).await {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(error)) => {
                    (self.settle)(&reservation, None)?;
                    return Err(error);
                }
                Err(_) => {
                    (self.settle)(&reservation, None)?;
                    return Err("HTTP body timed out".into());
                }
            };
            let actual = match provider {
                "brave" => serde_json::from_slice::<Value>(&bytes).ok().map(|_| 1),
                "theirstack" => serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|json| {
                        json.get("data")
                            .and_then(Value::as_array)
                            .map(|rows| rows.len() as u64)
                    }),
                _ => Some(0),
            };
            (self.settle)(&reservation, actual)?;
            let text = String::from_utf8(bytes).map_err(|_| "response is not UTF-8")?;
            return Ok(HttpText {
                url: url.into(),
                body: text,
            });
        }
    }
}

fn response_delay(headers: &header::HeaderMap, provider: &str) -> Duration {
    let mut seconds = headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.parse::<u64>().ok().or_else(|| {
                time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc2822)
                    .ok()
                    .and_then(|date| {
                        u64::try_from((date - time::OffsetDateTime::now_utc()).whole_seconds()).ok()
                    })
            })
        })
        .unwrap_or(0);
    if provider == "theirstack" {
        let remaining = headers
            .get("ratelimit-remaining")
            .and_then(|value| value.to_str().ok());
        let reset = headers
            .get("ratelimit-reset")
            .and_then(|value| value.to_str().ok());
        if let (Some(remaining), Some(reset)) = (remaining, reset) {
            for (remaining, reset) in remaining.split(',').zip(reset.split(',')) {
                if remaining.trim() == "0" {
                    seconds = seconds.max(reset.trim().parse::<u64>().unwrap_or(0));
                }
            }
        }
    }
    Duration::from_secs(seconds.min(86400))
}

fn read_key(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| {
            !value.trim().is_empty() && value.len() <= 4096 && !value.contains(['\r', '\n'])
        })
        .ok_or_else(|| format!("credential_missing: {name}"))
}

/// Validates the static public URL policy; DNS addresses are checked at request time.
///
/// # Errors
/// Rejects non-HTTP schemes, userinfo, unusual ports, and non-public names or addresses.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // These are DNS suffixes, not file extensions.
pub fn public_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| "invalid URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 80 && port != 443)
    {
        return Err(
            "only public HTTP(S) URLs without credentials and with standard ports are supported"
                .into(),
        );
    }
    let host = url.host_str().ok_or("URL has no host")?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || !host.contains('.') && !host.contains(':')
    {
        return Err("non-public hostname is unsupported".into());
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>()
        && !public_ip(ip)
    {
        return Err("non-public IP address is unsupported".into());
    }
    Ok(url)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || a == 100 && (64..=127).contains(&b)
                || a == 198 && matches!(b, 18 | 19)
                || a == 192 && b == 0 && c == 0)
        }
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(ip));
            }
            let segments = ip.segments();
            // Only ordinary global unicast. Excludes local, multicast, link-local,
            // documentation and transition ranges that can embed private IPv4.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] == 0xdb8 || segments[1] < 0x0200))
                && segments[0] != 0x2002
        }
    }
}

async fn send_once(
    url: &Url,
    body: Option<&Value>,
    credential: Option<&(&str, String)>,
) -> Result<reqwest::Response, String> {
    let host = url.host_str().ok_or("URL has no host")?;
    let port = url.port_or_known_default().ok_or("URL has no port")?;
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "DNS resolution failed")?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
        return Err("DNS resolved to a non-public or empty address set".into());
    }
    // Pin the checked addresses for this request to prevent a second DNS lookup.
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(TIMEOUT)
        .resolve_to_addrs(host, &addresses)
        .user_agent("Cast/0.1 (+public job discovery)")
        .build()
        .map_err(|_| "HTTP client initialization failed")?;
    let mut request = client.request(
        if body.is_some() {
            Method::POST
        } else {
            Method::GET
        },
        url.clone(),
    );
    if let Some(body) = body {
        request = request.json(body);
    }
    if let Some((name, value)) = credential {
        request = request.header(*name, value);
    }
    request
        .send()
        .await
        .map_err(|_| "HTTP transport failed".into())
}

async fn read_body(response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BYTES as u64)
    {
        return Err("HTTP response exceeds 4 MiB limit".into());
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "HTTP body transport failed")?;
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err("HTTP response exceeds 4 MiB limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn public_network_only() {
        for url in [
            "http://127.0.0.1",
            "http://[::1]",
            "http://[::ffff:127.0.0.1]",
            "http://169.254.169.254",
            "http://10.1.1.1",
            "http://100.64.0.1",
            "http://localhost",
            "file:///tmp/jobs",
            "https://user:secret@example.com",
            "https://example.com:8000",
        ] {
            assert!(public_url(url).is_err(), "{url}");
        }
        assert!(public_url("https://boards.greenhouse.io/acme").is_ok());
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn retry_headers_honor_all_exhausted_provider_windows() {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::RETRY_AFTER, header::HeaderValue::from_static("5"));
        headers.insert(
            "ratelimit-remaining",
            header::HeaderValue::from_static("0, 9, 0, 300"),
        );
        headers.insert(
            "ratelimit-reset",
            header::HeaderValue::from_static("1, 60, 3600, 86400"),
        );
        assert_eq!(
            response_delay(&headers, "theirstack"),
            Duration::from_secs(3600)
        );
        assert_eq!(response_delay(&headers, "http"), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn provider_credential_cannot_target_a_different_host() {
        let client = HttpClient::new(
            Arc::new(|_, _| panic!("must reject before reservation or network")),
            Arc::new(|_, _| Ok(())),
        )
        .unwrap();
        let error = client
            .provider_json("brave", "https://example.com", None, 1)
            .await
            .unwrap_err();
        assert!(error.contains("exact HTTPS API host"));
    }
}
