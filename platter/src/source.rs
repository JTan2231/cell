use crate::agent::CareerEntry;
use anyhow::{Context, Result, bail, ensure};
use cast::models::{Job, Snapshot};
use crm::api::{Client, Data, Request};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Posting {
    pub url: String,
    pub retrieved_at: String,
    pub text: String,
}

pub fn discovery(executable: &Path) -> Result<Snapshot> {
    let output = std::process::Command::new(executable)
        .args(["export", "--json"])
        .output()?;
    ensure!(
        output.status.success(),
        "Cast export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout)?;
    ensure!(
        snapshot.schema_version == 1,
        "unsupported Cast export schema"
    );
    Ok(snapshot)
}

pub fn career_library(executable: &Path) -> Result<Vec<CareerEntry>> {
    let client = Client::new(executable);
    let list = || -> Result<_> {
        let Data::ProfileList { entries, has_more } = client
            .execute(&Request::ListProfileEntries { limit: 1000 })?
            .data
        else {
            bail!("CRM returned the wrong profile-list result")
        };
        ensure!(!has_more, "CRM profile list is incomplete");
        ensure!(!entries.is_empty(), "CRM has no career entries");
        let versions: BTreeMap<_, _> = entries
            .iter()
            .map(|entry| {
                (
                    entry.id.clone(),
                    (entry.title.clone(), entry.updated_at.clone()),
                )
            })
            .collect();
        ensure!(
            versions.len() == entries.len(),
            "CRM returned duplicate profile identities"
        );
        Ok((entries, versions))
    };
    let (entries, before) = list()?;
    let mut captured = Vec::new();
    for entry in entries {
        let Data::ProfileEntry { entry: body } = client
            .execute(&Request::ShowProfileEntry {
                entry: entry.id.clone(),
            })?
            .data
        else {
            bail!("CRM returned the wrong profile-read result")
        };
        ensure!(
            body.id == entry.id && body.title == entry.title && body.updated_at == entry.updated_at,
            "career material changed during capture; retry capture"
        );
        captured.push(CareerEntry {
            id: body.id,
            title: body.title,
            markdown: body.body_md,
        });
    }
    let (_, after) = list()?;
    ensure!(
        before == after,
        "career material changed during capture; retry capture"
    );
    Ok(captured)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AtsPosting {
    provider: String,
    board: String,
    id: String,
    eu: bool,
}

impl AtsPosting {
    fn from_key(key: &str) -> Option<Self> {
        let parts: Vec<_> = key.split(':').collect();
        let (provider, board, id, eu) = match parts.as_slice() {
            [provider @ ("greenhouse" | "ashby"), board, id] => (*provider, *board, *id, false),
            ["lever", region @ ("eu" | "global"), board, id] => {
                ("lever", *board, *id, *region == "eu")
            }
            _ => return None,
        };
        if ![board, id].into_iter().all(valid_token) {
            return None;
        }
        Some(Self {
            provider: provider.into(),
            board: board.into(),
            id: id.into(),
            eu,
        })
    }

    fn from_url(url: &url::Url) -> Option<Self> {
        let parts: Vec<_> = url
            .path_segments()?
            .filter(|part| !part.is_empty())
            .collect();
        let (provider, board, id, eu) = match (url.host_str()?, parts.as_slice()) {
            ("boards.greenhouse.io" | "job-boards.greenhouse.io", [board, "jobs", id]) => {
                ("greenhouse", *board, *id, false)
            }
            ("jobs.ashbyhq.com", [board, id] | [board, id, "application"]) => {
                ("ashby", *board, *id, false)
            }
            (host @ ("jobs.lever.co" | "jobs.eu.lever.co"), [board, id] | [board, id, "apply"]) => {
                ("lever", *board, *id, host == "jobs.eu.lever.co")
            }
            _ => return None,
        };
        if ![board, id].into_iter().all(valid_token) {
            return None;
        }
        Some(Self {
            provider: provider.into(),
            board: board.into(),
            id: id.into(),
            eu,
        })
    }

    fn key(&self) -> String {
        if self.provider == "lever" {
            format!(
                "lever:{}:{}:{}",
                if self.eu { "eu" } else { "global" },
                self.board,
                self.id
            )
        } else {
            format!("{}:{}:{}", self.provider, self.board, self.id)
        }
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn ats(job: &Job) -> Result<Option<AtsPosting>> {
    let source = AtsPosting::from_key(&job.source_key);
    let hosted = AtsPosting::from_url(&url::Url::parse(&job.url)?);
    if let (Some(source), Some(hosted)) = (&source, &hosted) {
        ensure!(
            source == hosted,
            "Cast source identity and hosted posting URL disagree"
        );
    }
    Ok(source.or(hosted))
}

pub fn identity(job: &Job) -> Result<String> {
    if let Some(posting) = ats(job)? {
        return Ok(posting.key());
    }
    cast::normalize_url(&job.url).map_err(|error| anyhow::anyhow!(error.to_string()))
}

#[must_use]
pub fn eligible(job: &Job) -> bool {
    if matches!(
        job.availability.as_str(),
        "unlisted" | "missing" | "presumed_closed"
    ) {
        return false;
    }
    // Unknown or differently denominated compensation is not a disqualification.
    !job.compensation.iter().any(|pay| {
        pay.currency.as_deref() == Some("USD")
            && matches!(pay.period.as_deref(), Some("year" | "yearly" | "annual"))
            && pay.component.to_lowercase().contains("base")
            && pay.maximum.is_some_and(|maximum| maximum < 80_000.0)
    })
}

pub async fn posting(root: &Path, job: &Job) -> Result<Posting> {
    let parsed = url::Url::parse(&job.url)?;
    validate_public_url(&parsed)?;
    let mut retrieved_at = None;
    let value = if let Some(ats) = ats(job)? {
        let endpoint = match ats.provider.as_str() {
            "greenhouse" => format!(
                "https://boards-api.greenhouse.io/v1/boards/{}/jobs/{}",
                ats.board, ats.id
            ),
            "ashby" => format!(
                "https://api.ashbyhq.com/posting-api/job-board/{}?includeCompensation=true",
                ats.board
            ),
            "lever" => format!(
                "https://api.{}lever.co/v0/postings/{}/{}?mode=json",
                if ats.eu { "eu." } else { "" },
                ats.board,
                ats.id
            ),
            _ => unreachable!(),
        };
        let endpoint = url::Url::parse(&endpoint)?;
        let value: Value = if ats.provider == "ashby" {
            let cached = ashby_board(root, &ats, &endpoint).await?;
            retrieved_at = Some(cached.retrieved_at);
            cached.response
        } else {
            let body = fetch(&endpoint, Some(4_000_000)).await?;
            serde_json::from_slice(&body).context("invalid ATS JSON response")?
        };
        let found = if ats.provider == "ashby" {
            let matches: Vec<_> = value
                .get("jobs")
                .and_then(Value::as_array)
                .context("invalid Ashby board")?
                .iter()
                .filter(|row| ashby_job_matches(row, &ats))
                .collect();
            ensure!(
                matches.len() == 1,
                "posting absent or ambiguous in employer board"
            );
            ensure!(
                matches[0].get("isListed").and_then(Value::as_bool) != Some(false),
                "posting is unlisted"
            );
            matches[0].clone()
        } else {
            ensure!(
                value
                    .get("id")
                    .is_some_and(
                        |id| id.as_str().map_or_else(|| id.to_string(), str::to_owned) == ats.id
                    ),
                "ATS response is for a different posting"
            );
            value
        };
        let description = if ats.provider == "greenhouse" {
            "content"
        } else if ats.provider == "ashby" {
            "descriptionHtml"
        } else {
            "description"
        };
        let content = found
            .get(description)
            .and_then(Value::as_str)
            .or_else(|| found.get("descriptionPlain").and_then(Value::as_str))
            .context("full employer posting description is absent")?;
        validate_description(content)?;
        found
    } else {
        let bytes = fetch(&parsed, Some(4_000_000)).await?;
        let html = std::str::from_utf8(&bytes).context("employer posting is not UTF-8")?;
        select_jsonld(html, &parsed, &job.title)?
    };
    let text = value.to_string();
    ensure!(
        text.len() <= 1_000_000,
        "full posting exceeds the supported input size"
    );
    Ok(Posting {
        url: job.url.clone(),
        retrieved_at: retrieved_at.unwrap_or_else(cast::now),
        text,
    })
}

#[derive(Serialize, Deserialize)]
struct AshbyBoard {
    retrieved_at: String,
    response: Value,
}

fn ashby_job_matches(row: &Value, ats: &AtsPosting) -> bool {
    row.get("jobUrl")
        .and_then(Value::as_str)
        .and_then(|value| url::Url::parse(value).ok())
        .and_then(|url| AtsPosting::from_url(&url))
        .as_ref()
        == Some(ats)
}

fn read_ashby_cache(
    path: &Path,
    ats: &AtsPosting,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<AshbyBoard> {
    let cached: AshbyBoard = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let retrieved_at = chrono::DateTime::parse_from_rfc3339(&cached.retrieved_at).ok()?;
    let age = now.signed_duration_since(retrieved_at);
    let jobs = cached.response.get("jobs")?.as_array()?;
    (age >= chrono::Duration::zero()
        && age < chrono::Duration::days(14)
        && jobs.iter().any(|row| ashby_job_matches(row, ats)))
    .then_some(cached)
}

async fn ashby_board(root: &Path, ats: &AtsPosting, endpoint: &url::Url) -> Result<AshbyBoard> {
    let path = root.join("ashby-cache").join(format!("{}.json", ats.board));
    if let Some(cached) = read_ashby_cache(&path, ats, chrono::Utc::now()) {
        return Ok(cached);
    }
    // Ashby returns the whole board. Share it across packets without a byte cap.
    let bytes = fetch(endpoint, None).await?;
    let response: Value = serde_json::from_slice(&bytes).context("invalid Ashby JSON response")?;
    ensure!(
        response.get("jobs").and_then(Value::as_array).is_some(),
        "invalid Ashby board"
    );
    let cached = AshbyBoard {
        retrieved_at: cast::now(),
        response,
    };
    crate::write_json(&path, &cached).context("write Ashby board cache")?;
    Ok(cached)
}

fn validate_public_url(url: &url::Url) -> Result<()> {
    ensure!(
        url.scheme() == "https" && url.username().is_empty() && url.password().is_none(),
        "posting must use public HTTPS"
    );
    let host = url.host_str().context("posting has no host")?;
    ensure!(
        host != "localhost"
            && !host.ends_with(".localhost")
            && !host
                .rsplit('.')
                .next()
                .is_some_and(|label| label.eq_ignore_ascii_case("local"))
            && host.parse::<std::net::IpAddr>().is_err(),
        "posting must use a public employer hostname"
    );
    ensure!(
        url.port().is_none_or(|port| port == 443),
        "posting must use standard HTTPS"
    );
    Ok(())
}

async fn fetch(url: &url::Url, max_bytes: Option<usize>) -> Result<Vec<u8>> {
    validate_public_url(url)?;
    let host = url.host_str().context("posting has no host")?;
    let addresses: Vec<_> = tokio::net::lookup_host((host, 443)).await?.collect();
    ensure!(
        !addresses.is_empty() && addresses.iter().all(|address| public_ip(address.ip())),
        "posting hostname resolves to a non-public address"
    );
    // Pin the addresses checked above and refuse redirects: an unrelated target
    // must not become evidence for the selected opportunity.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .no_proxy()
        .user_agent("JobPackets/0.1 (public job preparation)")
        .build()?;
    let mut response = client.get(url.clone()).send().await?.error_for_status()?;
    ensure!(
        response.status().is_success(),
        "posting redirects require a new supported source URL"
    );
    ensure!(
        max_bytes.is_none_or(|max| response
            .content_length()
            .is_none_or(|length| length <= max as u64)),
        "posting response exceeds supported size"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            max_bytes.is_none_or(|max| bytes.len() + chunk.len() <= max),
            "posting response exceeds supported size"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn public_ip(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])))
        }
        std::net::IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or_else(
            || {
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local())
            },
            |ip| public_ip(ip.into()),
        ),
    }
}

fn validate_description(description: &str) -> Result<()> {
    let document = Html::parse_fragment(description);
    let visible = document.root_element().text().collect::<Vec<_>>().join(" ");
    ensure!(
        visible.trim().len() >= 200,
        "posting description is absent or below the adapter's minimum length"
    );
    Ok(())
}

fn select_jsonld(html: &str, page: &url::Url, title: &str) -> Result<Value> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("script[type='application/ld+json']")
        .map_err(|error| anyhow::anyhow!("selector: {error}"))?;
    let mut candidates = Vec::new();
    for element in document.select(&selector) {
        if let Ok(value) = serde_json::from_str::<Value>(&element.inner_html()) {
            find_postings(value, &mut candidates);
        }
    }
    let expected = normalized_posting_url(page.as_str())?;
    let mut matches = Vec::new();
    for candidate in &candidates {
        let declared = candidate
            .get("url")
            .or_else(|| candidate.get("mainEntityOfPage"))
            .or_else(|| candidate.get("@id"));
        let declared = declared.and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("@id").and_then(Value::as_str))
        });
        let same = match declared {
            Some(value) => {
                page.join(value)
                    .ok()
                    .and_then(|url| normalized_posting_url(url.as_str()).ok())
                    .as_ref()
                    == Some(&expected)
            }
            None => {
                candidates.len() == 1
                    && candidate
                        .get("title")
                        .and_then(Value::as_str)
                        .is_some_and(|value| normalized_title(value) == normalized_title(title))
            }
        };
        if same && !matches.contains(&candidate) {
            matches.push(candidate);
        }
    }
    ensure!(
        matches.len() == 1,
        "no unique full JobPosting matching the selected opportunity URL/title"
    );
    let selected = matches[0];
    validate_description(
        selected
            .get("description")
            .and_then(Value::as_str)
            .context("JobPosting has no complete description")?,
    )?;
    Ok(selected.clone())
}

fn normalized_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn normalized_posting_url(value: &str) -> Result<String> {
    let normalized =
        cast::normalize_url(value).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(normalized.trim_end_matches('/').to_owned())
}

fn find_postings(value: Value, result: &mut Vec<Value>) {
    match value {
        Value::Object(ref map)
            if map.get("@type").is_some_and(|kind| {
                kind.as_str() == Some("JobPosting")
                    || kind.as_array().is_some_and(|kinds| {
                        kinds.iter().any(|kind| kind.as_str() == Some("JobPosting"))
                    })
            }) =>
        {
            result.push(value);
        }
        Value::Object(mut map) => {
            if let Some(value) = map.remove("@graph") {
                find_postings(value, result);
            }
        }
        Value::Array(values) => {
            for value in values {
                find_postings(value, result);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixtures should fail immediately when malformed.
mod tests {
    use super::*;
    use serde_json::json;

    #[allow(clippy::needless_pass_by_value)]
    fn html(value: Value) -> String {
        format!("<script type='application/ld+json'>{value}</script>")
    }
    fn role(url: &str) -> Value {
        json!({"@type":"JobPosting", "title":"Engineer", "url":url,"description":"Complete role requirements and responsibilities. ".repeat(10)})
    }

    #[tokio::test]
    async fn ashby_cache_shares_large_boards_and_preserves_timestamps() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ashby-cache/employer.json");
        let cached = AshbyBoard {
            retrieved_at: cast::now(),
            response: json!({"jobs":[
                {"jobUrl":"https://jobs.ashbyhq.com/employer/one"},
                {"jobUrl":"https://jobs.ashbyhq.com/employer/two", "descriptionHtml":"x".repeat(4_000_001)}
            ]}),
        };
        crate::write_json(&path, &cached).unwrap();
        // Any attempted fetch fails URL validation, without making a request.
        let endpoint = url::Url::parse("http://example.com").unwrap();
        for id in ["one", "two"] {
            let ats = AtsPosting::from_key(&format!("ashby:employer:{id}")).unwrap();
            let found = ashby_board(root.path(), &ats, &endpoint).await.unwrap();
            assert_eq!(found.retrieved_at, cached.retrieved_at);
            assert_eq!(found.response, cached.response);
        }

        let original = std::fs::read(&path).unwrap();
        let missing = AtsPosting::from_key("ashby:employer:new").unwrap();
        assert!(ashby_board(root.path(), &missing, &endpoint).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn ashby_cache_expires_and_rejects_invalid_or_missing_entries() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("employer.json");
        let ats = AtsPosting::from_key("ashby:employer:one").unwrap();
        let now = chrono::Utc::now();
        assert!(read_ashby_cache(&path, &ats, now).is_none());
        let mut cached = AshbyBoard {
            retrieved_at: now.to_rfc3339(),
            response: json!({"jobs":[{"jobUrl":"https://jobs.ashbyhq.com/employer/one"}]}),
        };
        crate::write_json(&path, &cached).unwrap();
        assert!(read_ashby_cache(&path, &ats, now + chrono::Duration::days(13)).is_some());
        assert!(read_ashby_cache(&path, &ats, now + chrono::Duration::days(14)).is_none());
        assert!(read_ashby_cache(&path, &ats, now - chrono::Duration::seconds(1)).is_none());

        cached.response = json!({"jobs":[{"jobUrl":"https://jobs.ashbyhq.com/other/one"}]});
        crate::write_json(&path, &cached).unwrap();
        assert!(read_ashby_cache(&path, &ats, now).is_none());
        cached.response = json!({"jobs":null});
        crate::write_json(&path, &cached).unwrap();
        assert!(read_ashby_cache(&path, &ats, now).is_none());
        std::fs::write(&path, b"incomplete JSON").unwrap();
        assert!(read_ashby_cache(&path, &ats, now).is_none());
    }

    #[test]
    fn jsonld_selects_exact_posting_and_rejects_mismatch_or_ambiguity() {
        let page = url::Url::parse("https://employer.example/jobs/one").unwrap();
        let selected = select_jsonld(
            &html(json!({"@graph":[role("/jobs/two"),role("/jobs/one")]})),
            &page,
            "Engineer",
        )
        .unwrap();
        assert_eq!(selected["url"], "/jobs/one");
        assert!(select_jsonld(&html(role("/jobs/two")), &page, "Engineer").is_err());
        let mut conflicting = role("/jobs/one");
        conflicting["title"] = json!("Different role");
        assert!(
            select_jsonld(
                &html(json!([role("/jobs/one"), conflicting])),
                &page,
                "Engineer"
            )
            .is_err()
        );
        let mut sparse = role("/jobs/one");
        sparse["description"] = json!("short");
        assert!(select_jsonld(&html(sparse), &page, "Engineer").is_err());
    }

    #[test]
    fn native_keys_survive_custom_domains_and_host_aliases() {
        let key = AtsPosting::from_key("greenhouse:employer:123").unwrap();
        assert_eq!(
            Some(key.clone()),
            AtsPosting::from_url(
                &url::Url::parse("https://job-boards.greenhouse.io/employer/jobs/123").unwrap()
            )
        );
        assert_eq!(
            Some(key),
            AtsPosting::from_url(
                &url::Url::parse("https://boards.greenhouse.io/employer/jobs/123").unwrap()
            )
        );
        assert_eq!(
            AtsPosting::from_key("lever:eu:employer:abc").unwrap().key(),
            "lever:eu:employer:abc"
        );
        assert!(AtsPosting::from_key("greenhouse:../../evil:123").is_none());
        assert!(
            AtsPosting::from_url(
                &url::Url::parse("https://evilgreenhouse.io/employer/jobs/123").unwrap()
            )
            .is_none()
        );
    }

    #[test]
    fn private_networks_and_deceptive_urls_are_rejected() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()));
        }
        for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
            assert!(public_ip(ip.parse().unwrap()));
        }
        for url in [
            "http://example.com/job",
            "https://localhost/job",
            "https://user:secret@example.com/job",
            "https://example.com:9000/job",
        ] {
            assert!(validate_public_url(&url::Url::parse(url).unwrap()).is_err());
        }
    }

    #[test]
    fn career_capture_detects_an_edit_after_the_entry_was_read() {
        use std::os::unix::fs::PermissionsExt;
        for changes in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let executable = directory.path().join("crm");
            let initial = json!({"ok":true,"data":{"type":"profile_list","entries":[{"id":"entry-one","title":"Career","updated_at":"2026-09-01T00:00:00Z"}],"has_more":false}});
            let read = json!({"ok":true,"data":{"type":"profile_entry","entry":{"id":"entry-one","title":"Career","updated_at":"2026-09-01T00:00:00Z","body_md":"Exact career text"}}});
            let mut after = initial.clone();
            if changes {
                after["data"]["entries"][0]["updated_at"] = json!("2026-09-02T00:00:00Z");
            }
            let script = format!(
                "#!/bin/sh\ncase \"$*\" in\n  *'profile list'*)\n    if [ -f \"$0.seen\" ]; then\n      printf '%s\\n' '{after}'\n    else\n      printf '%s\\n' '{initial}'\n    fi;;\n  *'profile show'*)\n    touch \"$0.seen\"\n    printf '%s\\n' '{read}';;\n  *) exit 2;;\nesac\n"
            );
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let captured = career_library(&executable);
            if changes {
                assert!(
                    captured
                        .unwrap_err()
                        .to_string()
                        .contains("changed during capture")
                );
            } else {
                assert_eq!(captured.unwrap()[0].markdown, "Exact career text");
            }
        }
    }
}
