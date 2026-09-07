use super::{
    Board, evidence, id, is_discovery_directory, links, string, text, unsupported_shared_ats,
};
use crate::{
    http::{HttpClient, public_url},
    models::{Compensation, JobDraft, VerificationResult},
};
use reqwest::Url;
use scraper::{Html, Selector};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Reads one employer page or ATS listing page without following application actions.
///
/// # Errors
/// Returns budget, transport, cursor, URL-policy, or ATS schema errors.
pub async fn verify(
    http: &HttpClient,
    input: &str,
    cursor: Option<&Value>,
) -> Result<VerificationResult, String> {
    let url = public_url(input)?;
    if let Some(board) = Board::from_url(&url) {
        return verify_board(http, &board, cursor).await;
    }
    if is_discovery_directory(input) {
        return Ok(directory_result());
    }
    if cursor.is_some() {
        return Err("HTML collection does not accept a pagination cursor".into());
    }
    let response = http.get_text(input).await?;
    let final_url = public_url(&response.url)?;
    if let Some(board) = Board::from_url(&final_url) {
        return Ok(VerificationResult {
            careers_urls: vec![board.url()],
            outcome: "resolved".into(),
            ..Default::default()
        });
    }
    Ok(parse_html(&response.body, &final_url))
}

async fn verify_board(
    http: &HttpClient,
    board: &Board,
    cursor: Option<&Value>,
) -> Result<VerificationResult, String> {
    match board {
        Board::Greenhouse(token) => {
            let json = http
                .get_json(&format!(
                    "https://boards-api.greenhouse.io/v1/boards/{token}/jobs?content=true"
                ))
                .await?;
            parse_greenhouse(&json, board)
        }
        Board::Ashby(token) => {
            let json = http
                .get_json(&format!(
                    "https://api.ashbyhq.com/posting-api/job-board/{token}?includeCompensation=true"
                ))
                .await?;
            parse_ashby(&json, board)
        }
        Board::Lever { site, eu } => {
            let skip = cursor
                .and_then(|cursor| cursor.get("skip"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if skip > 10000 {
                return Ok(VerificationResult {
                    outcome: "capped".into(),
                    warnings: vec![
                        "Lever listing exceeded 10,000 postings; missing-scan counters are not advanced".into(),
                    ],
                    ..Default::default()
                });
            }
            let json = http
                .get_json(&format!(
                    "https://api.{}lever.co/v0/postings/{site}?mode=json&limit=100&skip={skip}",
                    if *eu { "eu." } else { "" }
                ))
                .await?;
            parse_lever(&json, board, skip)
        }
    }
}

fn validated_job(title: Option<String>, url: Option<String>) -> Result<(String, String), String> {
    let title = title.ok_or("job has no nonempty title")?;
    let url = public_url(&url.ok_or("job has no URL")?)?;
    Ok((title, url.into()))
}

fn parse_greenhouse(json: &Value, board: &Board) -> Result<VerificationResult, String> {
    let rows = json
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("invalid Greenhouse response: no jobs array")?;
    let mut jobs = vec![];
    for row in rows {
        let public_id = id(row, "id").ok_or("Greenhouse posting has no public id")?;
        let (title, url) = validated_job(string(row, "title"), string(row, "absolute_url"))?;
        if let Some(hosted_board) =
            Board::from_url(&Url::parse(&url).map_err(|_| "invalid Greenhouse job URL")?)
            && &hosted_board != board
        {
            return Err("Greenhouse job URL belongs to a different ATS board".into());
        }
        jobs.push(JobDraft {
            source_key: format!("{}:{public_id}", board.identity()), title, url,
            location: row.pointer("/location/name").and_then(Value::as_str).map(ToOwned::to_owned),
            source_internal_id: id(row, "internal_job_id"), source_updated_at: string(row, "updated_at"),
            source_published_at: string(row, "first_published"),
            published_at_semantics: string(row, "first_published").map(|_| "source_first_published".into()),
            description: string(row, "content").map(|content| text(&text(&content))), is_listed: true,
            evidence: vec![evidence(&board.url(), "employer_ats", "Greenhouse public posting id; updated_at is an update time, not publication time; null internal_job_id may denote a prospect post")], ..Default::default()
        });
    }
    let complete = json
        .pointer("/meta/total")
        .and_then(Value::as_u64)
        .is_none_or(|total| total == rows.len() as u64);
    let company_names: BTreeSet<_> = rows
        .iter()
        .filter_map(|row| string(row, "company_name"))
        .collect();
    Ok(VerificationResult {
        company_name: (company_names.len() == 1)
            .then(|| company_names.into_iter().next())
            .flatten(),
        jobs,
        complete,
        outcome: if complete { "complete" } else { "partial" }.into(),
        warnings: if complete {
            vec![]
        } else {
            vec!["Greenhouse total differs from returned jobs; missing-scan counters are not advanced".into()]
        },
        ..Default::default()
    })
}

fn parse_ashby(json: &Value, board: &Board) -> Result<VerificationResult, String> {
    if json.get("apiVersion").and_then(Value::as_str) != Some("1") {
        return Err("unsupported or missing Ashby apiVersion".into());
    }
    let rows = json
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("invalid Ashby response: no jobs array")?;
    let mut jobs = vec![];
    for row in rows {
        let (title, url) = validated_job(string(row, "title"), string(row, "jobUrl"))?;
        let listed = row
            .get("isListed")
            .and_then(Value::as_bool)
            .ok_or("Ashby job missing isListed; listing status unknown")?;
        let parsed_url = Url::parse(&url).map_err(|_| "invalid Ashby job URL")?;
        let posting_id = parsed_url
            .path_segments()
            .and_then(|mut parts| parts.rfind(|part| !part.is_empty()))
            .ok_or("Ashby job URL missing posting identity")?
            .to_owned();
        if Board::from_url(&parsed_url).as_ref() != Some(board) {
            return Err("Ashby job URL does not belong to requested board".into());
        }
        jobs.push(JobDraft {
            source_key: format!("{}:{posting_id}", board.identity()), title, url,
            apply_url: string(row, "applyUrl").and_then(|url| public_url(&url).ok().map(Into::into)),
            location: string(row, "location"), remote: row.get("isRemote").and_then(Value::as_bool), employment_type: string(row, "employmentType"),
            source_published_at: string(row, "publishedAt"), description: string(row, "descriptionPlain"), is_listed: listed,
            published_at_semantics: Some("last_published_at".into()), compensation: ashby_compensation(row),
            evidence: vec![evidence(&board.url(), "employer_ats", "Ashby publishedAt means when last published; isListed=false is direct-link-only and must not appear as a listed opening")], ..Default::default()
        });
    }
    Ok(VerificationResult {
        jobs,
        complete: true,
        outcome: "complete".into(),
        ..Default::default()
    })
}

fn parse_lever(json: &Value, board: &Board, skip: u64) -> Result<VerificationResult, String> {
    let rows = json
        .as_array()
        .ok_or("invalid Lever response: expected jobs array")?;
    let mut jobs = vec![];
    for row in rows {
        let posting_id = id(row, "id").ok_or("Lever posting missing id")?;
        let (title, url) = validated_job(string(row, "text"), string(row, "hostedUrl"))?;
        if Board::from_url(&Url::parse(&url).map_err(|_| "invalid Lever URL")?).as_ref()
            != Some(board)
        {
            return Err("Lever job URL does not belong to requested board".into());
        }
        let remote = match row.get("workplaceType").and_then(Value::as_str) {
            Some("remote") => Some(true),
            Some("on-site" | "hybrid") => Some(false),
            _ => None,
        };
        jobs.push(JobDraft {
            source_key: format!("{}:{posting_id}", board.identity()), title, url,
            apply_url: string(row, "applyUrl").and_then(|url| public_url(&url).ok().map(Into::into)),
            location: row.pointer("/categories/location").and_then(Value::as_str).map(ToOwned::to_owned), remote,
            employment_type: row.pointer("/categories/commitment").and_then(Value::as_str).map(ToOwned::to_owned),
            description: string(row, "descriptionPlain"), is_listed: true,
            compensation: row.get("salaryRange").map(|salary| vec![Compensation { currency:string(salary,"currency"), period:string(salary,"interval"), component:"salary".into(), minimum:salary.get("min").and_then(Value::as_f64), maximum:salary.get("max").and_then(Value::as_f64) }]).unwrap_or_default(),
            evidence: vec![evidence(&board.url(), "employer_ats", "Lever public posting; publication and update dates are not supplied by the documented public listing contract")], ..Default::default()
        });
    }
    let complete = rows.len() < 100;
    Ok(VerificationResult {
        jobs,
        next_cursor: (!complete).then(|| json!({"skip":skip + rows.len() as u64})),
        complete,
        outcome: if complete { "complete" } else { "partial" }.into(),
        ..Default::default()
    })
}

fn parse_html(html: &str, url: &Url) -> VerificationResult {
    if is_discovery_directory(url.as_str()) {
        return directory_result();
    }
    let document = Html::parse_document(html);
    let mut result = VerificationResult::default();
    let mut careers = BTreeSet::new();
    for (link, label) in links(html, url) {
        if let Some(board) = Board::from_url(&link) {
            careers.insert(board.url());
        } else if link.host_str() == url.host_str() && link != *url {
            let phrase = format!("{} {label}", link.path()).to_lowercase();
            if [
                "career",
                "jobs",
                "join-us",
                "join our",
                "open roles",
                "open positions",
            ]
            .iter()
            .any(|word| phrase.contains(word))
            {
                careers.insert(link.into());
            }
        }
    }
    // Embedded ATS script URLs are common when the page has no visible link.
    let escaped = html.replace("\\/", "/");
    let ats = regex::Regex::new(r"https://(?:jobs\.ashbyhq\.com|(?:job-)?boards\.greenhouse\.io|jobs\.(?:eu\.)?lever\.co)/[A-Za-z0-9_-]+").unwrap_or_else(|_| unreachable!("constant regex"));
    for matched in ats.find_iter(&escaped) {
        if let Ok(link) = Url::parse(matched.as_str())
            && let Some(board) = Board::from_url(&link)
        {
            careers.insert(board.url());
        }
    }
    let script_selector = Selector::parse("script[type='application/ld+json']")
        .unwrap_or_else(|_| unreachable!("constant selector"));
    for script in document.select(&script_selector) {
        match serde_json::from_str::<Value>(&script.inner_html()) {
            Ok(value) => jsonld_jobs(&value, url, &mut result),
            Err(_) => result.warnings.push("Invalid JSON-LD block skipped".into()),
        }
    }
    careers.extend(result.careers_urls.drain(..));
    let mut job_urls = BTreeSet::new();
    result.jobs.retain(|job| job_urls.insert(job.url.clone()));
    result.careers_urls = careers.into_iter().take(6).collect();
    let board_count = result
        .careers_urls
        .iter()
        .filter_map(|url| super::board_identity(url))
        .collect::<BTreeSet<_>>()
        .len();
    if board_count > 1 {
        result.jobs.clear();
        result.company_name = None;
        result.warnings.push(
            "Multiple ATS tenant URLs found; each uses its own provider/tenant company identity"
                .into(),
        );
    }
    result.outcome = if !result.careers_urls.is_empty() {
        "resolved"
    } else if !result.jobs.is_empty() {
        "observed"
    } else {
        "needs_adapter"
    }
    .into();
    result.warnings.push(
        "HTML/JSON-LD adapter records extracted jobs without adding missing observations".into(),
    );
    result
}

fn jsonld_jobs(value: &Value, page: &Url, result: &mut VerificationResult) {
    if let Some(array) = value.as_array() {
        for value in array {
            jsonld_jobs(value, page, result);
        }
        return;
    }
    let Some(object) = value.as_object() else {
        return;
    };
    let is_job = object.get("@type").is_some_and(|kind| {
        kind.as_str() == Some("JobPosting")
            || kind
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("JobPosting")))
    });
    if is_job && !owns_jsonld_job(value, page) {
        result.warnings.push(
            "JobPosting does not match this page's employer URL rule; job and company name omitted"
                .into(),
        );
    } else if is_job {
        let raw_url = string(value, "url").unwrap_or_else(|| page.as_str().into());
        let job_url = page
            .join(&raw_url)
            .ok()
            .filter(|url| public_url(url.as_str()).is_ok())
            .filter(|url| {
                if let Some(board) = Board::from_url(url) {
                    result.careers_urls.push(board.url());
                    result.warnings.push("ATS job URL uses a separate tenant adapter; generic page identity is not copied".into());
                    false
                } else { owns_jsonld_job(value, url) }
            });
        if let (Some(title), Some(url)) = (string(value, "title"), job_url) {
            let external_id = value
                .pointer("/identifier/value")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let source_key = format!("jsonld:{}", url.as_str());
            let location = value
                .pointer("/jobLocation/address/addressLocality")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let remote = (value.get("jobLocationType").and_then(Value::as_str)
                == Some("TELECOMMUTE"))
            .then_some(true);
            let expired =
                string(value, "validThrough").is_some_and(|date| valid_through_expired(&date));
            result.jobs.push(JobDraft { source_key, title, url: url.into(), location, remote, employment_type: string(value, "employmentType"), source_published_at: string(value, "datePosted"), source_internal_id: external_id, description: string(value, "description").map(|html| text(&html)), is_listed: !expired, published_at_semantics: Some("publisher_reported_date_posted".into()), geographic_eligibility: named_locations(value.get("applicantLocationRequirements")), compensation: jsonld_compensation(value), evidence: vec![evidence(page.as_str(), "employer_jsonld_owned", "Observed JobPosting structured data; datePosted is publisher-reported and expired validThrough is not listed")], ..Default::default() });
            if result.company_name.is_none() {
                result.company_name = value
                    .pointer("/hiringOrganization/name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
        } else {
            result
                .warnings
                .push("JobPosting is missing a valid title or URL".into());
        }
    }
    for key in ["@graph", "itemListElement", "item"] {
        if let Some(nested) = object.get(key) {
            jsonld_jobs(nested, page, result);
        }
    }
}

fn directory_result() -> VerificationResult {
    VerificationResult {
        outcome: "needs_adapter".into(),
        warnings: vec![
            "Third-party directory collected as discovery; a tenant adapter is required for jobs"
                .into(),
        ],
        ..Default::default()
    }
}

fn owns_jsonld_job(value: &Value, page: &Url) -> bool {
    if unsupported_shared_ats(page) {
        return false;
    }
    let Some(organization) = value.get("hiringOrganization") else {
        return false;
    };
    let Some(page_host) = page.host_str().map(|host| host.trim_start_matches("www.")) else {
        return false;
    };
    ["url", "sameAs"]
        .iter()
        .filter_map(|field| organization.get(*field))
        .flat_map(|value| match value {
            Value::Array(values) => values.iter().collect::<Vec<_>>(),
            _ => vec![value],
        })
        .filter_map(Value::as_str)
        .filter_map(|value| public_url(value).ok())
        .filter(|url| {
            matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none()
                && !unsupported_shared_ats(url)
        })
        .any(|url| {
            let Some(employer_host) = url.host_str().map(|host| host.trim_start_matches("www."))
            else {
                return false;
            };
            page_host == employer_host || page_host.ends_with(&format!(".{employer_host}"))
        })
}

fn ashby_compensation(job: &Value) -> Vec<Compensation> {
    job.pointer("/compensation/summaryComponents")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| Compensation {
                    currency: string(item, "currencyCode"),
                    period: string(item, "interval"),
                    component: string(item, "compensationType")
                        .unwrap_or_else(|| "unspecified".into()),
                    minimum: item.get("minValue").and_then(Value::as_f64),
                    maximum: item.get("maxValue").and_then(Value::as_f64),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn named_locations(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| string(item, "name"))
            .collect(),
        Some(value) => string(value, "name").into_iter().collect(),
        None => vec![],
    }
}

fn jsonld_compensation(job: &Value) -> Vec<Compensation> {
    let Some(salary) = job.get("baseSalary") else {
        return vec![];
    };
    let value = salary.get("value").unwrap_or(&Value::Null);
    vec![Compensation {
        currency: string(salary, "currency"),
        period: string(value, "unitText"),
        component: "base_salary".into(),
        minimum: value
            .get("minValue")
            .and_then(Value::as_f64)
            .or_else(|| value.get("value").and_then(Value::as_f64))
            .or_else(|| value.as_f64()),
        maximum: value
            .get("maxValue")
            .and_then(Value::as_f64)
            .or_else(|| value.get("value").and_then(Value::as_f64))
            .or_else(|| value.as_f64()),
    }]
}

fn valid_through_expired(date: &str) -> bool {
    if let Ok(timestamp) =
        time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc3339)
    {
        return timestamp < time::OffsetDateTime::now_utc();
    }
    if date.len() == 10
        && date.as_bytes().get(4) == Some(&b'-')
        && date.as_bytes().get(7) == Some(&b'-')
    {
        return date < time::OffsetDateTime::now_utc().date().to_string().as_str();
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn shared_ats_profile_urls_do_not_prove_employer_homepage_ownership() {
        for (page, organization) in [
            (
                "https://join.com/companies/aimitechnology/123",
                json!({"name":"AiMi","url":"https://join.com/companies/aimitechnology","sameAs":"https://aimi.technology"}),
            ),
            (
                "https://jobs.curriculo.me/3c-digital/job/123",
                json!({"name":"3C Digital","url":"https://jobs.curriculo.me/3c-digital","sameAs":"https://jobs.curriculo.me/3c-digital"}),
            ),
            (
                "https://unknown-shared.example/tenant-b/job/123",
                json!({"name":"Tenant A","url":"https://unknown-shared.example/tenant-a"}),
            ),
            (
                "https://unknown-shared.example/tenant-b/job/123",
                json!({"name":"Tenant A","url":"https://unknown-shared.example/?tenant=a"}),
            ),
        ] {
            assert!(
                !owns_jsonld_job(
                    &json!({"hiringOrganization":organization}),
                    &Url::parse(page).unwrap()
                ),
                "{page}"
            );
        }
        assert!(owns_jsonld_job(
            &json!({"hiringOrganization":{"name":"Acme","sameAs":"https://acme.example/"}}),
            &Url::parse("https://careers.acme.example/jobs/123").unwrap()
        ));
        assert!(!owns_jsonld_job(
            &json!({"hiringOrganization":{"name":"Tenant","url":"https://join.com/"}}),
            &Url::parse("https://join.com/companies/tenant/job").unwrap()
        ));
    }
    #[test]
    fn unknown_aggregator_cannot_claim_external_employers_or_ats_jobs() {
        let html = r#"<a href="https://job-boards.greenhouse.io/coinbase">Coinbase jobs</a><a href="https://jobs.ashbyhq.com/openteams">OpenTeams jobs</a><script type="application/ld+json">[{"@type":"JobPosting","title":"Engineer","url":"/jobs/1","hiringOrganization":{"name":"Kalepa","sameAs":"https://kalepa.example"}},{"@type":"JobPosting","title":"Engineer","url":"/jobs/2","hiringOrganization":{"name":"Tastylive"}}]</script>"#;
        let result = parse_html(
            html,
            &Url::parse("https://unknown-directory.example/jobs").unwrap(),
        );
        assert!(result.jobs.is_empty());
        assert!(result.company_name.is_none());
        assert!(!result.complete);
        assert_eq!(result.careers_urls.len(), 2);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning
                    .contains("each uses its own provider/tenant company identity"))
        );
    }
    #[test]
    fn name_only_jsonld_does_not_establish_job_ownership() {
        let html = r#"<script type="application/ld+json">{"@type":"JobPosting","title":"Engineer","url":"/jobs/1","hiringOrganization":{"name":"Unrelated company"}}</script>"#;
        let result = parse_html(html, &Url::parse("https://unknown.example/jobs").unwrap());
        assert!(result.jobs.is_empty());
        assert!(result.company_name.is_none());
        assert_eq!(result.outcome, "needs_adapter");
    }
    #[test]
    fn owned_page_cannot_reassign_an_external_ats_job() {
        let html = r#"<script type="application/ld+json">{"@type":"JobPosting","title":"Engineer","url":"https://jobs.ashbyhq.com/other/123","hiringOrganization":{"name":"Acme","sameAs":"https://acme.example"}}</script>"#;
        let result = parse_html(html, &Url::parse("https://acme.example/jobs").unwrap());
        assert!(result.jobs.is_empty());
        assert!(result.company_name.is_none());
        assert_eq!(result.careers_urls, vec!["https://jobs.ashbyhq.com/other"]);
    }
    #[tokio::test]
    async fn known_directory_is_quarantined_before_network_or_budget() {
        let http = HttpClient::new(
            std::sync::Arc::new(|_, _| panic!("directory must not make HTTP requests")),
            std::sync::Arc::new(|_, _| Ok(())),
        )
        .unwrap();
        let result = verify(&http, "https://web3.career/infrastructure-jobs", None)
            .await
            .unwrap();
        assert_eq!(result.outcome, "needs_adapter");
        assert!(result.jobs.is_empty());
        assert!(result.careers_urls.is_empty());
    }
    #[test]
    fn greenhouse_public_id_is_identity_and_updated_is_not_published() {
        let result = parse_greenhouse(&json!({"jobs":[{"id":11,"internal_job_id":7,"title":"Engineer","absolute_url":"https://acme.example/job/11","updated_at":"2026-03-01"},{"id":12,"internal_job_id":7,"title":"Engineer EU","absolute_url":"https://acme.example/job/12"}],"meta":{"total":2}}), &Board::Greenhouse("acme".into())).unwrap();
        assert_ne!(result.jobs[0].source_key, result.jobs[1].source_key);
        assert!(result.jobs[0].source_published_at.is_none());
        assert_eq!(
            result.jobs[0].source_updated_at.as_deref(),
            Some("2026-03-01")
        );
        assert!(result.complete);
    }
    #[test]
    fn malformed_or_truncated_board_never_establishes_absence() {
        let board = Board::Greenhouse("acme".into());
        assert!(parse_greenhouse(&json!({"error":"blocked"}), &board).is_err());
        assert!(
            !parse_greenhouse(&json!({"jobs":[],"meta":{"total":3}}), &board)
                .unwrap()
                .complete
        );
        assert!(parse_greenhouse(&json!({"jobs":[{"id":1,"title":"Engineer","absolute_url":"https://job-boards.greenhouse.io/other/jobs/1"}]}), &board).is_err());
    }
    #[test]
    fn greenhouse_escaped_html_and_first_publication_are_preserved() {
        let result = parse_greenhouse(&json!({"jobs":[{"id":1,"title":"Engineer","absolute_url":"https://job-boards.greenhouse.io/acme/jobs/1","first_published":"2026-08-28T17:07:26-04:00","updated_at":"2026-08-31T12:05:03-04:00","content":"&lt;p&gt;Build &lt;strong&gt;systems&lt;/strong&gt; &amp;amp; tools.&lt;/p&gt;"}]}), &Board::Greenhouse("acme".into())).unwrap();
        assert_eq!(
            result.jobs[0].description.as_deref(),
            Some("Build systems & tools.")
        );
        assert_eq!(
            result.jobs[0].source_published_at.as_deref(),
            Some("2026-08-28T17:07:26-04:00")
        );
        assert_eq!(
            result.jobs[0].published_at_semantics.as_deref(),
            Some("source_first_published")
        );
    }
    #[test]
    fn ashby_unlisted_is_not_an_open_listed_job() {
        let result = parse_ashby(&json!({"apiVersion":"1","jobs":[{"title":"Engineer","jobUrl":"https://jobs.ashbyhq.com/acme/123","isListed":false,"publishedAt":"2026-01-02"}]}), &Board::Ashby("acme".into())).unwrap();
        assert!(!result.jobs[0].is_listed);
        assert!(result.complete);
        assert_eq!(
            result.jobs[0].source_published_at.as_deref(),
            Some("2026-01-02")
        );
    }
    #[test]
    fn html_extracts_jsonld_and_ats_without_claiming_completeness() {
        let html = r#"<a href="https://jobs.eu.lever.co/acme/123">Open positions</a><script type="application/ld+json">{"@graph":[{"@type":"JobPosting","title":"Engineer","url":"/jobs/1","datePosted":"2026-01-01","hiringOrganization":{"name":"Acme","sameAs":"https://acme.example"}}]}</script>"#;
        let result = parse_html(html, &Url::parse("https://acme.example/careers").unwrap());
        assert!(!result.complete);
        assert_eq!(result.jobs[0].url, "https://acme.example/jobs/1");
        assert_eq!(result.careers_urls, vec!["https://jobs.eu.lever.co/acme"]);
        assert_eq!(result.company_name.as_deref(), Some("Acme"));
        assert_eq!(
            parse_html(
                "<html>Loading…</html>",
                &Url::parse("https://acme.example").unwrap()
            )
            .outcome,
            "needs_adapter"
        );
    }
    #[test]
    fn lever_region_and_missing_dates_remain_explicit() {
        let result = parse_lever(&json!([{"id":"1","text":"Engineer","hostedUrl":"https://jobs.eu.lever.co/acme/1","createdAt":12345}]), &Board::Lever { site:"acme".into(), eu:true }, 0).unwrap();
        assert_eq!(result.jobs[0].source_key, "lever:eu:acme:1");
        assert!(result.jobs[0].source_published_at.is_none());
    }

    #[test]
    fn jsonld_preserves_compensation_units_and_explicit_eligibility() {
        let html = r#"<script type="application/ld+json">{"@type":"JobPosting","title":"Engineer","url":"/jobs/1","hiringOrganization":{"name":"Acme","url":"https://acme.example"},"applicantLocationRequirements":{"@type":"Country","name":"Canada"},"baseSalary":{"currency":"CAD","value":{"minValue":70,"maxValue":90,"unitText":"HOUR"}}}</script>"#;
        let result = parse_html(html, &Url::parse("https://acme.example/jobs").unwrap());
        assert_eq!(result.jobs[0].geographic_eligibility, vec!["Canada"]);
        assert_eq!(
            result.jobs[0].compensation[0].period.as_deref(),
            Some("HOUR")
        );
        assert_eq!(
            result.jobs[0].compensation[0].currency.as_deref(),
            Some("CAD")
        );
        assert!(result.jobs[0].remote.is_none());
    }
}
