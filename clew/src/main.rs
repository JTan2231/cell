use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use clew::store::{Record, Store};
use serde_json::json;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Record your application status and notes for Platter opportunities"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// All results use JSON; this flag is accepted for explicit callers.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize an empty private ledger, or check the existing schema.
    Init,
    /// Check local ledger integrity without reading Platter or creating entries.
    Doctor,
    /// Find candidates by retained URL, company, title, reference or Clew notes.
    Find { query: String },
    /// Append an explicitly supplied status and/or note to one exact opportunity.
    Record {
        reference: String,
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
    /// List the latest supplied status for each currently tracked opportunity.
    List,
    /// Read the full recorded history for an exact opportunity reference.
    Show { reference: String },
}

fn main() {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "clew",
        env!("CARGO_PKG_VERSION"),
        vec![iatreion_api::declared_unit(
            "clew",
            "clew/ledger",
            None,
            iatreion_api::Intent::OnDemand,
            "clew.application.track",
        )],
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
    let root = cli.state_dir.unwrap_or(clew::state_dir(&clew::home()?));
    ensure!(root.is_absolute(), "Clew state directory must be absolute");
    let data = match cli.command {
        Command::Init => {
            Store::initialize(&root)?;
            json!({"initialized":true,"schema_version":1})
        }
        Command::Doctor => {
            let store = Store::open(&root, false)?;
            store.check()?;
            json!({"schema_version":1,"entries":store.entries()?.len()})
        }
        Command::Find { query } => find(&root, &query)?,
        Command::Record {
            reference,
            id,
            status,
            notes,
            replaces,
        } => {
            let mut store = Store::open(&root, true)?;
            let record = Record {
                id,
                platter_job_ref: reference,
                status,
                notes,
                replaces,
            };
            let entry = if let Some(entry) = store.existing_record(&record)? {
                entry
            } else {
                if !store.knows(&record.platter_job_ref)? {
                    clew::platter_client()?.show(&record.platter_job_ref)?;
                }
                store.record(&record)?
            };
            serde_json::to_value(entry)?
        }
        Command::Retract {
            entry_id,
            id,
            notes,
        } => serde_json::to_value(Store::open(&root, true)?.retract(
            &id,
            &entry_id,
            notes.as_deref(),
        )?)?,
        Command::List => serde_json::to_value(Store::open(&root, false)?.current()?)?,
        Command::Show { reference } => {
            let store = Store::open(&root, false)?;
            ensure!(store.knows(&reference)?, "opportunity has no Clew history");
            json!({"platter_job_ref":reference,"current":store.current()?.into_iter().find(|item| item.platter_job_ref == reference),"history":store.history(&reference)?})
        }
    };
    println!("{}", json!({"ok":true,"schema_version":1,"data":data}));
    Ok(())
}

fn find(root: &std::path::Path, query: &str) -> Result<serde_json::Value> {
    ensure!(!query.trim().is_empty(), "search query must be nonblank");
    let entries = Store::open(root, false)?.entries()?;
    let text = query.to_lowercase();
    let references: BTreeSet<_> = entries
        .iter()
        .filter(|entry| {
            entry.platter_job_ref.to_lowercase().contains(&text)
                || entry
                    .notes
                    .as_ref()
                    .is_some_and(|notes| notes.to_lowercase().contains(&text))
        })
        .map(|entry| entry.platter_job_ref.clone())
        .collect();
    let client = clew::platter_client()?;
    let result = client
        .list(None)
        .context("cannot complete candidate search; Clew list/show remain available")?;
    let mut candidates: Vec<_> = result
        .items
        .into_iter()
        .filter(|item| {
            references.contains(&item.reference) || platter::opportunities::matches(item, query)
        })
        .collect();
    candidates.sort_by(|a, b| a.reference.cmp(&b.reference));
    let unmatched: Vec<_> = references
        .into_iter()
        .filter(|reference| !candidates.iter().any(|item| item.reference == *reference))
        .collect();
    Ok(
        json!({"candidates":candidates,"retained_references_without_platter_record":unmatched,"complete":true}),
    )
}
