use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use nucleus_core::{
    AttemptOutputV1, AttemptState, JobState, JobV1, LogRecordV1, LogStream, ReasoningEffort,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::config::UsageConfig;
use crate::protocol::{ProtocolEvent, decode_output};
use crate::types::{ThreadTokenUsage, TokenUsageBreakdown};

#[derive(Debug)]
pub(crate) struct NucleusObservation {
    pub(crate) job: JobV1,
    pub(crate) records: Vec<LogRecordV1>,
}

pub(crate) struct ReportScope {
    delivery_tokens: HashSet<String>,
    attributed_tokens: HashSet<String>,
    unattributed_limit: usize,
}

impl ReportScope {
    pub(crate) fn load(config: &UsageConfig, limit: usize) -> Result<Self, ReportError> {
        let library = Connection::open_with_flags(
            &config.library,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let delivery_ids = read_deliveries(&library, limit)?
            .into_iter()
            .map(|delivery| delivery.id)
            .collect::<HashSet<_>>();
        let receipts = read_receipts(&config.spool)?;
        let delivery_tokens = receipts
            .by_token
            .iter()
            .filter(|(_, receipt)| {
                receipt
                    .ingestion_id
                    .is_some_and(|id| delivery_ids.contains(&id))
            })
            .map(|(token, _)| token.clone())
            .collect();
        let attributed_tokens = receipts
            .by_token
            .iter()
            .filter(|(_, receipt)| receipt.ingestion_id.is_some())
            .map(|(token, _)| token.clone())
            .collect();
        Ok(Self {
            delivery_tokens,
            attributed_tokens,
            unattributed_limit: limit,
        })
    }

    pub(crate) fn includes_delivery(&self, token: &str) -> bool {
        self.delivery_tokens.contains(token)
    }

    pub(crate) fn is_unattributed(&self, token: &str) -> bool {
        !self.attributed_tokens.contains(token)
    }

    pub(crate) const fn unattributed_limit(&self) -> usize {
        self.unattributed_limit
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConsumptionReport {
    generated_at: String,
    projection_version: &'static str,
    deliveries: Vec<DeliveryReport>,
    unattributed_runs: Vec<RunReport>,
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
    attempts: Vec<RunReport>,
    known_credit_equivalent: Option<f64>,
    unpriced_cache_write_tokens: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunReport {
    job_id: String,
    model_run_token: String,
    annals_model_run_id: Option<i64>,
    delivery_id: Option<i64>,
    inbox_job_id: Option<String>,
    inbox_attempt: Option<u32>,
    work_id: Option<i64>,
    work_label: Option<String>,
    base_revision: Option<i64>,
    model: String,
    reasoning_effort: Option<String>,
    codex_version: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    status: String,
    coverage: String,
    started_at_ms: i64,
    completed_at_ms: Option<i64>,
    usage: Option<TokenUsageBreakdown>,
    model_context_window: Option<i64>,
    exact_response_stream_complete: bool,
    error: Option<String>,
    response_count: usize,
    responses: Vec<ResponseUsage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseUsage {
    sequence: u64,
    observed_at_ms: i64,
    response_id: String,
    thread_id: String,
    turn_id: String,
    usage: TokenUsageBreakdown,
}

#[derive(Debug)]
struct DeliveryRecord {
    id: i64,
    source_name: String,
    status: String,
    result: Option<String>,
    work_id: Option<i64>,
}

#[derive(Debug)]
struct ModelRunIdentity {
    id: i64,
    work_id: i64,
    work_label: String,
    base_revision: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ReceiptSummary {
    id: String,
    #[serde(default)]
    attempts: u32,
    ingestion_id: Option<i64>,
    model_run_token: Option<String>,
    reconciliation_id: Option<i64>,
    result_status: Option<String>,
}

#[derive(Debug, Default)]
struct Receipts {
    by_token: HashMap<String, ReceiptSummary>,
    by_delivery: HashMap<i64, ReceiptSummary>,
}

pub(crate) fn build_report(
    config: &UsageConfig,
    observations: Vec<NucleusObservation>,
    limit: usize,
) -> Result<ConsumptionReport, ReportError> {
    let library = Connection::open_with_flags(
        &config.library,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let deliveries = read_deliveries(&library, limit)?;
    let identities = read_model_run_identities(&library)?;
    let receipts = read_receipts(&config.spool)?;
    let mut runs_by_delivery: HashMap<i64, Vec<RunReport>> = HashMap::new();
    let mut unattributed = Vec::new();
    for observation in observations {
        let run = project_run(observation, &identities, &receipts)?;
        if let Some(delivery_id) = run.delivery_id {
            runs_by_delivery.entry(delivery_id).or_default().push(run);
        } else if unattributed.len() < limit {
            unattributed.push(run);
        }
    }

    let mut reports = Vec::with_capacity(deliveries.len());
    for delivery in deliveries {
        let attempts = runs_by_delivery.remove(&delivery.id).unwrap_or_default();
        let receipt = receipts.by_delivery.get(&delivery.id);
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
        generated_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unavailable".to_owned()),
        projection_version: env!("CARGO_PKG_VERSION"),
        deliveries: reports,
        unattributed_runs: unattributed,
        notes: vec![
            "this report is calculated live from Nucleus model-output records and Annals attribution",
            "cached and cache-write tokens are subsets of input; reasoning tokens are a subset of output",
            "known credit-equivalent excludes cache-write tokens because the rate card does not price them separately",
        ],
    })
}

pub(crate) fn print_human(report: &ConsumptionReport) {
    println!("Annals consumption report");
    println!("Generated: {}", report.generated_at);
    println!("Projection: annals-usage {}", report.projection_version);
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
            print_usage(usage, "  ");
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

fn print_usage(usage: TokenUsageBreakdown, indent: &str) {
    println!("{indent}Total:       {}", grouped(usage.total_tokens));
    println!("{indent}Input:       {}", grouped(usage.input_tokens));
    println!(
        "{indent}  ordinary:  {}",
        optional_grouped(usage.ordinary_input_tokens())
    );
    println!(
        "{indent}  cached:    {}",
        grouped(usage.cached_input_tokens)
    );
    println!(
        "{indent}  cache write: {}",
        grouped(usage.cache_write_input_tokens)
    );
    println!("{indent}Output:      {}", grouped(usage.output_tokens));
    println!(
        "{indent}  reasoning: {}",
        grouped(usage.reasoning_output_tokens)
    );
}

fn print_run(run: &RunReport, indent: &str) {
    println!(
        "{indent}Run {}: {} / {} / {} / {} responses",
        run.annals_model_run_id
            .map_or(run.model_run_token.as_str().to_owned(), |id| id.to_string()),
        run.model,
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

fn project_run(
    observation: NucleusObservation,
    identities: &HashMap<String, ModelRunIdentity>,
    receipts: &Receipts,
) -> Result<RunReport, ReportError> {
    let job = observation.job;
    let token = job.summary.requester.id.clone();
    let identity = identities.get(&token);
    let receipt = receipts.by_token.get(&token);
    let attempt = job.attempts.last();
    let started_at = attempt.map_or(job.summary.created_at.as_str(), |attempt| {
        attempt.started_at.as_deref().unwrap_or(&attempt.created_at)
    });
    let completed_at = attempt
        .and_then(|attempt| attempt.completed_at.as_deref())
        .or(job.summary.completed_at.as_deref());
    let attempt_output = attempt
        .filter(|attempt| attempt.state == AttemptState::Completed)
        .and_then(|attempt| attempt.output.as_ref());
    let reduction = reduce_records(&observation.records, job.summary.state, attempt_output)?;
    let status = reduction
        .turn_status
        .unwrap_or_else(|| job_status(job.summary.state).to_owned());
    Ok(RunReport {
        job_id: job.summary.id.to_string(),
        model_run_token: token,
        annals_model_run_id: identity.map(|identity| identity.id),
        delivery_id: receipt.and_then(|receipt| receipt.ingestion_id),
        inbox_job_id: receipt.map(|receipt| receipt.id.clone()),
        inbox_attempt: receipt.map(|receipt| receipt.attempts),
        work_id: identity.map(|identity| identity.work_id),
        work_label: identity.map(|identity| identity.work_label.clone()),
        base_revision: identity.map(|identity| identity.base_revision),
        model: job.request.invocation.model.to_string(),
        reasoning_effort: job
            .request
            .invocation
            .reasoning_effort
            .map(reasoning_effort),
        codex_version: attempt.map(|attempt| attempt.harness.harness_version.clone()),
        thread_id: reduction.thread_id,
        turn_id: reduction.turn_id,
        status,
        coverage: reduction.coverage,
        started_at_ms: parse_timestamp(started_at)?,
        completed_at_ms: completed_at.map(parse_timestamp).transpose()?,
        usage: reduction.usage,
        model_context_window: reduction.model_context_window,
        exact_response_stream_complete: reduction.exact_stream_complete,
        error: (!reduction.warnings.is_empty()).then(|| reduction.warnings.join("; ")),
        response_count: reduction.responses.len(),
        responses: reduction.responses,
    })
}

fn reasoning_effort(effort: ReasoningEffort) -> String {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Max => "max",
    }
    .to_owned()
}

fn job_status(state: JobState) -> &'static str {
    match state {
        JobState::Accepted => "accepted",
        JobState::Running => "running",
        JobState::WaitingOnRequester => "waiting_on_requester",
        JobState::Completed => "completed",
        JobState::Failed => "nucleus-job-failed",
        JobState::Cancelled => "nucleus-job-cancelled",
    }
}

struct Reduction {
    thread_id: Option<String>,
    turn_id: Option<String>,
    turn_status: Option<String>,
    coverage: String,
    usage: Option<TokenUsageBreakdown>,
    model_context_window: Option<i64>,
    exact_stream_complete: bool,
    warnings: Vec<String>,
    responses: Vec<ResponseUsage>,
}

#[derive(Default)]
struct Reducer {
    thread_id: Option<String>,
    turn_id: Option<String>,
    turn_status: Option<String>,
    cumulative: Option<ThreadTokenUsage>,
    exact_stream_complete: bool,
    coverage_gap: bool,
    frozen: bool,
    warnings: Vec<String>,
    response_indexes: HashMap<String, usize>,
    responses: Vec<ResponseUsage>,
}

impl Reducer {
    fn new(output: Option<&AttemptOutputV1>) -> Self {
        let (thread_id, turn_id) = output.map_or((None, None), |output| {
            (Some(output.thread_id.clone()), Some(output.turn_id.clone()))
        });
        Self {
            thread_id,
            turn_id,
            exact_stream_complete: true,
            ..Self::default()
        }
    }

    fn observe(&mut self, record: &LogRecordV1) -> Result<(), ReportError> {
        if self.frozen || self.thread_id.is_none() {
            return Ok(());
        }
        if record.stream != LogStream::HarnessOutput {
            self.exact_gap("Nucleus returned a non-output reporting record");
            return Ok(());
        }
        if !record
            .schema_id
            .as_str()
            .starts_with("codex.app-server.protocol.")
        {
            self.exact_gap(format!(
                "Nucleus retained a Codex output record under incompatible schema {}",
                record.schema_id
            ));
            return Ok(());
        }
        let message = match serde_json::from_str::<Value>(record.payload.get()) {
            Ok(message) => message,
            Err(error) => {
                self.exact_gap(format!(
                    "Nucleus retained an undecodable Codex record: {error}"
                ));
                return Ok(());
            }
        };
        let method = message.get("method").and_then(Value::as_str);
        if matches!(
            method,
            Some("thread/tokenUsage/updated" | "rawResponse/completed" | "turn/completed")
        ) {
            let Some((thread_id, turn_id)) = message_turn_ids(&message, method.unwrap_or_default())
            else {
                if method == Some("turn/completed") {
                    self.force_gap("the authoritative turn-completion output was malformed");
                    self.frozen = true;
                } else {
                    self.exact_gap(format!(
                        "Nucleus retained an undecodable {} event",
                        method.unwrap_or("Codex")
                    ));
                }
                return Ok(());
            };
            if !self.matches_turn(thread_id, turn_id) {
                return Ok(());
            }
        }
        let event = decode_output(&message);
        if event.is_none()
            && matches!(
                method,
                Some("thread/tokenUsage/updated" | "rawResponse/completed" | "turn/completed")
            )
        {
            if method == Some("turn/completed") {
                self.force_gap("the authoritative turn-completion output was malformed");
                self.frozen = true;
            } else {
                self.exact_gap(format!(
                    "Nucleus retained an undecodable {} event",
                    method.unwrap_or("Codex")
                ));
            }
        }
        if let Some(event) = event {
            self.observe_event(event, record)?;
        }
        Ok(())
    }

    fn observe_event(
        &mut self,
        event: ProtocolEvent,
        record: &LogRecordV1,
    ) -> Result<(), ReportError> {
        match event {
            ProtocolEvent::TokenUsageUpdated {
                thread_id,
                turn_id,
                usage,
            } => {
                if !self.matches_turn(&thread_id, &turn_id) {
                    return Ok(());
                }
                if !usage.last.is_consistent() || !usage.total.is_consistent() {
                    self.cumulative = None;
                    self.force_gap("an upstream cumulative token snapshot was inconsistent");
                } else if !usage_is_componentwise_at_most(usage.last, usage.total) {
                    self.cumulative = None;
                    self.force_gap(
                        "an upstream cumulative token snapshot's last usage exceeded its total",
                    );
                } else if self.cumulative.as_ref().is_some_and(|previous| {
                    !usage_is_componentwise_at_most(previous.total, usage.total)
                }) {
                    self.cumulative = None;
                    self.force_gap("an upstream cumulative token total regressed");
                } else {
                    self.cumulative = Some(usage);
                }
            }
            ProtocolEvent::RawResponseCompleted {
                thread_id,
                turn_id,
                response_id,
                usage,
            } => {
                if !self.matches_turn(&thread_id, &turn_id) {
                    return Ok(());
                }
                let Some(usage) = usage else {
                    self.exact_gap("an upstream response omitted token usage");
                    return Ok(());
                };
                if !usage.is_consistent() {
                    self.exact_gap("an upstream response reported inconsistent token usage");
                    return Ok(());
                }
                if let Some(index) = self.response_indexes.get(&response_id).copied() {
                    let replay = &self.responses[index];
                    if replay.thread_id != thread_id
                        || replay.turn_id != turn_id
                        || replay.usage != usage
                    {
                        self.exact_gap(format!(
                            "response ID {response_id:?} was replayed with conflicting attribution or usage"
                        ));
                    }
                } else {
                    self.response_indexes
                        .insert(response_id.clone(), self.responses.len());
                    self.responses.push(ResponseUsage {
                        sequence: record.sequence,
                        observed_at_ms: parse_timestamp(&record.observed_at)?,
                        response_id,
                        thread_id,
                        turn_id,
                        usage,
                    });
                }
            }
            ProtocolEvent::TurnCompleted {
                thread_id,
                turn_id,
                status,
            } => {
                if !self.matches_turn(&thread_id, &turn_id) {
                    return Ok(());
                }
                if status != "completed" {
                    self.force_gap(format!(
                        "the authoritative turn-completion output had invalid status {status:?}"
                    ));
                    self.frozen = true;
                    return Ok(());
                }
                self.turn_status = Some(status);
                self.frozen = true;
            }
        }
        Ok(())
    }

    fn matches_turn(&self, thread_id: &str, turn_id: &str) -> bool {
        self.thread_id.as_deref() == Some(thread_id) && self.turn_id.as_deref() == Some(turn_id)
    }

    fn exact_gap(&mut self, warning: impl Into<String>) {
        self.exact_stream_complete = false;
        self.warnings.push(warning.into());
    }

    fn force_gap(&mut self, warning: impl Into<String>) {
        self.coverage_gap = true;
        self.exact_gap(warning);
    }

    fn finish(mut self, state: JobState) -> Reduction {
        let terminal = state.is_terminal();
        if state != JobState::Completed {
            self.turn_status = None;
        }
        let cumulative = self.cumulative.as_ref();
        let cumulative_usage = cumulative.map(|usage| usage.total);
        let model_context_window = cumulative.and_then(|usage| usage.model_context_window);
        let exact = sum_response_usage(&self.responses);
        if exact.is_none() {
            self.exact_gap("exact response token totals overflowed");
        }
        let (coverage, usage) = if !terminal {
            ("pending", cumulative_usage)
        } else if state != JobState::Completed {
            self.force_gap("the Nucleus job did not complete successfully");
            ("gap", None)
        } else if self.thread_id.is_none() {
            self.force_gap("the completed Nucleus attempt had no authoritative output");
            ("gap", None)
        } else if self.coverage_gap {
            ("gap", None)
        } else if self.turn_status.is_none() {
            self.exact_gap("the Nucleus job ended before a turn-completion output was observed");
            ("gap", cumulative_usage)
        } else if self.exact_stream_complete
            && !self.responses.is_empty()
            && exact
                .is_some_and(|exact| cumulative_usage.is_none_or(|cumulative| cumulative == exact))
        {
            ("exact", exact)
        } else if let Some(cumulative) = cumulative_usage {
            if self.responses.is_empty() && self.exact_stream_complete {
                self.exact_gap("no exact response usage events were observed");
            } else if self.exact_stream_complete {
                self.exact_gap("exact response totals differed from the final cumulative snapshot");
            }
            ("cumulative", Some(cumulative))
        } else {
            self.exact_gap("no usable terminal token telemetry was observed");
            ("gap", None)
        };
        Reduction {
            thread_id: self.thread_id,
            turn_id: self.turn_id,
            turn_status: self.turn_status,
            coverage: coverage.to_owned(),
            usage,
            model_context_window,
            exact_stream_complete: self.exact_stream_complete,
            warnings: self.warnings,
            responses: self.responses,
        }
    }
}

fn message_turn_ids<'a>(message: &'a Value, method: &str) -> Option<(&'a str, &'a str)> {
    let thread_id = message.pointer("/params/threadId")?.as_str()?;
    let turn_id = match method {
        "turn/completed" => message.pointer("/params/turn/id")?.as_str()?,
        _ => message.pointer("/params/turnId")?.as_str()?,
    };
    Some((thread_id, turn_id))
}

fn reduce_records(
    records: &[LogRecordV1],
    state: JobState,
    output: Option<&AttemptOutputV1>,
) -> Result<Reduction, ReportError> {
    let mut reducer = Reducer::new(output);
    for record in records {
        reducer.observe(record)?;
    }
    Ok(reducer.finish(state))
}

fn sum_response_usage(responses: &[ResponseUsage]) -> Option<TokenUsageBreakdown> {
    let mut total = TokenUsageBreakdown::default();
    for response in responses {
        total.input_tokens = total
            .input_tokens
            .checked_add(response.usage.input_tokens)?;
        total.cached_input_tokens = total
            .cached_input_tokens
            .checked_add(response.usage.cached_input_tokens)?;
        total.cache_write_input_tokens = total
            .cache_write_input_tokens
            .checked_add(response.usage.cache_write_input_tokens)?;
        total.output_tokens = total
            .output_tokens
            .checked_add(response.usage.output_tokens)?;
        total.reasoning_output_tokens = total
            .reasoning_output_tokens
            .checked_add(response.usage.reasoning_output_tokens)?;
        total.total_tokens = total
            .total_tokens
            .checked_add(response.usage.total_tokens)?;
    }
    Some(total)
}

fn usage_is_componentwise_at_most(usage: TokenUsageBreakdown, total: TokenUsageBreakdown) -> bool {
    usage.input_tokens <= total.input_tokens
        && usage.cached_input_tokens <= total.cached_input_tokens
        && usage.cache_write_input_tokens <= total.cache_write_input_tokens
        && usage.output_tokens <= total.output_tokens
        && usage.reasoning_output_tokens <= total.reasoning_output_tokens
        && usage.total_tokens <= total.total_tokens
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

fn read_model_run_identities(
    connection: &Connection,
) -> Result<HashMap<String, ModelRunIdentity>, ReportError> {
    let mut statement = connection.prepare(
        "SELECT m.token, m.id, m.work_id, w.label, m.base_revision \
         FROM model_runs AS m JOIN works AS w ON w.id = m.work_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ModelRunIdentity {
                id: row.get(1)?,
                work_id: row.get(2)?,
                work_label: row.get(3)?,
                base_revision: row.get(4)?,
            },
        ))
    })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

fn read_receipts(spool: &Path) -> Result<Receipts, ReportError> {
    let mut receipts = Receipts::default();
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
                receipts.by_delivery.insert(delivery_id, receipt.clone());
            }
            if let Some(token) = receipt.model_run_token.clone() {
                if let Some(existing) = receipts.by_token.get(&token)
                    && existing.id != receipt.id
                {
                    return Err(ReportError::DuplicateModelRunReceipt {
                        token,
                        first_job: existing.id.clone(),
                        second_job: receipt.id,
                    });
                }
                receipts.by_token.insert(token, receipt);
            }
        }
    }
    Ok(receipts)
}

fn delivery_usage(
    library: &Connection,
    delivery: &DeliveryRecord,
    receipt: Option<&ReceiptSummary>,
    attempts: &[RunReport],
) -> Result<(String, Option<TokenUsageBreakdown>), ReportError> {
    if delivery.status == "processing" {
        return Ok(("pending".to_owned(), None));
    }
    if !attempts.is_empty() {
        let mut aggregate = TokenUsageBreakdown::default();
        let mut coverage = "exact";
        for attempt in attempts {
            let Some(usage) = attempt.usage else {
                return Ok(("gap".to_owned(), None));
            };
            if !matches!(attempt.coverage.as_str(), "exact" | "cumulative") {
                return Ok(("gap".to_owned(), None));
            }
            if add_usage(&mut aggregate, usage).is_err() {
                return Ok(("gap".to_owned(), None));
            }
            if attempt.coverage == "cumulative" {
                coverage = "cumulative";
            }
        }
        return Ok((coverage.to_owned(), Some(aggregate)));
    }
    if delivery.result.as_deref() == Some("retained")
        || (delivery.status == "failed" && delivery.work_id.is_none())
        || receipt.and_then(|receipt| receipt.result_status.as_deref()) == Some("retained")
    {
        return Ok(("no-model".to_owned(), Some(TokenUsageBreakdown::default())));
    }
    if let Some(receipt) = receipt
        && receipt.reconciliation_id.is_some()
    {
        let Some(token) = receipt.model_run_token.as_deref() else {
            return Ok((
                "reused-no-new-usage".to_owned(),
                Some(TokenUsageBreakdown::default()),
            ));
        };
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
    Ok(("gap".to_owned(), None))
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
        .optional()
        .map(Option::flatten)
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
fn credit_equivalent(attempts: &[RunReport]) -> (Option<f64>, i64) {
    let mut credits = 0.0;
    let mut cache_writes = 0_i64;
    for attempt in attempts {
        let Some(usage) = attempt.usage else {
            return (None, cache_writes);
        };
        let Some((input_rate, cached_rate, output_rate)) = credit_rates(&attempt.model) else {
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

fn parse_timestamp(timestamp: &str) -> Result<i64, ReportError> {
    let parsed = OffsetDateTime::parse(timestamp, &Rfc3339).map_err(|source| {
        ReportError::InvalidNucleusTimestamp {
            timestamp: timestamp.to_owned(),
            source,
        }
    })?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| ReportError::TimestampOverflow(timestamp.to_owned()))
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
    #[error("model run token {token} appears in multiple inbox jobs: {first_job} and {second_job}")]
    DuplicateModelRunReceipt {
        token: String,
        first_job: String,
        second_job: String,
    },
    #[error("Nucleus returned invalid timestamp {timestamp:?}: {source}")]
    InvalidNucleusTimestamp {
        timestamp: String,
        source: time::error::Parse,
    },
    #[error("Nucleus timestamp {0:?} does not fit in milliseconds")]
    TimestampOverflow(String),
    #[error("token total overflow while aggregating model responses")]
    TokenOverflow,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nucleus_core::{
        AttemptOutputV1, JobId, JobState, LogRecordV1, LogStream, PROTOCOL_VERSION_V1, SchemaId,
    };
    use rusqlite::Connection;
    use serde_json::{Value, json, value::RawValue};

    use super::{
        DeliveryRecord, ReceiptSummary, ReportScope, delivery_usage, grouped, read_receipts,
        reduce_records,
    };
    use crate::config::UsageConfig;

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
            r#"{"id":"job-1","ingestion_id":42,"model_run_token":"run-token"}"#,
        )?;

        let receipts = read_receipts(spool.path())?;
        let receipt = receipts
            .by_delivery
            .get(&42)
            .ok_or("skipped receipt was not discovered")?;
        assert_eq!(receipt.model_run_token.as_deref(), Some("run-token"));
        Ok(())
    }

    #[test]
    fn report_scope_fetches_only_recent_delivery_runs_and_unattributed_runs()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let library = directory.path().join("annals.db");
        let connection = Connection::open(&library)?;
        connection.execute_batch(
            "CREATE TABLE ingestions(\
                 id INTEGER PRIMARY KEY, source_name TEXT NOT NULL, status TEXT NOT NULL, \
                 result TEXT, work_id INTEGER\
             ); \
             INSERT INTO ingestions VALUES(1, 'old.md', 'completed', 'applied', 1); \
             INSERT INTO ingestions VALUES(2, 'recent.md', 'completed', 'applied', 2);",
        )?;
        let spool = directory.path().join("spool/done");
        for (job, delivery, token) in [
            ("job-old", 1, "token-old"),
            ("job-recent", 2, "token-recent"),
        ] {
            let envelope = spool.join(job);
            fs::create_dir_all(&envelope)?;
            fs::write(
                envelope.join("job.json"),
                serde_json::to_vec(&json!({
                    "id": job,
                    "ingestion_id": delivery,
                    "model_run_token": token
                }))?,
            )?;
        }
        let config = UsageConfig {
            library,
            spool: directory.path().join("spool"),
            ..UsageConfig::default()
        };

        let scope = ReportScope::load(&config, 1)?;
        assert!(scope.includes_delivery("token-recent"));
        assert!(!scope.includes_delivery("token-old"));
        assert!(!scope.is_unattributed("token-old"));
        assert!(scope.is_unattributed("token-manual"));
        assert_eq!(scope.unattributed_limit(), 1);
        Ok(())
    }

    #[test]
    fn retry_child_reusing_a_reconciliation_has_zero_new_usage()
    -> Result<(), Box<dyn std::error::Error>> {
        let library = Connection::open_in_memory()?;
        library.execute_batch("CREATE TABLE model_runs(token TEXT NOT NULL)")?;
        let delivery = DeliveryRecord {
            id: 7,
            source_name: "retry.md".to_owned(),
            status: "completed".to_owned(),
            result: Some("applied".to_owned()),
            work_id: Some(3),
        };
        let receipt = ReceiptSummary {
            id: "retry-job".to_owned(),
            attempts: 1,
            ingestion_id: Some(7),
            model_run_token: None,
            reconciliation_id: Some(11),
            result_status: Some("applied".to_owned()),
        };

        let (coverage, usage) = delivery_usage(&library, &delivery, Some(&receipt), &[])?;
        assert_eq!(coverage, "reused-no-new-usage");
        assert_eq!(usage.map(|usage| usage.total_tokens), Some(0));
        Ok(())
    }

    #[test]
    fn exact_usage_is_reduced_from_output_records() -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": {
                            "last": usage,
                            "total": usage,
                            "modelContextWindow": 1000
                        }
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": usage
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "exact");
        assert_eq!(reduction.usage.map(|usage| usage.total_tokens), Some(120));
        assert_eq!(reduction.responses.len(), 1);
        assert_eq!(reduction.model_context_window, Some(1000));
        assert!(reduction.exact_stream_complete);
        Ok(())
    }

    #[test]
    fn identical_response_replays_are_deduplicated() -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let response = json!({
            "method": "rawResponse/completed",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "responseId": "response-1",
                "usage": usage
            }
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": usage, "total": usage }
                    }
                }),
            )?,
            record(2, &response)?,
            record(3, &response)?,
            record(
                4,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "exact");
        assert_eq!(reduction.responses.len(), 1);
        assert_eq!(reduction.usage.map(|usage| usage.total_tokens), Some(120));
        Ok(())
    }

    #[test]
    fn conflicting_response_replays_create_an_exact_gap() -> Result<(), Box<dyn std::error::Error>>
    {
        let first_usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let second_usage = json!({
            "inputTokens": 101,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 121
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": first_usage
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": second_usage
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "gap");
        assert_eq!(reduction.responses.len(), 1);
        assert!(!reduction.exact_stream_complete);
        assert!(
            reduction
                .warnings
                .iter()
                .any(|warning| warning.contains("conflicting attribution or usage"))
        );
        Ok(())
    }

    #[test]
    fn final_cumulative_usage_covers_missing_response_events()
    -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 80,
            "cachedInputTokens": 20,
            "cacheWriteInputTokens": 0,
            "outputTokens": 10,
            "reasoningOutputTokens": 5,
            "totalTokens": 90
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": usage, "total": usage }
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "cumulative");
        assert_eq!(reduction.usage.map(|usage| usage.total_tokens), Some(90));
        assert!(!reduction.exact_stream_complete);
        assert!(reduction.warnings[0].contains("no exact response"));
        Ok(())
    }

    #[test]
    fn only_the_attempt_output_turn_is_reduced() -> Result<(), Box<dyn std::error::Error>> {
        let unrelated_usage = json!({
            "inputTokens": 7,
            "cachedInputTokens": 0,
            "cacheWriteInputTokens": 0,
            "outputTokens": 3,
            "reasoningOutputTokens": 0,
            "totalTokens": 10
        });
        let authoritative_usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-other",
                        "turnId": "turn-other",
                        "responseId": "response-other",
                        "usage": unrelated_usage
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-other",
                        "turn": { "id": "turn-other", "status": "completed" }
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-authoritative",
                        "turnId": "turn-authoritative",
                        "responseId": "response-authoritative",
                        "usage": authoritative_usage
                    }
                }),
            )?,
            record(
                4,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-authoritative",
                        "turn": { "id": "turn-authoritative", "status": "completed" }
                    }
                }),
            )?,
            record(
                5,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-trailing",
                        "turnId": "turn-trailing",
                        "responseId": "response-trailing",
                        "usage": unrelated_usage
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-authoritative", "turn-authoritative");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "exact");
        assert_eq!(reduction.thread_id.as_deref(), Some("thread-authoritative"));
        assert_eq!(reduction.turn_id.as_deref(), Some("turn-authoritative"));
        assert_eq!(reduction.usage.map(|usage| usage.total_tokens), Some(120));
        assert_eq!(reduction.responses.len(), 1);
        assert_eq!(reduction.responses[0].response_id, "response-authoritative");
        assert!(reduction.warnings.is_empty());
        Ok(())
    }

    #[test]
    fn matching_turn_completion_freezes_the_reducer() -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let invalid_usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 999
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": usage
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": invalid_usage, "total": invalid_usage }
                    }
                }),
            )?,
            record(
                4,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-after-completion",
                        "usage": usage
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "exact");
        assert_eq!(reduction.usage.map(|usage| usage.total_tokens), Some(120));
        assert_eq!(reduction.responses.len(), 1);
        assert!(reduction.exact_stream_complete);
        assert!(reduction.warnings.is_empty());
        Ok(())
    }

    #[test]
    fn inconsistent_cumulative_usage_forces_a_gap_with_exact_responses()
    -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let invalid_usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 999
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": usage
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": invalid_usage, "total": invalid_usage }
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "gap");
        assert_eq!(reduction.usage, None);
        assert!(!reduction.exact_stream_complete);
        assert!(
            reduction
                .warnings
                .iter()
                .any(|warning| warning.contains("cumulative token snapshot was inconsistent"))
        );
        Ok(())
    }

    #[test]
    fn cumulative_last_must_be_componentwise_at_most_total()
    -> Result<(), Box<dyn std::error::Error>> {
        let last = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let total = json!({
            "inputTokens": 80,
            "cachedInputTokens": 20,
            "cacheWriteInputTokens": 0,
            "outputTokens": 40,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": last, "total": total }
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": total, "total": total }
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": total
                    }
                }),
            )?,
            record(
                4,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "gap");
        assert_eq!(reduction.usage, None);
        assert!(!reduction.exact_stream_complete);
        assert!(
            reduction
                .warnings
                .iter()
                .any(|warning| warning.contains("last usage exceeded its total"))
        );
        Ok(())
    }

    #[test]
    fn cumulative_totals_cannot_regress_or_recover_coverage()
    -> Result<(), Box<dyn std::error::Error>> {
        let usage_120 = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let usage_60 = json!({
            "inputTokens": 50,
            "cachedInputTokens": 10,
            "cacheWriteInputTokens": 0,
            "outputTokens": 10,
            "reasoningOutputTokens": 5,
            "totalTokens": 60
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": usage_120, "total": usage_120 }
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": usage_60, "total": usage_60 }
                    }
                }),
            )?,
            record(
                3,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "tokenUsage": { "last": usage_120, "total": usage_120 }
                    }
                }),
            )?,
            record(
                4,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": usage_120
                    }
                }),
            )?,
            record(
                5,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "gap");
        assert_eq!(reduction.usage, None);
        assert!(!reduction.exact_stream_complete);
        assert!(
            reduction
                .warnings
                .iter()
                .any(|warning| warning.contains("cumulative token total regressed"))
        );
        Ok(())
    }

    #[test]
    fn malformed_and_nonterminal_turn_statuses_force_a_gap()
    -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let output = attempt_output("thread-1", "turn-1");
        for (case, status) in [
            ("malformed", json!(17)),
            ("nonterminal", json!("running")),
            ("other", json!("failed")),
        ] {
            let records = vec![
                record(
                    1,
                    &json!({
                        "method": "rawResponse/completed",
                        "params": {
                            "threadId": "thread-1",
                            "turnId": "turn-1",
                            "responseId": "response-1",
                            "usage": usage
                        }
                    }),
                )?,
                record(
                    2,
                    &json!({
                        "method": "turn/completed",
                        "params": {
                            "threadId": "thread-1",
                            "turn": { "id": "turn-1", "status": status }
                        }
                    }),
                )?,
            ];

            let reduction = reduce_records(&records, JobState::Completed, Some(&output))?;
            assert_eq!(reduction.coverage, "gap", "{case}");
            assert_eq!(reduction.usage, None, "{case}");
            assert_eq!(reduction.turn_status, None, "{case}");
            assert!(!reduction.exact_stream_complete, "{case}");
            assert!(
                reduction
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("turn-completion")),
                "{case}"
            );
        }
        Ok(())
    }

    #[test]
    fn failed_job_cannot_be_exact_even_with_completed_turn_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let records = vec![
            record(
                1,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "responseId": "response-1",
                        "usage": usage
                    }
                }),
            )?,
            record(
                2,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turn": { "id": "turn-1", "status": "completed" }
                    }
                }),
            )?,
        ];

        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&records, JobState::Failed, Some(&output))?;
        assert_eq!(reduction.coverage, "gap");
        assert_eq!(reduction.usage, None);
        assert_eq!(reduction.turn_status, None);
        assert!(!reduction.exact_stream_complete);
        assert!(
            reduction
                .warnings
                .iter()
                .any(|warning| warning.contains("did not complete successfully"))
        );
        Ok(())
    }

    #[test]
    fn terminal_job_without_turn_completion_is_a_gap() -> Result<(), Box<dyn std::error::Error>> {
        let output = attempt_output("thread-1", "turn-1");
        let reduction = reduce_records(&[], JobState::Completed, Some(&output))?;
        assert_eq!(reduction.coverage, "gap");
        assert!(!reduction.exact_stream_complete);
        assert!(reduction.warnings[0].contains("turn-completion"));
        Ok(())
    }

    fn attempt_output(thread_id: &str, turn_id: &str) -> AttemptOutputV1 {
        AttemptOutputV1 {
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            final_message: "done".to_owned(),
        }
    }

    fn record(sequence: u64, payload: &Value) -> Result<LogRecordV1, Box<dyn std::error::Error>> {
        Ok(LogRecordV1 {
            version: PROTOCOL_VERSION_V1,
            job_id: JobId::new("annals-run"),
            attempt_id: None,
            sequence,
            observed_at: format!("2026-08-27T12:10:{sequence:02}Z"),
            stream: LogStream::HarnessOutput,
            schema_id: SchemaId::new("codex.app-server.protocol.test"),
            payload: RawValue::from_string(serde_json::to_string(&payload)?)?,
            payload_digest: "fixture-digest".to_owned(),
        })
    }
}
