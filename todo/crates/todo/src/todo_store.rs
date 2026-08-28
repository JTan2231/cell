use std::path::Path;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension as _, Row, TransactionBehavior, params};

use crate::error::{AppError, AppResult};
use crate::model::{Todo, TodoId, TodoStatus, TodoSummary, TodoView, WorkingNote};

#[derive(Clone, Copy)]
pub(crate) struct CreateTodo<'a> {
    pub(crate) title: &'a str,
    pub(crate) note: &'a str,
    pub(crate) pointer: &'a str,
    pub(crate) source_path: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transition {
    pub(crate) todo: Todo,
    pub(crate) changed: bool,
}

pub(crate) fn create(connection: &mut Connection, input: CreateTodo<'_>) -> AppResult<Todo> {
    validate_content("title", input.title)?;
    if input.title.contains('\n') || input.title.contains('\r') {
        return Err(AppError::invalid(
            "invalid_todo_title",
            "todo title must be one line",
        ));
    }
    validate_content("note", input.note)?;
    validate_content("pointer", input.pointer)?;
    if !input.source_path.is_absolute() {
        return Err(AppError::invalid(
            "invalid_source_path",
            "source path must be absolute",
        ));
    }
    let source_path = input.source_path.to_str().ok_or_else(|| {
        AppError::invalid(
            "invalid_source_path",
            "source path must contain valid UTF-8",
        )
    })?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT INTO todos(title, note, pointer, source_path)
         VALUES(?1, ?2, ?3, ?4)",
        params![input.title, input.note, input.pointer, source_path],
    )?;
    let id = TodoId::from_storage(transaction.last_insert_rowid()).map_err(|error| {
        AppError::database(
            "invalid_stored_todo_id",
            format!("database generated an invalid todo ID: {error}"),
        )
    })?;
    let todo = get_todo(&transaction, id)?;
    transaction.commit()?;
    Ok(todo)
}

pub(crate) fn list(
    connection: &Connection,
    include_done: bool,
    limit: u32,
) -> AppResult<Vec<TodoSummary>> {
    let mut statement = connection.prepare(
        "SELECT id, title, status, created_at, completed_at
         FROM todos
         WHERE ?1 OR status = 'open'
         ORDER BY created_at DESC, id DESC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![include_done, i64::from(limit)], summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn list_open(connection: &Connection) -> AppResult<Vec<TodoSummary>> {
    let mut statement = connection.prepare(
        "SELECT id, title, status, created_at, completed_at
         FROM todos
         WHERE status = 'open'
         ORDER BY created_at DESC, id DESC",
    )?;
    let rows = statement.query_map([], summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn search(
    connection: &Connection,
    query: &str,
    include_done: bool,
    limit: u32,
) -> AppResult<Vec<TodoSummary>> {
    validate_content("search query", query)?;
    let mut statement = connection.prepare(
        "SELECT id, title, status, created_at, completed_at
         FROM todos
         WHERE (?1 OR status = 'open')
           AND (
               instr(lower(title), lower(?2)) > 0
               OR instr(lower(note), lower(?2)) > 0
               OR EXISTS (
                   SELECT 1
                   FROM todo_notes
                   WHERE todo_notes.todo_id = todos.id
                     AND instr(lower(todo_notes.text), lower(?2)) > 0
               )
           )
         ORDER BY created_at DESC, id DESC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(
        params![include_done, query, i64::from(limit)],
        summary_from_row,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn show(connection: &Connection, id: TodoId) -> AppResult<TodoView> {
    let todo = get_todo(connection, id)?;
    let mut statement = connection.prepare(
        "SELECT text, created_at
         FROM todo_notes
         WHERE todo_id = ?1
         ORDER BY created_at, id",
    )?;
    let rows = statement.query_map([id.storage_id()], |row| {
        Ok(WorkingNote {
            text: row.get(0)?,
            created_at: row.get(1)?,
        })
    })?;
    let working_notes = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(TodoView {
        todo,
        working_notes,
    })
}

pub(crate) fn append_note(
    connection: &mut Connection,
    id: TodoId,
    text: &str,
) -> AppResult<WorkingNote> {
    validate_content("working note", text)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_todo(&transaction, id)?;
    transaction.execute(
        "INSERT INTO todo_notes(todo_id, text) VALUES(?1, ?2)",
        params![id.storage_id(), text],
    )?;
    let row_id = transaction.last_insert_rowid();
    let working_note = transaction.query_row(
        "SELECT text, created_at FROM todo_notes WHERE id = ?1",
        [row_id],
        |row| {
            Ok(WorkingNote {
                text: row.get(0)?,
                created_at: row.get(1)?,
            })
        },
    )?;
    transaction.commit()?;
    Ok(working_note)
}

pub(crate) fn mark_done(connection: &mut Connection, id: TodoId) -> AppResult<Transition> {
    transition(connection, id, TodoStatus::Done)
}

pub(crate) fn reopen(connection: &mut Connection, id: TodoId) -> AppResult<Transition> {
    transition(connection, id, TodoStatus::Open)
}

fn transition(
    connection: &mut Connection,
    id: TodoId,
    target: TodoStatus,
) -> AppResult<Transition> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let previous = get_todo(&transaction, id)?;
    let changed = previous.status != target;
    if changed {
        match target {
            TodoStatus::Open => {
                transaction.execute(
                    "UPDATE todos
                     SET status = 'open', completed_at = NULL
                     WHERE id = ?1",
                    [id.storage_id()],
                )?;
            }
            TodoStatus::Done => {
                transaction.execute(
                    "UPDATE todos
                     SET status = 'done',
                         completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    [id.storage_id()],
                )?;
            }
        }
    }
    let todo = get_todo(&transaction, id)?;
    transaction.commit()?;
    Ok(Transition { todo, changed })
}

fn get_todo(connection: &Connection, id: TodoId) -> AppResult<Todo> {
    connection
        .query_row(
            "SELECT id, title, note, pointer, source_path, status,
                    created_at, completed_at
             FROM todos
             WHERE id = ?1",
            [id.storage_id()],
            todo_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("todo_not_found", format!("todo not found: {id}")))
}

fn require_todo(connection: &Connection, id: TodoId) -> AppResult<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM todos WHERE id = ?1)",
        [id.storage_id()],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found(
            "todo_not_found",
            format!("todo not found: {id}"),
        ))
    }
}

fn todo_from_row(row: &Row<'_>) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: id_from_row(row, 0)?,
        title: row.get(1)?,
        note: row.get(2)?,
        pointer: row.get(3)?,
        source_path: row.get::<_, String>(4)?.into(),
        status: status_from_row(row, 5)?,
        created_at: row.get(6)?,
        completed_at: row.get(7)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<TodoSummary> {
    Ok(TodoSummary {
        id: id_from_row(row, 0)?,
        title: row.get(1)?,
        status: status_from_row(row, 2)?,
        created_at: row.get(3)?,
        completed_at: row.get(4)?,
    })
}

fn id_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<TodoId> {
    let value = row.get::<_, i64>(index)?;
    TodoId::from_storage(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}

fn status_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<TodoStatus> {
    let value = row.get::<_, String>(index)?;
    value.parse().map_err(|error: &'static str| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })
}

fn validate_content(name: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::invalid(
            "blank_text",
            format!("{name} must not be blank"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        CreateTodo, append_note, create, list, list_open, mark_done, reopen, search, show,
    };
    use crate::db;
    use crate::model::TodoStatus;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn creates_reads_searches_and_transitions_todos() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let todo = create(
            &mut connection,
            CreateTodo {
                title: "Measure model usage",
                note: "Expose token consumption from liaison runs.",
                pointer: "Need token stats",
                source_path: Path::new("/tmp/source.md"),
            },
        )?;
        assert_eq!(todo.id.to_string(), "t1");
        assert_eq!(list(&connection, false, 20)?.len(), 1);
        assert_eq!(search(&connection, "TOKEN", false, 20)?.len(), 1);

        let first = append_note(&mut connection, todo.id, "Investigating usage events")?;
        let second = append_note(&mut connection, todo.id, "Found the event fields")?;
        let view = show(&connection, todo.id)?;
        assert_eq!(view.working_notes, vec![first, second]);

        let done = mark_done(&mut connection, todo.id)?;
        assert!(done.changed);
        assert_eq!(done.todo.status, TodoStatus::Done);
        assert!(!mark_done(&mut connection, todo.id)?.changed);
        assert!(list(&connection, false, 20)?.is_empty());
        assert_eq!(list(&connection, true, 20)?.len(), 1);

        let reopened = reopen(&mut connection, todo.id)?;
        assert!(reopened.changed);
        assert_eq!(reopened.todo.status, TodoStatus::Open);
        assert!(!reopen(&mut connection, todo.id)?.changed);
        Ok(())
    }

    #[test]
    fn todo_content_and_working_notes_are_immutable_in_sqlite() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let todo = create(
            &mut connection,
            CreateTodo {
                title: "Original",
                note: "Original note",
                pointer: "Original direction",
                source_path: Path::new("/tmp/source.md"),
            },
        )?;
        append_note(&mut connection, todo.id, "Working note")?;

        assert!(
            connection
                .execute("UPDATE todos SET title = 'Changed' WHERE id = 1", [])
                .is_err()
        );
        assert!(
            connection
                .execute("UPDATE todo_notes SET text = 'Changed' WHERE id = 1", [])
                .is_err()
        );
        assert!(
            connection
                .execute("DELETE FROM todo_notes WHERE id = 1", [])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn digest_query_returns_every_open_todo() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let transaction = connection.transaction()?;
        for number in 0..1_005 {
            transaction.execute(
                "INSERT INTO todos(title, note, pointer, source_path)
                 VALUES(?1, 'note', 'pointer', '/tmp/source.md')",
                [format!("Todo {number}")],
            )?;
        }
        transaction.execute(
            "INSERT INTO todos(title, note, pointer, source_path, status, completed_at)
             VALUES('Done', 'note', 'pointer', '/tmp/source.md', 'done',
                    '2026-08-27T12:00:00.000Z')",
            [],
        )?;
        transaction.commit()?;

        let todos = list_open(&connection)?;
        assert_eq!(todos.len(), 1_005);
        assert_eq!(
            todos.first().map(|todo| todo.title.as_str()),
            Some("Todo 1004")
        );
        assert!(todos.iter().all(|todo| todo.status == TodoStatus::Open));
        Ok(())
    }
}
