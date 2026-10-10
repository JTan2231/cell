#![cfg_attr(test, allow(clippy::unwrap_used))]

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(
    version,
    about = "Run configured productions and email their successful renderer stdout"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    /// Absolute production-definition TOML path. Defaults to Paperboy/paperboy.toml.
    #[arg(long, global = true)]
    manifest: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create an absent empty production-definition file.
    Init,
    /// Read production definitions without running renderers or sending email.
    List,
    /// Start one production run and submit successful nonempty renderer stdout.
    Run {
        #[arg(value_name = "PRODUCTION_ID")]
        id: String,
    },
    /// Apply schedule snapshots. New schedules stay disabled; existing intent is kept.
    Apply,
    /// Enable or disable future scheduling, or inspect selected production schedules.
    Schedule {
        #[arg(value_parser = ["enable", "disable", "status"])]
        operation: String,
        #[arg(value_name = "PRODUCTION_ID")]
        id: Option<String>,
    },
    /// Check production definitions and executable files without running them.
    Doctor,
    /// Read aggregate production readiness. Clockwork owns scheduled run history.
    StatusSnapshot,
    /// Exact selected command used by Clockwork. Reads no mutable manifest.
    #[command(hide = true)]
    Execute {
        #[arg(long, alias = "job")]
        production: String,
        #[arg(long, allow_hyphen_values = true)]
        subject: String,
        #[arg(long)]
        email: PathBuf,
        #[arg(last = true, num_args = 1.., allow_hyphen_values = true)]
        render: Vec<String>,
    },
}

fn selected_manifest(cli: &Cli) -> Result<PathBuf> {
    cli.manifest
        .clone()
        .map_or_else(paperboy::manifest_path, Ok)
}

async fn execute(cli: &Cli) -> Result<Value> {
    match &cli.command {
        Commands::Init => {
            let path = selected_manifest(cli)?;
            Ok(
                json!({"initialized":paperboy::manifest::Manifest::initialize(&path)?,"manifest":path,"version":1}),
            )
        }
        Commands::List => {
            let manifest = paperboy::manifest::Manifest::load(&selected_manifest(cli)?)?;
            Ok(json!({"version":manifest.version,"jobs":manifest.jobs}))
        }
        Commands::Run { id } => {
            let manifest = paperboy::manifest::Manifest::load(&selected_manifest(cli)?)?;
            let job = manifest.job(id)?;
            paperboy::runtime::run(
                id,
                job.subject(id),
                &job.render,
                &paperboy::home()?.join(".local/bin/email"),
            )
            .await
        }
        Commands::Apply => {
            let manifest = paperboy::manifest::Manifest::load(&selected_manifest(cli)?)?;
            paperboy::schedule::apply(&paperboy::state_root()?, &manifest)
        }
        Commands::Schedule { operation, id } => {
            let root = paperboy::state_root()?;
            if operation == "status" {
                paperboy::schedule::status(&root, id.as_deref())
            } else {
                let id = id
                    .as_deref()
                    .context("schedule enable/disable requires a production ID")?;
                if operation == "enable" {
                    paperboy::manifest::Manifest::load(&selected_manifest(cli)?)?.job(id)?;
                }
                paperboy::schedule::set_enabled(&root, id, operation == "enable")
            }
        }
        Commands::Doctor => doctor(&selected_manifest(cli)?),
        Commands::StatusSnapshot => Ok(status_snapshot(cli)),
        Commands::Execute {
            production,
            subject,
            email,
            render,
        } => {
            ensure!(
                cli.manifest.is_none(),
                "execute uses its selected arguments, not a production-definition override"
            );
            paperboy::runtime::run(production, subject, render, email).await
        }
    }
}

fn doctor(path: &std::path::Path) -> Result<Value> {
    let manifest = paperboy::manifest::Manifest::load(path)?;
    for job in manifest.jobs.values() {
        paperboy::runtime::executable(std::path::Path::new(&job.render[0]))?;
    }
    for command in ["clockwork", "email"] {
        paperboy::runtime::executable(&paperboy::home()?.join(".local/bin").join(command))?;
    }
    Ok(
        json!({"ready":true,"scope":"production definitions and executable files only","job_count":manifest.jobs.len()}),
    )
}

fn status_snapshot(cli: &Cli) -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let local = selected_manifest(cli).and_then(|path| doctor(&path));
    let (readiness, reasons, evidence) = match local {
        Ok(result) => (
            "ready",
            vec![],
            json!({"counts":[{"name":"configured_productions","value":result["job_count"],"unit":"productions","scope":"selected production definitions"}]}),
        ),
        Err(error) => (
            "blocked",
            vec![json!({"code":"local_prerequisites_unavailable","summary":format!("{error:#}")})],
            json!({}),
        ),
    };
    json!({
        "schema_version":1,"product_id":"paperboy","provider_release":env!("CARGO_PKG_VERSION"),
        "observed_at_start":now,"observed_at_end":now,"complete":false,
        "units":[{
            "id":"paperboy/productions","owning_product_id":"paperboy","intent":"on_demand",
            "admission":{"state":"unknown","reasons":[]},"activity":"unknown",
            "readiness":{"state":readiness,"scope":"production definitions and executable files only","reasons":reasons},
            "evidence":evidence,"inspection":[{"capability_id":"paperboy.install.operate"}]
        }],
        "diagnostics":[{"code":"product_observation_incomplete","summary":"Clockwork owns production schedule enablement, activity and run history; Email acceptance is not retained by Paperboy"}]
    })
}

fn human_output(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        for (previous, current) in [
            ("jobs", "productions"),
            ("job_id", "production_id"),
            ("job_count", "production_count"),
            ("manifest", "definition_file"),
        ] {
            if let Some(entry) = object.remove(previous) {
                object.insert(current.to_owned(), entry);
            }
        }
    }
    value
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = chancery_usage::cli::parse::<Cli>("paperboy", "");
    match interruptible_execute(&cli).await {
        Ok(value) => {
            if matches!(cli.command, Commands::StatusSnapshot) {
                println!("{value}");
            } else if cli.json {
                println!("{}", json!({"ok":true,"data":value}));
            } else {
                let value = human_output(value);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                );
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            if cli.json {
                eprintln!("{}", json!({"ok":false,"error":format!("{error:#}")}));
            } else {
                eprintln!("paperboy: {error:#}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}

async fn interruptible_execute(cli: &Cli) -> Result<Value> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).context("cannot observe SIGTERM")?;
    let mut interrupt = signal(SignalKind::interrupt()).context("cannot observe SIGINT")?;
    tokio::select! {
        result = execute(cli) => result,
        _ = terminate.recv() => anyhow::bail!("Paperboy interrupted by SIGTERM; inspect any unconfirmed send before resending"),
        _ = interrupt.recv() => anyhow::bail!("Paperboy interrupted by SIGINT; inspect any unconfirmed send before resending"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_execution_preserves_literal_renderer_arguments() {
        let cli = Cli::try_parse_from([
            "paperboy",
            "execute",
            "--job",
            "report",
            "--subject",
            "-Subject",
            "--email",
            "/private/email",
            "--",
            "/private/render",
            "--daily",
            "two words",
        ])
        .unwrap();
        let Commands::Execute {
            render, subject, ..
        } = cli.command
        else {
            panic!("expected execution");
        };
        assert_eq!(render, ["/private/render", "--daily", "two words"]);
        assert_eq!(subject, "-Subject");
        assert!(Cli::try_parse_from(["paperboy", "run", "--brief", "old"]).is_err());
        assert!(Cli::try_parse_from(["paperboy", "reconcile", "old"]).is_err());
    }

    #[test]
    fn production_runner_accepts_current_and_retained_flags() {
        for flag in ["--production", "--job"] {
            let cli = Cli::try_parse_from([
                "paperboy",
                "execute",
                flag,
                "daily",
                "--subject",
                "Daily",
                "--email",
                "/private/email",
                "--",
                "/private/render",
            ])
            .unwrap();
            assert!(
                matches!(cli.command, Commands::Execute { production, .. } if production == "daily")
            );
        }
    }

    #[test]
    fn human_labels_do_not_change_structured_results() {
        let wire =
            json!({"jobs":{},"job_id":"daily","job_count":1,"manifest":"/private/paperboy.toml"});
        let human = human_output(wire.clone());
        assert_eq!(human["production_id"], "daily");
        assert_eq!(human["production_count"], 1);
        assert_eq!(human["productions"], json!({}));
        assert_eq!(human["definition_file"], "/private/paperboy.toml");
        assert!(human.get("job_id").is_none());
        assert_eq!(wire["job_id"], "daily");
    }
}
