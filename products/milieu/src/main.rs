use clap::{Parser, Subcommand};
use milieu::{Result, store::Store};
use serde_json::{Value, json};
use std::path::PathBuf;

mod output;

fn emit_iatreion_snapshot() -> bool {
    if let Some(snapshot) = iatreion_api::requested_on_demand_snapshot_json(
        "milieu",
        env!("CARGO_PKG_VERSION"),
        "milieu/discovery",
        "milieu.discovery.explore",
    ) {
        chancery_usage::observe("milieu", "status-snapshot");
        println!("{snapshot}");
        true
    } else {
        false
    }
}

#[derive(Parser)]
#[command(name = "milieu", version, about = "Private company and job records")]
struct Cli {
    #[arg(long, global = true, env = "MILIEU_STATE_DIR")]
    state_dir: Option<PathBuf>,
    /// Emit structured JSON instead of readable text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    Status,
    Doctor,
    #[command(alias = "snapshot")]
    Export {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Companies {
        #[command(subcommand)]
        command: ListCommand,
    },
    Jobs {
        #[command(subcommand)]
        command: ListCommand,
    },
    Sources {
        #[command(subcommand)]
        command: ListCommand,
    },
    Company {
        #[command(subcommand)]
        command: ShowCommand,
    },
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}
#[derive(Subcommand)]
enum ListCommand {
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}
#[derive(Subcommand)]
enum ShowCommand {
    Show { id: String },
}
#[derive(Subcommand)]
enum JobCommand {
    Show { id: String },
}
#[derive(Subcommand)]
enum SourceCommand {
    Add {
        url: String,
        #[arg(long)]
        company_id: Option<String>,
    },
}

fn main() {
    if emit_iatreion_snapshot() {
        return;
    }
    let cli = chancery_usage::cli::parse::<Cli>("milieu", "");
    let json_output = cli.json;
    if let Err(error) = execute(cli) {
        if json_output {
            eprintln!(
                "{}",
                json!({"schema_version":1,"ok":false,"error":{"detail":error.to_string()}})
            );
        } else {
            eprintln!("milieu: {error}");
        }
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
fn execute(cli: Cli) -> Result<()> {
    let directory = cli.state_dir.unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/milieu")
    });
    let store = if matches!(&cli.command, Command::Init) {
        Store::init(&directory)?
    } else {
        Store::open(&directory)?
    };
    let human_command = if cli.json { None } else { Some(&cli.command) };
    let output: Value = match &cli.command {
        Command::Init => {
            let _lock = store.lock()?;
            json!({"schema_version":1,"state_dir":directory,"initialized":true})
        }
        Command::Status => store.status()?,
        Command::Doctor => {
            json!({"schema_version":2,"database":"ready","database_schema":2,"state_dir":directory})
        }
        Command::Export { output, .. } => {
            if let Some(path) = output {
                store.atomic_export(path)?;
                json!({"exported":path})
            } else {
                serde_json::to_value(store.snapshot()?)?
            }
        }
        Command::Companies {
            command: ListCommand::List { limit },
        } => {
            let snapshot = store.snapshot()?;
            page(
                snapshot.snapshot_revision,
                snapshot.companies.iter().map(company_summary).collect(),
                *limit,
            )?
        }
        Command::Jobs {
            command: ListCommand::List { limit },
        } => {
            let snapshot = store.snapshot()?;
            page(
                snapshot.snapshot_revision,
                snapshot
                    .jobs
                    .iter()
                    .map(|job| job_summary(job, &snapshot))
                    .collect(),
                *limit,
            )?
        }
        Command::Sources {
            command: ListCommand::List { limit },
        } => {
            let snapshot = store.snapshot()?;
            page(
                snapshot.snapshot_revision,
                snapshot.source_health.iter().map(source_summary).collect(),
                *limit,
            )?
        }
        Command::Company {
            command: ShowCommand::Show { id },
        } => serde_json::to_value(
            store
                .snapshot()?
                .companies
                .into_iter()
                .find(|c| c.id == *id)
                .ok_or("company not found")?,
        )?,
        Command::Job {
            command: JobCommand::Show { id },
        } => serde_json::to_value(
            store
                .snapshot()?
                .jobs
                .into_iter()
                .find(|j| j.id == *id)
                .ok_or("job not found")?,
        )?,
        Command::Source {
            command: SourceCommand::Add { url, company_id },
        } => serde_json::to_value(store.add_manual_source(url, company_id.as_deref())?)?,
        Command::Search { query, limit } => {
            if query.trim().is_empty() {
                return Err("search query must not be empty".into());
            }
            let folded = query.to_lowercase();
            let snapshot = store.snapshot()?;
            let mut items = Vec::new();
            for company in &snapshot.companies {
                if let Some((field, text)) = [
                    ("name", company.name.as_str()),
                    ("domain", company.domain.as_deref().unwrap_or_default()),
                ]
                .into_iter()
                .find(|(_, text)| text.to_lowercase().contains(&folded))
                {
                    let mut item = company_summary(company);
                    item["matched_field"] = json!(field);
                    item["excerpt"] = json!(match_excerpt(text, query));
                    items.push(item);
                }
            }
            for job in &snapshot.jobs {
                if let Some((field, text)) = [
                    ("title", job.title.as_str()),
                    (
                        "description",
                        job.description.as_deref().unwrap_or_default(),
                    ),
                ]
                .into_iter()
                .find(|(_, text)| text.to_lowercase().contains(&folded))
                {
                    let mut item = job_summary(job, &snapshot);
                    item["matched_field"] = json!(field);
                    item["excerpt"] = json!(match_excerpt(text, query));
                    items.push(item);
                }
            }
            page(snapshot.snapshot_revision, items, *limit)?
        }
    };
    if let Some(command) = human_command {
        print!("{}", output::human(command, &output));
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }
    Ok(())
}

fn company_summary(company: &milieu::models::Company) -> Value {
    json!({"kind":"company","id":company.id,"revision":company.revision,"name":company.name,"domain":company.domain})
}
fn job_summary(job: &milieu::models::Job, snapshot: &milieu::models::Snapshot) -> Value {
    let company = snapshot
        .companies
        .iter()
        .find(|company| company.id == job.company_id);
    json!({"kind":"job","id":job.id,"revision":job.revision,"company_id":job.company_id,
        "company":company.map(|company| &company.name),"title":job.title,"location":job.location,"remote":job.remote,
        "geographic_eligibility":job.geographic_eligibility,"availability":job.availability,"last_seen_at":job.last_seen_at})
}
fn source_summary(source: &milieu::models::Source) -> Value {
    json!({"kind":"source","id":source.id,"url":source.url,"company_id":source.company_id,"enabled":source.enabled,"status":source.status,
        "last_attempt_at":source.last_attempt_at,"last_success_at":source.last_success_at,"next_due_at":source.next_due_at,"note":source.note})
}
fn page(revision: u64, mut items: Vec<Value>, limit: usize) -> Result<Value> {
    if limit == 0 {
        return Err("--limit must be positive".into());
    }
    let has_more = items.len() > limit;
    items.truncate(limit);
    Ok(json!({"schema_version":2,"snapshot_revision":revision,"items":items,"has_more":has_more}))
}

fn match_excerpt(text: &str, query: &str) -> String {
    let position = text.to_lowercase().find(&query.to_lowercase()).unwrap_or(0);
    let mut folded_bytes = 0;
    let matched = text
        .chars()
        .take_while(|ch| {
            let before = folded_bytes;
            folded_bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
            before < position
        })
        .count();
    let chars: Vec<_> = text.chars().collect();
    let start = matched.saturating_sub(60);
    let end = (start + 238).min(chars.len());
    let mut result = String::new();
    if start > 0 {
        result.push('…');
    }
    result.extend(
        chars[start..end]
            .iter()
            .map(|ch| if ch.is_whitespace() { ' ' } else { *ch }),
    );
    if end < chars.len() {
        result.push('…');
    }
    result
}
