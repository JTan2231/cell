use std::env;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser as _;
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{CaseCommand, Cli, Command, TellArgs, UpdateCommand, WorkerCommand};
use crate::model::{CaseListItem, CaseRevision, SearchResult, StewardUpdate};
use crate::nucleus::NucleusSteward;
use crate::store::Store;
use crate::worker::{Worker, activate, activate_resume};
use crate::{Error, Result};

const MAX_TEXT_BYTES: u64 = 1024 * 1024;

struct Output {
    data: Value,
    human: String,
}

struct CommandFailure {
    error: Error,
    context: Option<Box<UpdateView>>,
}

impl CommandFailure {
    fn for_update(store: &Store, update: StewardUpdate, error: Error) -> Self {
        Self {
            error,
            context: update_view(store, update).map(Box::new).ok(),
        }
    }
}

impl From<Error> for CommandFailure {
    fn from(error: Error) -> Self {
        Self {
            error,
            context: None,
        }
    }
}

type CommandResult<T> = std::result::Result<T, CommandFailure>;

#[derive(Serialize)]
struct UpdateView {
    #[serde(flatten)]
    update: StewardUpdate,
    advisory: Option<String>,
    attention: bool,
}

pub fn main_entry() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _printed = error.print();
            return exit_code;
        }
    };
    let compact = cli.json;
    match run(cli) {
        Ok(Some(output)) => {
            let rendered = if compact {
                serde_json::to_string(&json!({"ok": true, "data": output.data}))
                    .unwrap_or_else(|_| "{\"ok\":false}".to_owned())
            } else {
                output.human
            };
            println!("{rendered}");
            0
        }
        Ok(None) => 0,
        Err(failure) => {
            let error = &failure.error;
            if compact {
                let mut response = json!({
                    "ok": false,
                    "error": {"code": error.code(), "message": error.to_string()}
                });
                if let Some(update) = failure.context.as_ref() {
                    response["context"] = json!({
                        "type": "update",
                        "update": update
                    });
                }
                eprintln!(
                    "{}",
                    serde_json::to_string(&response)
                        .unwrap_or_else(|_| "{\"ok\":false}".to_owned())
                );
            } else {
                let advisory = failure
                    .context
                    .as_ref()
                    .and_then(|context| context.advisory.as_deref());
                eprintln!("{}crm: {error}", advisory_prefix(advisory));
            }
            1
        }
    }
}

fn run(cli: Cli) -> CommandResult<Option<Output>> {
    let database = resolve_database(cli.database.as_deref())?;
    match cli.command {
        Command::Init => {
            let result = Store::init(&database)?;
            Ok(Some(Output {
                data: json!({
                    "type": "init",
                    "database": database,
                    "schema_version": result.schema_version,
                    "created": result.created
                }),
                human: if result.created {
                    format!("Initialized CRM at {}", database.display())
                } else {
                    format!("CRM is already initialized at {}", database.display())
                },
            }))
        }
        Command::Doctor => {
            let database_health = Store::doctor(&database)?;
            NucleusSteward::for_current_user().doctor()?;
            Ok(Some(Output {
                data: json!({
                    "type": "doctor",
                    "database": database,
                    "schema_version": database_health.schema_version,
                    "foreign_keys": database_health.foreign_keys,
                    "integrity": database_health.integrity,
                    "nucleus": "ready"
                }),
                human: format!(
                    "ready: CRM schema {}, SQLite {}, Nucleus ready ({})",
                    database_health.schema_version,
                    database_health.integrity,
                    database.display()
                ),
            }))
        }
        Command::Case { command } => {
            let store = Store::open(database)?;
            Ok(Some(case_command(&store, command)?))
        }
        Command::Search(arguments) => {
            let store = Store::open(database)?;
            let results = store.search(&arguments.query, arguments.limit)?;
            Ok(Some(Output {
                data: json!({"type": "search_results", "results": results}),
                human: render_search(&results),
            }))
        }
        Command::Tell(arguments) => {
            let store = Store::open(database)?;
            Ok(Some(tell(&store, &arguments)?))
        }
        Command::Update { command } => {
            let store = Store::open(database)?;
            Ok(Some(update_command(&store, command)?))
        }
        Command::Worker { command } => {
            let store = Store::open(database)?;
            match command {
                WorkerCommand::Drain => {
                    Worker::new(&store).drain()?;
                }
                WorkerCommand::Resume { update } => {
                    Worker::new(&store).resume(&update)?;
                }
            }
            Ok(None)
        }
    }
}

fn case_command(store: &Store, command: CaseCommand) -> Result<Output> {
    match command {
        CaseCommand::New {
            title,
            input,
            stage,
        } => {
            let markdown = match input {
                Some(path) => read_text(&path)?,
                None => format!(
                    "# {}\n\n## Current picture\n\n## People\n\n## Chronicle\n\n## Open threads\n",
                    title.trim()
                ),
            };
            let revision = store.create_case(&title, &markdown, stage)?;
            Ok(Output {
                data: json!({"type": "case_created", "case": revision}),
                human: format!(
                    "Created {} revision {} [{}]",
                    revision.case_id, revision.revision, revision.stage
                ),
            })
        }
        CaseCommand::List { limit } => {
            let cases = store.list_cases(limit)?;
            Ok(Output {
                data: json!({"type": "case_list", "cases": cases}),
                human: render_case_list(&cases),
            })
        }
        CaseCommand::Show { case, revision } => {
            let revision = store.case_revision(&case, revision)?;
            Ok(Output {
                data: json!({"type": "case_revision", "case": revision}),
                human: render_case(&revision),
            })
        }
        CaseCommand::History { case } => {
            let revisions = store.case_history(&case)?;
            Ok(Output {
                data: json!({"type": "case_history", "revisions": revisions}),
                human: render_history(&revisions),
            })
        }
    }
}

fn tell(store: &Store, arguments: &TellArgs) -> Result<Output> {
    let body = read_text(&arguments.input)?;
    let label = arguments.name.as_deref().unwrap_or("New information");
    let update =
        store.enqueue_delivery(&arguments.case, label, &body, arguments.source.as_deref())?;
    let activation_warning = activate(store.path()).err().map(|error| error.to_string());
    let view = update_view(store, update)?;
    Ok(Output {
        data: json!({
            "type": "update_queued",
            "update": view,
            "activation_warning": activation_warning
        }),
        human: match activation_warning {
            Some(warning) => format!(
                "{}Queued {} for {}. Worker activation needs attention: {warning}",
                advisory_prefix(view.advisory.as_deref()),
                view.update.id,
                view.update.case_id
            ),
            None => format!(
                "{}Queued {} for {}",
                advisory_prefix(view.advisory.as_deref()),
                view.update.id,
                view.update.case_id
            ),
        },
    })
}

fn update_command(store: &Store, command: UpdateCommand) -> CommandResult<Output> {
    match command {
        UpdateCommand::List { limit } => {
            let updates = store.list_updates(limit)?;
            let views = updates
                .into_iter()
                .map(|update| update_view(store, update))
                .collect::<Result<Vec<_>>>()?;
            Ok(Output {
                data: json!({"type": "update_list", "updates": views}),
                human: render_updates(&views),
            })
        }
        UpdateCommand::Show { update } => Ok(update_output(store, store.update(&update)?)?),
        UpdateCommand::Wait { update, timeout } => {
            let started = Instant::now();
            let timeout = Duration::from_secs(timeout);
            let initial = store.update(&update)?;
            if initial.needs_worker() {
                if matches!(
                    initial.status,
                    crate::model::UpdateStatus::Running | crate::model::UpdateStatus::Applied
                ) {
                    if let Err(error) = activate_resume(store.path(), &update) {
                        return Err(CommandFailure::for_update(store, initial, error));
                    }
                } else if let Err(error) = activate(store.path()) {
                    return Err(CommandFailure::for_update(store, initial, error));
                }
            }
            loop {
                let current = store.update(&update)?;
                if current.is_settled() {
                    break Ok(update_output(store, current)?);
                }
                if started.elapsed() >= timeout {
                    let error = Error::domain(
                        "update_wait_timeout",
                        format!("update {update} is still {}", current.status.as_str()),
                    );
                    return Err(CommandFailure::for_update(store, current, error));
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
        UpdateCommand::Resume { update } => {
            let initial = store.update(&update)?;
            if let Err(error) = Worker::new(store).resume(&update) {
                let current = store.update(&update).unwrap_or(initial);
                return Err(CommandFailure::for_update(store, current, error));
            }
            Ok(update_output(store, store.update(&update)?)?)
        }
        UpdateCommand::Retry { update } => {
            let retry = match store.enqueue_retry(&update) {
                Ok(retry) => retry,
                Err(error) => {
                    let failure = match store.update(&update) {
                        Ok(current) => CommandFailure::for_update(store, current, error),
                        Err(_) => CommandFailure::from(error),
                    };
                    return Err(failure);
                }
            };
            let activation_warning = activate(store.path()).err().map(|error| error.to_string());
            let view = update_view(store, retry)?;
            Ok(Output {
                data: json!({
                    "type": "update_retried",
                    "update": view,
                    "activation_warning": activation_warning
                }),
                human: match activation_warning {
                    Some(warning) => format!(
                        "{}Queued retry {} for {}. Worker activation needs attention: {warning}",
                        advisory_prefix(view.advisory.as_deref()),
                        view.update.id,
                        view.update.case_id
                    ),
                    None => format!(
                        "{}Queued retry {} for {}",
                        advisory_prefix(view.advisory.as_deref()),
                        view.update.id,
                        view.update.case_id
                    ),
                },
            })
        }
    }
}

fn update_output(store: &Store, update: StewardUpdate) -> Result<Output> {
    let view = update_view(store, update)?;
    Ok(Output {
        data: json!({"type": "update", "update": view}),
        human: render_update(&view),
    })
}

fn update_view(store: &Store, update: StewardUpdate) -> Result<UpdateView> {
    let relevant_revision = update.applied_revision.or(update.base_revision);
    let revision = store.case_revision(&update.case_id, relevant_revision)?;
    let advisory = revision.advisory;
    Ok(UpdateView {
        attention: advisory.is_some(),
        advisory,
        update,
    })
}

fn resolve_database(explicit: Option<&Path>) -> Result<PathBuf> {
    let path = if let Some(path) = explicit {
        if path.as_os_str().is_empty() {
            return Err(Error::domain(
                "database_path_empty",
                "--database must not be empty",
            ));
        }
        path.to_path_buf()
    } else if let Some(path) = env::var_os("CRM_DATABASE").filter(|value| !value.is_empty()) {
        PathBuf::from(path)
    } else {
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::domain(
                    "home_unavailable",
                    "HOME is required when --database and CRM_DATABASE are not set",
                )
            })?;
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("CRM")
            .join("crm.db")
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|source| crate::error::io("current directory", source))
    }
}

fn read_text(path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    if path == Path::new("-") {
        std::io::stdin()
            .lock()
            .take(MAX_TEXT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| crate::error::io("standard input", source))?;
    } else {
        let metadata =
            fs::symlink_metadata(path).map_err(|source| crate::error::io(path, source))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::domain(
                "input_not_regular",
                format!("input must be a regular file: {}", path.display()),
            ));
        }
        fs::File::open(path)
            .map_err(|source| crate::error::io(path, source))?
            .take(MAX_TEXT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| crate::error::io(path, source))?;
    }
    if bytes.len() > usize::try_from(MAX_TEXT_BYTES).unwrap_or(usize::MAX) {
        return Err(Error::domain(
            "input_too_large",
            "input exceeds 1048576 bytes",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| Error::domain("input_not_utf8", "input must be valid UTF-8"))
}

fn advisory_prefix(advisory: Option<&str>) -> String {
    advisory.map_or_else(String::new, |value| {
        format!("ATTENTION — STEWARD ADVISORY (NON-BLOCKING)\n{value}\n\n")
    })
}

fn render_case(revision: &CaseRevision) -> String {
    format!(
        "{}{} revision {} [{}]\nSummary: {}\nRecorded: {}\n\n{}",
        advisory_prefix(revision.advisory.as_deref()),
        revision.case_id,
        revision.revision,
        revision.stage,
        revision.summary,
        revision.created_at,
        revision.markdown
    )
}

fn render_case_list(cases: &[CaseListItem]) -> String {
    if cases.is_empty() {
        return "No cases.".to_owned();
    }
    cases
        .iter()
        .map(|case| {
            format!(
                "{}{}@{} [{}] {}\n  {}",
                advisory_prefix(case.advisory.as_deref()),
                case.case_id,
                case.revision,
                case.stage,
                one_line(&case.title),
                one_line(&case.summary)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_history(revisions: &[CaseRevision]) -> String {
    revisions
        .iter()
        .map(|revision| {
            format!(
                "{}{}@{} [{}] — {} ({})",
                advisory_prefix(revision.advisory.as_deref()),
                revision.case_id,
                revision.revision,
                revision.stage,
                one_line(&revision.summary),
                revision.created_at
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_search(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No matching cases.".to_owned();
    }
    results
        .iter()
        .map(|result| {
            format!(
                "{}{}@{} [{}] {}\n  {}",
                advisory_prefix(result.advisory.as_deref()),
                result.case_id,
                result.revision,
                result.stage,
                one_line(&result.title),
                one_line(&result.snippet)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_updates(updates: &[UpdateView]) -> String {
    if updates.is_empty() {
        return "No steward updates.".to_owned();
    }
    updates
        .iter()
        .map(render_update)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_update(view: &UpdateView) -> String {
    let update = &view.update;
    let revision = update
        .applied_revision
        .map_or_else(|| "-".to_owned(), |value| value.to_string());
    let error = update
        .last_error
        .as_deref()
        .map_or_else(String::new, |value| {
            format!("\n  diagnostic: {}", one_line(value))
        });
    let runtime = update.runtime_state.as_deref().map_or_else(
        || {
            if update.status == crate::model::UpdateStatus::Applied {
                "\n  runtime: awaiting terminal observation".to_owned()
            } else {
                String::new()
            }
        },
        |state| {
            let detail = update
                .runtime_detail
                .as_deref()
                .map_or_else(String::new, |value| format!(" — {}", one_line(value)));
            format!("\n  runtime: {state}{detail}")
        },
    );
    format!(
        "{}{} [{}] case={} revision={}{}{}",
        advisory_prefix(view.advisory.as_deref()),
        update.id,
        update.status.as_str(),
        update.case_id,
        revision,
        error,
        runtime
    )
}

fn one_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}
