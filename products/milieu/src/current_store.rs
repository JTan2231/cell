//! Persistence and the existing snapshot projection for accepted current state.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{Result, adapters, current, models};

pub(crate) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch(current::SCHEMA)?;
    connection.execute_batch(
        "ALTER TABLE company ADD COLUMN compatibility TEXT NOT NULL DEFAULT '{}';
         ALTER TABLE job ADD COLUMN compatibility TEXT NOT NULL DEFAULT '{}';
         ALTER TABLE source ADD COLUMN compatibility TEXT NOT NULL DEFAULT '{}';",
    )?;
    Ok(())
}

pub(crate) fn all<T: DeserializeOwned>(connection: &Connection, table: &str) -> Result<Vec<T>> {
    let core = core_table(table)?;
    let mut statement = connection.prepare(&format!("SELECT id FROM {core} ORDER BY id"))?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|id| get(connection, table, id)?.ok_or_else(|| "current record disappeared".into()))
        .collect()
}

pub(crate) fn get<T: DeserializeOwned>(
    connection: &Connection,
    table: &str,
    id: &str,
) -> Result<Option<T>> {
    let value = match core_table(table)? {
        "company" => company(connection, id)?,
        "source" => source(connection, id)?,
        "job" => job(connection, id)?,
        _ => unreachable!(),
    };
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn core_table(table: &str) -> Result<&'static str> {
    match table {
        "companies" => Ok("company"),
        "jobs" => Ok("job"),
        "sources" => Ok("source"),
        _ => Err(format!("unsupported current record table: {table}").into()),
    }
}

pub(crate) fn put_company(tx: &Connection, company: &models::Company) -> Result<()> {
    require_text("company id", &company.id)?;
    require_text("company name", &company.name)?;
    let mut projected = company.clone();
    canonicalize_seen_at(&mut projected.first_seen_at, &mut projected.last_seen_at)?;
    let previous: Option<models::Company> = get(tx, "companies", &company.id)?;
    if let Some(previous) = &previous {
        retain_seen_at(
            &previous.first_seen_at,
            &previous.last_seen_at,
            &projected.first_seen_at,
            &projected.last_seen_at,
        )?;
    }
    projected.revision = revision(
        previous.as_ref().map(|company| (company, company.revision)),
        &projected,
    )?;
    let compatibility = compatibility(&projected, &["id", "name"])?;
    tx.execute(
        "INSERT INTO company(id, name, compatibility) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, compatibility=excluded.compatibility",
        params![company.id, company.name, compatibility],
    )?;
    Ok(())
}

pub(crate) fn put_source(tx: &Connection, source: &models::Source) -> Result<()> {
    require_text("source id", &source.id)?;
    crate::normalize_url(&source.url)?;
    if !source.company_id.is_empty() {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM company WHERE id=?1)",
            [&source.company_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err("source compatibility company does not exist".into());
        }
    }
    let compatibility = compatibility(source, &["id", "url"])?;
    tx.execute(
        "INSERT INTO source(id, name, url, operator_id, compatibility) VALUES (?1, ?2, ?2, NULL, ?3)
         ON CONFLICT(id) DO UPDATE SET url=excluded.url, compatibility=excluded.compatibility",
        params![source.id, source.url, compatibility],
    )?;
    Ok(())
}

pub(crate) fn put_current_source(
    tx: &Transaction<'_>,
    source: &current::Source,
    compatibility: &models::Source,
) -> Result<()> {
    if source.id != compatibility.id || source.url != compatibility.url {
        return Err("source compatibility id and URL must match the current source".into());
    }
    require_text("source name", &source.name)?;
    put_source(tx, compatibility)?;
    tx.execute(
        "UPDATE source SET name=?2, operator_id=?3 WHERE id=?1",
        params![source.id, source.name, source.operator_id],
    )?;
    Ok(())
}

// Keep the acceptance checks and complete link replacement in one transaction.
#[allow(clippy::too_many_lines)]
pub(crate) fn replace_job(tx: &Transaction<'_>, accepted: &current::AcceptedJob) -> Result<()> {
    let record = &accepted.job;
    let mut projected = accepted.compatibility.clone();
    canonicalize_seen_at(&mut projected.first_seen_at, &mut projected.last_seen_at)?;
    let public = &projected;
    require_text("opportunity id", &record.id)?;
    require_text("opportunity employer", &record.employer_id)?;
    require_text("opportunity title", &record.title)?;
    require_text("opportunity status", &record.status)?;
    if ![
        "listed",
        "unlisted",
        "missing",
        "presumed_closed",
        "unknown",
    ]
    .contains(&record.status.as_str())
    {
        return Err(
            "opportunity status must be listed, unlisted, missing, presumed_closed, or unknown"
                .into(),
        );
    }
    let remote = remote(record.work_mode.as_deref())?;
    let mut locations = accepted.locations.iter().collect::<Vec<_>>();
    locations.sort_by(|left, right| left.id.cmp(&right.id));
    let location = location_names(locations.iter().map(|location| location.name.as_str()));
    if record.id != public.id
        || record.employer_id != public.company_id
        || record.title != public.title
        || record.description != public.description
        || record.status != public.availability
        || remote != public.remote
        || location != public.location
    {
        return Err("opportunity compatibility fields must match the current record".into());
    }
    adapters::validate_job_url(&public.url)?;
    if !adapters::job_url_matches(public, &public.url)?
        || (native_key(&public.source_key) && adapters::ats_provider(&public.url).is_none())
    {
        return Err("opportunity source key and primary URL identify different postings".into());
    }
    if let Some(apply_url) = &public.apply_url {
        adapters::validate_job_url(apply_url)?;
        if adapters::ats_provider(apply_url).is_some()
            && !adapters::job_url_matches(public, apply_url)?
        {
            return Err("application URL identifies a different ATS posting".into());
        }
    }
    let previous: Option<models::Job> = get(tx, "jobs", &record.id)?;
    if let Some(previous) = &previous {
        if previous.source_id != public.source_id
            || previous.url != public.url
            || previous.source_key != public.source_key
        {
            return Err(
                "the existing primary opportunity source, URL and source key must be retained"
                    .into(),
            );
        }
        retain_seen_at(
            &previous.first_seen_at,
            &previous.last_seen_at,
            &public.first_seen_at,
            &public.last_seen_at,
        )?;
    }
    let mut source_ids = BTreeSet::new();
    let mut primary = false;
    for appearance in &accepted.appearances {
        if appearance.job_id != record.id || !source_ids.insert(&appearance.source_id) {
            return Err(
                "opportunity appearances must have this opportunity id and distinct source ids"
                    .into(),
            );
        }
        adapters::validate_job_url(&appearance.url)?;
        primary |= appearance.source_id == public.source_id && appearance.url == public.url;
    }
    if !primary {
        return Err("opportunity appearances must contain the primary source and URL".into());
    }
    let mut location_ids = BTreeSet::new();
    for location in &locations {
        require_text("location id", &location.id)?;
        require_text("location name", &location.name)?;
        if !location_ids.insert(&location.id) {
            return Err("opportunity locations must have distinct ids".into());
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT name FROM location WHERE id=?1",
                [&location.id],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some_and(|name| name != location.name) {
            return Err("an existing location id must retain its name".into());
        }
    }
    projected.revision = revision(previous.as_ref().map(|job| (job, job.revision)), public)?;
    let compatibility = compatibility(
        &projected,
        &[
            "id",
            "company_id",
            "title",
            "description",
            "availability",
            "remote",
            "location",
            "url",
        ],
    )?;
    tx.execute(
        "INSERT INTO job(id, employer_id, title, description, work_mode, status, compatibility)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET employer_id=excluded.employer_id, title=excluded.title,
         description=excluded.description, work_mode=excluded.work_mode, status=excluded.status,
         compatibility=excluded.compatibility",
        params![
            record.id,
            record.employer_id,
            record.title,
            record.description,
            record.work_mode,
            record.status,
            compatibility
        ],
    )?;
    tx.execute("DELETE FROM job_location WHERE job_id=?1", [&record.id])?;
    tx.execute("DELETE FROM job_source WHERE job_id=?1", [&record.id])?;
    for location in locations {
        tx.execute(
            "INSERT INTO location(id, name) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
            params![location.id, location.name],
        )?;
        tx.execute(
            "INSERT INTO job_location(job_id, location_id) VALUES (?1, ?2)",
            params![record.id, location.id],
        )?;
    }
    for appearance in &accepted.appearances {
        tx.execute(
            "INSERT INTO job_source(job_id, source_id, url) VALUES (?1, ?2, ?3)",
            params![appearance.job_id, appearance.source_id, appearance.url],
        )?;
    }
    Ok(())
}

fn native_key(key: &str) -> bool {
    ["ashby:", "greenhouse:", "lever:"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

fn canonicalize_seen_at(first: &mut String, last: &mut String) -> Result<()> {
    use time::{
        OffsetDateTime, UtcOffset, format_description, format_description::well_known::Rfc3339,
    };
    let format = format_description::parse_borrowed::<2>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z",
    )?;
    let first_time = OffsetDateTime::parse(first, &Rfc3339)?;
    let last_time = OffsetDateTime::parse(last, &Rfc3339)?;
    if first_time > last_time {
        return Err("first_seen_at must not follow last_seen_at".into());
    }
    *first = first_time.to_offset(UtcOffset::UTC).format(&format)?;
    *last = last_time.to_offset(UtcOffset::UTC).format(&format)?;
    Ok(())
}

fn retain_seen_at(first: &str, last: &str, next_first: &str, next_last: &str) -> Result<()> {
    if first != next_first {
        return Err("an existing record must retain first_seen_at".into());
    }
    if next_last < last {
        return Err("an existing record must not move last_seen_at backwards".into());
    }
    Ok(())
}

fn revision<T: Serialize>(previous: Option<(&T, u64)>, current: &T) -> Result<u64> {
    let Some((previous, previous_revision)) = previous else {
        return Ok(1);
    };
    let old = compatibility(previous, &["revision", "last_seen_at"])?;
    let new = compatibility(current, &["revision", "last_seen_at"])?;
    if old == new {
        Ok(previous_revision)
    } else {
        previous_revision
            .checked_add(1)
            .ok_or_else(|| "record revision overflow".into())
    }
}

fn remote(work_mode: Option<&str>) -> Result<Option<bool>> {
    match work_mode {
        None => Ok(None),
        Some("remote") => Ok(Some(true)),
        Some("hybrid" | "on-site") => Ok(Some(false)),
        Some(_) => Err("work_mode must be remote, hybrid, on-site, or null".into()),
    }
}

fn location_names<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let names = names.collect::<Vec<_>>();
    (!names.is_empty()).then(|| names.join(", "))
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty").into());
    }
    Ok(())
}

fn compatibility(value: &impl Serialize, core_fields: &[&str]) -> Result<String> {
    let mut value = serde_json::to_value(value)?;
    let object = value
        .as_object_mut()
        .ok_or("compatibility must be an object")?;
    for field in core_fields {
        object.remove(*field);
    }
    Ok(serde_json::to_string(&value)?)
}

fn company(connection: &Connection, id: &str) -> Result<Option<Value>> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT name, compatibility FROM company WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(name, body)| {
        let mut value = metadata(&body)?;
        value["id"] = id.into();
        value["name"] = name.into();
        Ok(value)
    })
    .transpose()
}

fn source(connection: &Connection, id: &str) -> Result<Option<Value>> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT url, compatibility FROM source WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(url, body)| {
        let mut value = metadata(&body)?;
        value["id"] = id.into();
        value["url"] = url.into();
        Ok(value)
    })
    .transpose()
}

fn job(connection: &Connection, id: &str) -> Result<Option<Value>> {
    let row: Option<(current::Job, String)> = connection
        .query_row(
            "SELECT employer_id, title, description, work_mode, status, compatibility
             FROM job WHERE id=?1",
            [id],
            |row| {
                Ok((
                    current::Job {
                        id: id.into(),
                        employer_id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        work_mode: row.get(3)?,
                        status: row.get(4)?,
                    },
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    row.map(|(record, body)| {
        let mut value = metadata(&body)?;
        let source_id = value["source_id"].as_str().ok_or("opportunity primary source id is missing")?;
        let url: String = connection.query_row(
            "SELECT url FROM job_source WHERE job_id=?1 AND source_id=?2",
            params![id, source_id],
            |row| row.get(0),
        )?;
        let mut statement = connection.prepare(
            "SELECT location.name FROM job_location JOIN location ON location.id=job_location.location_id
             WHERE job_location.job_id=?1 ORDER BY location.id",
        )?;
        let names = statement.query_map([id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        value["id"] = id.into();
        value["company_id"] = record.employer_id.into();
        value["title"] = record.title.into();
        value["description"] = serde_json::to_value(record.description)?;
        value["availability"] = record.status.into();
        value["remote"] = serde_json::to_value(remote(record.work_mode.as_deref())?)?;
        value["location"] = serde_json::to_value(location_names(names.iter().map(String::as_str)))?;
        value["url"] = url.into();
        Ok(value)
    }).transpose()
}

fn metadata(body: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(body)?;
    if !value.is_object() {
        return Err("stored compatibility must be an object".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn accepted_job_preserves_consumer_identity_and_rolls_back_invalid_links() -> Result<()> {
        let mut connection = Connection::open_in_memory()?;
        initialize(&connection)?;
        let tx = connection.transaction()?;
        for id in ["employer", "operator"] {
            put_company(
                &tx,
                &models::Company {
                    id: id.into(),
                    revision: 0,
                    name: id.into(),
                    domain: None,
                    website_url: None,
                    first_seen_at: "2026-10-01T00:00:00Z".into(),
                    last_seen_at: "2026-10-01T00:00:00Z".into(),
                    relevance_reasons: vec![],
                    evidence: vec![],
                },
            )?;
        }
        for id in ["primary", "secondary"] {
            put_current_source(
                &tx,
                &current::Source {
                    id: id.into(),
                    name: id.into(),
                    url: "https://jobs.ashbyhq.com/board".into(),
                    operator_id: Some("operator".into()),
                },
                &models::Source {
                    id: id.into(),
                    company_id: "operator".into(),
                    url: "https://jobs.ashbyhq.com/board".into(),
                    discovery_depth: 0,
                    enabled: true,
                    status: "never_checked".into(),
                    last_attempt_at: None,
                    last_success_at: None,
                    next_due_at: 0,
                    cursor: None,
                    note: None,
                },
            )?;
        }
        let public = models::Job {
            id: "accepted".into(),
            revision: 0,
            company_id: "employer".into(),
            source_id: "primary".into(),
            source_key: "ashby:board:role".into(),
            title: "Engineer".into(),
            url: "https://jobs.ashbyhq.com/board/role".into(),
            apply_url: Some("https://jobs.ashbyhq.com/board/role/application".into()),
            location: Some("London".into()),
            remote: Some(false),
            employment_type: None,
            source_published_at: None,
            source_updated_at: None,
            source_internal_id: None,
            first_seen_at: "2026-10-01T02:00:00+02:00".into(),
            last_seen_at: "2026-10-01T00:00:00Z".into(),
            availability: "listed".into(),
            missing_complete_snapshots: 0,
            first_missing_at: None,
            description: None,
            evidence: vec![],
            compensation: vec![models::Compensation {
                currency: Some("USD".into()),
                period: Some("year".into()),
                component: "base".into(),
                minimum: Some(100_000.0),
                maximum: Some(150_000.0),
            }],
            geographic_eligibility: vec![],
            published_at_semantics: None,
            content_fingerprint: None,
            parser_version: "accepted-fixture".into(),
        };
        let mut accepted = current::AcceptedJob {
            job: current::Job {
                id: public.id.clone(),
                employer_id: public.company_id.clone(),
                title: public.title.clone(),
                description: None,
                work_mode: Some("hybrid".into()),
                status: "listed".into(),
            },
            locations: vec![current::Location {
                id: "london".into(),
                name: "London".into(),
            }],
            appearances: vec![current::JobSource {
                job_id: public.id.clone(),
                source_id: public.source_id.clone(),
                url: public.url.clone(),
            }],
            compatibility: public,
        };
        replace_job(&tx, &accepted)?;
        tx.commit()?;
        accepted.appearances.push(current::JobSource {
            job_id: "accepted".into(),
            source_id: "secondary".into(),
            url: "https://board.example/jobs/role".into(),
        });
        let tx = connection.transaction()?;
        replace_job(&tx, &accepted)?;
        tx.commit()?;
        let retained: models::Job =
            get(&connection, "jobs", "accepted")?.ok_or("missing test job")?;
        assert_eq!(retained.company_id, "employer");
        assert_eq!(retained.source_id, "primary");
        assert_eq!(retained.source_key, "ashby:board:role");
        assert_eq!(retained.url, accepted.compatibility.url);
        assert_eq!(retained.location.as_deref(), Some("London"));
        assert_eq!(retained.remote, Some(false));
        assert_eq!(retained.revision, 1);
        assert_eq!(retained.first_seen_at, "2026-10-01T00:00:00.000000000Z");
        assert_eq!(retained.last_seen_at, "2026-10-01T00:00:00.000000000Z");
        assert_eq!(retained.compensation[0].maximum, Some(150_000.0));
        let operator: String = connection.query_row(
            "SELECT operator_id FROM source WHERE id='primary'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(operator, "operator");
        let mut invalid = accepted.clone();
        invalid.compatibility.source_key = "ashby:board:other".into();
        let tx = connection.transaction()?;
        assert!(replace_job(&tx, &invalid).is_err());
        drop(tx);
        invalid = accepted.clone();
        invalid.job.title = "Changed".into();
        invalid.compatibility.title = "Changed".into();
        invalid.appearances[1].source_id = "missing-source".into();
        let tx = connection.transaction()?;
        assert!(replace_job(&tx, &invalid).is_err());
        drop(tx);
        let retained: models::Job =
            get(&connection, "jobs", "accepted")?.ok_or("missing test job")?;
        assert_eq!(retained.title, "Engineer");
        assert_eq!(retained.revision, 1);
        let links: i64 = connection.query_row(
            "SELECT COUNT(*) FROM job_source WHERE job_id='accepted'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(links, 2);
        assert_eq!(all::<models::Job>(&connection, "jobs")?.len(), 1);
        Ok(())
    }
}
