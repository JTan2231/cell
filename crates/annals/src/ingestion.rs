use std::fs;
use std::path::Path;
use std::time::SystemTime;

use rusqlite::{Connection, params};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, Month, OffsetDateTime, Time, UtcOffset};

use crate::cli::{IngestionChannel, IngestionStatus, LatelyArgs, LatelyTime};
use crate::corpus::now;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub(crate) struct SourceMetadata {
    pub source_name: String,
    pub source_size_bytes: Option<u64>,
    pub source_created_at: Option<String>,
    pub source_modified_at: Option<String>,
    pub first_seen_at: String,
}

impl SourceMetadata {
    pub(crate) fn manual(path: &Path, explicit_name: Option<&str>) -> Result<Self, AppError> {
        let first_seen_at = now()?;
        if path == Path::new("-") {
            return Ok(Self {
                source_name: explicit_name
                    .filter(|name| !name.is_empty())
                    .unwrap_or("standard input")
                    .to_owned(),
                source_size_bytes: None,
                source_created_at: None,
                source_modified_at: None,
                first_seen_at,
            });
        }
        let source_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                let displayed = path.display().to_string();
                if displayed.is_empty() {
                    "unnamed source".to_owned()
                } else {
                    displayed
                }
            });
        let metadata = fs::metadata(path).ok();
        let source_size_bytes = metadata.as_ref().map(fs::Metadata::len);
        let source_created_at = metadata
            .as_ref()
            .and_then(|metadata| metadata.created().ok())
            .map(format_system_time)
            .transpose()?;
        let source_modified_at = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .map(format_system_time)
            .transpose()?;
        Ok(Self {
            source_name,
            source_size_bytes,
            source_created_at,
            source_modified_at,
            first_seen_at,
        })
    }
}

#[derive(Debug)]
pub(crate) struct NewIngestion<'a> {
    pub delivery_key: Option<&'a str>,
    pub channel: &'a str,
    pub metadata: &'a SourceMetadata,
}

pub(crate) fn begin(
    connection: &Connection,
    ingestion: &NewIngestion<'_>,
) -> Result<i64, AppError> {
    let size = ingestion
        .metadata
        .source_size_bytes
        .map(|size| {
            i64::try_from(size).map_err(|_| {
                AppError::invalid(
                    "source_too_large",
                    "source byte length exceeds the supported range",
                )
            })
        })
        .transpose()?;
    connection.execute(
        "INSERT INTO ingestions(\
             delivery_key, source_name, channel, source_size_bytes, source_created_at, \
             source_modified_at, first_seen_at, status\
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'processing') \
         ON CONFLICT(delivery_key) DO NOTHING",
        params![
            ingestion.delivery_key,
            ingestion.metadata.source_name,
            ingestion.channel,
            size,
            ingestion.metadata.source_created_at,
            ingestion.metadata.source_modified_at,
            ingestion.metadata.first_seen_at,
        ],
    )?;
    if let Some(delivery_key) = ingestion.delivery_key {
        return connection
            .query_row(
                "SELECT id FROM ingestions WHERE delivery_key = ?1",
                [delivery_key],
                |row| row.get(0),
            )
            .map_err(AppError::from);
    }
    Ok(connection.last_insert_rowid())
}

pub(crate) fn complete(
    connection: &Connection,
    ingestion_id: i64,
    result: &str,
    result_revision: Option<i64>,
) -> Result<(), AppError> {
    let updated = connection.execute(
        "UPDATE ingestions SET status = 'completed', completed_at = ?1, result = ?2, \
             result_revision = ?3, error_code = NULL, error_message = NULL \
         WHERE id = ?4 AND status = 'processing' AND work_id IS NOT NULL",
        params![now()?, result, result_revision, ingestion_id],
    )?;
    if updated == 0 {
        let already_complete = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ingestions \
                 WHERE id = ?1 AND status = 'completed' AND result = ?2 \
                       AND result_revision IS ?3)",
            params![ingestion_id, result, result_revision],
            |row| row.get::<_, bool>(0),
        )?;
        if already_complete {
            return Ok(());
        }
        return Err(AppError::database(
            "ingestion_completion_failed",
            "unable to complete the source delivery receipt",
        ));
    }
    Ok(())
}

pub(crate) fn fail(
    connection: &Connection,
    ingestion_id: i64,
    error: &AppError,
) -> Result<(), AppError> {
    let updated = connection.execute(
        "UPDATE ingestions SET status = 'failed', completed_at = ?1, result = NULL, \
             result_revision = NULL, error_code = ?2, error_message = ?3 \
         WHERE id = ?4 AND status = 'processing'",
        params![now()?, error.code(), "source delivery failed", ingestion_id],
    )?;
    if updated == 0 {
        let already_failed = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ingestions WHERE id = ?1 AND status = 'failed')",
            [ingestion_id],
            |row| row.get::<_, bool>(0),
        )?;
        if already_failed {
            return Ok(());
        }
        return Err(AppError::database(
            "ingestion_failure_record_failed",
            "unable to fail the source delivery receipt",
        ));
    }
    Ok(())
}

pub(crate) fn record_retryable_error(
    connection: &Connection,
    ingestion_id: i64,
    error: &AppError,
) -> Result<(), AppError> {
    let updated = connection.execute(
        "UPDATE ingestions SET error_code = ?1, error_message = ?2 \
         WHERE id = ?3 AND status = 'processing'",
        params![
            error.code(),
            "source delivery processing encountered a retryable error",
            ingestion_id
        ],
    )?;
    if updated == 0 {
        let terminal = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ingestions \
             WHERE id = ?1 AND status IN ('completed', 'failed'))",
            [ingestion_id],
            |row| row.get::<_, bool>(0),
        )?;
        if terminal {
            return Ok(());
        }
        return Err(AppError::database(
            "ingestion_error_record_failed",
            "unable to update the source delivery receipt",
        ));
    }
    Ok(())
}

pub(crate) fn fail_interrupted_manual(connection: &Connection) -> Result<usize, AppError> {
    Ok(connection.execute(
        "UPDATE ingestions SET status = 'failed', completed_at = ?1, result = NULL, \
             result_revision = NULL, error_code = 'manual_ingestion_interrupted', \
             error_message = 'manual source delivery was interrupted' \
         WHERE channel = 'manual' AND status = 'processing'",
        [now()?],
    )?)
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct IngestionErrorView {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct IngestionView {
    pub source_name: String,
    pub channel: String,
    pub status: String,
    pub retention: Option<String>,
    pub result: Option<String>,
    pub work: Option<String>,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub source_created_at: Option<String>,
    pub source_modified_at: Option<String>,
    pub first_seen_at: String,
    pub ingested_at: Option<String>,
    pub completed_at: Option<String>,
    pub applied_revision: Option<i64>,
    pub error: Option<IngestionErrorView>,
}

impl IngestionView {
    pub(crate) fn selected_timestamp(&self, basis: LatelyTime) -> Option<&str> {
        match basis {
            LatelyTime::Created => self.source_created_at.as_deref(),
            LatelyTime::Modified => self.source_modified_at.as_deref(),
            LatelyTime::FirstSeen => Some(&self.first_seen_at),
            LatelyTime::Ingested => self.ingested_at.as_deref(),
            LatelyTime::Completed => self.completed_at.as_deref(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct LatelyReport {
    pub since: String,
    pub until: String,
    pub time_basis: String,
    pub status: Option<String>,
    pub channel: Option<String>,
    pub delivery_count: usize,
    pub processing_count: usize,
    pub completed_count: usize,
    pub failed_count: usize,
    pub new_work_count: usize,
    pub duplicate_count: usize,
    pub missing_time_count: usize,
    pub deliveries: Vec<IngestionView>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedWindow {
    since: OffsetDateTime,
    until: OffsetDateTime,
}

impl ResolvedWindow {
    fn since_text(self) -> Result<String, AppError> {
        format_timestamp(self.since)
    }

    fn until_text(self) -> Result<String, AppError> {
        format_timestamp(self.until)
    }
}

pub(crate) fn lately(connection: &Connection, args: &LatelyArgs) -> Result<LatelyReport, AppError> {
    let window = resolve_window(args, OffsetDateTime::now_utc())?;
    let status_filter = args.status.map(IngestionStatus::as_str);
    let channel_filter = args.channel.map(IngestionChannel::as_str);
    let mut statement = connection.prepare(
        "SELECT i.id, i.source_name, i.channel, i.source_size_bytes, i.source_created_at, \
                i.source_modified_at, i.first_seen_at, i.ingested_at, i.completed_at, \
                i.status, i.new_work, i.result, i.result_revision, i.error_code, \
                w.label, w.sha256 \
         FROM ingestions AS i LEFT JOIN works AS w ON w.id = i.work_id \
         WHERE (?1 IS NULL OR i.status = ?1) AND (?2 IS NULL OR i.channel = ?2)",
    )?;
    let rows = statement.query_map(params![status_filter, channel_filter], |row| {
        let size = row
            .get::<_, Option<i64>>(3)?
            .map(|value| {
                u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, value))
            })
            .transpose()?;
        let new_work = row.get::<_, Option<bool>>(10)?;
        let status = row.get::<_, String>(9)?;
        let error = row
            .get::<_, Option<String>>(13)?
            .map(|code| IngestionErrorView {
                code,
                message: if status == "failed" {
                    "source delivery failed"
                } else {
                    "source delivery processing encountered a retryable error"
                }
                .to_owned(),
            });
        Ok((
            row.get::<_, i64>(0)?,
            IngestionView {
                source_name: row.get(1)?,
                channel: row.get(2)?,
                size_bytes: size,
                source_created_at: row.get(4)?,
                source_modified_at: row.get(5)?,
                first_seen_at: row.get(6)?,
                ingested_at: row.get(7)?,
                completed_at: row.get(8)?,
                status,
                retention: new_work
                    .map(|is_new| if is_new { "new" } else { "duplicate" }.to_owned()),
                result: row.get(11)?,
                applied_revision: row.get(12)?,
                error,
                work: row.get(14)?,
                sha256: row.get(15)?,
            },
        ))
    })?;

    let mut missing_time_count = 0;
    let mut selected = Vec::new();
    for row in rows {
        let (id, view) = row?;
        let Some(timestamp) = view.selected_timestamp(args.by) else {
            missing_time_count += 1;
            continue;
        };
        let parsed = parse_stored_timestamp(timestamp)?;
        if parsed >= window.since && parsed < window.until {
            selected.push((parsed, id, view));
        }
    }
    selected.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let deliveries = selected
        .into_iter()
        .map(|(_, _, view)| view)
        .collect::<Vec<_>>();
    let processing_count = count_status(&deliveries, "processing");
    let completed_count = count_status(&deliveries, "completed");
    let failed_count = count_status(&deliveries, "failed");
    let new_work_count = count_retention(&deliveries, "new");
    let duplicate_count = count_retention(&deliveries, "duplicate");
    Ok(LatelyReport {
        since: window.since_text()?,
        until: window.until_text()?,
        time_basis: args.by.as_str().to_owned(),
        status: status_filter.map(str::to_owned),
        channel: channel_filter.map(str::to_owned),
        delivery_count: deliveries.len(),
        processing_count,
        completed_count,
        failed_count,
        new_work_count,
        duplicate_count,
        missing_time_count,
        deliveries,
    })
}

fn count_status(deliveries: &[IngestionView], status: &str) -> usize {
    deliveries
        .iter()
        .filter(|delivery| delivery.status == status)
        .count()
}

fn count_retention(deliveries: &[IngestionView], retention: &str) -> usize {
    deliveries
        .iter()
        .filter(|delivery| delivery.retention.as_deref() == Some(retention))
        .count()
}

pub(crate) fn resolve_window(
    args: &LatelyArgs,
    now: OffsetDateTime,
) -> Result<ResolvedWindow, AppError> {
    let until = args
        .until
        .as_deref()
        .map(parse_absolute_time)
        .transpose()?
        .unwrap_or(now)
        .to_offset(UtcOffset::UTC);
    let since = if let Some(duration) = parse_relative_duration(&args.since)? {
        until.checked_sub(duration).ok_or_else(|| {
            AppError::invalid(
                "invalid_time",
                "relative start is outside the supported range",
            )
        })?
    } else {
        parse_absolute_time(&args.since)?
    };
    if since >= until {
        return Err(AppError::invalid(
            "invalid_time_range",
            "the start of the time window must be before its end",
        ));
    }
    Ok(ResolvedWindow { since, until })
}

fn parse_relative_duration(value: &str) -> Result<Option<Duration>, AppError> {
    let Some(unit) = value.chars().last() else {
        return Err(invalid_time(value));
    };
    if !matches!(unit, 's' | 'm' | 'h' | 'd' | 'w') {
        return Ok(None);
    }
    let digits = &value[..value.len() - unit.len_utf8()];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_time(value));
    }
    let amount = digits.parse::<u64>().map_err(|_| invalid_time(value))?;
    if amount == 0 {
        return Err(invalid_time(value));
    }
    let seconds_per_unit = match unit {
        's' => 1,
        'm' => 60,
        'h' => 60 * 60,
        'd' => 24 * 60 * 60,
        'w' => 7 * 24 * 60 * 60,
        _ => unreachable!(),
    };
    let seconds = amount
        .checked_mul(seconds_per_unit)
        .and_then(|seconds| i64::try_from(seconds).ok())
        .ok_or_else(|| invalid_time(value))?;
    Ok(Some(Duration::seconds(seconds)))
}

fn parse_absolute_time(value: &str) -> Result<OffsetDateTime, AppError> {
    if let Ok(timestamp) = OffsetDateTime::parse(value, &Rfc3339) {
        return Ok(timestamp.to_offset(UtcOffset::UTC));
    }
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..].iter().all(u8::is_ascii_digit)
    {
        return Err(invalid_time(value));
    }
    let year = value[..4].parse::<i32>().map_err(|_| invalid_time(value))?;
    let month = value[5..7].parse::<u8>().map_err(|_| invalid_time(value))?;
    let day = value[8..].parse::<u8>().map_err(|_| invalid_time(value))?;
    let month = Month::try_from(month).map_err(|_| invalid_time(value))?;
    let date = Date::from_calendar_date(year, month, day).map_err(|_| invalid_time(value))?;
    Ok(date.with_time(Time::MIDNIGHT).assume_utc())
}

fn parse_stored_timestamp(value: &str) -> Result<OffsetDateTime, AppError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|timestamp| timestamp.to_offset(UtcOffset::UTC))
        .map_err(|error| {
            AppError::database(
                "invalid_ingestion_timestamp",
                format!("source delivery has an invalid timestamp {value:?}: {error}"),
            )
        })
}

fn invalid_time(value: &str) -> AppError {
    AppError::invalid(
        "invalid_time",
        format!(
            "invalid time {value:?}; use an RFC 3339 timestamp, a UTC date, or a positive duration such as 24h or 7d for --since"
        ),
    )
}

pub(crate) fn format_system_time(value: SystemTime) -> Result<String, AppError> {
    format_timestamp(OffsetDateTime::from(value))
}

fn format_timestamp(value: OffsetDateTime) -> Result<String, AppError> {
    value
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|error| AppError::unexpected("timestamp_failed", error.to_string()))
}

impl LatelyTime {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Modified => "modified",
            Self::FirstSeen => "first-seen",
            Self::Ingested => "ingested",
            Self::Completed => "completed",
        }
    }

    #[must_use]
    pub(crate) const fn display(self) -> &'static str {
        match self {
            Self::Created => "Source created",
            Self::Modified => "Source modified",
            Self::FirstSeen => "First seen",
            Self::Ingested => "Ingested",
            Self::Completed => "Completed",
        }
    }
}

impl IngestionStatus {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl IngestionChannel {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Inbox => "inbox",
        }
    }
}

#[cfg(test)]
mod tests {
    use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

    use super::resolve_window;
    use crate::cli::{LatelyArgs, LatelyTime};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn args(since: &str, until: Option<&str>) -> LatelyArgs {
        LatelyArgs {
            since: since.to_owned(),
            until: until.map(str::to_owned),
            by: LatelyTime::Ingested,
            status: None,
            channel: None,
        }
    }

    #[test]
    fn relative_and_absolute_windows_are_resolved_in_utc() -> TestResult {
        let now = OffsetDateTime::parse("2026-08-20T12:00:00Z", &Rfc3339)?;
        let relative = resolve_window(&args("7d", None), now)?;
        assert_eq!(relative.until - relative.since, Duration::days(7));

        let absolute = resolve_window(&args("2026-08-01", Some("2026-08-02T01:00:00+01:00")), now)?;
        assert_eq!(absolute.until - absolute.since, Duration::days(1));
        assert_eq!(absolute.since_text()?, "2026-08-01T00:00:00Z");
        assert_eq!(absolute.until_text()?, "2026-08-02T00:00:00Z");

        let anchored = resolve_window(&args("24h", Some("2026-08-10T12:00:00-05:00")), now)?;
        assert_eq!(anchored.since_text()?, "2026-08-09T17:00:00Z");
        assert_eq!(anchored.until_text()?, "2026-08-10T17:00:00Z");
        Ok(())
    }

    #[test]
    fn invalid_windows_are_rejected() -> TestResult {
        let now = OffsetDateTime::parse("2026-08-20T12:00:00Z", &Rfc3339)?;
        assert_eq!(
            resolve_window(&args("0d", None), now)
                .err()
                .map(|error| error.code()),
            Some("invalid_time")
        );
        for invalid in ["not-a-time", "2026-8-20", "18446744073709551616w"] {
            assert_eq!(
                resolve_window(&args(invalid, None), now)
                    .err()
                    .map(|error| error.code()),
                Some("invalid_time")
            );
        }
        assert_eq!(
            resolve_window(
                &args("2026-08-20T12:00:00Z", Some("2026-08-20T12:00:00Z")),
                now,
            )
            .err()
            .map(|error| error.code()),
            Some("invalid_time_range")
        );
        Ok(())
    }
}
