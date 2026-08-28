use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::config::UsageConfig;
use crate::database::{StoredQuotaSnapshot, StoredRun, UsageDatabase, now_millis};
use crate::types::TokenUsageBreakdown;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConsumptionReport {
    generated_at: String,
    usage_database: String,
    deliveries: Vec<DeliveryReport>,
    unattributed_runs: Vec<StoredRun>,
    latest_budget_snapshot: Option<StoredQuotaSnapshot>,
    notes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeliveryReport {
    delivery_id: i64,
    source_name: String,
    delivery_status: String,
    result: Option<String>,
    work_id: Option<i64>,
    reconciliation_id: Option<i64>,
    selected_model_run_id: Option<i64>,
    coverage: String,
    incremental_usage: Option<TokenUsageBreakdown>,
    attempts: Vec<StoredRun>,
    known_credit_equivalent: Option<f64>,
    unpriced_cache_write_tokens: i64,
}

#[derive(Debug)]
struct DeliveryRecord {
    id: i64,
    source_name: String,
    status: String,
    result: Option<String>,
    work_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReceiptSummary {
    ingestion_id: Option<i64>,
    model_run_token: Option<String>,
    reconciliation_id: Option<i64>,
    result_status: Option<String>,
}

pub(crate) fn build_report(
    config: &UsageConfig,
    database: &UsageDatabase,
    limit: usize,
) -> Result<ConsumptionReport, ReportError> {
    let library = Connection::open_with_flags(
        &config.library,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let deliveries = read_deliveries(&library, limit)?;
    let runs = database.runs(usize::MAX)?;
    let mut runs_by_delivery: HashMap<i64, Vec<StoredRun>> = HashMap::new();
    let mut unattributed = Vec::new();
    for run in runs {
        if let Some(delivery_id) = run.delivery_id {
            runs_by_delivery.entry(delivery_id).or_default().push(run);
        } else if unattributed.len() < limit {
            unattributed.push(run);
        }
    }
    let receipts = read_receipts(&config.spool)?;
    let mut reports = Vec::with_capacity(deliveries.len());
    for delivery in deliveries {
        let attempts = runs_by_delivery.remove(&delivery.id).unwrap_or_default();
        let receipt = receipts.get(&delivery.id);
        let reconciliation_id = receipt.and_then(|receipt| receipt.reconciliation_id);
        let selected_model_run_id = selected_model_run_id(&library, reconciliation_id)?;
        let (coverage, usage) = delivery_usage(&library, &delivery, receipt, &attempts)?;
        let (known_credit_equivalent, unpriced_cache_write_tokens) =
            if matches!(coverage.as_str(), "exact" | "cumulative") {
                credit_equivalent(&attempts)
            } else {
                (None, 0)
            };
        reports.push(DeliveryReport {
            delivery_id: delivery.id,
            source_name: delivery.source_name,
            delivery_status: delivery.status,
            result: delivery.result,
            work_id: delivery.work_id,
            reconciliation_id,
            selected_model_run_id,
            coverage,
            incremental_usage: usage,
            attempts,
            known_credit_equivalent,
            unpriced_cache_write_tokens,
        });
    }

    Ok(ConsumptionReport {
        generated_at: format_millis(now_millis()?),
        usage_database: config.database.display().to_string(),
        deliveries: reports,
        unattributed_runs: unattributed,
        latest_budget_snapshot: database.latest_account_snapshot()?,
        notes: vec![
            "cached and cache-write tokens are subsets of input; reasoning tokens are a subset of output",
            "known credit-equivalent excludes cache-write tokens because the ChatGPT rate card does not price them separately",
            "subscription quota snapshots are account-global and are not uniquely attributable to one delivery",
        ],
    })
}

pub(crate) fn print_human(report: &ConsumptionReport) {
    println!("Annals consumption report");
    println!("Generated: {}", report.generated_at);
    println!("Ledger:    {}", report.usage_database);
    if report.deliveries.is_empty() {
        println!("\nNo delivery records found.");
    }
    for delivery in &report.deliveries {
        println!();
        println!(
            "Delivery {}  {}  [{} / {}]",
            delivery.delivery_id,
            delivery.source_name,
            delivery.delivery_status,
            delivery.result.as_deref().unwrap_or("no result")
        );
        println!(
            "  Coverage: {}  Attempts: {}",
            delivery.coverage,
            delivery.attempts.len()
        );
        if let Some(model_run_id) = delivery.selected_model_run_id {
            println!("  Selected examination: model run {model_run_id}");
        }
        if let Some(usage) = delivery.incremental_usage {
            println!("  Total:       {}", grouped(usage.total_tokens));
            println!("  Input:       {}", grouped(usage.input_tokens));
            println!(
                "    ordinary:  {}",
                optional_grouped(usage.ordinary_input_tokens())
            );
            println!("    cached:    {}", grouped(usage.cached_input_tokens));
            println!(
                "    cache write: {}",
                grouped(usage.cache_write_input_tokens)
            );
            println!("  Output:      {}", grouped(usage.output_tokens));
            println!("    reasoning: {}", grouped(usage.reasoning_output_tokens));
        }
        if let Some(credits) = delivery.known_credit_equivalent {
            println!("  Known credit-equivalent: {credits:.3}");
        }
        if delivery.unpriced_cache_write_tokens > 0 {
            println!(
                "  Unpriced cache-write tokens: {}",
                grouped(delivery.unpriced_cache_write_tokens)
            );
        }
        for attempt in &delivery.attempts {
            print_run(attempt, "  ");
        }
    }
    if !report.unattributed_runs.is_empty() {
        println!();
        println!(
            "Examinations without a source delivery: {}",
            report.unattributed_runs.len()
        );
        for run in &report.unattributed_runs {
            print_run(run, "  ");
        }
    }
    println!();
    for note in &report.notes {
        println!("Note: {note}");
    }
}

fn print_run(run: &StoredRun, indent: &str) {
    println!(
        "{indent}Run {}: {} / {} / {} / {} responses",
        run.annals_model_run_id.unwrap_or(run.id),
        run.model.as_deref().unwrap_or("unknown model"),
        run.status,
        run.coverage,
        run.response_count
    );
    if let Some(usage) = run.usage {
        println!(
            "{indent}  tokens: total {}; input {} (cached {}, cache write {}); output {} (reasoning {})",
            grouped(usage.total_tokens),
            grouped(usage.input_tokens),
            grouped(usage.cached_input_tokens),
            grouped(usage.cache_write_input_tokens),
            grouped(usage.output_tokens),
            grouped(usage.reasoning_output_tokens)
        );
    }
    if let Some(error) = &run.error {
        println!("{indent}  telemetry warning: {error}");
    }
}

fn read_deliveries(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<DeliveryRecord>, ReportError> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(
        "SELECT id, source_name, status, result, work_id \
         FROM ingestions ORDER BY id DESC LIMIT ?1",
    )?;
    let records = statement.query_map([limit], |row| {
        Ok(DeliveryRecord {
            id: row.get(0)?,
            source_name: row.get(1)?,
            status: row.get(2)?,
            result: row.get(3)?,
            work_id: row.get(4)?,
        })
    })?;
    records.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_receipts(spool: &Path) -> Result<HashMap<i64, ReceiptSummary>, ReportError> {
    let mut receipts = HashMap::new();
    for state in ["processing", "done", "duplicates", "failed", "skipped"] {
        let directory = spool.join(state);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(ReportError::ReadDirectory { directory, source }),
        };
        for entry in entries {
            let path = entry?.path().join("job.json");
            let document = match fs::read_to_string(&path) {
                Ok(document) => document,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(ReportError::ReadReceipt { path, source }),
            };
            let receipt: ReceiptSummary = serde_json::from_str(&document)?;
            if let Some(delivery_id) = receipt.ingestion_id {
                receipts.insert(delivery_id, receipt);
            }
        }
    }
    Ok(receipts)
}

fn delivery_usage(
    library: &Connection,
    delivery: &DeliveryRecord,
    receipt: Option<&ReceiptSummary>,
    attempts: &[StoredRun],
) -> Result<(String, Option<TokenUsageBreakdown>), ReportError> {
    if delivery.status == "processing" {
        return Ok(("pending".to_owned(), None));
    }
    if !attempts.is_empty() {
        let mut aggregate = TokenUsageBreakdown::default();
        let mut coverage = "exact";
        for attempt in attempts {
            if attempt.status == "running" {
                return Ok(("gap".to_owned(), None));
            }
            let Some(usage) = attempt.usage else {
                return Ok(("gap".to_owned(), None));
            };
            if !matches!(attempt.coverage.as_str(), "exact" | "cumulative") {
                return Ok(("gap".to_owned(), None));
            }
            add_usage(&mut aggregate, usage)?;
            if attempt.coverage == "cumulative" && coverage == "exact" {
                coverage = "cumulative";
            } else if attempt.coverage != "exact" && attempt.coverage != "cumulative" {
                coverage = "gap";
            }
        }
        return Ok((coverage.to_owned(), Some(aggregate)));
    }
    if delivery.result.as_deref() == Some("retained")
        || (delivery.status == "failed" && delivery.work_id.is_none())
    {
        return Ok(("no-model".to_owned(), Some(TokenUsageBreakdown::default())));
    }
    if let Some(receipt) = receipt
        && receipt.reconciliation_id.is_some()
        && let Some(token) = receipt.model_run_token.as_deref()
    {
        let exists = library.query_row(
            "SELECT EXISTS(SELECT 1 FROM model_runs WHERE token = ?1)",
            [token],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Ok((
                "reused-no-new-usage".to_owned(),
                Some(TokenUsageBreakdown::default()),
            ));
        }
    }
    if receipt.and_then(|receipt| receipt.result_status.as_deref()) == Some("retained") {
        return Ok(("no-model".to_owned(), Some(TokenUsageBreakdown::default())));
    }
    Ok(("legacy-unobserved".to_owned(), None))
}

fn selected_model_run_id(
    library: &Connection,
    reconciliation_id: Option<i64>,
) -> Result<Option<i64>, ReportError> {
    let Some(reconciliation_id) = reconciliation_id else {
        return Ok(None);
    };
    library
        .query_row(
            "SELECT model_run_id FROM reconciliations WHERE id = ?1",
            [reconciliation_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn add_usage(
    aggregate: &mut TokenUsageBreakdown,
    usage: TokenUsageBreakdown,
) -> Result<(), ReportError> {
    aggregate.input_tokens = checked_add(aggregate.input_tokens, usage.input_tokens)?;
    aggregate.cached_input_tokens =
        checked_add(aggregate.cached_input_tokens, usage.cached_input_tokens)?;
    aggregate.cache_write_input_tokens = checked_add(
        aggregate.cache_write_input_tokens,
        usage.cache_write_input_tokens,
    )?;
    aggregate.output_tokens = checked_add(aggregate.output_tokens, usage.output_tokens)?;
    aggregate.reasoning_output_tokens = checked_add(
        aggregate.reasoning_output_tokens,
        usage.reasoning_output_tokens,
    )?;
    aggregate.total_tokens = checked_add(aggregate.total_tokens, usage.total_tokens)?;
    Ok(())
}

fn checked_add(left: i64, right: i64) -> Result<i64, ReportError> {
    left.checked_add(right).ok_or(ReportError::TokenOverflow)
}

#[allow(clippy::cast_precision_loss)]
fn credit_equivalent(attempts: &[StoredRun]) -> (Option<f64>, i64) {
    let mut credits = 0.0;
    let mut cache_writes = 0_i64;
    for attempt in attempts {
        let (Some(model), Some(usage)) = (attempt.model.as_deref(), attempt.usage) else {
            return (None, cache_writes);
        };
        let Some((input_rate, cached_rate, output_rate)) = credit_rates(model) else {
            return (None, cache_writes);
        };
        let Some(ordinary) = usage.ordinary_input_tokens() else {
            return (None, cache_writes);
        };
        credits += (ordinary as f64).mul_add(
            input_rate / 1_000_000.0,
            (usage.cached_input_tokens as f64).mul_add(
                cached_rate / 1_000_000.0,
                usage.output_tokens as f64 * output_rate / 1_000_000.0,
            ),
        );
        cache_writes = cache_writes.saturating_add(usage.cache_write_input_tokens);
    }
    if attempts.is_empty() {
        (None, cache_writes)
    } else {
        (Some(credits), cache_writes)
    }
}

fn credit_rates(model: &str) -> Option<(f64, f64, f64)> {
    match model {
        "gpt-5.6-sol" | "daybreak-blue" => Some((100.0, 10.0, 500.0)),
        "daybreak-red" => Some((312.5, 31.25, 1_875.0)),
        "gpt-5.6-terra" => Some((50.0, 5.0, 300.0)),
        "gpt-5.6-luna" => Some((5.0, 0.5, 30.0)),
        "gpt-5.5" => Some((125.0, 12.5, 750.0)),
        "gpt-5.4" => Some((62.5, 6.25, 375.0)),
        "gpt-5.4-mini" => Some((18.75, 1.875, 113.0)),
        _ => None,
    }
}

fn format_millis(millis: i64) -> String {
    let nanos = i128::from(millis) * 1_000_000;
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp_nanos(nanos) else {
        return millis.to_string();
    };
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| millis.to_string())
}

fn grouped(value: i64) -> String {
    let text = value.to_string();
    let (sign, digits) = text
        .strip_prefix('-')
        .map_or(("", text.as_str()), |digits| ("-", digits));
    let mut result = String::with_capacity(text.len() + text.len() / 3);
    result.push_str(sign);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

fn optional_grouped(value: Option<i64>) -> String {
    value.map_or_else(|| "invalid".to_owned(), grouped)
}

#[derive(Debug, Error)]
pub(crate) enum ReportError {
    #[error(transparent)]
    Database(#[from] crate::database::DatabaseError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unable to read inbox directory {directory}: {source}")]
    ReadDirectory {
        directory: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to read job receipt {path}: {source}")]
    ReadReceipt {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("token total overflow while aggregating attempts")]
    TokenOverflow,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{grouped, read_receipts};

    #[test]
    fn groups_integer_digits() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(grouped(-12_345), "-12,345");
    }

    #[test]
    fn reads_receipts_from_the_skipped_archive() -> Result<(), Box<dyn std::error::Error>> {
        let spool = tempfile::tempdir()?;
        let envelope = spool.path().join("skipped/job-1");
        fs::create_dir_all(&envelope)?;
        fs::write(
            envelope.join("job.json"),
            r#"{"ingestion_id":42,"model_run_token":"run-token"}"#,
        )?;

        let receipts = read_receipts(spool.path())?;
        let receipt = receipts
            .get(&42)
            .ok_or("skipped receipt was not discovered")?;
        assert_eq!(receipt.model_run_token.as_deref(), Some("run-token"));
        Ok(())
    }
}
