use super::{
    Board, employer_domain, evidence, id, links, plausible_company_name, string, text,
    unsupported_shared_ats,
};
use crate::{
    http::HttpClient,
    models::{CompanyDraft, DiscoveryQuery, DiscoveryResult, JobDraft},
};
use reqwest::Url;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

/// Freezes a paid search interval before the runner commits it and sends HTTP.
///
/// # Errors
/// Returns an error for invalid title filters or an invalid prior watermark.
pub fn prepare_query(
    query: &DiscoveryQuery,
    last_complete: Option<&str>,
) -> Result<DiscoveryQuery, String> {
    let mut prepared = query.clone();
    if query.provider == "theirstack" && query.cursor.is_none() {
        let now = OffsetDateTime::now_utc();
        let limit = query
            .params
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 25);
        let mut body = theirstack_body(query, now, limit)?;
        if query.params.get("discovered_at_gte").is_none()
            && let Some(previous) = last_complete
        {
            let previous = OffsetDateTime::parse(previous, &Rfc3339)
                .map_err(|_| "invalid TheirStack prior complete timestamp")?;
            body["discovered_at_gte"] = json!(
                (previous - Duration::days(1))
                    .format(&Rfc3339)
                    .map_err(|_| "cannot format interval")?
            );
        }
        prepared.cursor = Some(json!({"page":0,"body":body}));
    }
    Ok(prepared)
}

/// Executes one checkpointable unit of company discovery.
///
/// # Errors
/// Returns budget, transport, credential, cursor, or source schema errors.
pub async fn discover(
    http: &HttpClient,
    query: &DiscoveryQuery,
) -> Result<DiscoveryResult, String> {
    match query.provider.as_str() {
        "hn" => hn(http, query).await,
        "brave" => brave(http, query).await,
        "theirstack" => theirstack(http, query).await,
        _ => Ok(DiscoveryResult {
            outcome: "needs_adapter".into(),
            warnings: vec![format!(
                "unsupported discovery provider: {}",
                query.provider
            )],
            ..Default::default()
        }),
    }
}

fn progress(cursor: Value) -> DiscoveryResult {
    DiscoveryResult {
        next_cursor: Some(cursor),
        outcome: "partial".into(),
        ..Default::default()
    }
}

#[allow(clippy::too_many_lines)]
async fn hn(http: &HttpClient, query: &DiscoveryQuery) -> Result<DiscoveryResult, String> {
    let Some(cursor) = query.cursor.as_ref() else {
        let user = http
            .get_json("https://hacker-news.firebaseio.com/v0/user/whoishiring.json")
            .await?;
        let mut submitted: Vec<u64> = user
            .get("submitted")
            .and_then(Value::as_array)
            .ok_or("HN user has no submitted items")?
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        submitted.sort_unstable_by(|a, b| b.cmp(a));
        submitted.truncate(60);
        return Ok(progress(
            json!({"stage":"thread", "ids":submitted,"index":0}),
        ));
    };
    let ids = cursor
        .get("ids")
        .and_then(Value::as_array)
        .ok_or("invalid HN cursor IDs")?;
    let index = usize::try_from(cursor.get("index").and_then(Value::as_u64).unwrap_or(0))
        .map_err(|_| "HN cursor index exceeds supported range")?;
    let stage = cursor
        .get("stage")
        .and_then(Value::as_str)
        .ok_or("invalid HN cursor stage")?;
    let Some(item_id) = ids.get(index).and_then(Value::as_u64) else {
        return Ok(DiscoveryResult {
            complete: stage == "comments",
            outcome: if stage == "comments" {
                "complete"
            } else {
                "unknown"
            }
            .into(),
            warnings: if stage == "comments" {
                vec![]
            } else {
                vec!["monthly hiring thread was not found in latest 60 submissions".into()]
            },
            ..Default::default()
        });
    };
    let item = http
        .get_json(&format!(
            "https://hacker-news.firebaseio.com/v0/item/{item_id}.json"
        ))
        .await?;
    if item.is_null() {
        return Err("HN item unavailable; monthly thread collection stopped".into());
    }
    if stage == "thread" {
        if item.get("by").and_then(Value::as_str) == Some("whoishiring")
            && item
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| title.starts_with("Ask HN: Who is hiring? ("))
            && item.get("type").and_then(Value::as_str) == Some("story")
        {
            let comments = item
                .get("kids")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            return Ok(progress(
                json!({"stage":"comments","thread":item_id,"ids":comments,"index":0}),
            ));
        }
        let mut next = cursor.clone();
        next["index"] = json!(index + 1);
        return Ok(progress(next));
    }
    if stage != "comments" {
        return Err("invalid HN cursor stage".into());
    }
    let mut result = DiscoveryResult::default();
    if !item.is_null()
        && item.get("deleted") != Some(&Value::Bool(true))
        && item.get("dead") != Some(&Value::Bool(true))
    {
        if item.get("parent") != cursor.get("thread")
            || item.get("type").and_then(Value::as_str) != Some("comment")
        {
            return Err("HN item is not a direct comment of selected hiring thread".into());
        }
        if let Some(company) = hn_company(&item, &query.terms) {
            result.companies.push(company);
        }
    }
    result.complete = index + 1 >= ids.len();
    result.outcome = if result.complete {
        "complete"
    } else {
        "partial"
    }
    .into();
    if !result.complete {
        let mut next = cursor.clone();
        next["index"] = json!(index + 1);
        result.next_cursor = Some(next);
    }
    Ok(result)
}

#[allow(clippy::too_many_lines)] // Keep one comment's identity checks and extraction together.
fn hn_company(item: &Value, terms: &[String]) -> Option<CompanyDraft> {
    let html = item.get("text")?.as_str()?;
    let plain = text(html);
    if !terms.is_empty()
        && !terms
            .iter()
            .any(|term| plain.to_lowercase().contains(&term.to_lowercase()))
    {
        return None;
    }
    let source = format!(
        "https://news.ycombinator.com/item?id={}",
        item.get("id")?.as_u64()?
    );
    let base = Url::parse(&source).ok()?;
    let found = links(html, &base);
    let domains: BTreeSet<_> = found
        .iter()
        .filter_map(|(url, _)| employer_domain(url))
        .collect();
    let boards: BTreeSet<_> = found
        .iter()
        .filter_map(|(url, _)| Board::from_url(url).map(|board| board.identity()))
        .collect();
    let website = (domains.len() == 1)
        .then(|| {
            found
                .iter()
                .find(|(url, _)| employer_domain(url).is_some())
                .map(|(url, _)| url.clone())
        })
        .flatten();
    let mut careers: BTreeSet<String> = found
        .iter()
        .filter_map(|(url, label)| {
            Board::from_url(url)
                .filter(|_| boards.len() == 1)
                .map(|board| board.url())
                .or_else(|| {
                    let phrase = format!("{} {label}", url.path()).to_lowercase();
                    if website
                        .as_ref()
                        .is_some_and(|website| website.host_str() == url.host_str())
                        && ["career", "jobs", "hiring", "join"]
                            .iter()
                            .any(|word| phrase.contains(word))
                    {
                        Some(url.as_str().into())
                    } else {
                        None
                    }
                })
        })
        .collect();
    if let Some(website) = &website {
        careers.insert(website.origin().ascii_serialization());
    }
    let board_id = found.iter().find_map(|(url, _)| {
        Board::from_url(url)
            .filter(|_| boards.len() == 1)
            .map(|board| format!("ats:{}", board.identity()))
    });
    let unresolved_url = found
        .iter()
        .find(|(url, _)| unsupported_shared_ats(url))
        .map(|(url, _)| url);
    if website.is_none() && board_id.is_none() && unresolved_url.is_none() {
        return None;
    }
    let name = hn_header_name(html)
        .or_else(|| {
            found.iter().find_map(|(url, _)| {
                Board::from_url(url)
                    .filter(|_| boards.len() == 1)
                    .map(|board| match board {
                        Board::Greenhouse(tenant) | Board::Ashby(tenant) => tenant,
                        Board::Lever { site, .. } => site,
                    })
            })
        })
        .or_else(|| website.as_ref().and_then(employer_domain))?;
    if let Some(url) = unresolved_url {
        careers.insert(url.as_str().into());
    }
    Some(CompanyDraft {
        name,
        domain: website.as_ref().and_then(employer_domain),
        website_url: website.map(|url| url.origin().ascii_serialization()),
        provider_id: board_id.or_else(|| {
            unresolved_url.map(|_| {
                format!(
                    "hn:comment:{}",
                    item.get("id").and_then(Value::as_u64).unwrap_or(0)
                )
            })
        }),
        careers_urls: careers.into_iter().collect(),
        evidence: vec![evidence(
            &source,
            "hn_hiring_comment",
            plain.chars().take(1000).collect::<String>(),
        )],
        relevance_reasons: vec!["Employer posted in monthly Hacker News hiring thread".into()],
        ..Default::default()
    })
}

fn hn_header_name(html: &str) -> Option<String> {
    let header = text(html.split("<p>").next().unwrap_or(html));
    let (name, _) = header.split_once('|')?;
    let name = name.trim();
    plausible_company_name(name).then(|| name.into())
}

async fn brave(http: &HttpClient, query: &DiscoveryQuery) -> Result<DiscoveryResult, String> {
    if query.terms.is_empty() {
        return Err("Brave query has no search terms".into());
    }
    let cursor = query.cursor.as_ref().unwrap_or(&Value::Null);
    let term_index = usize::try_from(cursor.get("term").and_then(Value::as_u64).unwrap_or(0))
        .map_err(|_| "Brave term index exceeds supported range")?;
    let offset = cursor.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let max_pages = query
        .params
        .get("max_pages")
        .and_then(Value::as_u64)
        .unwrap_or(2)
        .clamp(1, 10);
    let term = query
        .terms
        .get(term_index)
        .ok_or("invalid Brave term cursor")?;
    if offset >= max_pages {
        return Err("invalid Brave page cursor".into());
    }
    let mut url = Url::parse("https://api.search.brave.com/res/v1/web/search")
        .map_err(|_| "invalid built-in Brave URL")?;
    url.query_pairs_mut()
        .append_pair("q", term)
        .append_pair("count", "20")
        .append_pair("offset", &offset.to_string());
    if let Some(freshness) = query.params.get("freshness").and_then(Value::as_str) {
        url.query_pairs_mut().append_pair("freshness", freshness);
    }
    let json = http.provider_json("brave", url.as_str(), None, 1).await?;
    let rows = json.pointer("/web/results").and_then(Value::as_array);
    if rows.is_none() && json.get("query").is_none() {
        return Err("invalid Brave response shape".into());
    }
    let companies = rows
        .into_iter()
        .flatten()
        .filter_map(|row| brave_company(row, term))
        .collect();
    let more = json
        .pointer("/query/more_results_available")
        .and_then(Value::as_bool);
    let mut warnings = vec![];
    let capped = more == Some(true) && offset + 1 >= max_pages;
    if capped {
        warnings.push(format!("Brave query page cap reached: {term}"));
    }
    if more.is_none() {
        warnings.push("Brave response omitted its pagination flag".into());
    }
    let (next_term, next_offset) = if more == Some(true) && offset + 1 < max_pages {
        (term_index, offset + 1)
    } else {
        (term_index + 1, 0)
    };
    let prior_partial = cursor
        .get("partial")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let partial = prior_partial || capped || more.is_none();
    let done = next_term >= query.terms.len();
    Ok(DiscoveryResult {
        companies,
        next_cursor: (!done)
            .then(|| json!({"term":next_term,"offset":next_offset,"partial":partial})),
        complete: done && !partial,
        outcome: if done && !partial {
            "complete"
        } else if done {
            "capped"
        } else {
            "partial"
        }
        .into(),
        warnings,
    })
}

fn brave_company(row: &Value, term: &str) -> Option<CompanyDraft> {
    let url = crate::http::public_url(row.get("url")?.as_str()?).ok()?;
    let board = Board::from_url(&url);
    let domain = employer_domain(&url);
    if domain.is_none() && board.is_none() {
        return None;
    }
    let title =
        string(row, "title").unwrap_or_else(|| url.host_str().unwrap_or_default().to_owned());
    if board.is_none() && !employer_search_surface(&url, &title) {
        return None;
    }
    let company_name = board.as_ref().map_or_else(
        || domain.clone().unwrap_or_default(),
        |board| match board {
            Board::Greenhouse(name) | Board::Ashby(name) => name.clone(),
            Board::Lever { site, .. } => site.clone(),
        },
    );
    Some(CompanyDraft {
        name: company_name,
        domain,
        website_url: board.is_none().then(|| url.origin().ascii_serialization()),
        provider_id: board
            .as_ref()
            .map(|board| format!("ats:{}", board.identity())),
        careers_urls: vec![board.map_or_else(|| url.as_str().into(), |board| board.url())],
        evidence: vec![evidence(
            url.as_str(),
            "search_result_unverified",
            format!(
                "{title}: {}",
                string(row, "description").unwrap_or_default()
            ),
        )],
        relevance_reasons: vec![format!("Company collected from search term: {term}")],
        ..Default::default()
    })
}

fn employer_search_surface(url: &Url, title: &str) -> bool {
    let title = text(title).to_lowercase();
    let path = url.path().to_lowercase();
    let segments: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    if segments.iter().any(|part| {
        [
            "blog",
            "blogs",
            "article",
            "articles",
            "academy",
            "resources",
            "salary",
            "salaries",
            "job-descriptions",
        ]
        .contains(part)
    }) {
        return false;
    }
    if [
        "what is ",
        "what does ",
        "how to ",
        "career overview",
        "job description",
        "salaries",
        "fastest growing",
        "top 10 ",
        "top 20 ",
        "best companies",
    ]
    .iter()
    .any(|phrase| title.contains(phrase))
    {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    // Article URLs are excluded from this employer-search adapter.
    // Unknown surfaces remain outside this deterministic adapter until qualified.
    host.split('.')
        .any(|part| matches!(part, "jobs" | "careers") || part.ends_with("careers"))
        || segments.iter().any(|part| {
            part.starts_with("career")
                || matches!(*part, "job" | "jobs" | "joinus" | "join-us" | "jointheteam")
        })
        || title.contains("careers at ")
        || title.contains("join the team")
}

async fn theirstack(http: &HttpClient, query: &DiscoveryQuery) -> Result<DiscoveryResult, String> {
    let cursor = query.cursor.as_ref().unwrap_or(&Value::Null);
    let page = cursor.get("page").and_then(Value::as_u64).unwrap_or(0);
    let limit = query
        .params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 25);
    let max_pages = query
        .params
        .get("max_pages")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 5);
    if page >= max_pages || cursor.get("capped").and_then(Value::as_bool) == Some(true) {
        return Ok(DiscoveryResult { next_cursor: Some(cursor.clone()), outcome: "capped".into(), warnings: vec!["TheirStack interval needs narrowing: the free plan page cap was reached; frozen interval retained".into()], ..Default::default() });
    }
    let mut body = if let Some(body) = cursor.get("body") {
        if !body.is_object() {
            return Err("invalid TheirStack frozen request body".into());
        }
        let mut body = body.clone();
        body["page"] = json!(page);
        body
    } else {
        theirstack_body(query, OffsetDateTime::now_utc(), limit)?
    };
    body["limit"] = json!(limit);
    let json = http
        .provider_json(
            "theirstack",
            "https://api.theirstack.com/v1/jobs/search",
            Some(&body),
            limit,
        )
        .await?;
    let rows = json
        .get("data")
        .and_then(Value::as_array)
        .ok_or("invalid TheirStack response: no data array")?;
    let companies: Vec<_> = rows.iter().filter_map(theirstack_company).collect();
    let unresolved = companies.len() != rows.len();
    let truncated = json
        .pointer("/metadata/truncated_results")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
        || json
            .pointer("/metadata/truncated_companies")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0;
    let total = json
        .pointer("/metadata/total_results")
        .and_then(Value::as_u64);
    let exhausted =
        (rows.len() as u64) < limit || total.is_some_and(|total| (page + 1) * limit >= total);
    let capped = !exhausted && page + 1 >= max_pages;
    let complete = exhausted && !truncated && !unresolved;
    let warnings = if capped || truncated || unresolved {
        vec![
            "TheirStack search recorded a page cap, provider truncation, or an unmatched result"
                .into(),
        ]
    } else {
        vec![]
    };
    Ok(DiscoveryResult {
        companies,
        next_cursor: (!complete).then(
            || json!({"page":page + 1,"body":body,"capped":capped || truncated || unresolved}),
        ),
        complete,
        outcome: if complete {
            "complete"
        } else if capped || truncated || unresolved {
            "capped"
        } else {
            "partial"
        }
        .into(),
        warnings,
    })
}

fn theirstack_body(
    query: &DiscoveryQuery,
    now: OffsetDateTime,
    limit: u64,
) -> Result<Value, String> {
    if query.terms.is_empty() {
        return Err("TheirStack query has no job title terms".into());
    }
    let days = query
        .params
        .get("posted_at_max_age_days")
        .and_then(Value::as_i64)
        .unwrap_or(90)
        .clamp(0, 365);
    let mut body = json!({"limit":limit,"page":0,"job_title_pattern_or":query.terms.iter().map(|term| regex::escape(term)).collect::<Vec<_>>(),"company_type":"direct_employer","is_closed":false,
        "posted_at_gte":(now - Duration::days(days)).date().to_string(), "posted_at_lte":now.date().to_string(),
        "discovered_at_gte":(now - Duration::days(8)).format(&Rfc3339).map_err(|_| "cannot format date")?, "discovered_at_lte":now.format(&Rfc3339).map_err(|_| "cannot format date")? });
    for field in [
        "posted_at_gte",
        "posted_at_lte",
        "discovered_at_gte",
        "discovered_at_lte",
        "job_country_code_or",
        "workplace_types_or",
    ] {
        if let Some(value) = query.params.get(field) {
            body[field] = value.clone();
        }
    }
    Ok(body)
}

fn theirstack_company(row: &Value) -> Option<CompanyDraft> {
    let object = row.get("company_object").unwrap_or(&Value::Null);
    if row.get("has_blurred_data") == Some(&json!(true))
        || object.get("has_blurred_data") == Some(&json!(true))
    {
        return None;
    }
    let name = string(object, "name").or_else(|| string(row, "company"))?;
    let raw_domain = string(object, "domain").or_else(|| string(row, "company_domain"));
    let website = raw_domain
        .as_ref()
        .and_then(|domain| {
            crate::http::public_url(&format!(
                "https://{}",
                domain
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
            ))
            .ok()
        })
        .filter(|url| employer_domain(url).is_some());
    let domain = website.as_ref().and_then(employer_domain);
    let provider_id = id(object, "id").map(|id| format!("theirstack:company:{id}"));
    if domain.is_none() && provider_id.is_none() {
        return None;
    }
    let job_url = string(row, "final_url")
        .or_else(|| string(row, "url"))
        .and_then(|url| crate::http::public_url(&url).ok());
    let source_url = string(row, "source_url")
        .or_else(|| string(row, "url"))
        .unwrap_or_else(|| "https://api.theirstack.com/v1/jobs/search".into());
    let mut careers = BTreeSet::new();
    if let Some(website) = &website {
        careers.insert(website.origin().ascii_serialization());
    }
    if let Some(url) = &job_url {
        if let Some(board) = Board::from_url(url) {
            careers.insert(board.url());
        } else if employer_domain(url) == domain && domain.is_some() {
            careers.insert(url.as_str().into());
        }
    }
    let jobs = match (id(row, "id"), string(row, "job_title"), job_url) {
        (Some(job_id), Some(title), Some(url)) => vec![JobDraft {
            source_key: format!("theirstack:{job_id}"),
            title,
            url: url.into(),
            location: string(row, "location"),
            remote: row.get("remote").and_then(Value::as_bool),
            employment_type: row
                .get("employment_statuses")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                }),
            source_published_at: string(row, "date_posted"),
            published_at_semantics: Some("aggregator_reported_date_posted".into()),
            compensation: theirstack_compensation(row),
            description: string(row, "description"),
            is_listed: true,
            evidence: vec![evidence(
                &source_url,
                "aggregator_job",
                format!(
                    "TheirStack job {job_id}; source publication date is provider-reported; recorded status: unknown"
                ),
            )],
            ..Default::default()
        }],
        _ => vec![],
    };
    Some(CompanyDraft {
        name,
        domain,
        website_url: website.map(|url| url.origin().ascii_serialization()),
        provider_id,
        careers_urls: careers.into_iter().collect(),
        evidence: vec![evidence(
            &source_url,
            "aggregator_employer",
            "Employer identity supplied by TheirStack",
        )],
        relevance_reasons: vec!["Employer matched configured job title search".into()],
        jobs,
    })
}

fn theirstack_compensation(row: &Value) -> Vec<crate::models::Compensation> {
    let minimum = row.get("min_annual_salary").and_then(Value::as_f64);
    let maximum = row.get("max_annual_salary").and_then(Value::as_f64);
    if minimum.is_none() && maximum.is_none() {
        return vec![];
    }
    vec![crate::models::Compensation {
        currency: string(row, "salary_currency"),
        period: Some("year".into()),
        component: "salary".into(),
        minimum,
        maximum,
    }]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn hn_prose_falls_back_to_exact_tenant_or_domain() {
        let row = json!({"id":4,"text":"We're building a unified platform for credit and financial data to power financial inclusion globally.<p>Apply at <a href=\"https://job-boards.greenhouse.io/novacredit\">our board</a>"});
        assert_eq!(hn_company(&row, &[]).unwrap().name, "novacredit");
        let row = json!({"id":5,"text":"We are hiring several software engineers.<p><a href=\"https://acme.example/careers\">Careers</a>"});
        assert_eq!(hn_company(&row, &[]).unwrap().name, "acme.example");
        assert_eq!(
            hn_header_name("Nova Credit | Engineer | Remote").as_deref(),
            Some("Nova Credit")
        );
    }
    #[test]
    fn unsupported_shared_ats_lead_does_not_alias_the_host_as_company() {
        for (comment, url) in [
            (4, "https://join.com/companies/aimitechnology/123"),
            (5, "https://jobs.curriculo.me/3c-digital/job/123"),
        ] {
            let row = json!({"id":comment,"text":format!("Acme | Engineer | Remote<p><a href=\"{url}\">Apply</a>")});
            let company = hn_company(&row, &[]).unwrap();
            assert!(company.domain.is_none());
            assert!(company.website_url.is_none());
            assert_eq!(company.provider_id, Some(format!("hn:comment:{comment}")));
            assert_eq!(company.careers_urls, vec![url]);
        }
    }
    #[test]
    fn live_directory_and_editorial_false_positives_are_rejected() {
        for url in [
            "https://web3.career/infrastructure-jobs",
            "https://builtinaustin.com/jobs/engineering",
            "https://builtincolorado.com/jobs/engineering",
            "https://naukri.com/infrastructure-jobs",
            "https://ziprecruiter.com/Jobs/Infrastructure",
            "https://6figr.com/us/salary/infrastructure",
            "https://remoterocketship.com/jobs/infrastructure",
            "https://trueup.io/ai-infra",
        ] {
            assert!(
                brave_company(
                    &json!({"url":url,"title":"Infrastructure jobs"}),
                    "engineering"
                )
                .is_none(),
                "{url}"
            );
        }
        for (url, title) in [
            (
                "https://unknown-publisher.example/blog/companies",
                "AI infrastructure companies",
            ),
            (
                "https://unknown-guide.example/careers/infrastructure",
                "What does an infrastructure engineer do?",
            ),
            (
                "https://unknown-research.example/ai-companies",
                "Top 10 AI Infrastructure Companies & Applications",
            ),
        ] {
            assert!(
                brave_company(&json!({"url":url,"title":title}), "engineering").is_none(),
                "{url}"
            );
        }
    }
    #[test]
    fn direct_employer_careers_and_ats_candidates_remain_discoverable() {
        for (url, title) in [
            (
                "https://www.stackinfra.com/about/careers/",
                "Careers at STACK",
            ),
            (
                "https://careers.datadoghq.com/detail/3851927",
                "Senior Software Engineer",
            ),
            (
                "https://metacareers.com/teams/infrastructure",
                "Infrastructure Jobs at Meta",
            ),
            ("https://jobs.ashbyhq.com/acme/123", "Engineer"),
        ] {
            assert!(
                brave_company(&json!({"url":url,"title":title}), "engineering").is_some(),
                "{url}"
            );
        }
    }
    #[test]
    fn hn_extracts_employer_without_assigning_shared_ats_domain() {
        let company = hn_company(&json!({"id":4,"text":"Acme | Backend | Remote<p><a href=\"https://jobs.ashbyhq.com/acme/123\">Apply</a>"}), &[]).unwrap();
        assert_eq!(company.name, "Acme");
        assert!(company.domain.is_none());
        assert_eq!(company.provider_id.as_deref(), Some("ats:ashby:acme"));
        assert_eq!(company.careers_urls, vec!["https://jobs.ashbyhq.com/acme"]);
    }
    #[test]
    fn search_results_do_not_turn_directories_into_companies() {
        assert!(
            brave_company(
                &json!({"url":"https://www.ycombinator.com/companies","title":"Companies"}),
                "engineering"
            )
            .is_none()
        );
    }
    #[test]
    fn theirstack_dates_remain_reported_publication_dates() {
        let company = theirstack_company(&json!({"id":1,"company":"Acme","company_object":{"id":"acme","domain":"acme.example"},"job_title":"Engineer","url":"https://acme.example/job/1","date_posted":"2026-01-01","discovered_at":"2026-02-01","date_reposted":"2026-03-01"})).unwrap();
        assert_eq!(
            company.jobs[0].source_published_at.as_deref(),
            Some("2026-01-01")
        );
        assert!(company.jobs[0].source_updated_at.is_none());
    }

    #[test]
    fn ambiguous_hn_domains_do_not_create_a_false_company_alias() {
        let row = json!({"id":4,"text":"Acme | Engineer<p><a href=\"https://acme.example\">Company</a><a href=\"https://other.example/blog\">Article</a><a href=\"https://jobs.ashbyhq.com/acme/123\">Apply</a>"});
        let company = hn_company(&row, &[]).unwrap();
        assert!(company.domain.is_none());
        assert!(company.website_url.is_none());
        assert_eq!(company.careers_urls, vec!["https://jobs.ashbyhq.com/acme"]);
    }
    #[test]
    fn hn_encoded_href_entities_resolve_to_the_employer_board() {
        let company = hn_company(&json!({"id":4,"text":"Acme | Engineer<p><a href=\"https:&#x2F;&#x2F;job-boards.greenhouse.io&#x2F;acme&#x2F;jobs&#x2F;1?gh_src=abc\">Apply</a>"}), &[]).unwrap();
        assert_eq!(
            company.careers_urls,
            vec!["https://job-boards.greenhouse.io/acme"]
        );
        assert!(company.domain.is_none());
    }

    #[test]
    fn paid_query_interval_is_frozen_and_resumes_without_recalculation() {
        let query = DiscoveryQuery {
            id: "test".into(),
            provider: "theirstack".into(),
            terms: vec!["backend engineer".into()],
            params: json!({"limit":20}),
            cursor: None,
            enabled: true,
            interval_seconds: 86400,
        };
        let prepared = prepare_query(&query, Some("2026-01-03T00:00:00Z")).unwrap();
        assert_eq!(
            prepared
                .cursor
                .as_ref()
                .unwrap()
                .pointer("/body/discovered_at_gte")
                .unwrap(),
            "2026-01-02T00:00:00Z"
        );
        assert_eq!(
            prepare_query(&prepared, Some("2026-02-01T00:00:00Z"))
                .unwrap()
                .cursor,
            prepared.cursor
        );
    }
}
