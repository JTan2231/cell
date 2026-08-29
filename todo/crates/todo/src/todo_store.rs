use std::collections::BTreeSet;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension as _, Row, TransactionBehavior, params};

use crate::error::{AppError, AppResult};
use crate::model::{
    ConcernId, DesignId, DesignSummary, SituationAssessmentId, SituationAssessmentSummary, Todo,
    TodoConcern, TodoId, TodoStatus, TodoSummary, TodoView, WorkingNote, WorkingNoteId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transition {
    pub(crate) todo: Todo,
    pub(crate) changed: bool,
}

pub(crate) fn list(
    connection: &Connection,
    include_done: bool,
    limit: u32,
) -> AppResult<Vec<TodoSummary>> {
    let mut statement = connection.prepare(
        "SELECT t.id, d.title, t.status, t.created_at, t.completed_at
         FROM todos AS t
         JOIN todo_direction_revisions AS d
           ON d.todo_id = t.id
          AND d.revision = (
              SELECT max(current.revision)
              FROM todo_direction_revisions AS current
              WHERE current.todo_id = t.id
          )
         WHERE (?1 OR t.status = 'open')
           AND NOT EXISTS (
               SELECT 1 FROM todo_supersessions AS supersession
               WHERE supersession.superseded_todo_id = t.id
           )
         ORDER BY t.created_at DESC, t.id DESC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![include_done, i64::from(limit)], summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn list_open(connection: &Connection) -> AppResult<Vec<TodoSummary>> {
    let mut statement = connection.prepare(
        "SELECT t.id, d.title, t.status, t.created_at, t.completed_at
         FROM todos AS t
         JOIN todo_direction_revisions AS d
           ON d.todo_id = t.id
          AND d.revision = (
              SELECT max(current.revision)
              FROM todo_direction_revisions AS current
              WHERE current.todo_id = t.id
          )
         WHERE t.status = 'open'
           AND NOT EXISTS (
               SELECT 1 FROM todo_supersessions AS supersession
               WHERE supersession.superseded_todo_id = t.id
           )
         ORDER BY t.created_at DESC, t.id DESC",
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
        "WITH RECURSIVE family(root_id, member_id) AS (
             SELECT t.id, t.id
             FROM todos AS t
             WHERE NOT EXISTS (
                 SELECT 1 FROM todo_supersessions AS supersession
                 WHERE supersession.superseded_todo_id = t.id
             )
             UNION
             SELECT family.root_id, supersession.superseded_todo_id
             FROM family
             JOIN todo_supersessions AS supersession
               ON supersession.surviving_todo_id = family.member_id
         )
         SELECT t.id, d.title, t.status, t.created_at, t.completed_at
         FROM todos AS t
         JOIN todo_direction_revisions AS d
           ON d.todo_id = t.id
          AND d.revision = (
              SELECT max(current.revision)
              FROM todo_direction_revisions AS current
              WHERE current.todo_id = t.id
          )
         WHERE (?1 OR t.status = 'open')
           AND NOT EXISTS (
               SELECT 1 FROM todo_supersessions AS supersession
               WHERE supersession.superseded_todo_id = t.id
           )
           AND (
               instr(lower(d.title), lower(?2)) > 0
               OR instr(lower(d.body), lower(?2)) > 0
               OR EXISTS (
                   SELECT 1
                   FROM family
                   JOIN todo_concerns AS attachment
                     ON attachment.todo_id = family.member_id
                   JOIN concerns AS concern ON concern.id = attachment.concern_id
                   WHERE family.root_id = t.id
                     AND instr(lower(concern.body), lower(?2)) > 0
               )
               OR EXISTS (
                   SELECT 1
                   FROM family
                   JOIN todo_notes AS note ON note.todo_id = family.member_id
                   WHERE family.root_id = t.id
                     AND instr(lower(note.text), lower(?2)) > 0
               )
               OR EXISTS (
                   SELECT 1
                   FROM todo_situation_assessments AS assessment
                   WHERE assessment.todo_id = t.id
                     AND assessment.id = (
                         SELECT max(current.id)
                         FROM todo_situation_assessments AS current
                         WHERE current.todo_id = t.id
                     )
                     AND (
                         instr(lower(assessment.subject_label), lower(?2)) > 0
                         OR instr(lower(assessment.summary), lower(?2)) > 0
                     )
               )
               OR EXISTS (
                   SELECT 1
                   FROM todo_designs AS design
                   WHERE design.todo_id = t.id
                     AND design.state IN (
                         'open', 'ready', 'authorized', 'legacy_unreviewed'
                     )
                     AND design.revision = (
                         SELECT max(current.revision)
                         FROM todo_designs AS current
                         WHERE current.todo_id = t.id
                           AND current.state IN (
                               'open', 'ready', 'authorized', 'legacy_unreviewed'
                           )
                     )
                     AND instr(lower(design.summary), lower(?2)) > 0
               )
           )
         ORDER BY t.created_at DESC, t.id DESC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(
        params![include_done, query, i64::from(limit)],
        summary_from_row,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn show(connection: &Connection, requested_id: TodoId) -> AppResult<TodoView> {
    let resolution_path = resolve_path(connection, requested_id)?;
    let id = *resolution_path
        .last()
        .ok_or_else(|| AppError::database("invalid_supersession", "empty resolution path"))?;
    let todo = get_todo(connection, id)?;
    let concerns = effective_concerns(connection, id)?;
    let working_notes = effective_notes(connection, id)?;
    let latest_assessment = latest_assessment(connection, id)?;
    let latest_design = latest_design(connection, id)?;
    Ok(TodoView {
        requested_id,
        resolution_path,
        todo,
        concerns,
        working_notes,
        latest_assessment,
        latest_design,
    })
}

pub(crate) fn append_note(
    connection: &mut Connection,
    requested_id: TodoId,
    text: &str,
) -> AppResult<WorkingNote> {
    validate_content("working note", text)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = resolve_id(&transaction, requested_id)?;
    transaction.execute(
        "INSERT INTO todo_notes(todo_id, text) VALUES(?1, ?2)",
        params![id.storage_id(), text],
    )?;
    let row_id = transaction.last_insert_rowid();
    let working_note = transaction.query_row(
        "SELECT id, todo_id, text, created_at FROM todo_notes WHERE id = ?1",
        [row_id],
        note_from_row,
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

pub(crate) fn resolve_id(connection: &Connection, requested_id: TodoId) -> AppResult<TodoId> {
    resolve_path(connection, requested_id).and_then(|path| {
        path.last()
            .copied()
            .ok_or_else(|| AppError::database("invalid_supersession", "empty resolution path"))
    })
}

fn transition(
    connection: &mut Connection,
    requested_id: TodoId,
    target: TodoStatus,
) -> AppResult<Transition> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = resolve_id(&transaction, requested_id)?;
    let previous = get_todo(&transaction, id)?;
    let changed = previous.status != target;
    if changed {
        match target {
            TodoStatus::Open => {
                transaction.execute(
                    "UPDATE todos SET status = 'open', completed_at = NULL WHERE id = ?1",
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
            "SELECT t.id, d.title, d.body, d.revision, t.status,
                    t.created_at, t.completed_at
             FROM todos AS t
             JOIN todo_direction_revisions AS d
               ON d.todo_id = t.id
              AND d.revision = (
                  SELECT max(current.revision)
                  FROM todo_direction_revisions AS current
                  WHERE current.todo_id = t.id
              )
             WHERE t.id = ?1",
            [id.storage_id()],
            todo_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("todo_not_found", format!("todo not found: {id}")))
}

fn effective_concerns(connection: &Connection, id: TodoId) -> AppResult<Vec<TodoConcern>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE family(todo_id) AS (
             SELECT ?1
             UNION
             SELECT supersession.superseded_todo_id
             FROM family
             JOIN todo_supersessions AS supersession
               ON supersession.surviving_todo_id = family.todo_id
         )
         SELECT concern.id, attachment.todo_id, concern.body, concern.source_path,
                concern.source_thread_id, concern.source_turn_id, concern.source_item_id,
                concern.created_at
         FROM family
         JOIN todo_concerns AS attachment ON attachment.todo_id = family.todo_id
         JOIN concerns AS concern ON concern.id = attachment.concern_id
         ORDER BY concern.created_at, concern.id",
    )?;
    let rows = statement.query_map([id.storage_id()], concern_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn effective_notes(connection: &Connection, id: TodoId) -> AppResult<Vec<WorkingNote>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE family(todo_id) AS (
             SELECT ?1
             UNION
             SELECT supersession.superseded_todo_id
             FROM family
             JOIN todo_supersessions AS supersession
               ON supersession.surviving_todo_id = family.todo_id
         )
         SELECT note.id, note.todo_id, note.text, note.created_at
         FROM family
         JOIN todo_notes AS note ON note.todo_id = family.todo_id
         ORDER BY note.created_at, note.id",
    )?;
    let rows = statement.query_map([id.storage_id()], note_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn latest_assessment(
    connection: &Connection,
    id: TodoId,
) -> AppResult<Option<SituationAssessmentSummary>> {
    connection
        .query_row(
            "SELECT id, disposition, subject_label, summary, observed_at, created_at
             FROM todo_situation_assessments
             WHERE todo_id = ?1
             ORDER BY id DESC
             LIMIT 1",
            [id.storage_id()],
            assessment_summary_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn latest_design(connection: &Connection, id: TodoId) -> AppResult<Option<DesignSummary>> {
    connection
        .query_row(
            "SELECT id, revision, state, summary, created_at
             FROM todo_designs
             WHERE todo_id = ?1
             ORDER BY revision DESC, id DESC
             LIMIT 1",
            [id.storage_id()],
            design_summary_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn resolve_path(connection: &Connection, requested_id: TodoId) -> AppResult<Vec<TodoId>> {
    require_todo(connection, requested_id)?;
    let mut seen = BTreeSet::from([requested_id]);
    let mut path = vec![requested_id];
    let mut current = requested_id;
    loop {
        let next = connection
            .query_row(
                "SELECT surviving_todo_id
                 FROM todo_supersessions
                 WHERE superseded_todo_id = ?1",
                [current.storage_id()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(next) = next else {
            return Ok(path);
        };
        let next = todo_id_from_storage(next, 0)?;
        if !seen.insert(next) {
            return Err(AppError::database(
                "todo_supersession_cycle",
                format!("todo supersession cycle encountered while resolving {requested_id}"),
            ));
        }
        path.push(next);
        current = next;
    }
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
        id: todo_id_from_row(row, 0)?,
        title: row.get(1)?,
        direction: row.get(2)?,
        direction_revision: row.get(3)?,
        status: status_from_row(row, 4)?,
        created_at: row.get(5)?,
        completed_at: row.get(6)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<TodoSummary> {
    Ok(TodoSummary {
        id: todo_id_from_row(row, 0)?,
        title: row.get(1)?,
        status: status_from_row(row, 2)?,
        created_at: row.get(3)?,
        completed_at: row.get(4)?,
    })
}

fn concern_from_row(row: &Row<'_>) -> rusqlite::Result<TodoConcern> {
    Ok(TodoConcern {
        id: concern_id_from_row(row, 0)?,
        attached_todo_id: todo_id_from_row(row, 1)?,
        body: row.get(2)?,
        source_path: row.get::<_, String>(3)?.into(),
        source_thread_id: row.get(4)?,
        source_turn_id: row.get(5)?,
        source_item_id: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<WorkingNote> {
    Ok(WorkingNote {
        id: working_note_id_from_row(row, 0)?,
        todo_id: todo_id_from_row(row, 1)?,
        text: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn assessment_summary_from_row(row: &Row<'_>) -> rusqlite::Result<SituationAssessmentSummary> {
    Ok(SituationAssessmentSummary {
        id: assessment_id_from_row(row, 0)?,
        disposition: row.get(1)?,
        subject_label: row.get(2)?,
        summary: row.get(3)?,
        observed_at: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn design_summary_from_row(row: &Row<'_>) -> rusqlite::Result<DesignSummary> {
    Ok(DesignSummary {
        id: design_id_from_row(row, 0)?,
        revision: row.get(1)?,
        state: row.get(2)?,
        summary: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn todo_id_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<TodoId> {
    todo_id_from_storage(row.get(index)?, index)
}

fn todo_id_from_storage(value: i64, index: usize) -> rusqlite::Result<TodoId> {
    TodoId::from_storage(value).map_err(|error| conversion_error(index, error))
}

fn concern_id_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<ConcernId> {
    ConcernId::from_storage(row.get(index)?).map_err(|error| conversion_error(index, error))
}

fn working_note_id_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<WorkingNoteId> {
    WorkingNoteId::from_storage(row.get(index)?).map_err(|error| conversion_error(index, error))
}

fn assessment_id_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<SituationAssessmentId> {
    SituationAssessmentId::from_storage(row.get(index)?)
        .map_err(|error| conversion_error(index, error))
}

fn design_id_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<DesignId> {
    DesignId::from_storage(row.get(index)?).map_err(|error| conversion_error(index, error))
}

fn conversion_error(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
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
    use rusqlite::{Connection, params};

    use super::{append_note, list, list_open, mark_done, reopen, search, show};
    use crate::db;
    use crate::model::{TodoId, TodoStatus};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn reads_latest_direction_searches_and_transitions() -> TestResult {
        let (_directory, _database, mut connection) = database()?;
        let todo = insert_todo(
            &connection,
            "Initial title",
            "Initial direction",
            "Raw concern",
        )?;
        let concern_id: i64 = connection.query_row(
            "SELECT concern_id FROM todo_concerns WHERE todo_id = ?1",
            [todo.storage_id()],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO todo_direction_revisions(
                 todo_id, revision, title, body, source_concern_id, provenance_kind
             ) VALUES(?1, 2, 'Current title', 'Current direction token', ?2, 'legacy_v1')",
            params![todo.storage_id(), concern_id],
        )?;

        assert_eq!(list(&connection, false, 20)?[0].title, "Current title");
        assert_eq!(search(&connection, "direction token", false, 20)?.len(), 1);
        assert!(search(&connection, "Initial direction", false, 20)?.is_empty());

        let note = append_note(&mut connection, todo, "Observed state")?;
        assert_eq!(note.id.to_string(), "n1");
        assert_eq!(note.todo_id, todo);
        let view = show(&connection, todo)?;
        assert_eq!(view.todo.direction_revision, 2);
        assert_eq!(view.todo.direction, "Current direction token");
        assert_eq!(view.working_notes, vec![note]);

        assert!(mark_done(&mut connection, todo)?.changed);
        assert_eq!(
            mark_done(&mut connection, todo)?.todo.status,
            TodoStatus::Done
        );
        assert!(list(&connection, false, 20)?.is_empty());
        assert_eq!(list(&connection, true, 20)?.len(), 1);
        assert!(reopen(&mut connection, todo)?.changed);
        assert_eq!(list_open(&connection)?.len(), 1);
        Ok(())
    }

    #[test]
    fn superseded_ids_resolve_and_inherit_concerns_and_notes() -> TestResult {
        let (_directory, _database, mut connection) = database()?;
        let old = insert_todo(&connection, "Old", "Old direction", "Old concern token")?;
        let survivor = insert_todo(
            &connection,
            "Survivor",
            "Survivor direction",
            "Survivor concern",
        )?;
        append_note(&mut connection, old, "Old note token")?;
        append_note(&mut connection, survivor, "Survivor note")?;
        insert_supersession(&connection, old, survivor)?;

        let view = show(&connection, old)?;
        assert_eq!(view.requested_id, old);
        assert_eq!(view.todo.id, survivor);
        assert_eq!(view.resolution_path, vec![old, survivor]);
        assert_eq!(view.concerns.len(), 2);
        assert_eq!(view.working_notes.len(), 2);
        assert!(
            view.concerns
                .iter()
                .any(|item| item.attached_todo_id == old)
        );
        assert!(view.working_notes.iter().any(|item| item.todo_id == old));

        assert_eq!(list(&connection, false, 20)?.len(), 1);
        assert_eq!(list(&connection, false, 20)?[0].id, survivor);
        assert_eq!(
            search(&connection, "Old concern token", false, 20)?[0].id,
            survivor
        );
        assert_eq!(
            search(&connection, "Old note token", false, 20)?[0].id,
            survivor
        );

        let appended = append_note(&mut connection, old, "After unification")?;
        assert_eq!(appended.todo_id, survivor);
        assert_eq!(mark_done(&mut connection, old)?.todo.id, survivor);
        Ok(())
    }

    #[test]
    fn show_includes_latest_assessment_and_design_summaries() -> TestResult {
        let (_directory, _database, connection) = database()?;
        let todo = insert_todo(&connection, "Subject", "Direction", "Concern")?;
        let direction_id: i64 = connection.query_row(
            "SELECT id FROM todo_direction_revisions WHERE todo_id = ?1",
            [todo.storage_id()],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO todo_agent_jobs(
                 stage, todo_id, base_digest, nucleus_requester_id, nucleus_job_id,
                 prompt_identity, toolset_identity
             ) VALUES(
                 'situation_assessment', ?1, 'base', 'requester-assessment', 'job-assessment',
                 'prompt-v1', 'assessment-v1'
             )",
            [todo.storage_id()],
        )?;
        let job_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO todo_situation_assessments(
                 todo_id, agent_job_id, direction_revision_id, concern_set_digest,
                 disposition, summary, subject_label, observed_at,
                 producer_tool_call_id
             ) VALUES(
                 ?1, ?2, ?3, 'concerns', 'ready', 'Assessment summary',
                 'Current subject', '2026-08-28T12:00:00Z', 'assessment-call'
             )",
            params![todo.storage_id(), job_id, direction_id],
        )?;
        connection.execute(
            "INSERT INTO todo_designs(
                 todo_id, revision, state, summary
             ) VALUES(?1, 1, 'legacy_unreviewed', 'Imported design summary')",
            [todo.storage_id()],
        )?;

        let view = show(&connection, todo)?;
        assert_eq!(
            view.latest_assessment
                .as_ref()
                .map(|item| item.summary.as_str()),
            Some("Assessment summary")
        );
        assert_eq!(
            view.latest_design
                .as_ref()
                .map(|item| item.summary.as_str()),
            Some("Imported design summary")
        );
        assert_eq!(
            search(&connection, "Assessment summary", false, 20)?.len(),
            1
        );
        assert_eq!(search(&connection, "Imported design", false, 20)?.len(), 1);
        Ok(())
    }

    fn database() -> TestResult<(tempfile::TempDir, std::path::PathBuf, Connection)> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        let connection = db::init(&path)?;
        Ok((directory, path, connection))
    }

    fn insert_todo(
        connection: &Connection,
        title: &str,
        direction: &str,
        concern: &str,
    ) -> TestResult<TodoId> {
        connection.execute("INSERT INTO todos DEFAULT VALUES", [])?;
        let todo_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO concerns(body, source_path, status, resolved_at)
             VALUES(?1, '/tmp/source.jsonl', 'attached', '2026-08-28T12:00:00Z')",
            [concern],
        )?;
        let concern_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO todo_direction_revisions(
                 todo_id, revision, title, body, source_concern_id, provenance_kind
             ) VALUES(?1, 1, ?2, ?3, ?4, 'legacy_v1')",
            params![todo_id, title, direction, concern_id],
        )?;
        connection.execute(
            "INSERT INTO todo_concerns(todo_id, concern_id) VALUES(?1, ?2)",
            params![todo_id, concern_id],
        )?;
        Ok(TodoId::from_storage(todo_id)?)
    }

    fn insert_supersession(
        connection: &Connection,
        old: TodoId,
        survivor: TodoId,
    ) -> TestResult<()> {
        let concern_id: i64 = connection.query_row(
            "SELECT concern_id FROM todo_concerns WHERE todo_id = ?1",
            [survivor.storage_id()],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO todo_agent_jobs(
                 stage, concern_id, base_digest, nucleus_requester_id, nucleus_job_id,
                 prompt_identity, toolset_identity
             ) VALUES(
                 'concern_routing', ?1, 'base', 'requester-routing', 'job-unify',
                 'prompt-v1', 'routing-v1'
             )",
            [concern_id],
        )?;
        let job_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO concern_routing_proposals(
                 concern_id, agent_job_id, action, proposed_title, proposed_direction,
                 rationale, proposal_digest, producer_tool_call_id, decision,
                 decision_source_path, decided_at
             ) VALUES(
                 ?1, ?2, 'unify', 'Survivor', 'Merged direction', 'Same umbrella',
                 'routing-digest', 'routing-call', 'authorized',
                 '/tmp/decision.jsonl', '2026-08-28T12:00:00Z'
             )",
            params![concern_id, job_id],
        )?;
        let routing_id = connection.last_insert_rowid();
        for (ordinal, todo) in [(0, old), (1, survivor)] {
            let revision: i64 = connection.query_row(
                "SELECT max(revision) FROM todo_direction_revisions WHERE todo_id = ?1",
                [todo.storage_id()],
                |row| row.get(0),
            )?;
            connection.execute(
                "INSERT INTO concern_routing_targets(
                     routing_id, ordinal, todo_id, direction_revision
                 ) VALUES(?1, ?2, ?3, ?4)",
                params![routing_id, ordinal, todo.storage_id(), revision],
            )?;
        }
        connection.execute(
            "INSERT INTO concern_routing_unifications(routing_id, survivor_todo_id)
             VALUES(?1, ?2)",
            params![routing_id, survivor.storage_id()],
        )?;
        connection.execute(
            "INSERT INTO todo_supersessions(
                 superseded_todo_id, surviving_todo_id, authorized_routing_id
             ) VALUES(?1, ?2, ?3)",
            params![old.storage_id(), survivor.storage_id(), routing_id],
        )?;
        Ok(())
    }
}
