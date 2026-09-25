use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use clew::store::{Record, Store};
use serde_json::json;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Record your application status and notes for Cast jobs"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Print JSON, including for email previews.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize an empty private ledger, or check the existing schema.
    Init,
    /// Check local ledger integrity without reading Cast or creating entries.
    Doctor,
    /// Find candidates by retained URL, company, title, reference or Clew notes.
    Find { query: String },
    /// Append an explicitly supplied status and/or note to one exact Cast job.
    Record {
        /// A retained legacy Platter reference, for old reports and exact retries.
        #[arg(required_unless_present = "cast_job", conflicts_with = "cast_job")]
        reference: Option<String>,
        /// The exact Cast job ID to track.
        #[arg(long)]
        cast_job: Option<String>,
        #[arg(long)]
        id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        /// Replace this active record with the complete supplied record.
        #[arg(long)]
        replaces: Option<String>,
    },
    /// Retract one mistaken record while preserving its original contents.
    Retract {
        entry_id: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List the latest supplied status for each currently tracked Cast job.
    List,
    /// Read history by Cast job ID or a retained legacy Platter reference.
    Show { reference: String },
    /// Preview or explicitly send the daily application snapshot.
    #[command(subcommand)]
    Email(EmailCommand),
}

#[derive(Subcommand)]
enum EmailCommand {
    Preview {
        /// Read the frozen message for a retained occurrence.
        #[arg(long)]
        occurrence: Option<String>,
    },
    Send {
        #[arg(long, conflicts_with = "retry")]
        scheduled: bool,
        /// Retry retained bytes after inspecting provider acceptance.
        #[arg(long)]
        retry: Option<String>,
    },
}

fn main() {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "clew",
        env!("CARGO_PKG_VERSION"),
        vec![
            iatreion_api::declared_unit(
                "clew",
                "clew/ledger",
                None,
                iatreion_api::Intent::OnDemand,
                "clew.application.track",
            ),
            iatreion_api::declared_unit(
                "clew",
                "clew/daily-email",
                Some("clew/daily-email"),
                iatreion_api::Intent::Active,
                "clew.digest.email",
            ),
        ],
        false,
    ) {
        chancery_usage::observe("clew", "status-snapshot");
        println!("{snapshot}");
        return;
    }
    if let Err(error) = run() {
        println!(
            "{}",
            json!({"ok":false,"error":{"detail":format!("{error:#}")}})
        );
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = chancery_usage::cli::parse::<Cli>("clew", "");
    let text_preview =
        !cli.json && matches!(&cli.command, Command::Email(EmailCommand::Preview { .. }));
    let root = cli.state_dir.unwrap_or(clew::state_dir(&clew::home()?));
    ensure!(root.is_absolute(), "Clew state directory must be absolute");
    let data = match cli.command {
        Command::Init => {
            Store::initialize(&root)?;
            json!({"initialized":true,"schema_version":2})
        }
        Command::Doctor => {
            let store = Store::open(&root, false)?;
            store.check()?;
            json!({"schema_version":2,"entries":store.entries()?.len()})
        }
        Command::Find { query } => find(&root, &query)?,
        Command::Record {
            reference,
            cast_job,
            id,
            status,
            notes,
            replaces,
        } => {
            let _admission = clew::gate(&root).enter()?;
            let mut store = Store::open(&root, true)?;
            let record = Record {
                id,
                platter_job_ref: reference,
                cast_job_id: cast_job,
                status,
                notes,
                replaces,
            };
            let entry = if let Some(entry) = store.existing_record(&record)? {
                entry
            } else {
                if let Some(job_id) = &record.cast_job_id {
                    if !store.knows_job(job_id)? {
                        ensure!(
                            clew::cast_jobs()?
                                .iter()
                                .any(|job| job.cast_job_id == *job_id),
                            "Cast job is not retained"
                        );
                    }
                } else {
                    ensure!(
                        store.knows(record.platter_job_ref.as_deref().context("reference")?)?,
                        "legacy reference has no Clew history; use --cast-job with a retained Cast job"
                    );
                }
                store.record(&record)?
            };
            serde_json::to_value(entry)?
        }
        Command::Retract {
            entry_id,
            id,
            notes,
        } => {
            let _admission = clew::gate(&root).enter()?;
            serde_json::to_value(Store::open(&root, true)?.retract(
                &id,
                &entry_id,
                notes.as_deref(),
            )?)?
        }
        Command::List => serde_json::to_value(Store::open(&root, false)?.current()?)?,
        Command::Show { reference } => {
            let store = Store::open(&root, false)?;
            ensure!(store.knows(&reference)?, "job has no Clew history");
            let job_id = store.canonical_reference(&reference)?;
            json!({"cast_job_id":job_id,"current":store.current()?.into_iter().find(|item| item.cast_job_id == job_id),"history":store.history(&reference)?})
        }
        Command::Email(EmailCommand::Preview { occurrence }) => {
            clew::digest::preview(&root, occurrence.as_deref())?
        }
        Command::Email(EmailCommand::Send { scheduled, retry }) => {
            clew::digest::send(&root, scheduled, retry.as_deref())?
        }
    };
    if text_preview {
        println!(
            "From: {}\nTo: {}\nSubject: {}\n\n{}",
            data["from"].as_str().context("preview sender")?,
            data["to"].as_str().context("preview recipient")?,
            data["digest"]["subject"]
                .as_str()
                .context("preview subject")?,
            data["digest"]["body"].as_str().context("preview body")?
        );
    } else {
        println!("{}", json!({"ok":true,"schema_version":2,"data":data}));
    }
    Ok(())
}

fn find(root: &std::path::Path, query: &str) -> Result<serde_json::Value> {
    ensure!(!query.trim().is_empty(), "search query must be nonblank");
    let store = Store::open(root, false)?;
    let entries = store.entries()?;
    let tracked: BTreeSet<_> = store
        .current()?
        .into_iter()
        .map(|item| item.cast_job_id)
        .collect();
    let text = query.to_lowercase();
    let references: BTreeSet<_> = entries
        .iter()
        .filter(|entry| {
            entry.cast_job_id.to_lowercase().contains(&text)
                || entry
                    .platter_job_ref
                    .as_ref()
                    .is_some_and(|reference| reference.to_lowercase().contains(&text))
                || entry
                    .notes
                    .as_ref()
                    .is_some_and(|notes| notes.to_lowercase().contains(&text))
        })
        .map(|entry| entry.cast_job_id.clone())
        .collect();
    let jobs = clew::cast_jobs()
        .context("cannot complete candidate search; Clew list/show remain available")?;
    let mut candidates: Vec<_> = jobs
        .into_iter()
        .filter(|item| references.contains(&item.cast_job_id) || item.matches(query))
        .collect();
    candidates.sort_by(|a, b| a.cast_job_id.cmp(&b.cast_job_id));
    let unmatched: Vec<_> = references
        .into_iter()
        .filter(|reference| !candidates.iter().any(|item| item.cast_job_id == *reference))
        .collect();
    let candidates: Vec<_> = candidates.into_iter().map(|item| {
        json!({"cast_job_id":item.cast_job_id,"company":item.company,"title":item.title,"urls":item.urls,"tracked":tracked.contains(&item.cast_job_id)})
    }).collect();
    Ok(
        json!({"candidates":candidates,"retained_references_without_cast_record":unmatched,"complete":true}),
    )
}
