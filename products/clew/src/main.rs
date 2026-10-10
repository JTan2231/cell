use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use clew::store::{Record, Reference, Store};
use serde_json::json;
use std::{collections::BTreeSet, path::PathBuf};

mod output;

#[derive(Parser)]
#[command(
    version,
    about = "Keep an append-only ledger with named threads and optional external references"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Print the machine JSON response instead of readable text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Subcommand)]
enum Command {
    /// Initialize an empty private ledger, or check the existing schema.
    Init,
    /// Check local ledger integrity without reading Milieu or creating entries.
    Doctor,
    /// Find candidates by retained URL, company, title, reference or Clew notes.
    Find { query: String },
    /// Search local entries, thread names, statuses and external references.
    Search { query: String },
    /// Read the complete history and current status of one named thread.
    Thread { name: String },
    /// Read one exact ledger entry and its correction state.
    Entry { id: String },
    /// Append a status and/or note, optionally in a named thread.
    Record {
        /// A retained legacy Platter reference, for old reports and exact retries.
        #[arg(conflicts_with = "milieu_job")]
        reference: Option<String>,
        /// The exact Milieu opportunity ID to track.
        #[arg(
            long = "milieu-opportunity",
            alias = "milieu-job",
            value_name = "OPPORTUNITY_ID"
        )]
        milieu_job: Option<String>,
        /// Find or create this named ledger thread.
        #[arg(long)]
        thread: Option<String>,
        /// Attach a plain external link. Repeat for additional links.
        #[arg(long = "ref", value_names = ["NAMESPACE", "EXTERNAL_ID"], num_args = 2)]
        references: Vec<String>,
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
    /// List the latest supplied status for each currently tracked Milieu opportunity.
    List,
    /// Read history by Milieu opportunity ID or a retained legacy Platter reference.
    Show { reference: String },
    /// Preview or explicitly send the daily application snapshot.
    #[command(subcommand)]
    Email(EmailCommand),
}

#[derive(Clone, Subcommand)]
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
                "clew.ledger.use",
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
    let cli = chancery_usage::cli::parse::<Cli>("clew", "");
    let json_output = cli.json;
    let command = cli.command.clone();
    match execute(cli) {
        Ok(data) if json_output => {
            println!("{}", json!({"ok":true,"schema_version":4,"data":data}));
        }
        Ok(data) => print!("{}", output::render(&command, &data)),
        Err(error) => {
            if json_output {
                println!(
                    "{}",
                    json!({"ok":false,"error":{"detail":format!("{error:#}")}})
                );
            } else {
                eprintln!("Error: {error:#}");
            }
            std::process::exit(1);
        }
    }
}

fn execute(cli: Cli) -> Result<serde_json::Value> {
    let root = cli.state_dir.unwrap_or(clew::state_dir(&clew::home()?));
    ensure!(root.is_absolute(), "Clew state directory must be absolute");
    let data = match cli.command {
        Command::Init => {
            Store::initialize(&root)?;
            json!({"initialized":true,"schema_version":clew::store::SCHEMA_VERSION})
        }
        Command::Doctor => {
            let store = Store::open(&root, false)?;
            store.check()?;
            json!({"schema_version":clew::store::SCHEMA_VERSION,"entries":store.entries()?.len()})
        }
        Command::Find { query } => find(&root, &query)?,
        Command::Search { query } => {
            serde_json::to_value(Store::open(&root, false)?.search(&query)?)?
        }
        Command::Thread { name } => {
            serde_json::to_value(Store::open(&root, false)?.thread(&name)?)?
        }
        Command::Entry { id } => serde_json::to_value(
            Store::open(&root, false)?
                .history_entry(&id)?
                .context("ledger entry is absent")?,
        )?,
        Command::Record {
            reference,
            milieu_job,
            thread,
            references,
            id,
            status,
            notes,
            replaces,
        } => {
            let record = Record {
                id,
                platter_job_ref: reference,
                milieu_job_id: milieu_job,
                status,
                notes,
                replaces,
                thread,
                references: references
                    .chunks_exact(2)
                    .map(|pair| Reference {
                        namespace: pair[0].clone(),
                        external_id: pair[1].clone(),
                    })
                    .collect(),
            };
            serde_json::to_value(append_record(&root, &record)?)?
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
            ensure!(store.knows(&reference)?, "opportunity has no Clew history");
            let job_id = store.canonical_reference(&reference)?;
            json!({"milieu_job_id":job_id,"current":store.current()?.into_iter().find(|item| item.milieu_job_id == job_id),"history":store.history(&reference)?})
        }
        Command::Email(EmailCommand::Preview { occurrence }) => {
            clew::digest::preview(&root, occurrence.as_deref())?
        }
        Command::Email(EmailCommand::Send { scheduled, retry }) => {
            clew::digest::send(&root, scheduled, retry.as_deref())?
        }
    };
    Ok(data)
}

fn append_record(root: &std::path::Path, record: &Record) -> Result<clew::store::Entry> {
    let _admission = clew::gate(root).enter()?;
    let mut store = Store::open(root, true)?;
    if let Some(entry) = store.existing_record(record)? {
        return Ok(entry);
    }
    if let Some(job_id) = &record.milieu_job_id {
        if !store.knows_job(job_id)? {
            ensure!(
                clew::milieu_jobs()?
                    .iter()
                    .any(|job| job.milieu_job_id == *job_id),
                "Milieu opportunity is not retained"
            );
        }
    } else if let Some(reference) = &record.platter_job_ref {
        ensure!(
            store.knows(reference)?,
            "legacy reference has no Clew history; use --milieu-opportunity with a retained Milieu opportunity"
        );
    }
    store.record(record)
}

fn find(root: &std::path::Path, query: &str) -> Result<serde_json::Value> {
    ensure!(!query.trim().is_empty(), "search query must be nonblank");
    let store = Store::open(root, false)?;
    let entries = store.entries()?;
    let tracked: BTreeSet<_> = store
        .current()?
        .into_iter()
        .map(|item| item.milieu_job_id)
        .collect();
    let text = query.to_lowercase();
    let mut references: BTreeSet<_> = entries
        .iter()
        .filter(|entry| {
            entry.references.iter().any(|reference| {
                reference.namespace == "milieu.job"
                    && reference.external_id.to_lowercase().contains(&text)
            }) || entry
                .thread
                .as_ref()
                .is_some_and(|thread| thread.name.to_lowercase().contains(&text))
                || entry
                    .notes
                    .as_ref()
                    .is_some_and(|notes| notes.to_lowercase().contains(&text))
        })
        .flat_map(|entry| {
            entry
                .references
                .iter()
                .filter(|reference| reference.namespace == "milieu.job")
                .map(|reference| reference.external_id.clone())
        })
        .collect();
    references.extend(
        store
            .legacy_aliases()?
            .into_iter()
            .filter(|(alias, _)| alias.to_lowercase().contains(&text))
            .map(|(_, job_id)| job_id),
    );
    let jobs = clew::milieu_jobs()
        .context("cannot complete candidate search; Clew list/show remain available")?;
    let mut candidates: Vec<_> = jobs
        .into_iter()
        .filter(|item| references.contains(&item.milieu_job_id) || item.matches(query))
        .collect();
    candidates.sort_by(|a, b| a.milieu_job_id.cmp(&b.milieu_job_id));
    let unmatched: Vec<_> = references
        .into_iter()
        .filter(|reference| {
            !candidates
                .iter()
                .any(|item| item.milieu_job_id == *reference)
        })
        .collect();
    let candidates: Vec<_> = candidates.into_iter().map(|item| {
        json!({"milieu_job_id":item.milieu_job_id,"company":item.company,"title":item.title,"urls":item.urls,"tracked":tracked.contains(&item.milieu_job_id)})
    }).collect();
    Ok(
        json!({"candidates":candidates,"retained_references_without_milieu_record":unmatched,"complete":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn opportunity_flag_keeps_the_legacy_target_and_conflict_rules() {
        for spelling in ["--milieu-opportunity", "--milieu-job"] {
            let cli = Cli::try_parse_from([
                "clew",
                "record",
                spelling,
                "retained-id",
                "--id",
                "write-id",
                "--status",
                "applied",
            ])
            .expect("supported application argument");
            assert!(matches!(
                cli.command,
                Command::Record {
                    reference: None,
                    milieu_job: Some(id),
                    ..
                } if id == "retained-id"
            ));
            assert!(
                Cli::try_parse_from([
                    "clew",
                    "record",
                    "legacy-ref",
                    spelling,
                    "retained-id",
                    "--id",
                    "write-id",
                    "--status",
                    "applied",
                ])
                .is_err()
            );
        }
        let help = Cli::command()
            .find_subcommand_mut("record")
            .expect("record command")
            .render_long_help()
            .to_string();
        assert!(help.contains("--milieu-opportunity <OPPORTUNITY_ID>"));
        assert!(!help.contains("--milieu-job"));
    }
}
