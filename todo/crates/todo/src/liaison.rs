use std::path::Path;

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::ResolvedNewArgs;
use crate::config::Config;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::model::Todo;
use crate::model_runner::{ModelSettings, Runner};
use crate::render::{CommandOutput, terminal_text};
use crate::todo_store::{self, CreateTodo};
use crate::tool_server::{Backend, Tool, ToolFailure, ToolSuccess};

pub(crate) fn create(
    path: &Path,
    config: &Config,
    args: &ResolvedNewArgs,
    forward_progress: bool,
) -> AppResult<CommandOutput> {
    let quality = args.quality.unwrap_or(config.liaison.quality);
    let model = args.model.as_deref().or(config.liaison.model.as_deref());
    let settings = ModelSettings::new(quality, model);
    let runner = Runner::for_current_user();
    let working_directory = std::env::current_dir().map_err(|error| {
        AppError::unexpected(
            "working_directory_unavailable",
            format!("unable to determine the caller's working directory: {error}"),
        )
    })?;
    create_with_runner(
        path,
        args,
        &settings,
        &runner,
        &working_directory,
        forward_progress,
    )
}

fn create_with_runner(
    path: &Path,
    args: &ResolvedNewArgs,
    settings: &ModelSettings,
    runner: &Runner,
    working_directory: &Path,
    forward_progress: bool,
) -> AppResult<CommandOutput> {
    let connection = db::open_write(path)?;
    let mut backend = CreationBackend::new(connection, &args.direction, &args.source);
    let prompt = invocation_prompt(&args.source, &args.direction, working_directory)?;
    let result = runner.run_liaison(
        settings,
        &prompt,
        working_directory,
        &mut backend,
        forward_progress,
    );

    if let Some(todo) = backend.created.take() {
        let mut output = CommandOutput::new(
            json!({ "todo": todo }),
            format!("Created {}: {}", todo.id, terminal_text(&todo.title, false)),
        )
        .mutation();
        if let Err(error) = result {
            output.diagnostics = format!(
                "todo: the todo was created, but the research liaison ended afterward: {error}"
            );
        }
        return Ok(output);
    }

    if let Some(error) = backend.failure.take() {
        return Err(error);
    }
    match result {
        Ok(_) => Err(AppError::unexpected(
            "model_did_not_create_todo",
            "the research liaison exited without creating a todo",
        )),
        Err(error) => Err(error),
    }
}

fn invocation_prompt(
    source: &Path,
    direction: &str,
    working_directory: &Path,
) -> AppResult<String> {
    let source = source.to_str().ok_or_else(|| {
        AppError::invalid(
            "invalid_source_path",
            "resolved source path must contain valid UTF-8",
        )
    })?;
    let working_directory = working_directory.to_str().ok_or_else(|| {
        AppError::invalid(
            "invalid_working_directory",
            "caller working directory must contain valid UTF-8",
        )
    })?;
    let source = serde_json::to_string(source)?;
    let direction = serde_json::to_string(direction)?;
    let working_directory = serde_json::to_string(working_directory)?;
    Ok(format!(
        "Research this need and create one actionable todo. The source is the provenance and first research lead, not the boundary of your investigation. Use the caller's working directory to identify the exact local subject before considering analogies.\n\nSource path:\n{source}\n\nCaller working directory:\n{working_directory}\n\nDirection:\n{direction}"
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateTodoArguments {
    title: String,
    note: String,
}

struct CreationBackend<'a> {
    connection: Connection,
    direction: &'a str,
    source: &'a Path,
    created: Option<Todo>,
    failure: Option<AppError>,
}

impl<'a> CreationBackend<'a> {
    fn new(connection: Connection, direction: &'a str, source: &'a Path) -> Self {
        Self {
            connection,
            direction,
            source,
            created: None,
            failure: None,
        }
    }

    fn create_todo(&mut self, arguments: Value) -> Result<ToolSuccess, ToolFailure> {
        if self.created.is_some() {
            return Err(ToolFailure::new(
                "todo_already_created",
                "this liaison session has already created its todo",
            ));
        }
        let arguments: CreateTodoArguments =
            serde_json::from_value(arguments).map_err(|error| {
                ToolFailure::new(
                    "invalid_arguments",
                    format!("create_todo arguments are invalid: {error}"),
                )
            })?;
        if arguments.title.trim().is_empty() {
            return Err(ToolFailure::new(
                "invalid_title",
                "todo title must not be blank",
            ));
        }
        if arguments.title.contains(['\n', '\r']) {
            return Err(ToolFailure::new(
                "invalid_title",
                "todo title must be one line",
            ));
        }
        if arguments.note.trim().is_empty() {
            return Err(ToolFailure::new(
                "invalid_note",
                "todo note must not be blank",
            ));
        }

        match todo_store::create(
            &mut self.connection,
            CreateTodo {
                title: &arguments.title,
                note: &arguments.note,
                pointer: self.direction,
                source_path: self.source,
            },
        ) {
            Ok(todo) => {
                let output = json!({
                    "created": true,
                    "todo": {
                        "id": todo.id,
                        "title": todo.title
                    }
                });
                self.created = Some(todo);
                Ok(ToolSuccess::created(output))
            }
            Err(error) => {
                let code = error.code();
                let message = error.to_string();
                if self.failure.is_none() {
                    self.failure = Some(error);
                }
                Err(ToolFailure::new(code, message))
            }
        }
    }
}

impl Backend for CreationBackend<'_> {
    fn call(&mut self, tool: Tool, arguments: Value) -> Result<ToolSuccess, ToolFailure> {
        match tool {
            Tool::CreateTodo => self.create_todo(arguments),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{CreationBackend, invocation_prompt};
    use crate::db;
    use crate::tool_server::{Backend, Tool, ToolFailure};

    #[test]
    fn invocation_keeps_source_and_direction_roles_distinct()
    -> Result<(), Box<dyn std::error::Error>> {
        let prompt = invocation_prompt(
            Path::new("/tmp/source transcript.md"),
            "need to provide token consumption stats",
            Path::new("/tmp/project"),
        )?;
        assert!(prompt.contains("provenance and first research lead"));
        assert!(prompt.contains(r#""/tmp/source transcript.md""#));
        assert!(prompt.contains(r#""/tmp/project""#));
        assert!(prompt.contains(r#""need to provide token consumption stats""#));
        Ok(())
    }

    #[test]
    fn backend_validates_then_durably_creates_exactly_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let connection = db::init(&database)?;
        let mut backend = CreationBackend::new(
            connection,
            "research usage reporting",
            Path::new("/tmp/source.md"),
        );

        let blank = backend.call(
            Tool::CreateTodo,
            json!({ "title": " ", "note": "Useful note" }),
        );
        assert_eq!(
            blank.as_ref().err().map(ToolFailure::code),
            Some("invalid_title")
        );
        assert!(backend.created.is_none());

        let Ok(created) = backend.call(
            Tool::CreateTodo,
            json!({ "title": "Report liaison token usage", "note": "Actionable note" }),
        ) else {
            panic!("valid creation was rejected");
        };
        assert!(created.todo_created());
        assert_eq!(
            backend.created.as_ref().map(|todo| todo.id.to_string()),
            Some("t1".to_owned())
        );

        let duplicate = backend.call(
            Tool::CreateTodo,
            json!({ "title": "Another", "note": "Another note" }),
        );
        assert_eq!(
            duplicate.as_ref().err().map(ToolFailure::code),
            Some("todo_already_created")
        );
        Ok(())
    }
}
