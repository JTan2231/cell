pub mod adapters;
pub mod http;
pub mod models;
pub mod runner;
pub mod store;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[must_use]
pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[must_use]
pub fn timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

#[must_use]
pub fn stable_id(prefix: &str, value: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(value.as_bytes());
    format!("{prefix}_{hash:x}")
}

/// Normalizes an HTTP URL while retaining identity-bearing query parameters.
///
/// # Errors
/// Returns an error for malformed URLs, unsupported schemes, or embedded credentials.
pub fn normalize_url(value: &str) -> Result<String> {
    let mut url = url::Url::parse(value)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("expected an HTTP(S) URL without credentials".into());
    }
    url.set_fragment(None);
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !k.starts_with("utm_") && k != "gh_src")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Ok(url.into())
}

pub mod installation;
