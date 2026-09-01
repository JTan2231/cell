use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use rusqlite::{Connection, OptionalExtension as _, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::domain::{
    DecisionEvent, Intake, IntakeStatus, PathHistory, Project, ProjectDetail, ProjectStatus,
    ReconciliationProposal, Repository, RepositoryDiff, Revision, SemanticEffect,
    validate_effects_for_next_ids,
};
use crate::error::io;
use crate::{Error, Result};

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Correlation {
    pub event_id: String,
    pub requester_id: String,
    pub job_id: String,
    pub request_json: String,
    pub request_sha256: String,
    pub tool_after: u64,
    pub admitted: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MailboxReceipt {
    pub arguments_sha256: String,
    pub result_json: String,
    pub is_error: bool,
    pub committed_revision: Option<u64>,
}

impl Store {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(Error::domain(
                "database_path_relative",
                format!("database path must be absolute: {}", path.display()),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| io(parent, source))?;
        }
        let store = Self { path };
        store.initialize()?;
        Ok(store)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i64> {
        let connection = self.connection()?;
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub fn register_project(&self, id: &str, root: &Path, activation_cursor: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let now = now();
        let root = path_text(root)?;
        transaction
            .execute(
                "INSERT INTO projects
                    (id, status, current_path, activation_cursor, scan_cursor,
                     next_concept_number, created_at, updated_at)
                 VALUES (?1, 'active', ?2, ?3, ?3, 1, ?4, ?4)",
                params![id, root, activation_cursor, now],
            )
            .map_err(|error| map_project_constraint(error, id, &root))?;
        transaction.execute(
            "INSERT INTO project_paths
                (project_id, path, activation_cursor, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, NULL)",
            params![id, root, activation_cursor, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT p.id, p.status, p.current_path, p.activation_cursor, p.scan_cursor,
                    p.next_concept_number,
                    COALESCE(MAX(r.revision), 0)
             FROM projects p
             LEFT JOIN semantic_revisions r ON r.project_id = p.id
             GROUP BY p.id
             ORDER BY p.id",
        )?;
        let rows = statement.query_map([], project_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn project(&self, id: &str) -> Result<Project> {
        let connection = self.connection()?;
        project_with_connection(&connection, id)
    }

    pub fn project_detail(&self, id: &str) -> Result<ProjectDetail> {
        let connection = self.connection()?;
        let project = project_with_connection(&connection, id)?;
        let mut statement = connection.prepare(
            "SELECT path, activation_cursor, opened_at, closed_at
             FROM project_paths WHERE project_id = ?1
             ORDER BY opened_at, rowid",
        )?;
        let paths = statement
            .query_map([id], |row| {
                Ok(PathHistory {
                    path: row.get(0)?,
                    activation_cursor: row.get(1)?,
                    opened_at: row.get(2)?,
                    closed_at: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ProjectDetail { project, paths })
    }

    pub fn move_project(&self, id: &str, new_root: &Path) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project = project_with_connection(&transaction, id)?;
        let root = path_text(new_root)?;
        let now = now();
        transaction
            .execute(
                "UPDATE projects
                 SET current_path = ?2, updated_at = ?3
                 WHERE id = ?1 AND status != 'retired'",
                params![id, root, now],
            )
            .map_err(|error| map_project_constraint(error, id, &root))?;
        if transaction.changes() == 0 {
            return Err(Error::domain(
                "project_retired",
                format!("retired project {id} cannot move"),
            ));
        }
        transaction.execute(
            "UPDATE project_paths SET closed_at = ?2
             WHERE project_id = ?1 AND closed_at IS NULL",
            params![id, now],
        )?;
        transaction.execute(
            "INSERT INTO project_paths
                (project_id, path, activation_cursor, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, NULL)",
            params![id, root, project.activation_cursor, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_project_status(&self, id: &str, status: ProjectStatus) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current: String = transaction
            .query_row("SELECT status FROM projects WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| {
                Error::domain("project_not_found", format!("project {id} does not exist"))
            })?;
        let current = ProjectStatus::from_str(&current)?;
        let allowed = match (current, status) {
            (ProjectStatus::Active, ProjectStatus::Paused)
            | (ProjectStatus::Paused, ProjectStatus::Active | ProjectStatus::Retired) => true,
            (left, right) if left == right => true,
            _ => false,
        };
        if !allowed {
            return Err(Error::domain(
                "project_transition_invalid",
                format!("project {id} cannot transition from {current} to {status}"),
            ));
        }
        if status == ProjectStatus::Retired {
            let outstanding: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM intake_events
                 WHERE project_id = ?1
                   AND status IN ('pending', 'awaiting_review', 'paused', 'processing', 'failed')",
                [id],
                |row| row.get(0),
            )?;
            if outstanding != 0 {
                return Err(Error::domain(
                    "project_intake_outstanding",
                    format!("project {id} has {outstanding} unresolved assigned intake events"),
                ));
            }
        }
        transaction.execute(
            "UPDATE projects SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), now()],
        )?;
        if status == ProjectStatus::Retired {
            transaction.execute(
                "UPDATE project_paths SET closed_at = ?2
                 WHERE project_id = ?1 AND closed_at IS NULL",
                params![id, now()],
            )?;
        }
        if status == ProjectStatus::Paused {
            transaction.execute(
                "UPDATE intake_events SET status = 'paused', updated_at = ?2
                 WHERE project_id = ?1 AND status = 'pending'",
                params![id, now()],
            )?;
        } else if status == ProjectStatus::Active {
            transaction.execute(
                "UPDATE intake_events SET status = 'pending', updated_at = ?2
                 WHERE project_id = ?1 AND status = 'paused'",
                params![id, now()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn deepest_project_for_path(&self, cwd: &Path) -> Result<Option<Project>> {
        let mut candidates = self
            .list_projects()?
            .into_iter()
            .filter(|project| project.status != ProjectStatus::Retired)
            .filter(|project| cwd.starts_with(Path::new(&project.current_path)))
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            Path::new(&right.current_path)
                .components()
                .count()
                .cmp(&Path::new(&left.current_path).components().count())
                .then_with(|| left.id.cmp(&right.id))
        });
        if candidates.len() > 1 {
            let first_depth = Path::new(&candidates[0].current_path).components().count();
            let second_depth = Path::new(&candidates[1].current_path).components().count();
            if first_depth == second_depth {
                return Err(Error::domain(
                    "project_route_ambiguous",
                    format!("multiple registered roots own {}", cwd.display()),
                ));
            }
        }
        Ok(candidates.into_iter().next())
    }

    pub(crate) fn assigned_project_for_decision(
        &self,
        decision_id: &str,
    ) -> Result<Option<Project>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT project_id FROM intake_events
             WHERE decision_id = ?1 AND project_id IS NOT NULL ORDER BY project_id",
        )?;
        let project_ids = statement
            .query_map([decision_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if project_ids.len() > 1 {
            return Err(Error::domain(
                "decision_project_conflict",
                format!(
                    "decision {decision_id} has lifecycle events assigned to multiple projects"
                ),
            ));
        }
        project_ids
            .first()
            .map(|project_id| project_with_connection(&connection, project_id))
            .transpose()
    }

    pub fn advance_scan_cursor(&self, project_id: &str, from: &str, to: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE projects SET scan_cursor = ?3, updated_at = ?4
             WHERE id = ?1 AND scan_cursor = ?2 AND status != 'retired'",
            params![project_id, from, to, now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "scan_cursor_conflict",
                format!("scan cursor for project {project_id} changed concurrently"),
            ));
        }
        Ok(())
    }

    pub fn repository(&self, project_id: &str, through: Option<u64>) -> Result<Repository> {
        let connection = self.connection()?;
        let project = project_with_connection(&connection, project_id)?;
        if through.is_some_and(|revision| revision > project.current_revision) {
            return Err(Error::domain(
                "revision_not_found",
                format!(
                    "project {project_id} HEAD is {}, requested revision {}",
                    project.current_revision,
                    through.unwrap_or_default()
                ),
            ));
        }
        repository_with_connection(&connection, project_id, through)
    }

    pub fn revisions(&self, project_id: &str, from: u64, to: Option<u64>) -> Result<Vec<Revision>> {
        let connection = self.connection()?;
        let project = project_with_connection(&connection, project_id)?;
        if to.is_some_and(|revision| revision > project.current_revision)
            || from > project.current_revision.saturating_add(1)
        {
            return Err(Error::domain(
                "revision_not_found",
                format!("project {project_id} HEAD is {}", project.current_revision),
            ));
        }
        revisions_with_connection(&connection, project_id, from, to)
    }

    pub fn diff(&self, project_id: &str, from: u64, to: u64) -> Result<RepositoryDiff> {
        if from > to {
            return Err(Error::domain(
                "revision_range_invalid",
                "from revision must not exceed to revision",
            ));
        }
        let head = self.project(project_id)?.current_revision;
        if from > head || to > head {
            return Err(Error::domain(
                "revision_not_found",
                format!("project {project_id} HEAD is {head}, requested {from}..{to}"),
            ));
        }
        Ok(RepositoryDiff {
            project_id: project_id.to_owned(),
            from_revision: from,
            to_revision: to,
            revisions: self.revisions(project_id, from.saturating_add(1), Some(to))?,
        })
    }

    pub fn commit_revision(
        &self,
        project_id: &str,
        base_revision: u64,
        summary: &str,
        source_event_id: Option<&str>,
        effects: &[SemanticEffect],
    ) -> Result<u64> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let revision = commit_revision_tx(
            &transaction,
            project_id,
            base_revision,
            summary,
            source_event_id,
            effects,
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    pub fn insert_intake(
        &self,
        event: &DecisionEvent,
        project_id: Option<&str>,
        cwd: Option<&Path>,
    ) -> Result<bool> {
        let connection = self.connection()?;
        let prior_project: Option<String> = connection
            .query_row(
                "SELECT project_id FROM intake_events
                 WHERE decision_id = ?1 AND project_id IS NOT NULL ORDER BY rowid LIMIT 1",
                [&event.decision_id],
                |row| row.get(0),
            )
            .optional()?;
        let routing_conflict = project_id.is_some()
            && prior_project.is_some()
            && project_id != prior_project.as_deref();
        let effective_project = if routing_conflict { None } else { project_id };
        let status = match effective_project {
            None => IntakeStatus::Unassigned,
            Some(id) => match self.project(id)?.status {
                ProjectStatus::Active => IntakeStatus::Pending,
                ProjectStatus::Paused => IntakeStatus::Paused,
                ProjectStatus::Retired => IntakeStatus::Unassigned,
            },
        };
        let anchor = event.anchors.first();
        let decision_json = serde_json::to_string(event)?;
        let cwd = cwd.map(path_text).transpose()?;
        let routing_error = routing_conflict.then(|| {
            format!(
                "decision {} is already assigned to project {}",
                event.decision_id,
                prior_project.as_deref().unwrap_or("unknown")
            )
        });
        let changed = connection.execute(
            "INSERT OR IGNORE INTO intake_events
                (event_id, source_cursor, event_kind, project_id, status, cwd,
                 host_id, thread_id, turn_id, decision_id, decision_json, attempts,
                 last_error, terminal_reason, applied_revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, NULL, NULL, ?13, ?13)",
            params![
                event.event_id,
                event.cursor,
                event.event_kind,
                effective_project,
                status.as_str(),
                cwd,
                anchor.map(|value| value.host_id.as_str()),
                anchor.map(|value| value.thread_id.as_str()),
                anchor.map(|value| value.turn_id.as_str()),
                event.decision_id,
                decision_json,
                routing_error,
                now(),
            ],
        )?;
        if changed == 1 {
            return Ok(true);
        }
        let persisted = connection
            .query_row(
                "SELECT source_cursor, event_kind, decision_id, decision_json, project_id, cwd
                 FROM intake_events WHERE event_id = ?1",
                [&event.event_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let expected_project = effective_project.map(str::to_owned);
        let expected = (
            event.cursor.as_str(),
            event.event_kind.as_str(),
            event.decision_id.as_str(),
            decision_json.as_str(),
            expected_project.as_deref(),
            cwd.as_deref(),
        );
        let identical = persisted.as_ref().is_some_and(
            |(cursor, kind, decision_id, envelope, project, persisted_cwd)| {
                (
                    cursor.as_str(),
                    kind.as_str(),
                    decision_id.as_str(),
                    envelope.as_str(),
                    project.as_deref(),
                    persisted_cwd.as_deref(),
                ) == expected
            },
        );
        if !identical {
            return Err(Error::domain(
                "intake_replay_conflict",
                format!(
                    "Decisions event {} was replayed with different immutable envelope or routing facts",
                    event.event_id
                ),
            ));
        }
        Ok(false)
    }

    pub(crate) fn review_admission_block(&self, intake: &Intake) -> Result<Option<String>> {
        if intake.decision.event_kind != "decision_reviewed" {
            return Ok(None);
        }
        let connection = self.connection()?;
        let review_rowid: i64 = connection
            .query_row(
                "SELECT rowid FROM intake_events WHERE event_id = ?1",
                [&intake.event_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "intake_not_found",
                    format!("intake event {} does not exist", intake.event_id),
                )
            })?;
        let mut statement = connection.prepare(
            "SELECT event_id, project_id, status, rowid FROM intake_events
             WHERE decision_id = ?1 AND event_kind = 'decision_admitted'
             ORDER BY rowid",
        )?;
        let admissions = statement
            .query_map([&intake.decision.decision_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if admissions.is_empty() {
            // Registration begins at the current Decisions watermark, so a review may
            // legitimately be the first lifecycle event observed by this consumer.
            return Ok(None);
        }
        if admissions.len() != 1 {
            return Ok(Some(format!(
                "decision {} has {} observed admission events; exactly one is required",
                intake.decision.decision_id,
                admissions.len()
            )));
        }
        let (event_id, project_id, status, admission_rowid) = &admissions[0];
        if *admission_rowid >= review_rowid {
            return Ok(Some(format!(
                "review {} precedes its observed admission {event_id} in Decisions append order",
                intake.event_id
            )));
        }
        if project_id.as_deref() != intake.project_id.as_deref() {
            return Ok(Some(format!(
                "review {} cannot pass admission {event_id} assigned to {:?}",
                intake.event_id, project_id
            )));
        }
        let status = IntakeStatus::from_str(status)?;
        if !matches!(
            status,
            IntakeStatus::AwaitingReview | IntakeStatus::Applied | IntakeStatus::Ignored
        ) {
            return Ok(Some(format!(
                "review {} is blocked by admission {event_id} in {} state",
                intake.event_id,
                status.as_str()
            )));
        }
        Ok(None)
    }

    pub fn list_intake(&self, status: Option<IntakeStatus>) -> Result<Vec<Intake>> {
        let connection = self.connection()?;
        let sql = if status.is_some() {
            "SELECT event_id, source_cursor, project_id, status, cwd, decision_json,
                    attempts, last_error, applied_revision, terminal_reason
             FROM intake_events WHERE status = ?1 ORDER BY rowid"
        } else {
            "SELECT event_id, source_cursor, project_id, status, cwd, decision_json,
                    attempts, last_error, applied_revision, terminal_reason
             FROM intake_events ORDER BY rowid"
        };
        let mut statement = connection.prepare(sql)?;
        let mut rows = match status {
            Some(status) => statement.query([status.as_str()])?,
            None => statement.query([])?,
        };
        let mut values = Vec::new();
        while let Some(row) = rows.next()? {
            values.push(intake_from_row(row)?);
        }
        Ok(values)
    }

    pub fn intake(&self, event_id: &str) -> Result<Intake> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT event_id, source_cursor, project_id, status, cwd, decision_json,
                        attempts, last_error, applied_revision, terminal_reason
                 FROM intake_events WHERE event_id = ?1",
                [event_id],
                intake_from_row,
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "intake_not_found",
                    format!("intake event {event_id} does not exist"),
                )
            })
    }

    pub fn assign_intake(&self, event_id: &str, project_id: &str) -> Result<()> {
        let project = self.project(project_id)?;
        if project.status == ProjectStatus::Retired {
            return Err(Error::domain(
                "project_retired",
                format!("cannot assign intake to retired project {project_id}"),
            ));
        }
        let status = if project.status == ProjectStatus::Paused {
            IntakeStatus::Paused
        } else {
            IntakeStatus::Pending
        };
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let correlated: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM request_correlations WHERE event_id = ?1)",
            [event_id],
            |row| row.get(0),
        )?;
        if correlated {
            return Err(Error::domain(
                "intake_assignment_correlated",
                "intake with a Nucleus correlation cannot be reassigned; finish or safely retry it first",
            ));
        }
        let (previous, decision_json): (Option<String>, String) = transaction
            .query_row(
                "SELECT project_id, decision_json FROM intake_events WHERE event_id = ?1",
                [event_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "intake_not_found",
                    format!("intake event {event_id} does not exist"),
                )
            })?;
        let decision: DecisionEvent = serde_json::from_str(&decision_json)?;
        if decision
            .anchors
            .iter()
            .filter(|anchor| anchor.source_role == "authority")
            .count()
            != 1
        {
            return Err(Error::domain(
                "intake_authority_unresolved",
                "manual assignment cannot repair a missing or ambiguous authority source",
            ));
        }
        let prior_project: Option<String> = transaction
            .query_row(
                "SELECT project_id FROM intake_events
                 WHERE decision_id = ?1 AND event_id != ?2 AND project_id IS NOT NULL
                 ORDER BY rowid LIMIT 1",
                params![decision.decision_id, event_id],
                |row| row.get(0),
            )
            .optional()?;
        if prior_project
            .as_deref()
            .is_some_and(|prior| prior != project_id)
        {
            return Err(Error::domain(
                "decision_project_conflict",
                format!(
                    "decision {} already belongs to project {}",
                    decision.decision_id,
                    prior_project.as_deref().unwrap_or("unknown")
                ),
            ));
        }
        let changed = transaction.execute(
            "UPDATE intake_events
             SET project_id = ?2, status = ?3, last_error = NULL, updated_at = ?4
             WHERE event_id = ?1 AND status IN ('unassigned', 'failed', 'paused', 'pending')",
            params![event_id, project_id, status.as_str(), now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_not_assignable",
                format!("intake event {event_id} cannot be assigned in its current state"),
            ));
        }
        let ordinal: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM intake_assignments WHERE event_id = ?1",
            [event_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO intake_assignments
                (event_id, ordinal, previous_project_id, project_id, assigned_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![event_id, ordinal, previous, project_id, now()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn annotate_unassigned(&self, event_id: &str, reason: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE intake_events SET last_error = ?2, updated_at = ?3
             WHERE event_id = ?1 AND status = 'unassigned'",
            params![event_id, reason, now()],
        )?;
        Ok(())
    }

    pub fn retry_intake(&self, event_id: &str) -> Result<()> {
        let intake = self.intake(event_id)?;
        let project_id = intake.project_id.ok_or_else(|| {
            Error::domain(
                "intake_unassigned",
                format!("intake event {event_id} must be assigned before retry"),
            )
        })?;
        let project = self.project(&project_id)?;
        if project.status != ProjectStatus::Active {
            return Err(Error::domain(
                "project_not_active",
                format!("project {project_id} must be active before retry"),
            ));
        }
        if self.correlation(event_id)?.is_some() {
            return Err(Error::domain(
                "intake_retry_job_unresolved",
                "the prior Nucleus correlation must be proven terminal before retry",
            ));
        }
        self.reset_intake_for_retry(event_id)
    }

    pub(crate) fn reset_retry_after_terminal(&self, event_id: &str) -> Result<()> {
        self.reset_intake_for_retry(event_id)
    }

    fn reset_intake_for_retry(&self, event_id: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE intake_events
             SET status = 'pending', last_error = NULL, updated_at = ?2
             WHERE event_id = ?1 AND status = 'failed'",
            params![event_id, now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_not_retryable",
                format!("intake event {event_id} is not failed"),
            ));
        }
        transaction.execute(
            "DELETE FROM request_correlations WHERE event_id = ?1",
            [event_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn next_pending_intake(&self) -> Result<Option<Intake>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT i.event_id, i.source_cursor, i.project_id, i.status, i.cwd,
                        i.decision_json, i.attempts, i.last_error, i.applied_revision,
                        i.terminal_reason
                 FROM intake_events i
                 JOIN projects p ON p.id = i.project_id
                 WHERE i.status IN ('pending', 'processing') AND p.status = 'active'
                 ORDER BY i.rowid LIMIT 1",
                [],
                intake_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn processing_intake(&self) -> Result<Option<Intake>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT i.event_id, i.source_cursor, i.project_id, i.status, i.cwd,
                        i.decision_json, i.attempts, i.last_error, i.applied_revision,
                        i.terminal_reason
                 FROM intake_events i
                 WHERE i.status = 'processing'
                 ORDER BY i.rowid LIMIT 1",
                [],
                intake_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mark_processing(&self, event_id: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE intake_events
             SET status = 'processing', attempts = attempts + 1, updated_at = ?2
             WHERE event_id = ?1 AND status = 'pending'",
            params![event_id, now()],
        )?;
        if changed == 0 {
            let status: Option<String> = connection
                .query_row(
                    "SELECT status FROM intake_events WHERE event_id = ?1",
                    [event_id],
                    |row| row.get(0),
                )
                .optional()?;
            if status.as_deref() != Some("processing") {
                return Err(Error::domain(
                    "intake_not_processable",
                    format!("intake event {event_id} is not pending or processing"),
                ));
            }
        }
        Ok(())
    }

    pub fn mark_failed(&self, event_id: &str, detail: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE intake_events SET status = 'failed', last_error = ?2, updated_at = ?3
             WHERE event_id = ?1 AND status IN ('pending', 'processing')",
            params![event_id, detail, now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_not_failable",
                format!("intake event {event_id} is not pending or processing"),
            ));
        }
        Ok(())
    }

    pub fn record_processing_error(&self, event_id: &str, detail: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE intake_events SET last_error = ?2, updated_at = ?3
             WHERE event_id = ?1 AND status = 'processing'",
            params![event_id, detail, now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_not_processing",
                format!("intake event {event_id} is not processing"),
            ));
        }
        Ok(())
    }

    pub fn mark_awaiting_review(&self, event_id: &str, reason: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE intake_events
             SET status = 'awaiting_review', terminal_reason = ?2, updated_at = ?3
             WHERE event_id = ?1 AND status = 'pending'",
            params![event_id, reason, now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_not_awaiting_review",
                format!("intake event {event_id} cannot await review in its current state"),
            ));
        }
        Ok(())
    }

    pub fn put_correlation(&self, correlation: &Correlation) -> Result<Correlation> {
        let connection = self.connection()?;
        let tool_after = sql_u64(correlation.tool_after, "tool_after")?;
        connection.execute(
            "INSERT OR IGNORE INTO request_correlations
                (event_id, requester_id, job_id, request_json, request_sha256,
                 tool_after, admitted, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                correlation.event_id,
                correlation.requester_id,
                correlation.job_id,
                correlation.request_json,
                correlation.request_sha256,
                tool_after,
                correlation.admitted,
                now(),
            ],
        )?;
        let existing = self.correlation(&correlation.event_id)?.ok_or_else(|| {
            Error::domain(
                "correlation_missing",
                "failed to retain request correlation",
            )
        })?;
        if existing.request_sha256 != correlation.request_sha256
            || existing.request_json != correlation.request_json
        {
            return Err(Error::domain(
                "correlation_conflict",
                format!(
                    "intake event {} already has different immutable request bytes",
                    correlation.event_id
                ),
            ));
        }
        Ok(existing)
    }

    pub fn correlation(&self, event_id: &str) -> Result<Option<Correlation>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT event_id, requester_id, job_id, request_json, request_sha256,
                        tool_after, admitted
                 FROM request_correlations WHERE event_id = ?1",
                [event_id],
                |row| {
                    Ok(Correlation {
                        event_id: row.get(0)?,
                        requester_id: row.get(1)?,
                        job_id: row.get(2)?,
                        request_json: row.get(3)?,
                        request_sha256: row.get(4)?,
                        tool_after: row_u64(row, 5)?,
                        admitted: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mark_admitted(&self, event_id: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE request_correlations SET admitted = 1 WHERE event_id = ?1",
            [event_id],
        )?;
        Ok(())
    }

    pub fn advance_tool_after(&self, event_id: &str, sequence: u64) -> Result<()> {
        let connection = self.connection()?;
        let sequence = sql_u64(sequence, "tool sequence")?;
        connection.execute(
            "UPDATE request_correlations SET tool_after = MAX(tool_after, ?2)
             WHERE event_id = ?1",
            params![event_id, sequence],
        )?;
        Ok(())
    }

    pub fn mailbox_receipt(&self, job_id: &str, call_id: &str) -> Result<Option<MailboxReceipt>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT arguments_sha256, result_json, is_error, committed_revision
                 FROM mailbox_receipts WHERE job_id = ?1 AND call_id = ?2",
                params![job_id, call_id],
                |row| {
                    Ok(MailboxReceipt {
                        arguments_sha256: row.get(0)?,
                        result_json: row.get(1)?,
                        is_error: row.get(2)?,
                        committed_revision: row_optional_u64(row, 3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_mailbox_proposal(
        &self,
        event_id: &str,
        job_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        project_id: &str,
        proposal: &ReconciliationProposal,
    ) -> Result<u64> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (event_json, assigned_project, status, correlated_job): (
            String,
            Option<String>,
            String,
            String,
        ) = transaction
            .query_row(
                "SELECT i.decision_json, i.project_id, i.status, c.job_id
                 FROM intake_events i
                 JOIN request_correlations c ON c.event_id = i.event_id
                 WHERE i.event_id = ?1",
                [event_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "intake_correlation_missing",
                    format!("intake event {event_id} has no request correlation"),
                )
            })?;
        if assigned_project.as_deref() != Some(project_id) {
            return Err(Error::domain(
                "intake_project_conflict",
                format!("intake event {event_id} is not assigned to project {project_id}"),
            ));
        }
        if correlated_job != job_id {
            return Err(Error::domain(
                "intake_job_conflict",
                format!("job {job_id} does not own intake event {event_id}"),
            ));
        }
        if status != "processing" {
            return Err(Error::domain(
                "intake_not_processing",
                format!("intake event {event_id} is {status}, not processing"),
            ));
        }
        if let Some(receipt) = mailbox_receipt_tx(&transaction, job_id, call_id)? {
            if receipt.arguments_sha256 != arguments_sha256 {
                return Err(Error::domain(
                    "mailbox_arguments_conflict",
                    format!("tool call {call_id} was replayed with different arguments"),
                ));
            }
            return receipt.committed_revision.ok_or_else(|| {
                Error::domain(
                    "mailbox_receipt_failed",
                    format!("tool call {call_id} was previously rejected"),
                )
            });
        }
        let event: DecisionEvent = serde_json::from_str(&event_json)?;
        validate_proposal_provenance(&event, proposal)?;
        let revision = commit_revision_tx(
            &transaction,
            project_id,
            proposal.base_revision,
            &proposal.summary,
            Some(event_id),
            &proposal.effects,
        )?;
        let result_json = serde_json::to_string(&serde_json::json!({
            "accepted": true,
            "revision": revision
        }))?;
        let revision_sql = sql_u64(revision, "semantic revision")?;
        transaction.execute(
            "INSERT INTO mailbox_receipts
                (job_id, call_id, arguments_sha256, result_json, is_error,
                 committed_revision, created_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![
                job_id,
                call_id,
                arguments_sha256,
                result_json,
                revision_sql,
                now()
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE intake_events
             SET applied_revision = ?2, last_error = NULL, updated_at = ?3
             WHERE event_id = ?1 AND status = 'processing' AND project_id = ?4",
            params![event_id, revision_sql, now(), project_id],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_commit_conflict",
                format!("intake event {event_id} changed during semantic commit"),
            ));
        }
        transaction.commit()?;
        Ok(revision)
    }

    pub fn finalize_applied(&self, event_id: &str, job_id: &str) -> Result<u64> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (project_id, decision_id, event_kind, revision): (String, String, String, i64) =
            transaction
                .query_row(
                    "SELECT i.project_id, i.decision_id, i.event_kind, i.applied_revision
                     FROM intake_events i
                     JOIN request_correlations c ON c.event_id = i.event_id
                     WHERE i.event_id = ?1 AND c.job_id = ?2
                       AND i.status = 'processing' AND i.applied_revision IS NOT NULL",
                    params![event_id, job_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?
                .ok_or_else(|| {
                    Error::domain(
                        "intake_not_finalizable",
                        format!("intake event {event_id} has no acknowledged domain commit"),
                    )
                })?;
        transaction.execute(
            "UPDATE intake_events SET status = 'applied', updated_at = ?2 WHERE event_id = ?1",
            params![event_id, now()],
        )?;
        if event_kind == "decision_reviewed" {
            resolve_awaiting_tx(
                &transaction,
                &project_id,
                &decision_id,
                &format!("Resolved by applied Decisions review event {event_id}"),
            )?;
        }
        transaction.commit()?;
        u64::try_from(revision)
            .map_err(|_| Error::domain("revision_invalid", "committed revision is negative"))
    }

    pub fn pending_committed_revision(&self, event_id: &str, job_id: &str) -> Result<Option<u64>> {
        let connection = self.connection()?;
        let value = connection
            .query_row(
                "SELECT i.applied_revision
                 FROM intake_events i
                 JOIN request_correlations c ON c.event_id = i.event_id
                 WHERE i.event_id = ?1 AND c.job_id = ?2 AND i.status = 'processing'",
                params![event_id, job_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();
        value
            .map(|revision| {
                u64::try_from(revision).map_err(|_| {
                    Error::domain("revision_invalid", "committed revision is negative")
                })
            })
            .transpose()
    }

    pub fn record_mailbox_rejection(
        &self,
        job_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        result_json: &str,
    ) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(receipt) = mailbox_receipt_tx(&transaction, job_id, call_id)? {
            if receipt.arguments_sha256 != arguments_sha256
                || receipt.result_json != result_json
                || !receipt.is_error
            {
                return Err(Error::domain(
                    "mailbox_rejection_conflict",
                    format!("tool call {call_id} was replayed with different rejection bytes"),
                ));
            }
            return Ok(());
        }
        transaction.execute(
            "INSERT INTO mailbox_receipts
                (job_id, call_id, arguments_sha256, result_json, is_error,
                 committed_revision, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, NULL, ?5)",
            params![job_id, call_id, arguments_sha256, result_json, now()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_ignored(&self, event_id: &str, reason: &str) -> Result<()> {
        if reason.trim().is_empty() {
            return Err(Error::domain(
                "intake_reason_empty",
                "ignored intake requires a source-derived reason",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (project_id, decision_id, event_kind): (Option<String>, String, String) = transaction
            .query_row(
                "SELECT project_id, decision_id, event_kind FROM intake_events WHERE event_id = ?1",
                [event_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "intake_not_found",
                    format!("intake event {event_id} does not exist"),
                )
            })?;
        let changed = transaction.execute(
            "UPDATE intake_events
             SET status = 'ignored', terminal_reason = ?2, last_error = NULL, updated_at = ?3
             WHERE event_id = ?1 AND status IN ('pending', 'processing')",
            params![event_id, reason.trim(), now()],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "intake_not_ignorable",
                format!("intake event {event_id} is not in an ignorable state"),
            ));
        }
        if event_kind == "decision_reviewed"
            && let Some(project_id) = project_id
        {
            resolve_awaiting_tx(
                &transaction,
                &project_id,
                &decision_id,
                &format!("Resolved by ignored Decisions review event {event_id}"),
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn initialize(&self) -> Result<()> {
        let connection = self.connection()?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::domain(
                "schema_too_new",
                format!("database schema {version} is newer than supported {SCHEMA_VERSION}"),
            ));
        }
        if version == 0 {
            connection.execute_batch(SCHEMA)?;
        }
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        Ok(connection)
    }
}

fn commit_revision_tx(
    transaction: &Transaction<'_>,
    project_id: &str,
    base_revision: u64,
    summary: &str,
    source_event_id: Option<&str>,
    effects: &[SemanticEffect],
) -> Result<u64> {
    if summary.trim().is_empty() {
        return Err(Error::domain(
            "revision_summary_empty",
            "revision summary must not be empty",
        ));
    }
    let project = project_with_connection(transaction, project_id)?;
    if project.status != ProjectStatus::Active {
        return Err(Error::domain(
            "project_not_active",
            format!("cannot revise {project_id} while it is {}", project.status),
        ));
    }
    let repository = repository_with_connection(transaction, project_id, None)?;
    if repository.revision != base_revision {
        return Err(Error::domain(
            "base_revision_conflict",
            format!(
                "project {project_id} is at revision {}, proposal was based on {base_revision}",
                repository.revision
            ),
        ));
    }
    let next_concept_number =
        validate_effects_for_next_ids(&repository, effects, project.next_concept_number)?;
    let revision = base_revision + 1;
    let revision_sql = sql_u64(revision, "semantic revision")?;
    transaction.execute(
        "INSERT INTO semantic_revisions
            (project_id, revision, summary, source_event_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            project_id,
            revision_sql,
            summary.trim(),
            source_event_id,
            now()
        ],
    )?;
    for (ordinal, effect) in effects.iter().enumerate() {
        transaction.execute(
            "INSERT INTO semantic_effects
                (project_id, revision, ordinal, effect_kind, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                project_id,
                revision_sql,
                i64::try_from(ordinal).map_err(|_| {
                    Error::domain("effect_ordinal_overflow", "too many semantic effects")
                })?,
                effect.kind(),
                serde_json::to_string(effect)?,
            ],
        )?;
    }
    let next_concept_number_sql = sql_u64(next_concept_number, "next concept number")?;
    transaction.execute(
        "UPDATE projects SET next_concept_number = ?2, updated_at = ?3 WHERE id = ?1",
        params![project_id, next_concept_number_sql, now()],
    )?;
    Ok(revision)
}

fn repository_with_connection(
    connection: &Connection,
    project_id: &str,
    through: Option<u64>,
) -> Result<Repository> {
    let mut repository = Repository::empty(project_id);
    let revisions = revisions_with_connection(connection, project_id, 1, through)?;
    for revision in revisions {
        repository.apply_revision(revision.number, &revision.effects)?;
    }
    Ok(repository)
}

fn revisions_with_connection(
    connection: &Connection,
    project_id: &str,
    from: u64,
    to: Option<u64>,
) -> Result<Vec<Revision>> {
    let from_sql = sql_u64(from, "from revision")?;
    let upper_sql = match to {
        Some(value) => sql_u64(value, "to revision")?,
        None => i64::MAX,
    };
    let mut revision_statement = connection.prepare(
        "SELECT revision, summary, source_event_id, created_at
         FROM semantic_revisions
         WHERE project_id = ?1 AND revision >= ?2 AND revision <= ?3
         ORDER BY revision",
    )?;
    let rows = revision_statement.query_map(params![project_id, from_sql, upper_sql], |row| {
        Ok((
            row_u64(row, 0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut revisions = Vec::new();
    for row in rows {
        let (number, summary, source_event_id, created_at) = row?;
        let mut effect_statement = connection.prepare(
            "SELECT payload_json FROM semantic_effects
             WHERE project_id = ?1 AND revision = ?2 ORDER BY ordinal",
        )?;
        let effects = effect_statement
            .query_map(
                params![project_id, sql_u64(number, "semantic revision")?],
                |row| row.get::<_, String>(0),
            )?
            .map(|value| {
                let value = value?;
                serde_json::from_str::<SemanticEffect>(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        value.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        revisions.push(Revision {
            project_id: project_id.to_owned(),
            number,
            summary,
            source_event_id,
            created_at,
            effects,
        });
    }
    Ok(revisions)
}

fn project_with_connection(connection: &Connection, id: &str) -> Result<Project> {
    connection
        .query_row(
            "SELECT p.id, p.status, p.current_path, p.activation_cursor, p.scan_cursor,
                    p.next_concept_number, COALESCE(MAX(r.revision), 0)
             FROM projects p
             LEFT JOIN semantic_revisions r ON r.project_id = p.id
             WHERE p.id = ?1 GROUP BY p.id",
            [id],
            project_from_row,
        )
        .optional()?
        .ok_or_else(|| Error::domain("project_not_found", format!("project {id} does not exist")))
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let status = row.get::<_, String>(1)?;
    let status = ProjectStatus::from_str(&status).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            status.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(Project {
        id: row.get(0)?,
        status,
        current_path: row.get(2)?,
        activation_cursor: row.get(3)?,
        scan_cursor: row.get(4)?,
        next_concept_number: row_u64(row, 5)?,
        current_revision: row_u64(row, 6)?,
    })
}

fn intake_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Intake> {
    let status_text = row.get::<_, String>(3)?;
    let status = IntakeStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            status_text.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    let decision_json = row.get::<_, String>(5)?;
    let decision = serde_json::from_str(&decision_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            decision_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(Intake {
        event_id: row.get(0)?,
        source_cursor: row.get(1)?,
        project_id: row.get(2)?,
        status,
        cwd: row.get(4)?,
        decision,
        attempts: row_u64(row, 6)?,
        last_error: row.get(7)?,
        applied_revision: row_optional_u64(row, 8)?,
        terminal_reason: row.get(9)?,
    })
}

fn mailbox_receipt_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    call_id: &str,
) -> Result<Option<MailboxReceipt>> {
    transaction
        .query_row(
            "SELECT arguments_sha256, result_json, is_error, committed_revision
             FROM mailbox_receipts WHERE job_id = ?1 AND call_id = ?2",
            params![job_id, call_id],
            |row| {
                Ok(MailboxReceipt {
                    arguments_sha256: row.get(0)?,
                    result_json: row.get(1)?,
                    is_error: row.get(2)?,
                    committed_revision: row_optional_u64(row, 3)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn resolve_awaiting_tx(
    transaction: &Transaction<'_>,
    project_id: &str,
    decision_id: &str,
    reason: &str,
) -> Result<()> {
    transaction.execute(
        "UPDATE intake_events
         SET status = 'ignored', terminal_reason = ?3, updated_at = ?4
         WHERE project_id = ?1 AND decision_id = ?2 AND status = 'awaiting_review'",
        params![project_id, decision_id, reason, now()],
    )?;
    Ok(())
}

fn validate_proposal_provenance(
    event: &DecisionEvent,
    proposal: &ReconciliationProposal,
) -> Result<()> {
    let permits_ground = event.event_kind == "decision_admitted"
        || (event.event_kind == "decision_reviewed"
            && event.review_action.as_deref() == Some("confirm"));
    let permits_unground = event.event_kind == "decision_reviewed"
        && event.review_action.as_deref() == Some("dismiss");
    for effect in &proposal.effects {
        match effect {
            SemanticEffect::Ground {
                source:
                    crate::domain::GroundingSource::Decision {
                        event_id,
                        decision_id,
                    },
                ..
            } if permits_ground
                && event_id == &event.event_id
                && decision_id == &event.decision_id => {}
            SemanticEffect::Ground { .. } => {
                return Err(Error::domain(
                    "decision_grounding_forbidden",
                    "every decision Ground must exactly match an effective admission or confirmation",
                ));
            }
            SemanticEffect::Unground {
                decision_id,
                withdrawal_event_id,
                ..
            } if permits_unground
                && decision_id == &event.decision_id
                && withdrawal_event_id == &event.event_id => {}
            SemanticEffect::Unground { .. } => {
                return Err(Error::domain(
                    "decision_ungrounding_forbidden",
                    "every Unground must match the dismissed decision and current withdrawal event",
                ));
            }
            _ => {}
        }
    }
    match event.event_kind.as_str() {
        "decision_admitted" => {
            let exact_ground = proposal.effects.iter().any(|effect| {
                matches!(
                    effect,
                    SemanticEffect::Ground {
                        source: crate::domain::GroundingSource::Decision {
                            event_id,
                            decision_id,
                        },
                        ..
                    } if event_id == &event.event_id && decision_id == &event.decision_id
                )
            });
            if !exact_ground {
                return Err(Error::domain(
                    "decision_grounding_required",
                    format!(
                        "decision admission {} must ground its exact event and decision identity",
                        event.event_id
                    ),
                ));
            }
        }
        "decision_reviewed" if event.review_action.as_deref() == Some("dismiss") => {
            let exact_withdrawal = proposal.effects.iter().any(|effect| {
                matches!(
                    effect,
                    SemanticEffect::Unground {
                        decision_id,
                        withdrawal_event_id,
                        ..
                    } if decision_id == &event.decision_id
                        && withdrawal_event_id == &event.event_id
                )
            });
            if !exact_withdrawal {
                return Err(Error::domain(
                    "decision_withdrawal_required",
                    format!(
                        "dismissal {} must withdraw an active grounding for decision {}",
                        event.event_id, event.decision_id
                    ),
                ));
            }
        }
        "decision_reviewed" if event.review_action.as_deref() == Some("confirm") => {
            let exact_ground = proposal.effects.iter().any(|effect| {
                matches!(
                    effect,
                    SemanticEffect::Ground {
                        source: crate::domain::GroundingSource::Decision {
                            event_id,
                            decision_id,
                        },
                        ..
                    } if event_id == &event.event_id && decision_id == &event.decision_id
                )
            });
            if !exact_ground {
                return Err(Error::domain(
                    "confirmed_decision_grounding_required",
                    format!(
                        "effective confirmation {} must ground its exact event and decision identity",
                        event.event_id
                    ),
                ));
            }
        }
        "decision_reviewed" => {
            return Err(Error::domain(
                "review_not_effective",
                format!(
                    "review event {} is not an effective confirm or dismiss",
                    event.event_id
                ),
            ));
        }
        kind => {
            return Err(Error::domain(
                "decision_event_kind_unsupported",
                format!("unsupported Decisions lifecycle event kind {kind:?}"),
            ));
        }
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        Error::domain(
            "path_not_utf8",
            format!("path is not UTF-8: {}", path.display()),
        )
    })
}

fn sql_u64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        Error::domain(
            "integer_out_of_range",
            format!("{field} exceeds SQLite's signed integer range"),
        )
    })
}

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value = row.get::<_, i64>(index)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn row_optional_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn now() -> String {
    time::OffsetDateTime::now_utc().to_string()
}

fn map_project_constraint(error: rusqlite::Error, id: &str, root: &str) -> Error {
    if matches!(error, rusqlite::Error::SqliteFailure(_, _)) {
        Error::domain(
            "project_conflict",
            format!("project ID {id:?} or registered root {root:?} already exists"),
        )
    } else {
        Error::Sql(error)
    }
}

const SCHEMA: &str = r#"
BEGIN IMMEDIATE;
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'retired')),
    current_path TEXT NOT NULL UNIQUE,
    activation_cursor TEXT NOT NULL,
    scan_cursor TEXT NOT NULL,
    next_concept_number INTEGER NOT NULL CHECK (next_concept_number >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
CREATE TABLE project_paths (
    project_id TEXT NOT NULL REFERENCES projects(id),
    path TEXT NOT NULL,
    activation_cursor TEXT NOT NULL,
    opened_at TEXT NOT NULL,
    closed_at TEXT,
    PRIMARY KEY (project_id, path, opened_at)
) STRICT;
CREATE UNIQUE INDEX one_open_project_path
ON project_paths(project_id) WHERE closed_at IS NULL;
CREATE TABLE semantic_revisions (
    project_id TEXT NOT NULL REFERENCES projects(id),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    summary TEXT NOT NULL,
    source_event_id TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (project_id, revision),
    UNIQUE (project_id, source_event_id)
) STRICT;
CREATE TABLE semantic_effects (
    project_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    effect_kind TEXT NOT NULL CHECK (
        effect_kind IN ('define', 'revise', 'differentiate', 'reopen', 'retire', 'ground', 'unground')
    ),
    payload_json TEXT NOT NULL,
    PRIMARY KEY (project_id, revision, ordinal),
    FOREIGN KEY (project_id, revision)
        REFERENCES semantic_revisions(project_id, revision)
) STRICT;
CREATE TABLE intake_events (
    event_id TEXT PRIMARY KEY,
    source_cursor TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id),
    status TEXT NOT NULL CHECK (
        status IN ('unassigned', 'pending', 'awaiting_review', 'paused', 'processing', 'applied', 'ignored', 'failed')
    ),
    cwd TEXT,
    host_id TEXT,
    thread_id TEXT,
    turn_id TEXT,
    decision_id TEXT NOT NULL,
    decision_json TEXT NOT NULL,
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    last_error TEXT,
    terminal_reason TEXT,
    applied_revision INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (project_id, applied_revision)
        REFERENCES semantic_revisions(project_id, revision)
) STRICT;
CREATE TABLE request_correlations (
    event_id TEXT PRIMARY KEY REFERENCES intake_events(event_id),
    requester_id TEXT NOT NULL UNIQUE,
    job_id TEXT NOT NULL UNIQUE,
    request_json TEXT NOT NULL,
    request_sha256 TEXT NOT NULL,
    tool_after INTEGER NOT NULL CHECK (tool_after >= 0),
    admitted INTEGER NOT NULL CHECK (admitted IN (0, 1)),
    created_at TEXT NOT NULL
) STRICT;
CREATE TABLE intake_assignments (
    event_id TEXT NOT NULL REFERENCES intake_events(event_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    previous_project_id TEXT REFERENCES projects(id),
    project_id TEXT NOT NULL REFERENCES projects(id),
    assigned_at TEXT NOT NULL,
    PRIMARY KEY (event_id, ordinal)
) STRICT;
CREATE TABLE mailbox_receipts (
    job_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    arguments_sha256 TEXT NOT NULL,
    result_json TEXT NOT NULL,
    is_error INTEGER NOT NULL CHECK (is_error IN (0, 1)),
    committed_revision INTEGER,
    created_at TEXT NOT NULL,
    PRIMARY KEY (job_id, call_id)
) STRICT;
PRAGMA user_version = 1;
COMMIT;
"#;

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::domain::{
        DecisionAnchor, DecisionEvent, GroundingSource, IntakeStatus, ProjectStatus,
        ReconciliationProposal, SemanticEffect,
    };

    use super::{Correlation, Store, validate_proposal_provenance};

    fn fixture() -> (TempDir, Store) {
        let temporary = TempDir::new().expect("temporary directory");
        let root = temporary.path().join("project");
        fs::create_dir(&root).expect("project directory");
        let store = Store::open(temporary.path().join("semantics.db")).expect("database");
        store
            .register_project("cell", &root, "cursor-1")
            .expect("project registration");
        (temporary, store)
    }

    fn decision_event(
        event_id: &str,
        decision_id: &str,
        event_kind: &str,
        confidence: &str,
        review_action: Option<&str>,
    ) -> DecisionEvent {
        DecisionEvent {
            event_id: event_id.to_owned(),
            event_version: 1,
            cursor: format!("cursor-{event_id}"),
            event_kind: event_kind.to_owned(),
            occurred_at: 1,
            decision_id: decision_id.to_owned(),
            decided_at: 1,
            timestamp_precision: "second".to_owned(),
            statement: "Use stable semantic identities.".to_owned(),
            disposition: "adopt".to_owned(),
            confidence: confidence.to_owned(),
            rationale: Some("Preserve meaning.".to_owned()),
            supersedes_decision_id: None,
            authority_start: 0,
            authority_end: 10,
            review_state: review_action
                .map_or("unreviewed", |action| {
                    if action == "confirm" {
                        "confirmed"
                    } else {
                        "dismissed"
                    }
                })
                .to_owned(),
            review_id: review_action.map(|_| format!("review-{event_id}")),
            review_action: review_action.map(str::to_owned),
            reviewed_at: review_action.map(|_| 2),
            review_source: review_action.map(|_| "operator".to_owned()),
            anchors: vec![DecisionAnchor {
                source_role: "authority".to_owned(),
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: "item".to_owned(),
                message_role: "user".to_owned(),
                occurred_at: 1,
                timestamp_precision: "second".to_owned(),
            }],
        }
    }

    fn correlate(store: &Store, event_id: &str, job_id: &str) {
        store
            .put_correlation(&Correlation {
                event_id: event_id.to_owned(),
                requester_id: format!("request-{event_id}"),
                job_id: job_id.to_owned(),
                request_json: "{}".to_owned(),
                request_sha256: "digest".to_owned(),
                tool_after: 0,
                admitted: true,
            })
            .expect("correlation");
    }

    #[test]
    fn project_move_preserves_path_history_and_cursors() {
        let (temporary, store) = fixture();
        let moved = temporary.path().join("moved");
        fs::create_dir(&moved).expect("moved directory");
        store.move_project("cell", &moved).expect("project move");
        let detail = store.project_detail("cell").expect("project detail");
        assert_eq!(detail.paths.len(), 2);
        assert!(detail.paths[0].closed_at.is_some());
        assert_eq!(detail.project.activation_cursor, "cursor-1");
        assert_eq!(detail.project.scan_cursor, "cursor-1");
    }

    #[test]
    fn repository_replays_append_only_effects() {
        let (_temporary, store) = fixture();
        let revision = store
            .commit_revision(
                "cell",
                0,
                "Define concern",
                None,
                &[SemanticEffect::Define {
                    concept_id: "c000001".to_owned(),
                    label: "Concern".to_owned(),
                    meaning: "A durable open question.".to_owned(),
                }],
            )
            .expect("commit");
        assert_eq!(revision, 1);
        let repository = store.repository("cell", None).expect("repository");
        assert_eq!(repository.revision, 1);
        assert_eq!(repository.concepts["c000001"].label, "Concern");
    }

    #[test]
    fn paused_projects_keep_identity_but_stop_processing() {
        let (_temporary, store) = fixture();
        store
            .set_project_status("cell", ProjectStatus::Paused)
            .expect("pause");
        assert_eq!(
            store.project("cell").expect("project").status,
            ProjectStatus::Paused
        );
    }

    #[test]
    fn exact_revision_reads_reject_beyond_head() {
        let (_temporary, store) = fixture();
        assert_eq!(
            store
                .repository("cell", Some(1))
                .expect_err("missing revision must fail")
                .code(),
            "revision_not_found"
        );
        assert_eq!(
            store
                .diff("cell", 0, 1)
                .expect_err("missing diff endpoint must fail")
                .code(),
            "revision_not_found"
        );
    }

    #[test]
    fn intake_replay_requires_identical_immutable_envelope_and_routing() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        assert!(
            store
                .insert_intake(&event, Some("cell"), Some(&root))
                .expect("first insertion")
        );
        assert!(
            !store
                .insert_intake(&event, Some("cell"), Some(&root))
                .expect("identical replay")
        );

        let mut conflicting = event;
        conflicting.cursor = "different-cursor".to_owned();
        assert_eq!(
            store
                .insert_intake(&conflicting, Some("cell"), Some(&root))
                .expect_err("conflicting replay must fail")
                .code(),
            "intake_replay_conflict"
        );
    }

    #[test]
    fn automatic_routing_keeps_one_decision_in_one_project() {
        let (temporary, store) = fixture();
        let other = temporary.path().join("other");
        fs::create_dir(&other).expect("other root");
        store
            .register_project("other", &other, "cursor-2")
            .expect("other registration");
        let root = temporary.path().join("project");
        let admission = decision_event(
            "admission-1",
            "decision-1",
            "decision_admitted",
            "high",
            None,
        );
        store
            .insert_intake(&admission, Some("cell"), Some(&root))
            .expect("admission");
        let review = decision_event(
            "review-1",
            "decision-1",
            "decision_reviewed",
            "high",
            Some("dismiss"),
        );
        store
            .insert_intake(&review, Some("other"), Some(&other))
            .expect("conflicting review retained");
        let retained = store.intake("review-1").expect("review");
        assert_eq!(retained.status, IntakeStatus::Unassigned);
        assert!(retained.project_id.is_none());
        assert!(
            retained
                .last_error
                .as_deref()
                .is_some_and(|detail| detail.contains("already assigned to project cell"))
        );
    }

    #[test]
    fn manual_assignment_cannot_create_an_authority_source() {
        let (_temporary, store) = fixture();
        let mut event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        event.anchors.clear();
        store
            .insert_intake(&event, None, None)
            .expect("unattributable intake");
        assert_eq!(
            store
                .assign_intake("event-1", "cell")
                .expect_err("assignment cannot repair authority")
                .code(),
            "intake_authority_unresolved"
        );
    }

    #[test]
    fn review_is_blocked_by_unresolved_observed_admission() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let admission = decision_event(
            "admission-1",
            "decision-1",
            "decision_admitted",
            "high",
            None,
        );
        store
            .insert_intake(&admission, None, None)
            .expect("unassigned admission");
        let review = decision_event(
            "review-1",
            "decision-1",
            "decision_reviewed",
            "high",
            Some("confirm"),
        );
        store
            .insert_intake(&review, Some("cell"), Some(&root))
            .expect("assigned review");
        let review = store.intake("review-1").expect("review");
        let reason = store
            .review_admission_block(&review)
            .expect("admission gate")
            .expect("review must be blocked");
        assert!(reason.contains("assigned to None"));
    }

    #[test]
    fn processing_transition_is_idempotent_without_attempt_inflation() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&event, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("first claim");
        store.mark_processing("event-1").expect("resume claim");
        assert_eq!(store.intake("event-1").expect("intake").attempts, 1);
    }

    #[test]
    fn committed_mailbox_receipt_is_resumable_until_ack() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&event, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("claim");
        correlate(&store, "event-1", "job-1");
        let proposal = ReconciliationProposal {
            base_revision: 0,
            summary: "Define and ground concern".to_owned(),
            effects: vec![
                SemanticEffect::Define {
                    concept_id: "c000001".to_owned(),
                    label: "Concern".to_owned(),
                    meaning: "A durable open question.".to_owned(),
                },
                SemanticEffect::Ground {
                    concept_id: "c000001".to_owned(),
                    source: GroundingSource::Decision {
                        event_id: "event-1".to_owned(),
                        decision_id: "decision-1".to_owned(),
                    },
                    statement: "Use stable semantic identities.".to_owned(),
                },
            ],
        };
        let revision = store
            .commit_mailbox_proposal(
                "event-1",
                "job-1",
                "call-1",
                "arguments-digest",
                "cell",
                &proposal,
            )
            .expect("domain commit");
        assert_eq!(revision, 1);
        let intake = store.intake("event-1").expect("intake");
        assert_eq!(intake.status, IntakeStatus::Processing);
        assert_eq!(intake.applied_revision, Some(1));
        assert_eq!(
            store
                .pending_committed_revision("event-1", "job-1")
                .expect("pending commit"),
            Some(1)
        );
        let replay = store
            .commit_mailbox_proposal(
                "event-1",
                "job-1",
                "call-1",
                "arguments-digest",
                "cell",
                &proposal,
            )
            .expect("receipt replay");
        assert_eq!(replay, 1);
        assert_eq!(
            store.repository("cell", None).expect("repository").revision,
            1
        );
        store
            .finalize_applied("event-1", "job-1")
            .expect("ack finalization");
        assert_eq!(
            store.intake("event-1").expect("intake").status,
            IntakeStatus::Applied
        );
    }

    #[test]
    fn ignored_review_atomically_resolves_waiting_admission() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let admission = decision_event(
            "admission-1",
            "decision-1",
            "decision_admitted",
            "medium",
            None,
        );
        store
            .insert_intake(&admission, Some("cell"), Some(&root))
            .expect("admission");
        store
            .mark_awaiting_review("admission-1", "await confirmation")
            .expect("awaiting review");
        let dismissal = decision_event(
            "review-1",
            "decision-1",
            "decision_reviewed",
            "medium",
            Some("dismiss"),
        );
        store
            .insert_intake(&dismissal, Some("cell"), Some(&root))
            .expect("review");
        store
            .mark_ignored("review-1", "no active grounding")
            .expect("ignored review");
        assert_eq!(
            store.intake("admission-1").expect("admission").status,
            IntakeStatus::Ignored
        );
        assert_eq!(
            store.intake("review-1").expect("review").status,
            IntakeStatus::Ignored
        );
    }

    #[test]
    fn correlated_failed_intake_cannot_be_reassigned_across_projects() {
        let (temporary, store) = fixture();
        let other = temporary.path().join("other");
        fs::create_dir(&other).expect("other root");
        store
            .register_project("other", &other, "cursor-2")
            .expect("other registration");
        let root = temporary.path().join("project");
        let event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&event, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("claim");
        correlate(&store, "event-1", "job-1");
        store
            .mark_failed("event-1", "terminal model failure")
            .expect("failure");
        assert_eq!(
            store
                .assign_intake("event-1", "other")
                .expect_err("correlated assignment must fail")
                .code(),
            "intake_assignment_correlated"
        );
        assert_eq!(
            store
                .intake("event-1")
                .expect("intake")
                .project_id
                .as_deref(),
            Some("cell")
        );
    }

    #[test]
    fn retirement_requires_pause_and_no_unresolved_intake() {
        let (temporary, store) = fixture();
        assert_eq!(
            store
                .set_project_status("cell", ProjectStatus::Retired)
                .expect_err("active retirement must fail")
                .code(),
            "project_transition_invalid"
        );
        let root = temporary.path().join("project");
        let event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&event, Some("cell"), Some(&root))
            .expect("intake");
        store
            .set_project_status("cell", ProjectStatus::Paused)
            .expect("pause");
        assert_eq!(
            store
                .set_project_status("cell", ProjectStatus::Retired)
                .expect_err("unresolved intake must block retirement")
                .code(),
            "project_intake_outstanding"
        );
    }

    #[test]
    fn append_order_selects_admission_before_review_when_timestamps_match() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let admission = decision_event(
            "z-admission",
            "decision-1",
            "decision_admitted",
            "high",
            None,
        );
        let review = decision_event(
            "a-review",
            "decision-1",
            "decision_reviewed",
            "high",
            Some("dismiss"),
        );
        store
            .insert_intake(&admission, Some("cell"), Some(&root))
            .expect("admission");
        store
            .insert_intake(&review, Some("cell"), Some(&root))
            .expect("review");
        assert_eq!(
            store
                .next_pending_intake()
                .expect("next")
                .expect("pending")
                .event_id,
            "z-admission"
        );
    }

    #[test]
    fn source_provenance_rejects_extra_invented_decision_effects() {
        let (temporary, store) = fixture();
        let root = temporary.path().join("project");
        let event = decision_event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&event, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("claim");
        correlate(&store, "event-1", "job-1");
        let proposal = ReconciliationProposal {
            base_revision: 0,
            summary: "Invented evidence".to_owned(),
            effects: vec![
                SemanticEffect::Define {
                    concept_id: "c000001".to_owned(),
                    label: "Concern".to_owned(),
                    meaning: "One.".to_owned(),
                },
                SemanticEffect::Ground {
                    concept_id: "c000001".to_owned(),
                    source: GroundingSource::Decision {
                        event_id: "event-1".to_owned(),
                        decision_id: "decision-1".to_owned(),
                    },
                    statement: "Exact.".to_owned(),
                },
                SemanticEffect::Ground {
                    concept_id: "c000001".to_owned(),
                    source: GroundingSource::Decision {
                        event_id: "invented".to_owned(),
                        decision_id: "invented".to_owned(),
                    },
                    statement: "Invented.".to_owned(),
                },
            ],
        };
        assert_eq!(
            store
                .commit_mailbox_proposal("event-1", "job-1", "call-1", "digest", "cell", &proposal,)
                .expect_err("invented source must fail")
                .code(),
            "decision_grounding_forbidden"
        );
        assert_eq!(
            store.repository("cell", None).expect("repository").revision,
            0
        );
    }

    #[test]
    fn source_provenance_rejects_effects_for_the_wrong_lifecycle_kind() {
        let admission = decision_event(
            "admission-1",
            "decision-1",
            "decision_admitted",
            "high",
            None,
        );
        let admission_withdrawal = ReconciliationProposal {
            base_revision: 0,
            summary: "Invalid withdrawal".to_owned(),
            effects: vec![SemanticEffect::Unground {
                concept_id: "c000001".to_owned(),
                event_id: "prior-event".to_owned(),
                decision_id: "decision-1".to_owned(),
                withdrawal_event_id: "admission-1".to_owned(),
                reason: "Invalid".to_owned(),
            }],
        };
        assert_eq!(
            validate_proposal_provenance(&admission, &admission_withdrawal)
                .expect_err("admission cannot withdraw")
                .code(),
            "decision_ungrounding_forbidden"
        );

        let dismissal = decision_event(
            "review-1",
            "decision-1",
            "decision_reviewed",
            "high",
            Some("dismiss"),
        );
        let dismissal_ground = ReconciliationProposal {
            base_revision: 0,
            summary: "Invalid grounding".to_owned(),
            effects: vec![SemanticEffect::Ground {
                concept_id: "c000001".to_owned(),
                source: GroundingSource::Decision {
                    event_id: "review-1".to_owned(),
                    decision_id: "decision-1".to_owned(),
                },
                statement: "Invalid".to_owned(),
            }],
        };
        assert_eq!(
            validate_proposal_provenance(&dismissal, &dismissal_ground)
                .expect_err("dismissal cannot ground")
                .code(),
            "decision_grounding_forbidden"
        );
    }
}
