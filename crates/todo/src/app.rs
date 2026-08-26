use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{Cli, Command, ListArgs, NewArgs, NoteAddArgs, NoteCommand, SearchArgs};
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::model::{ModelQuality, TodoId, TodoSummary, TodoView};
use crate::render::{CommandOutput, terminal_text};
use crate::{db, liaison, todo_store};

const MAX_PAGE_LIMIT: u32 = 1_000;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedNewArgs {
    pub(crate) direction: String,
    pub(crate) source: PathBuf,
    pub(crate) quality: Option<ModelQuality>,
    pub(crate) model: Option<String>,
}

pub(crate) fn database_path(
    explicit: Option<&PathBuf>,
    config: &Config,
) -> Result<PathBuf, AppError> {
    let environment = std::env::var_os("TODO_DATABASE");
    resolve_database_path(
        explicit.map(PathBuf::as_path),
        environment.as_deref(),
        config.database.as_deref(),
    )
}

fn resolve_database_path(
    explicit: Option<&Path>,
    environment: Option<&OsStr>,
    configured: Option<&Path>,
) -> Result<PathBuf, AppError> {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| {
            environment
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| configured.map(Path::to_path_buf))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "database_not_configured",
                "no database is configured; pass --database, set TODO_DATABASE, or select a configuration that defines database",
            )
        })
}

pub(crate) fn run(cli: &Cli, config: &Config, database: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init => initialize(database),
        Command::New(args) => {
            let resolved = resolve_new_args(args)?;
            liaison::create(database, config, &resolved, !cli.json)
        }
        Command::List(args) => list_todos(database, args),
        Command::Search(args) => search_todos(database, args),
        Command::Show(args) => show_todo(database, args.id),
        Command::Note(command) => match command {
            NoteCommand::Add(args) => add_note(database, args),
        },
        Command::Done(args) => done(database, args.id),
        Command::Reopen(args) => reopen(database, args.id),
    }
}

fn initialize(database: &Path) -> AppResult<CommandOutput> {
    db::init(database)?;
    Ok(CommandOutput::new(
        json!({ "database": database.display().to_string() }),
        format!("Initialized Todo database {}", database.display()),
    )
    .mutation())
}

fn list_todos(database: &Path, args: &ListArgs) -> AppResult<CommandOutput> {
    validate_limit(args.limit)?;
    let connection = db::open_read(database)?;
    let todos = todo_store::list(&connection, args.all, args.limit)?;
    let human = render_summaries(
        &todos,
        if args.all {
            "No todos"
        } else {
            "No open todos"
        },
    );
    Ok(CommandOutput::new(json!({ "todos": todos }), human))
}

fn search_todos(database: &Path, args: &SearchArgs) -> AppResult<CommandOutput> {
    validate_limit(args.limit)?;
    if args.query.trim().is_empty() {
        return Err(AppError::invalid(
            "blank_search_query",
            "search query must not be blank",
        ));
    }
    let connection = db::open_read(database)?;
    let todos = todo_store::search(&connection, &args.query, args.all, args.limit)?;
    let human = render_summaries(&todos, "No matching todos");
    Ok(CommandOutput::new(
        json!({ "query": args.query, "todos": todos }),
        human,
    ))
}

fn show_todo(database: &Path, id: TodoId) -> AppResult<CommandOutput> {
    let connection = db::open_read(database)?;
    let view = todo_store::show(&connection, id)?;
    let human = render_todo(&view);
    Ok(CommandOutput::new(to_value(&view)?, human))
}

fn add_note(database: &Path, args: &NoteAddArgs) -> AppResult<CommandOutput> {
    let text = read_text_argument(&args.text)?;
    let mut connection = db::open_write(database)?;
    let note = todo_store::append_note(&mut connection, args.id, &text)?;
    Ok(CommandOutput::new(
        json!({ "todo": args.id, "working_note": note }),
        format!("Added working note to {}", args.id),
    )
    .mutation())
}

fn done(database: &Path, id: TodoId) -> AppResult<CommandOutput> {
    let mut connection = db::open_write(database)?;
    let transition = todo_store::mark_done(&mut connection, id)?;
    let human = if transition.changed {
        format!("Completed {id}")
    } else {
        format!("{id} is already done")
    };
    Ok(CommandOutput::new(
        json!({ "todo": transition.todo, "changed": transition.changed }),
        human,
    )
    .mutation())
}

fn reopen(database: &Path, id: TodoId) -> AppResult<CommandOutput> {
    let mut connection = db::open_write(database)?;
    let transition = todo_store::reopen(&mut connection, id)?;
    let human = if transition.changed {
        format!("Reopened {id}")
    } else {
        format!("{id} is already open")
    };
    Ok(CommandOutput::new(
        json!({ "todo": transition.todo, "changed": transition.changed }),
        human,
    )
    .mutation())
}

fn resolve_new_args(args: &NewArgs) -> AppResult<ResolvedNewArgs> {
    if args.direction.trim().is_empty() {
        return Err(AppError::invalid(
            "blank_direction",
            "direction must not be blank",
        ));
    }
    if args
        .model
        .as_ref()
        .is_some_and(|model| model.trim().is_empty())
    {
        return Err(AppError::invalid(
            "invalid_model",
            "model must not be blank",
        ));
    }
    let source = fs::canonicalize(&args.source).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AppError::not_found(
                "source_not_found",
                format!("source not found: {}", args.source.display()),
            )
        } else {
            AppError::invalid(
                "source_unreadable",
                format!(
                    "unable to resolve source {}: {error}",
                    args.source.display()
                ),
            )
        }
    })?;
    let metadata = fs::metadata(&source).map_err(|error| {
        AppError::invalid(
            "source_unreadable",
            format!("unable to inspect source {}: {error}", source.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::invalid(
            "invalid_source",
            format!("source is not a regular file: {}", source.display()),
        ));
    }
    fs::read_to_string(&source).map_err(|error| {
        AppError::invalid(
            "source_unreadable",
            format!(
                "source must be readable UTF-8 text {}: {error}",
                source.display()
            ),
        )
    })?;
    if source.to_str().is_none() {
        return Err(AppError::invalid(
            "invalid_source_path",
            "resolved source path must contain valid UTF-8",
        ));
    }
    Ok(ResolvedNewArgs {
        direction: args.direction.clone(),
        source,
        quality: args.quality,
        model: args.model.clone(),
    })
}

fn validate_limit(limit: u32) -> AppResult<()> {
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(AppError::invalid(
            "invalid_limit",
            format!("limit must be between 1 and {MAX_PAGE_LIMIT}"),
        ));
    }
    Ok(())
}

fn read_text_argument(argument: &str) -> AppResult<String> {
    if argument != "-" {
        return Ok(argument.to_owned());
    }
    let mut text = String::new();
    io::stdin().read_to_string(&mut text).map_err(|error| {
        AppError::invalid(
            "stdin_read_failed",
            format!("unable to read working note from standard input: {error}"),
        )
    })?;
    Ok(text)
}

fn render_summaries(todos: &[TodoSummary], empty: &str) -> String {
    if todos.is_empty() {
        return empty.to_owned();
    }
    todos
        .iter()
        .map(|todo| {
            format!(
                "{}\t{}\t{}",
                todo.id,
                todo.status,
                terminal_text(&todo.title, false)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_todo(view: &TodoView) -> String {
    let todo = &view.todo;
    let mut human = format!(
        "{} [{}] {}\nCreated: {}\nSource: {}\nDirection: {}",
        todo.id,
        todo.status,
        terminal_text(&todo.title, false),
        terminal_text(&todo.created_at, false),
        terminal_text(&todo.source_path.display().to_string(), false),
        terminal_text(&todo.pointer, false),
    );
    if let Some(completed_at) = &todo.completed_at {
        let _ = write!(human, "\nCompleted: {}", terminal_text(completed_at, false));
    }
    human.push_str("\n\n");
    human.push_str(&terminal_text(&todo.note, true));
    if !view.working_notes.is_empty() {
        human.push_str("\n\nWorking notes");
        for note in &view.working_notes {
            let _ = write!(
                human,
                "\n\n{}\n{}",
                terminal_text(&note.created_at, false),
                terminal_text(&note.text, true)
            );
        }
    }
    human
}

fn to_value<T: Serialize>(value: &T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::resolve_database_path;

    #[test]
    fn database_selection_has_explicit_environment_config_precedence()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            resolve_database_path(
                Some(Path::new("explicit.db")),
                Some(OsStr::new("environment.db")),
                Some(Path::new("configured.db"))
            )?,
            Path::new("explicit.db")
        );
        assert_eq!(
            resolve_database_path(
                None,
                Some(OsStr::new("environment.db")),
                Some(Path::new("configured.db"))
            )?,
            Path::new("environment.db")
        );
        assert_eq!(
            resolve_database_path(None, Some(OsStr::new("")), Some(Path::new("configured.db")))?,
            Path::new("configured.db")
        );
        Ok(())
    }
}
