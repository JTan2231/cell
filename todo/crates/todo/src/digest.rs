use rusqlite::{Connection, TransactionBehavior};

use crate::error::{AppError, AppResult};
use crate::model::{ConcernId, DesignId, RoutingProposalId, SituationAssessmentId, TodoSummary};
use crate::reconciliation_store::{self, DesignView, SituationAssessmentView};
use crate::todo_store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DailyDigest {
    pub(crate) decisions: Vec<DigestItem>,
    pub(crate) followups: Vec<DigestItem>,
    pub(crate) other_open: Vec<DigestItem>,
    pub(crate) open_todo_count: usize,
    pub(crate) pending_concern_count: usize,
}

impl DailyDigest {
    #[must_use]
    pub(crate) fn attention_count(&self) -> usize {
        self.decisions.len() + self.followups.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DigestItem {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) references: Vec<String>,
    pub(crate) inspect_commands: Vec<String>,
}

pub(crate) fn load(connection: &mut Connection) -> AppResult<DailyDigest> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut digest = DailyDigest {
        decisions: Vec::new(),
        followups: Vec::new(),
        other_open: Vec::new(),
        open_todo_count: 0,
        pending_concern_count: 0,
    };

    for (concern_id, routing_ids) in pending_concerns(&transaction)? {
        digest.pending_concern_count += 1;
        let item = concern_item(concern_id, &routing_ids);
        if routing_ids.is_empty() {
            digest.followups.push(item);
        } else {
            digest.decisions.push(item);
        }
    }

    let todos = todo_store::list_open(&transaction)?;
    digest.open_todo_count = todos.len();
    for todo in todos {
        let assessment = latest_assessment(&transaction, &todo)?
            .map(|id| reconciliation_store::get_assessment(&transaction, id))
            .transpose()?;
        let designs = designs(&transaction, &todo)?
            .into_iter()
            .map(|id| reconciliation_store::get_design(&transaction, id))
            .collect::<AppResult<Vec<_>>>()?;
        let returned = assessment
            .as_ref()
            .map(|assessment| assessment_was_returned(&transaction, assessment.id))
            .transpose()?
            .unwrap_or(false);
        let (section, item) = classify_todo(&todo, assessment.as_ref(), &designs, returned)?;
        match section {
            Section::Decision => digest.decisions.push(item),
            Section::Followup => digest.followups.push(item),
            Section::OtherOpen => digest.other_open.push(item),
        }
    }

    transaction.commit()?;
    Ok(digest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Decision,
    Followup,
    OtherOpen,
}

fn pending_concerns(
    connection: &Connection,
) -> AppResult<Vec<(ConcernId, Vec<RoutingProposalId>)>> {
    let raw_ids = connection
        .prepare(
            "SELECT id FROM concerns
             WHERE status = 'pending'
             ORDER BY created_at DESC, id DESC",
        )?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    raw_ids
        .into_iter()
        .map(|raw_id| {
            let concern_id = ConcernId::from_storage(raw_id)
                .map_err(|error| invalid_stored_id("concern", error))?;
            let routing_ids = connection
                .prepare(
                    "SELECT id FROM concern_routing_proposals
                     WHERE concern_id = ?1 AND decision = 'pending'
                     ORDER BY created_at DESC, id DESC",
                )?
                .query_map([raw_id], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|raw_routing_id| {
                    RoutingProposalId::from_storage(raw_routing_id)
                        .map_err(|error| invalid_stored_id("routing proposal", error))
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok((concern_id, routing_ids))
        })
        .collect()
}

fn latest_assessment(
    connection: &Connection,
    todo: &TodoSummary,
) -> AppResult<Option<SituationAssessmentId>> {
    let raw_id = connection.query_row(
        "SELECT max(id) FROM todo_situation_assessments WHERE todo_id = ?1",
        [todo.id.storage_id()],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    raw_id
        .map(|id| {
            SituationAssessmentId::from_storage(id)
                .map_err(|error| invalid_stored_id("situation assessment", error))
        })
        .transpose()
}

fn designs(connection: &Connection, todo: &TodoSummary) -> AppResult<Vec<DesignId>> {
    let raw_ids = connection
        .prepare(
            "SELECT id FROM todo_designs
             WHERE todo_id = ?1
             ORDER BY revision DESC, id DESC",
        )?
        .query_map([todo.id.storage_id()], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    raw_ids
        .into_iter()
        .map(|id| DesignId::from_storage(id).map_err(|error| invalid_stored_id("design", error)))
        .collect()
}

fn assessment_was_returned(
    connection: &Connection,
    assessment_id: SituationAssessmentId,
) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM todo_design_assessment_returns WHERE assessment_id = ?1
             )",
            [assessment_id.storage_id()],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn concern_item(concern_id: ConcernId, routing_ids: &[RoutingProposalId]) -> DigestItem {
    let mut references = vec![format!("Concern {concern_id}")];
    references.extend(
        routing_ids
            .iter()
            .map(|routing_id| format!("Routing proposal {routing_id}")),
    );
    let mut inspect_commands = vec![format!("todo concern show {concern_id}")];
    inspect_commands.extend(
        routing_ids
            .iter()
            .map(|routing_id| format!("todo routing show {routing_id}")),
    );
    DigestItem {
        title: "Captured concern".to_owned(),
        message: match routing_ids.len() {
            0 => "This captured concern remains unresolved.".to_owned(),
            1 => "A routing proposal is waiting for your decision.".to_owned(),
            count => format!("{count} routing proposals are waiting for your decision."),
        },
        references,
        inspect_commands,
    }
}

fn classify_todo(
    todo: &TodoSummary,
    assessment: Option<&SituationAssessmentView>,
    designs: &[DesignView],
    assessment_was_returned: bool,
) -> AppResult<(Section, DigestItem)> {
    let Some(assessment) = assessment else {
        if let Some(design) = designs
            .iter()
            .find(|design| design.state == "legacy_unreviewed")
        {
            return Ok(todo_item(
                Section::Followup,
                todo,
                None,
                &[design],
                "Legacy research has not been reviewed under the current model.",
            ));
        }
        return Ok(todo_item(
            Section::Followup,
            todo,
            None,
            &[],
            "No current situation assessment has been recorded.",
        ));
    };

    if !assessment.current {
        return Ok(todo_item(
            Section::Followup,
            todo,
            Some(assessment),
            &[],
            "The situation changed; a new assessment is needed.",
        ));
    }

    match assessment.disposition.as_str() {
        "needs_user_choice" => Ok(todo_item(
            Section::Decision,
            todo,
            Some(assessment),
            &[],
            "The current situation needs your choice.",
        )),
        "inconclusive" => Ok(todo_item(
            Section::Followup,
            todo,
            Some(assessment),
            &[],
            "More evidence is needed before the situation can be settled.",
        )),
        "ready" => classify_ready_todo(todo, assessment, designs, assessment_was_returned),
        other => Err(AppError::database(
            "invalid_assessment_disposition",
            format!("invalid stored situation assessment disposition: {other}"),
        )),
    }
}

fn classify_ready_todo(
    todo: &TodoSummary,
    assessment: &SituationAssessmentView,
    designs: &[DesignView],
    assessment_was_returned: bool,
) -> AppResult<(Section, DigestItem)> {
    let ready_designs = designs
        .iter()
        .filter(|design| design.current && design.state == "ready")
        .collect::<Vec<_>>();
    if !ready_designs.is_empty() {
        let message = match ready_designs.len() {
            1 => "A proposed desired state is ready for your review.".to_owned(),
            count => format!("{count} proposed desired states are ready for your review."),
        };
        return Ok(todo_item(
            Section::Decision,
            todo,
            Some(assessment),
            &ready_designs,
            &message,
        ));
    }
    if assessment_was_returned {
        return Ok(todo_item(
            Section::Followup,
            todo,
            Some(assessment),
            &[],
            "Design work found that more situation research is needed.",
        ));
    }

    let Some(design) = designs.iter().find(|design| design.current) else {
        return Ok(todo_item(
            Section::Followup,
            todo,
            Some(assessment),
            &[],
            "The current situation is ready for desired-state design.",
        ));
    };
    let (section, message) = match design.state.as_str() {
        "authorized" => (
            Section::OtherOpen,
            "The desired state is accepted; this todo remains open.",
        ),
        "open" => (
            Section::OtherOpen,
            "A desired-state design is being prepared; this todo remains open.",
        ),
        "rejected" => (
            Section::Followup,
            "The proposed desired state was rejected; a new design is needed.",
        ),
        "abandoned" => (
            Section::Followup,
            "The desired-state draft was left incomplete.",
        ),
        "discarded" => (
            Section::Followup,
            "The desired-state draft was discarded; a new design is needed.",
        ),
        "invalidated" => (
            Section::Followup,
            "The situation changed; a new desired-state design is needed.",
        ),
        "ready" => {
            return Err(AppError::database(
                "invalid_digest_state",
                "a current ready design was not classified",
            ));
        }
        "legacy_unreviewed" => (
            Section::Followup,
            "Legacy research has not been reviewed under the current model.",
        ),
        other => {
            return Err(AppError::database(
                "invalid_design_state",
                format!("invalid stored desired-state design state: {other}"),
            ));
        }
    };
    Ok(todo_item(
        section,
        todo,
        Some(assessment),
        &[design],
        message,
    ))
}

fn todo_item(
    section: Section,
    todo: &TodoSummary,
    assessment: Option<&SituationAssessmentView>,
    designs: &[&DesignView],
    message: &str,
) -> (Section, DigestItem) {
    let mut references = vec![format!("Todo {}", todo.id)];
    let mut inspect_commands = vec![format!("todo show {}", todo.id)];
    if let Some(assessment) = assessment {
        references.push(format!("Situation assessment {}", assessment.id));
        inspect_commands.push(format!("todo situation show {}", assessment.id));
    }
    for design in designs {
        references.push(format!("Desired-state design {}", design.id));
        inspect_commands.push(format!("todo design show {}", design.id));
    }
    (
        section,
        DigestItem {
            title: todo.title.clone(),
            message: message.to_owned(),
            references,
            inspect_commands,
        },
    )
}

fn invalid_stored_id(kind: &str, error: impl std::fmt::Display) -> AppError {
    AppError::database(
        "invalid_stored_id",
        format!("invalid stored {kind} ID: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use super::load;
    use crate::db;
    use crate::model::{DesignId, SituationAssessmentId, TodoId};
    use crate::reconciliation_store;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn groups_pending_routing_and_does_not_project_private_fields() -> TestResult {
        let (_directory, mut connection) = database()?;
        let routed = insert_pending_concern(&connection, "private concern body one")?;
        let unrouted = insert_pending_concern(&connection, "private concern body two")?;
        insert_routing(&connection, routed, "one", "private rationale one")?;
        insert_routing(&connection, routed, "two", "private rationale two")?;

        let digest = load(&mut connection)?;

        assert_eq!(digest.pending_concern_count, 2);
        assert_eq!(digest.decisions.len(), 1);
        assert_eq!(digest.followups.len(), 1);
        assert_eq!(digest.attention_count(), 2);
        assert_eq!(digest.decisions[0].title, "Captured concern");
        assert_eq!(
            digest.decisions[0].message,
            "2 routing proposals are waiting for your decision."
        );
        assert_eq!(digest.decisions[0].references.len(), 3);
        assert_eq!(digest.decisions[0].inspect_commands.len(), 3);
        assert_eq!(
            digest.followups[0].references,
            [format!("Concern {unrouted}")]
        );
        let projected = format!("{digest:?}");
        for private in [
            "private concern body one",
            "private concern body two",
            "private rationale one",
            "private rationale two",
            "/tmp/private-source.jsonl",
        ] {
            assert!(!projected.contains(private));
        }
        Ok(())
    }

    #[test]
    fn authorized_defer_remains_a_factually_labeled_followup() -> TestResult {
        let (_directory, mut connection) = database()?;
        let concern = insert_pending_concern(&connection, "private deferred concern")?;
        insert_routing(&connection, concern, "deferred", "private defer rationale")?;
        connection.execute(
            "UPDATE concern_routing_proposals
             SET decision = 'authorized',
                 decision_source_path = '/tmp/private-decision.jsonl',
                 decided_at = '2026-08-31T12:00:00Z'
             WHERE concern_id = ?1",
            [concern.storage_id()],
        )?;

        let digest = load(&mut connection)?;

        assert!(digest.decisions.is_empty());
        assert_eq!(digest.followups.len(), 1);
        assert_eq!(
            digest.followups[0].message,
            "This captured concern remains unresolved."
        );
        assert!(!format!("{digest:?}").contains("private deferred concern"));
        Ok(())
    }

    #[test]
    fn groups_ready_designs_and_keeps_accepted_todos_open() -> TestResult {
        let (_directory, mut connection) = database()?;
        let review_todo = insert_todo(&connection, "Review this desired state")?;
        let review_assessment = insert_assessment(&connection, review_todo, "ready", "review")?;
        let older = insert_design(&connection, review_todo, review_assessment, "ready", "old")?;
        let newer = insert_design(&connection, review_todo, review_assessment, "ready", "new")?;
        let accepted_todo = insert_todo(&connection, "Accepted but still open")?;
        let accepted_assessment =
            insert_assessment(&connection, accepted_todo, "ready", "accepted")?;
        let accepted = insert_design(
            &connection,
            accepted_todo,
            accepted_assessment,
            "authorized",
            "accepted",
        )?;

        let digest = load(&mut connection)?;

        assert_eq!(digest.open_todo_count, 2);
        assert_eq!(digest.decisions.len(), 1);
        assert_eq!(digest.other_open.len(), 1);
        assert_eq!(digest.decisions[0].title, "Review this desired state");
        assert_eq!(
            digest.decisions[0].message,
            "2 proposed desired states are ready for your review."
        );
        assert_eq!(
            digest.decisions[0].references,
            [
                format!("Todo {review_todo}"),
                format!("Situation assessment {review_assessment}"),
                format!("Desired-state design {newer}"),
                format!("Desired-state design {older}"),
            ]
        );
        assert_eq!(
            digest.other_open[0].message,
            "The desired state is accepted; this todo remains open."
        );
        assert!(
            digest.other_open[0]
                .references
                .contains(&format!("Desired-state design {accepted}"))
        );
        Ok(())
    }

    #[test]
    fn assessment_returns_and_staleness_override_raw_ready_state() -> TestResult {
        let (_directory, mut connection) = database()?;
        let returned_todo = insert_todo(&connection, "Returned assessment")?;
        let returned_assessment =
            insert_assessment(&connection, returned_todo, "ready", "returned")?;
        insert_assessment_return(&connection, returned_todo, returned_assessment, "returned")?;

        let stale_todo = insert_todo(&connection, "Stale assessment")?;
        insert_assessment(&connection, stale_todo, "ready", "stale")?;
        connection.execute(
            "INSERT INTO todo_notes(todo_id, text) VALUES(?1, 'private changed state')",
            [stale_todo.storage_id()],
        )?;

        let digest = load(&mut connection)?;

        assert_eq!(digest.followups.len(), 2);
        let returned = digest
            .followups
            .iter()
            .find(|item| item.title == "Returned assessment")
            .ok_or("returned todo missing")?;
        assert_eq!(
            returned.message,
            "Design work found that more situation research is needed."
        );
        let stale = digest
            .followups
            .iter()
            .find(|item| item.title == "Stale assessment")
            .ok_or("stale todo missing")?;
        assert_eq!(
            stale.message,
            "The situation changed; a new assessment is needed."
        );
        assert!(!format!("{digest:?}").contains("private changed state"));
        Ok(())
    }

    #[test]
    fn includes_only_canonical_open_todos() -> TestResult {
        let (_directory, mut connection) = database()?;
        let superseded = insert_todo(&connection, "Superseded private title")?;
        let survivor = insert_todo(&connection, "Canonical title")?;
        insert_supersession(&connection, superseded, survivor)?;

        let digest = load(&mut connection)?;

        assert_eq!(digest.open_todo_count, 1);
        let items = digest
            .decisions
            .iter()
            .chain(&digest.followups)
            .chain(&digest.other_open)
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Canonical title");
        assert!(!format!("{digest:?}").contains("Superseded private title"));
        Ok(())
    }

    #[test]
    fn uses_plain_language_for_assessment_and_design_states() -> TestResult {
        let (_directory, mut connection) = database()?;
        insert_todo(&connection, "Unassessed state")?;
        let digest = load(&mut connection)?;
        assert_eq!(
            digest.followups[0].message,
            "No current situation assessment has been recorded."
        );

        let assessment_cases = [
            (
                "needs_user_choice",
                "The current situation needs your choice.",
                true,
            ),
            (
                "inconclusive",
                "More evidence is needed before the situation can be settled.",
                false,
            ),
        ];
        for (disposition, expected, decision) in assessment_cases {
            let (_directory, mut connection) = database()?;
            let todo = insert_todo(&connection, "Assessment state")?;
            insert_assessment(&connection, todo, disposition, disposition)?;
            let digest = load(&mut connection)?;
            let items = if decision {
                &digest.decisions
            } else {
                &digest.followups
            };
            assert_eq!(items[0].message, expected);
            assert!(!items[0].message.contains(disposition));
        }

        let (_directory, mut connection) = database()?;
        let ready_todo = insert_todo(&connection, "Ready state")?;
        insert_assessment(&connection, ready_todo, "ready", "ready-no-design")?;
        let digest = load(&mut connection)?;
        assert_eq!(
            digest.followups[0].message,
            "The current situation is ready for desired-state design."
        );

        let (_directory, mut connection) = database()?;
        let open_todo = insert_todo(&connection, "Open design state")?;
        let open_assessment = insert_assessment(&connection, open_todo, "ready", "open-design")?;
        insert_design(
            &connection,
            open_todo,
            open_assessment,
            "open",
            "open-design",
        )?;
        let digest = load(&mut connection)?;
        assert_eq!(
            digest.other_open[0].message,
            "A desired-state design is being prepared; this todo remains open."
        );

        let design_cases = [
            (
                "rejected",
                "The proposed desired state was rejected; a new design is needed.",
            ),
            ("abandoned", "The desired-state draft was left incomplete."),
            (
                "discarded",
                "The desired-state draft was discarded; a new design is needed.",
            ),
            (
                "invalidated",
                "The situation changed; a new desired-state design is needed.",
            ),
        ];
        for (state, expected) in design_cases {
            let (_directory, mut connection) = database()?;
            let todo = insert_todo(&connection, "Design state")?;
            let assessment = insert_assessment(&connection, todo, "ready", state)?;
            insert_design(&connection, todo, assessment, state, state)?;
            let digest = load(&mut connection)?;
            assert_eq!(digest.followups[0].message, expected);
        }

        let (_directory, mut connection) = database()?;
        let legacy_todo = insert_todo(&connection, "Legacy state")?;
        insert_legacy_design(&connection, legacy_todo)?;
        let digest = load(&mut connection)?;
        assert_eq!(
            digest.followups[0].message,
            "Legacy research has not been reviewed under the current model."
        );
        Ok(())
    }

    fn database() -> TestResult<(tempfile::TempDir, Connection)> {
        let directory = tempfile::tempdir()?;
        let connection = db::init(&directory.path().join("todo.db"))?;
        Ok((directory, connection))
    }

    fn insert_pending_concern(
        connection: &Connection,
        body: &str,
    ) -> TestResult<crate::model::ConcernId> {
        connection.execute(
            "INSERT INTO concerns(body, source_path) VALUES(?1, '/tmp/private-source.jsonl')",
            [body],
        )?;
        Ok(crate::model::ConcernId::from_storage(
            connection.last_insert_rowid(),
        )?)
    }

    fn insert_routing(
        connection: &Connection,
        concern: crate::model::ConcernId,
        suffix: &str,
        rationale: &str,
    ) -> TestResult {
        let job_id = insert_job(
            connection,
            "concern_routing",
            Some(concern.storage_id()),
            None,
            suffix,
            "fixture-base",
        )?;
        connection.execute(
            "INSERT INTO concern_routing_proposals(
                 concern_id, agent_job_id, action, rationale, proposal_digest,
                 producer_tool_call_id
             ) VALUES(?1, ?2, 'defer', ?3, ?4, ?5)",
            params![
                concern.storage_id(),
                job_id,
                rationale,
                format!("routing-digest-{suffix}"),
                format!("routing-call-{suffix}"),
            ],
        )?;
        Ok(())
    }

    fn insert_todo(connection: &Connection, title: &str) -> TestResult<TodoId> {
        connection.execute("INSERT INTO todos DEFAULT VALUES", [])?;
        let todo_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO concerns(body, source_path, status, resolved_at)
             VALUES('private attached concern', '/tmp/private-source.jsonl',
                    'attached', '2026-08-31T12:00:00Z')",
            [],
        )?;
        let concern_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO todo_direction_revisions(
                 todo_id, revision, title, body, source_concern_id, provenance_kind
             ) VALUES(?1, 1, ?2, 'private direction', ?3, 'legacy_v1')",
            params![todo_id, title, concern_id],
        )?;
        connection.execute(
            "INSERT INTO todo_concerns(todo_id, concern_id) VALUES(?1, ?2)",
            params![todo_id, concern_id],
        )?;
        Ok(TodoId::from_storage(todo_id)?)
    }

    fn insert_assessment(
        connection: &Connection,
        todo: TodoId,
        disposition: &str,
        suffix: &str,
    ) -> TestResult<SituationAssessmentId> {
        let snapshot = reconciliation_store::assessment_snapshot(connection, todo)?;
        let job_id = insert_job(
            connection,
            "situation_assessment",
            None,
            Some(todo.storage_id()),
            &format!("assessment-{suffix}"),
            &snapshot.base_digest,
        )?;
        connection.execute(
            "INSERT INTO todo_situation_assessments(
                 todo_id, agent_job_id, direction_revision_id, concern_set_digest,
                 notes_through_id, based_on_design_id, disposition, summary,
                 subject_label, observed_at, producer_tool_call_id
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,'private assessment summary',
                      'private subject', '2026-08-31T12:00:00Z', ?8)",
            params![
                todo.storage_id(),
                job_id,
                snapshot.direction.id,
                snapshot.concern_set_digest,
                snapshot.notes_through_id,
                snapshot.based_on_design_id.map(DesignId::storage_id),
                disposition,
                format!("assessment-call-{suffix}"),
            ],
        )?;
        Ok(SituationAssessmentId::from_storage(
            connection.last_insert_rowid(),
        )?)
    }

    fn insert_design(
        connection: &Connection,
        todo: TodoId,
        assessment: SituationAssessmentId,
        state: &str,
        suffix: &str,
    ) -> TestResult<DesignId> {
        let job_id = insert_job(
            connection,
            "design_reconciliation",
            None,
            Some(todo.storage_id()),
            &format!("design-{suffix}"),
            "fixture-base",
        )?;
        let revision: i64 = connection.query_row(
            "SELECT coalesce(max(revision), 0) + 1 FROM todo_designs WHERE todo_id = ?1",
            [todo.storage_id()],
            |row| row.get(0),
        )?;
        let canonical_digest = matches!(state, "ready" | "authorized" | "rejected")
            .then(|| format!("canonical-{suffix}"));
        let decision_source =
            matches!(state, "authorized" | "rejected").then_some("/tmp/private-decision.jsonl");
        let decision_reason = matches!(
            state,
            "rejected" | "discarded" | "abandoned" | "invalidated"
        )
        .then_some("private terminal reason");
        let decided_at = (!matches!(state, "open" | "ready")).then_some("2026-08-31T12:00:00Z");
        connection.execute(
            "INSERT INTO todo_designs(
                 todo_id, revision, assessment_id, agent_job_id, state, summary,
                 canonical_digest, decision_source_path, decision_reason, decided_at,
                 producer_tool_call_id
             ) VALUES(?1,?2,?3,?4,?5,'private design summary',?6,?7,?8,?9,?10)",
            params![
                todo.storage_id(),
                revision,
                assessment.storage_id(),
                job_id,
                state,
                canonical_digest,
                decision_source,
                decision_reason,
                decided_at,
                format!("design-call-{suffix}"),
            ],
        )?;
        Ok(DesignId::from_storage(connection.last_insert_rowid())?)
    }

    fn insert_legacy_design(connection: &Connection, todo: TodoId) -> TestResult<DesignId> {
        connection.execute(
            "INSERT INTO todo_designs(todo_id, revision, state, summary)
             VALUES(?1, 1, 'legacy_unreviewed', 'private legacy summary')",
            [todo.storage_id()],
        )?;
        Ok(DesignId::from_storage(connection.last_insert_rowid())?)
    }

    fn insert_assessment_return(
        connection: &Connection,
        todo: TodoId,
        assessment: SituationAssessmentId,
        suffix: &str,
    ) -> TestResult {
        let job_id = insert_job(
            connection,
            "design_reconciliation",
            None,
            Some(todo.storage_id()),
            &format!("return-{suffix}"),
            "fixture-base",
        )?;
        connection.execute(
            "INSERT INTO todo_design_assessment_returns(
                 agent_job_id, assessment_id, reason, producer_tool_call_id
             ) VALUES(?1,?2,'private return reason',?3)",
            params![
                job_id,
                assessment.storage_id(),
                format!("return-call-{suffix}")
            ],
        )?;
        Ok(())
    }

    fn insert_supersession(
        connection: &Connection,
        superseded: TodoId,
        survivor: TodoId,
    ) -> TestResult {
        let concern_id = connection.query_row(
            "SELECT concern_id FROM todo_concerns WHERE todo_id = ?1 LIMIT 1",
            [superseded.storage_id()],
            |row| row.get::<_, i64>(0),
        )?;
        let job_id = insert_job(
            connection,
            "concern_routing",
            Some(concern_id),
            None,
            "supersession",
            "fixture-base",
        )?;
        connection.execute(
            "INSERT INTO concern_routing_proposals(
                 concern_id, agent_job_id, action, proposed_title, proposed_direction,
                 rationale, proposal_digest, producer_tool_call_id, decision,
                 decision_source_path, decided_at
             ) VALUES(?1,?2,'unify','Canonical title','Canonical direction',
                      'private rationale','supersession-digest','supersession-call',
                      'authorized','/tmp/private-decision.jsonl','2026-08-31T12:00:00Z')",
            params![concern_id, job_id],
        )?;
        let routing_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO todo_supersessions(
                 superseded_todo_id, surviving_todo_id, authorized_routing_id
             ) VALUES(?1,?2,?3)",
            params![superseded.storage_id(), survivor.storage_id(), routing_id],
        )?;
        Ok(())
    }

    fn insert_job(
        connection: &Connection,
        stage: &str,
        concern_id: Option<i64>,
        todo_id: Option<i64>,
        suffix: &str,
        base_digest: &str,
    ) -> TestResult<i64> {
        connection.execute(
            "INSERT INTO todo_agent_jobs(
                 stage, concern_id, todo_id, base_digest, nucleus_requester_id,
                 nucleus_job_id, prompt_identity, toolset_identity
             ) VALUES(?1,?2,?3,?4,'fixture-requester',?5,
                      'fixture-prompt','fixture-toolset')",
            params![
                stage,
                concern_id,
                todo_id,
                base_digest,
                format!("nucleus-job-{suffix}")
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }
}
