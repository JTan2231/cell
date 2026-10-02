/// Returns a canonical board URL only for an explicitly supported ATS host.
#[must_use]
pub fn canonical_board_url(input: &str) -> Option<String> {
    Board::from_url(&Url::parse(input).ok()?).map(|board| board.url())
}

/// Identifies an ATS tenant independently from job IDs and shared hosting domains.
#[must_use]
pub fn board_identity(input: &str) -> Option<String> {
    Board::from_url(&Url::parse(input).ok()?).map(|board| board.identity())
}

/// Identifies the supported ATS provider of a board, API or posting URL.
#[must_use]
pub fn ats_provider(input: &str) -> Option<&'static str> {
    Some(match Board::from_url(&Url::parse(input).ok()?)? {
        Board::Ashby(_) => "ashby",
        Board::Greenhouse(_) => "greenhouse",
        Board::Lever { .. } => "lever",
    })
}

/// Tests whether one stored job is the posting selected by a public job URL.
///
/// # Errors
/// Returns an error when the URL is not public or names a supported ATS board
/// without identifying one posting.
pub fn job_url_matches(job: &Job, input: &str) -> Result<bool, String> {
    Ok(JobSelector::from_url(input)?.matches(&job.source_key, &job.url))
}

/// Validates that a public URL can select one job.
///
/// # Errors
/// Returns an error for an unsafe URL or a supported ATS board URL without a
/// posting identity.
pub fn validate_job_url(input: &str) -> Result<(), String> {
    JobSelector::from_url(input).map(|_| ())
}

use crate::models::Job;
use std::net::IpAddr;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Board {
    Greenhouse(String),
    Ashby(String),
    Lever { site: String, eu: bool },
}

enum JobSelector {
    SourceKey(String),
    Url(String),
}

impl JobSelector {
    fn matches(&self, source_key: &str, job_url: &str) -> bool {
        match self {
            Self::SourceKey(key) => source_key == key,
            Self::Url(url) => crate::normalize_url(job_url)
                .is_ok_and(|stored| stored.trim_end_matches('/') == url.trim_end_matches('/')),
        }
    }

    fn from_url(input: &str) -> Result<Self, String> {
        let url = public_url(input)?;
        if let Some(key) = ats_job_source_key(&url) {
            return Ok(Self::SourceKey(key));
        }
        if Board::from_url(&url).is_some() {
            return Err("URL must identify one supported ATS job posting".into());
        }
        let normalized = crate::normalize_url(input).map_err(|error| error.to_string())?;
        Ok(Self::Url(normalized))
    }
}

fn ats_job_source_key(url: &Url) -> Option<String> {
    let path: Vec<_> = url
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect();
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    };
    match (url.host_str()?, path.as_slice()) {
        ("boards.greenhouse.io" | "job-boards.greenhouse.io", [board, "jobs", id])
            if valid(board) && valid(id) =>
        {
            Some(format!("greenhouse:{board}:{id}"))
        }
        ("jobs.ashbyhq.com", [board, id] | [board, id, "application"])
            if valid(board) && valid(id) =>
        {
            Some(format!("ashby:{board}:{id}"))
        }
        (host @ ("jobs.lever.co" | "jobs.eu.lever.co"), [board, id] | [board, id, "apply"])
            if valid(board) && valid(id) =>
        {
            Some(format!(
                "lever:{}:{board}:{id}",
                if host == "jobs.eu.lever.co" {
                    "eu"
                } else {
                    "global"
                }
            ))
        }
        _ => None,
    }
}

impl Board {
    fn from_url(url: &Url) -> Option<Self> {
        let path: Vec<_> = url
            .path_segments()?
            .filter(|part| !part.is_empty())
            .collect();
        let token = |index: usize| -> Option<String> {
            path.get(index)
                .filter(|part| {
                    !part.is_empty()
                        && part.len() <= 128
                        && part
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                })
                .map(|part| (*part).to_owned())
        };
        match url.host_str()? {
            "boards.greenhouse.io" | "job-boards.greenhouse.io" => {
                if path.first() == Some(&"embed") {
                    url.query_pairs()
                        .find(|(name, _)| name == "for")
                        .and_then(|(_, value)| {
                            let board = value.into_owned();
                            if board.len() <= 128
                                && !board.is_empty()
                                && board
                                    .chars()
                                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                            {
                                Some(Self::Greenhouse(board))
                            } else {
                                None
                            }
                        })
                } else {
                    token(0).map(Self::Greenhouse)
                }
            }
            "boards-api.greenhouse.io"
                if path.first() == Some(&"v1") && path.get(1) == Some(&"boards") =>
            {
                token(2).map(Self::Greenhouse)
            }
            "jobs.ashbyhq.com" => token(0).map(Self::Ashby),
            "api.ashbyhq.com"
                if path.first() == Some(&"posting-api") && path.get(1) == Some(&"job-board") =>
            {
                token(2).map(Self::Ashby)
            }
            "jobs.lever.co" | "jobs.eu.lever.co" => token(0).map(|site| Self::Lever {
                site,
                eu: url.host_str() == Some("jobs.eu.lever.co"),
            }),
            "api.lever.co" | "api.eu.lever.co"
                if path.first() == Some(&"v0") && path.get(1) == Some(&"postings") =>
            {
                token(2).map(|site| Self::Lever {
                    site,
                    eu: url.host_str() == Some("api.eu.lever.co"),
                })
            }
            _ => None,
        }
    }

    fn url(&self) -> String {
        match self {
            Self::Greenhouse(board) => format!("https://job-boards.greenhouse.io/{board}"),
            Self::Ashby(board) => format!("https://jobs.ashbyhq.com/{board}"),
            Self::Lever { site, eu } => format!(
                "https://jobs.{}lever.co/{site}",
                if *eu { "eu." } else { "" }
            ),
        }
    }

    fn identity(&self) -> String {
        match self {
            Self::Greenhouse(board) => format!("greenhouse:{board}"),
            Self::Ashby(board) => format!("ashby:{board}"),
            Self::Lever { site, eu } => {
                format!("lever:{}:{site}", if *eu { "eu" } else { "global" })
            }
        }
    }
}

/// # Errors
/// Rejects non-HTTP schemes, userinfo, unusual ports, and non-public names or addresses.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // These are DNS suffixes, not file extensions.
fn public_url(input: &str) -> Result<Url, String> {
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
    fn board_identity_keeps_hosted_tenants_separate() {
        let a =
            Board::from_url(&Url::parse("https://jobs.ashbyhq.com/acme/a-job").unwrap()).unwrap();
        let b =
            Board::from_url(&Url::parse("https://jobs.ashbyhq.com/other/a-job").unwrap()).unwrap();
        assert_ne!(a.identity(), b.identity());
        assert_ne!(
            Board::from_url(&Url::parse("https://jobs.eu.lever.co/acme/123").unwrap())
                .unwrap()
                .identity(),
            Board::from_url(&Url::parse("https://jobs.lever.co/acme/123").unwrap())
                .unwrap()
                .identity()
        );
    }

    #[test]
    fn exact_job_urls_match_native_ats_identity_or_normalized_url() {
        let mut job = Job {
            id: "job-one".into(),
            revision: 1,
            company_id: "company-one".into(),
            source_id: "source-one".into(),
            source_key: "ashby:acme:role-one".into(),
            title: "Engineer".into(),
            url: "https://jobs.ashbyhq.com/acme/role-one".into(),
            apply_url: None,
            location: None,
            remote: None,
            employment_type: None,
            source_published_at: None,
            source_updated_at: None,
            source_internal_id: None,
            first_seen_at: "2026-09-10T00:00:00Z".into(),
            last_seen_at: "2026-09-10T00:00:00Z".into(),
            availability: "listed".into(),
            missing_complete_snapshots: 0,
            first_missing_at: None,
            description: None,
            evidence: vec![],
            compensation: vec![],
            geographic_eligibility: vec![],
            published_at_semantics: None,
            content_fingerprint: None,
            parser_version: "fixture".into(),
        };
        assert!(
            job_url_matches(&job, "https://jobs.ashbyhq.com/acme/role-one/application").unwrap()
        );
        assert!(!job_url_matches(&job, "https://jobs.ashbyhq.com/acme/role-two").unwrap());
        assert!(job_url_matches(&job, "https://jobs.ashbyhq.com/acme").is_err());

        job.source_key = "jsonld:https://careers.acme.example/jobs/one".into();
        job.url = "https://careers.acme.example/jobs/one?utm_source=search".into();
        assert!(job_url_matches(&job, "https://careers.acme.example/jobs/one/").unwrap());
    }
}
