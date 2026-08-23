mod budget;
mod config;
mod database;
mod protocol;
mod report;
mod types;

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use clap::Parser;
use serde_json::json;
use thiserror::Error;

use crate::budget::BudgetReport;
use crate::config::UsageConfig;
use crate::database::{RunIdentity, UsageDatabase};
use crate::protocol::{ProtocolEvent, TelemetryGap};

fn main() {
    match run() {
        Ok(Outcome::Success) => {}
        Ok(Outcome::Child(status)) => {
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(error) => {
            eprintln!("annals-usage: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<Outcome, AppError> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().and_then(|argument| argument.to_str()) else {
        print_help();
        return Ok(Outcome::Success);
    };
    match command {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(Outcome::Success)
        }
        "-V" | "--version" | "version" => {
            println!("annals-usage {}", env!("CARGO_PKG_VERSION"));
            Ok(Outcome::Success)
        }
        "report" => {
            let options = ReportOptions::parse_from(command_arguments("report", &arguments[1..]));
            run_report(&options)?;
            Ok(Outcome::Success)
        }
        "budget" => {
            let options = BudgetOptions::parse_from(command_arguments("budget", &arguments[1..]));
            run_budget(&options)?;
            Ok(Outcome::Success)
        }
        "doctor" => {
            let options = DoctorOptions::parse_from(command_arguments("doctor", &arguments[1..]));
            run_doctor(&options)?;
            Ok(Outcome::Success)
        }
        _ => run_as_proxy(&arguments),
    }
}

fn run_report(options: &ReportOptions) -> Result<(), AppError> {
    let config = UsageConfig::load(options.config.as_deref())?;
    let database = UsageDatabase::open(&config.database)?;
    let report = report::build_report(&config, &database, options.limit)?;
    if options.json {
        write_json(&report)?;
    } else {
        report::print_human(&report);
    }
    Ok(())
}

fn run_budget(options: &BudgetOptions) -> Result<(), AppError> {
    let config = UsageConfig::load(options.config.as_deref())?;
    let snapshot = protocol::read_account_snapshot(&config.codex, Some(&config.codex_home))?;
    let database = UsageDatabase::open(&config.database)?;
    database.record_quota_snapshot(
        None,
        "account/rateLimits/read",
        &serde_json::to_value(&snapshot.rate_limits)?,
    )?;
    let snapshot = serde_json::to_value(snapshot)?;
    let report = BudgetReport::new(snapshot)?;
    if options.json {
        write_json(&report)?;
    } else {
        budget::print_human(&report);
    }
    Ok(())
}

fn run_doctor(options: &DoctorOptions) -> Result<(), AppError> {
    let config = UsageConfig::load(options.config.as_deref())?;
    let mut failures = Vec::new();
    for (name, path) in [
        ("real Codex", &config.codex),
        ("Annals library", &config.library),
        ("inbox spool", &config.spool),
        ("state-local Codex home", &config.codex_home),
    ] {
        if !path.exists() {
            failures.push(format!("{name} is missing: {}", path.display()));
        }
    }
    let _database = UsageDatabase::open(&config.database)?;
    let codex_version = codex_version(&config.codex)?;
    let budget = protocol::read_account_snapshot(&config.codex, Some(&config.codex_home));
    if let Err(error) = budget {
        failures.push(format!("account rate-limit read failed: {error}"));
    }
    if failures.is_empty() {
        println!("annals-usage doctor: healthy");
        println!("Configuration: {}", config.path.display());
        println!("Ledger:        {}", config.database.display());
        println!("Real Codex:    {codex_version}");
        return Ok(());
    }
    Err(AppError::Doctor(failures.join("; ")))
}

fn run_as_proxy(arguments: &[OsString]) -> Result<Outcome, AppError> {
    let config = UsageConfig::load(None)?;
    let is_stdio_app_server = arguments.iter().any(|argument| argument == "app-server")
        && arguments.iter().any(|argument| argument == "--stdio");
    if !is_stdio_app_server {
        return protocol::run_passthrough(&config.codex, arguments)
            .map(Outcome::Child)
            .map_err(Into::into);
    }

    let mut recorder = RunRecorder::start(&config);
    let result = protocol::run_stdio_proxy(&config.codex, arguments, |event| {
        let Some(recorder) = recorder.as_mut() else {
            return Ok(());
        };
        if let Err(error) = recorder.observe(event) {
            eprintln!("annals-usage: telemetry disabled for this examination: {error}");
            return Err(io::Error::other(error.to_string()));
        }
        Ok(())
    });
    if let Some(recorder) = recorder.as_mut()
        && let Err(error) = recorder.finish_after_proxy(result.as_ref().ok())
    {
        eprintln!("annals-usage: unable to finalize telemetry: {error}");
    }
    let status = result?;
    Ok(Outcome::Child(status))
}

struct RunRecorder {
    database: UsageDatabase,
    run_id: i64,
    completed: bool,
}

impl RunRecorder {
    fn start(config: &UsageConfig) -> Option<Self> {
        let result = (|| -> Result<Self, AppError> {
            let token = std::env::var("ANNALS_MODEL_RUN_TOKEN").ok();
            let identity = RunIdentity::resolve(config, token.as_deref())?;
            let mut database = UsageDatabase::open(&config.database)?;
            let codex_version = codex_version(&config.codex).ok();
            let run_id = database.begin_run(&identity, codex_version.as_deref())?;
            Ok(Self {
                database,
                run_id,
                completed: false,
            })
        })();
        match result {
            Ok(recorder) => Some(recorder),
            Err(error) => {
                eprintln!(
                    "annals-usage: telemetry unavailable for this examination; Codex will continue: {error}"
                );
                None
            }
        }
    }

    fn observe(&mut self, event: &ProtocolEvent) -> Result<(), AppError> {
        match event {
            ProtocolEvent::ThreadStarted { thread_id, .. } => {
                self.database.bind_thread(self.run_id, thread_id, None)?;
            }
            ProtocolEvent::TurnStarted {
                thread_id, turn_id, ..
            } => {
                self.database
                    .bind_thread(self.run_id, thread_id, Some(turn_id))?;
            }
            ProtocolEvent::TokenUsageUpdated {
                thread_id,
                turn_id,
                usage,
            } => {
                self.database
                    .record_token_usage(self.run_id, thread_id, turn_id, usage)?;
            }
            ProtocolEvent::RawResponseCompleted {
                thread_id,
                turn_id,
                response_id,
                usage,
            } => {
                if let Some(usage) = usage {
                    self.database.record_response_usage(
                        self.run_id,
                        response_id,
                        thread_id,
                        turn_id,
                        *usage,
                    )?;
                } else {
                    self.database.record_exact_gap(
                        self.run_id,
                        "an upstream response omitted token usage",
                    )?;
                }
            }
            ProtocolEvent::RateLimitsUpdated { rate_limits } => {
                let payload = json!({ "rateLimits": rate_limits });
                self.database.record_quota_snapshot(
                    Some(self.run_id),
                    "account/rateLimits/updated",
                    &payload,
                )?;
            }
            ProtocolEvent::TurnCompleted {
                thread_id,
                turn_id,
                status,
            } => {
                self.database
                    .complete_run(self.run_id, status, Some(thread_id), Some(turn_id))?;
                self.completed = true;
            }
            ProtocolEvent::TelemetryGap(TelemetryGap::RawResponseOptInNotApplied) => {
                self.database.record_exact_gap(
                    self.run_id,
                    "the proxy could not enable exact per-response usage events",
                )?;
            }
        }
        Ok(())
    }

    fn finish_after_proxy(&mut self, status: Option<&ExitStatus>) -> Result<(), AppError> {
        if self.completed {
            return Ok(());
        }
        let status = match status.and_then(ExitStatus::code) {
            Some(0) => "ended-without-turn-completion".to_owned(),
            Some(code) => format!("codex-exit-{code}"),
            None => "proxy-or-codex-terminated".to_owned(),
        };
        self.database.finish_incomplete_run(self.run_id, &status)?;
        self.completed = true;
        Ok(())
    }
}

fn codex_version(codex: &std::path::Path) -> Result<String, AppError> {
    let output = Command::new(codex).arg("--version").output()?;
    if !output.status.success() {
        return Err(AppError::CodexVersion(output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn command_arguments(name: &str, arguments: &[OsString]) -> Vec<OsString> {
    std::iter::once(OsString::from(name))
        .chain(arguments.iter().cloned())
        .collect()
}

fn write_json(value: &impl serde::Serialize) -> Result<(), AppError> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn print_help() {
    println!(
        "annals-usage {}\n\n\
         Usage:\n  annals-usage report [--json] [--limit N] [--config PATH]\n  \
         annals-usage budget [--json] [--config PATH]\n  \
         annals-usage doctor [--config PATH]\n\n\
         Any other invocation is forwarded to the real Codex executable. Annals uses this \
         behavior as its default liaison proxy.",
        env!("CARGO_PKG_VERSION")
    );
}

#[derive(Debug, Parser)]
struct ReportOptions {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Debug, Parser)]
struct BudgetOptions {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct DoctorOptions {
    #[arg(long)]
    config: Option<PathBuf>,
}

enum Outcome {
    Success,
    Child(ExitStatus),
}

#[derive(Debug, Error)]
enum AppError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Database(#[from] crate::database::DatabaseError),
    #[error(transparent)]
    Protocol(#[from] crate::protocol::ProtocolError),
    #[error(transparent)]
    Report(#[from] crate::report::ReportError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("real Codex --version failed with {0}")]
    CodexVersion(ExitStatus),
    #[error("health check failed: {0}")]
    Doctor(String),
}

#[cfg(test)]
mod tests {
    use super::command_arguments;
    use std::ffi::OsString;

    #[test]
    fn clap_subcommand_arguments_retain_their_values() {
        let arguments = [OsString::from("--limit"), OsString::from("3")];
        assert_eq!(
            command_arguments("report", &arguments),
            ["report", "--limit", "3"].map(OsString::from)
        );
    }
}
