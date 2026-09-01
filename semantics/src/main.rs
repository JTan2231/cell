#![allow(clippy::missing_errors_doc, clippy::too_many_lines)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use clap::{Args, Parser, Subcommand};
use conversations::{AppServerClient, ClientConfig, StderrPolicy};
use serde::Serialize;
use serde_json::json;

use semantics::adapters::{
    AppServerConversationLocator, DecisionEventSource, DecisionsCli, canonical_directory,
    require_participation_marker,
};
use semantics::domain::{IntakeStatus, ProjectStatus, validate_project_id};
use semantics::nucleus::NucleusReconciler;
use semantics::seed::{seed_markdown, seed_one};
use semantics::store::Store;
use semantics::worker::Worker;
use semantics::{Error, Result};

#[derive(Debug, Parser)]
#[command(version, about = "Project-scoped authoritative semantic repositories")]
struct Cli {
    #[arg(long, env = "SEMANTICS_DATABASE")]
    database: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Project(ProjectArgs),
    Repository(RepositoryArgs),
    Intake(IntakeArgs),
    Doctor,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    Register {
        id: String,
        root: PathBuf,
        #[arg(long, hide = true)]
        activation_cursor: Option<String>,
    },
    List,
    Show {
        id: String,
    },
    Move {
        id: String,
        root: PathBuf,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Retire {
        id: String,
    },
}

#[derive(Debug, Args)]
struct RepositoryArgs {
    #[command(subcommand)]
    command: RepositoryCommand,
}

#[derive(Debug, Subcommand)]
enum RepositoryCommand {
    Show {
        project: String,
        #[arg(long)]
        revision: Option<u64>,
    },
    Search {
        project: String,
        query: String,
        #[arg(long)]
        revision: Option<u64>,
    },
    Log {
        project: String,
        #[arg(long, default_value_t = 1)]
        from: u64,
        #[arg(long)]
        to: Option<u64>,
    },
    Diff {
        project: String,
        from: u64,
        to: u64,
    },
    Seed {
        project: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        meaning: String,
        #[arg(long)]
        grounding: Option<String>,
    },
    SeedMarkdown {
        project: String,
        path: PathBuf,
    },
}

#[derive(Debug, Args)]
struct IntakeArgs {
    #[command(subcommand)]
    command: IntakeCommand,
}

#[derive(Debug, Subcommand)]
enum IntakeCommand {
    Status {
        #[arg(long)]
        status: Option<String>,
    },
    Assign {
        event_id: String,
        project: String,
    },
    Retry {
        event_id: String,
    },
    #[command(hide = true)]
    Run,
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(error) = run(cli) {
        if json {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({
                    "ok": false,
                    "error": {"code": error.code(), "message": error.to_string()}
                }))
                .unwrap_or_else(|_| "{\"ok\":false}".to_owned())
            );
        } else {
            eprintln!("semantics: {error}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let database = cli.database.map_or_else(default_database, Ok)?;
    let store = Store::open(database)?;
    match cli.command {
        Command::Project(arguments) => project_command(&store, arguments.command, cli.json),
        Command::Repository(arguments) => repository_command(&store, arguments.command, cli.json),
        Command::Intake(arguments) => intake_command(&store, arguments.command, cli.json),
        Command::Doctor => doctor(&store, cli.json),
    }
}

fn project_command(store: &Store, command: ProjectCommand, compact: bool) -> Result<()> {
    match command {
        ProjectCommand::Register {
            id,
            root,
            activation_cursor,
        } => {
            validate_project_id(&id)?;
            let root = canonical_directory(&root)?;
            require_participation_marker(&root, &id)?;
            let cursor = match activation_cursor {
                Some(cursor) => cursor,
                None => DecisionsCli::for_current_user().watermark()?,
            };
            store.register_project(&id, &root, &cursor)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::List => print(&store.list_projects()?, compact),
        ProjectCommand::Show { id } => print(&store.project_detail(&id)?, compact),
        ProjectCommand::Move { id, root } => {
            let root = canonical_directory(&root)?;
            require_participation_marker(&root, &id)?;
            store.move_project(&id, &root)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::Pause { id } => {
            store.set_project_status(&id, ProjectStatus::Paused)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::Resume { id } => {
            let project = store.project(&id)?;
            require_participation_marker(Path::new(&project.current_path), &id)?;
            store.set_project_status(&id, ProjectStatus::Active)?;
            print(&store.project_detail(&id)?, compact)
        }
        ProjectCommand::Retire { id } => {
            store.set_project_status(&id, ProjectStatus::Retired)?;
            print(&store.project_detail(&id)?, compact)
        }
    }
}

fn repository_command(store: &Store, command: RepositoryCommand, compact: bool) -> Result<()> {
    match command {
        RepositoryCommand::Show { project, revision } => {
            print(&store.repository(&project, revision)?, compact)
        }
        RepositoryCommand::Search {
            project,
            query,
            revision,
        } => {
            let repository = store.repository(&project, revision)?;
            let needle = query.to_lowercase();
            let hits = repository
                .concepts
                .values()
                .filter(|concept| {
                    concept.label.to_lowercase().contains(&needle)
                        || concept.meaning.to_lowercase().contains(&needle)
                })
                .collect::<Vec<_>>();
            print(&hits, compact)
        }
        RepositoryCommand::Log { project, from, to } => {
            print(&store.revisions(&project, from, to)?, compact)
        }
        RepositoryCommand::Diff { project, from, to } => {
            print(&store.diff(&project, from, to)?, compact)
        }
        RepositoryCommand::Seed {
            project,
            label,
            meaning,
            grounding,
        } => {
            let revision = seed_one(store, &project, &label, &meaning, grounding.as_deref())?;
            print(
                &json!({"project_id": project, "revision": revision}),
                compact,
            )
        }
        RepositoryCommand::SeedMarkdown { project, path } => {
            let revision = seed_markdown(store, &project, &path)?;
            print(
                &json!({"project_id": project, "revision": revision}),
                compact,
            )
        }
    }
}

fn intake_command(store: &Store, command: IntakeCommand, compact: bool) -> Result<()> {
    match command {
        IntakeCommand::Status { status } => {
            let status = status.as_deref().map(IntakeStatus::from_str).transpose()?;
            print(&store.list_intake(status)?, compact)
        }
        IntakeCommand::Assign { event_id, project } => {
            let target = store.project(&project)?;
            require_participation_marker(Path::new(&target.current_path), &project)?;
            store.assign_intake(&event_id, &project)?;
            print(&store.intake(&event_id)?, compact)
        }
        IntakeCommand::Retry { event_id } => {
            NucleusReconciler::for_current_user().retry_failed(store, &event_id)?;
            print(&store.intake(&event_id)?, compact)
        }
        IntakeCommand::Run => {
            let conversations = AppServerConversationLocator::for_current_user()?;
            let worker = Worker::new(store, DecisionsCli::for_current_user(), conversations);
            print(&worker.run_once()?, compact)
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn doctor(store: &Store, compact: bool) -> Result<()> {
    let mut checks = Vec::new();
    checks.push(check("database", || {
        let version = store.schema_version()?;
        Ok(format!("schema {version} at {}", store.path().display()))
    }));
    checks.push(check("participation_markers", || {
        for project in store
            .list_projects()?
            .into_iter()
            .filter(|project| project.status != ProjectStatus::Retired)
        {
            require_participation_marker(Path::new(&project.current_path), &project.id)?;
        }
        Ok("all active and paused projects have exact-root markers".to_owned())
    }));
    checks.push(check("decisions_lifecycle", || {
        let cursor = DecisionsCli::for_current_user().watermark()?;
        Ok(format!("watermark captured ({} bytes)", cursor.len()))
    }));
    checks.push(check("conversations_exact_cwd", || {
        let mut client = AppServerClient::spawn(ClientConfig {
            stderr_policy: StderrPolicy::Suppress,
            ..ClientConfig::default()
        })?;
        let report = client.doctor()?;
        if !report.ok {
            return Err(Error::domain(
                "conversations_not_ready",
                "Conversations doctor reported not ready",
            ));
        }
        Ok(format!("{} visible threads", report.visible_threads))
    }));
    checks.push(check("nucleus_reconciliation", || {
        NucleusReconciler::for_current_user().doctor()?;
        Ok("health, capabilities, schemas, and immutable toolset verified".to_owned())
    }));
    let ok = checks.iter().all(|check| check.ok);
    print(&json!({"ok": ok, "checks": checks}), compact)?;
    if ok {
        Ok(())
    } else {
        Err(Error::domain(
            "doctor_failed",
            "one or more Semantics readiness checks failed",
        ))
    }
}

fn check(name: &'static str, operation: impl FnOnce() -> Result<String>) -> DoctorCheck {
    match operation() {
        Ok(detail) => DoctorCheck {
            name,
            ok: true,
            detail,
        },
        Err(error) => DoctorCheck {
            name,
            ok: false,
            detail: error.to_string(),
        },
    }
}

fn print(value: &impl Serialize, compact: bool) -> Result<()> {
    let output = if compact {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    println!("{output}");
    Ok(())
}

fn default_database() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            Error::domain(
                "home_unavailable",
                "HOME must be an absolute path or --database must be supplied",
            )
        })?;
    Ok(home.join("Library/Application Support/Semantics/semantics.db"))
}
