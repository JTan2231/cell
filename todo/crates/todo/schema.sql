-- Todo schema version 2.
-- Domain state is relational; JSON is reserved for protocol boundaries.

CREATE TABLE todos (
    id            INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    status        TEXT NOT NULL DEFAULT 'open'
                      CHECK (status IN ('open', 'done')),
    created_at    TEXT NOT NULL DEFAULT (
                      strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  ),
    completed_at  TEXT,
    CHECK (
        (status = 'open' AND completed_at IS NULL)
        OR (status = 'done' AND completed_at IS NOT NULL)
    )
);

CREATE INDEX todos_status_created
    ON todos(status, created_at DESC, id DESC);

CREATE TRIGGER todos_identity_immutable
BEFORE UPDATE OF id, created_at ON todos BEGIN
    SELECT RAISE(ABORT, 'todo identity is immutable');
END;

CREATE TRIGGER todos_cannot_be_deleted
BEFORE DELETE ON todos BEGIN
    SELECT RAISE(ABORT, 'todos cannot be deleted');
END;

CREATE TABLE concerns (
    id                INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    body              TEXT NOT NULL CHECK (length(trim(body)) > 0),
    source_path       TEXT NOT NULL CHECK (length(source_path) > 0),
    source_thread_id  TEXT CHECK (
                          source_thread_id IS NULL
                          OR length(trim(source_thread_id)) > 0
                      ),
    source_turn_id    TEXT CHECK (
                          source_turn_id IS NULL
                          OR length(trim(source_turn_id)) > 0
                      ),
    source_item_id    TEXT CHECK (
                          source_item_id IS NULL
                          OR length(trim(source_item_id)) > 0
                      ),
    status            TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'attached', 'dismissed')),
    created_at        TEXT NOT NULL DEFAULT (
                          strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                      ),
    resolved_at       TEXT,
    CHECK (
        source_thread_id IS NOT NULL
        OR (source_turn_id IS NULL AND source_item_id IS NULL)
    ),
    CHECK (
        (status = 'pending' AND resolved_at IS NULL)
        OR (status IN ('attached', 'dismissed') AND resolved_at IS NOT NULL)
    )
);

CREATE INDEX concerns_status_created
    ON concerns(status, created_at DESC, id DESC);

CREATE TRIGGER concerns_identity_immutable
BEFORE UPDATE OF id, body, source_path, source_thread_id, source_turn_id,
                 source_item_id, created_at ON concerns BEGIN
    SELECT RAISE(ABORT, 'concern provenance is immutable');
END;

CREATE TRIGGER concerns_terminal_once
BEFORE UPDATE OF status, resolved_at ON concerns
WHEN NOT (
    OLD.status = 'pending'
    AND NEW.status IN ('attached', 'dismissed')
    AND NEW.resolved_at IS NOT NULL
) BEGIN
    SELECT RAISE(ABORT, 'invalid concern transition');
END;

CREATE TRIGGER concerns_cannot_be_deleted
BEFORE DELETE ON concerns BEGIN
    SELECT RAISE(ABORT, 'concerns cannot be deleted');
END;

CREATE TABLE todo_agent_jobs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    stage              TEXT NOT NULL CHECK (
                           stage IN (
                               'concern_routing',
                               'situation_assessment',
                               'design_reconciliation'
                           )
                       ),
    concern_id         INTEGER REFERENCES concerns(id) ON DELETE RESTRICT,
    todo_id            INTEGER REFERENCES todos(id) ON DELETE RESTRICT,
    base_digest        TEXT NOT NULL CHECK (length(trim(base_digest)) > 0),
    nucleus_requester_id TEXT NOT NULL
                           CHECK (length(trim(nucleus_requester_id)) > 0),
    nucleus_job_id     TEXT NOT NULL UNIQUE
                           CHECK (length(trim(nucleus_job_id)) > 0),
    prompt_identity    TEXT NOT NULL CHECK (length(trim(prompt_identity)) > 0),
    toolset_identity   TEXT NOT NULL CHECK (length(trim(toolset_identity)) > 0),
    created_at         TEXT NOT NULL DEFAULT (
                           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                       ),
    CHECK (
        (stage = 'concern_routing'
         AND concern_id IS NOT NULL AND todo_id IS NULL)
        OR
        (stage IN ('situation_assessment', 'design_reconciliation')
         AND concern_id IS NULL AND todo_id IS NOT NULL)
    )
);

CREATE TRIGGER todo_agent_jobs_immutable_update
BEFORE UPDATE ON todo_agent_jobs BEGIN
    SELECT RAISE(ABORT, 'agent job correlation is immutable');
END;

CREATE TRIGGER todo_agent_jobs_immutable_delete
BEFORE DELETE ON todo_agent_jobs BEGIN
    SELECT RAISE(ABORT, 'agent job correlation is immutable');
END;

CREATE TABLE concern_routing_proposals (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    concern_id            INTEGER NOT NULL
                              REFERENCES concerns(id) ON DELETE RESTRICT,
    agent_job_id          INTEGER NOT NULL UNIQUE
                              REFERENCES todo_agent_jobs(id) ON DELETE RESTRICT,
    action                TEXT NOT NULL CHECK (
                              action IN (
                                  'attach', 'create', 'revise',
                                  'unify', 'dismiss', 'defer'
                              )
                          ),
    proposed_title        TEXT,
    proposed_direction    TEXT,
    rationale             TEXT NOT NULL CHECK (length(trim(rationale)) > 0),
    proposal_digest       TEXT NOT NULL UNIQUE
                              CHECK (length(trim(proposal_digest)) > 0),
    producer_tool_call_id TEXT NOT NULL
                              CHECK (length(trim(producer_tool_call_id)) > 0),
    decision              TEXT NOT NULL DEFAULT 'pending'
                              CHECK (
                                  decision IN (
                                      'pending', 'authorized',
                                      'rejected', 'invalidated'
                                  )
                              ),
    decision_source_path  TEXT,
    decision_thread_id    TEXT,
    decision_turn_id      TEXT,
    decision_reason       TEXT,
    decided_at            TEXT,
    created_at            TEXT NOT NULL DEFAULT (
                              strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                          ),
    UNIQUE (agent_job_id, producer_tool_call_id),
    CHECK (
        (action IN ('create', 'revise', 'unify')
         AND proposed_title IS NOT NULL
         AND length(trim(proposed_title)) > 0
         AND instr(proposed_title, char(10)) = 0
         AND instr(proposed_title, char(13)) = 0
         AND proposed_direction IS NOT NULL
         AND length(trim(proposed_direction)) > 0)
        OR
        (action IN ('attach', 'dismiss', 'defer')
         AND proposed_title IS NULL AND proposed_direction IS NULL)
    ),
    CHECK (
        (decision = 'pending'
         AND decision_source_path IS NULL
         AND decision_thread_id IS NULL
         AND decision_turn_id IS NULL
         AND decision_reason IS NULL
         AND decided_at IS NULL)
        OR
        (decision = 'authorized'
         AND decision_source_path IS NOT NULL
         AND length(decision_source_path) > 0
         AND decision_reason IS NULL
         AND decided_at IS NOT NULL)
        OR
        (decision = 'rejected'
         AND decision_source_path IS NOT NULL
         AND length(decision_source_path) > 0
         AND decision_reason IS NOT NULL
         AND length(trim(decision_reason)) > 0
         AND decided_at IS NOT NULL)
        OR
        (decision = 'invalidated'
         AND decision_source_path IS NULL
         AND decision_thread_id IS NULL
         AND decision_turn_id IS NULL
         AND decision_reason IS NOT NULL
         AND length(trim(decision_reason)) > 0
         AND decided_at IS NOT NULL)
    )
);

CREATE INDEX concern_routing_by_concern
    ON concern_routing_proposals(concern_id, created_at DESC, id DESC);

CREATE UNIQUE INDEX concern_one_authorized_resolution
    ON concern_routing_proposals(concern_id)
    WHERE decision = 'authorized' AND action <> 'defer';

CREATE TRIGGER concern_routing_content_immutable
BEFORE UPDATE OF id, concern_id, agent_job_id, action, proposed_title,
                 proposed_direction, rationale, proposal_digest,
                 producer_tool_call_id, created_at
ON concern_routing_proposals BEGIN
    SELECT RAISE(ABORT, 'routing proposal content is immutable');
END;

CREATE TRIGGER concern_routing_decision_once
BEFORE UPDATE OF decision, decision_source_path, decision_thread_id,
                 decision_turn_id, decision_reason, decided_at
ON concern_routing_proposals
WHEN OLD.decision <> 'pending' OR NEW.decision = 'pending' BEGIN
    SELECT RAISE(ABORT, 'routing decisions are one-way');
END;

CREATE TRIGGER concern_routing_cannot_be_deleted
BEFORE DELETE ON concern_routing_proposals BEGIN
    SELECT RAISE(ABORT, 'routing proposals cannot be deleted');
END;

CREATE TABLE concern_routing_targets (
    routing_id           INTEGER NOT NULL
                             REFERENCES concern_routing_proposals(id)
                             ON DELETE RESTRICT,
    ordinal              INTEGER NOT NULL CHECK (ordinal IN (0, 1)),
    todo_id              INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
    direction_revision   INTEGER NOT NULL CHECK (direction_revision > 0),
    PRIMARY KEY (routing_id, ordinal),
    UNIQUE (routing_id, todo_id),
    FOREIGN KEY (todo_id, direction_revision)
        REFERENCES todo_direction_revisions(todo_id, revision)
        ON DELETE RESTRICT
) WITHOUT ROWID;

CREATE TABLE concern_routing_unifications (
    routing_id         INTEGER PRIMARY KEY
                           REFERENCES concern_routing_proposals(id)
                           ON DELETE RESTRICT,
    survivor_todo_id   INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT
);

CREATE TABLE concern_routing_boundaries (
    id             INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    routing_id     INTEGER NOT NULL
                       REFERENCES concern_routing_proposals(id) ON DELETE RESTRICT,
    local_ref      TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    kind           TEXT NOT NULL CHECK (
                       kind IN (
                           'required', 'forbidden', 'authority',
                           'non_goal', 'unresolved'
                       )
                   ),
    statement      TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    attribution    TEXT NOT NULL CHECK (
                       attribution IN (
                           'explicit_user', 'governing_instruction',
                           'accepted_inference'
                       )
                   ),
    UNIQUE (routing_id, local_ref)
);

CREATE TABLE concern_routing_boundary_sources (
    boundary_id  INTEGER NOT NULL
                     REFERENCES concern_routing_boundaries(id) ON DELETE RESTRICT,
    ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
    source_ref   TEXT NOT NULL CHECK (length(trim(source_ref)) > 0),
    PRIMARY KEY (boundary_id, ordinal),
    UNIQUE (boundary_id, source_ref)
) WITHOUT ROWID;

CREATE TABLE concern_routing_evidence (
    routing_id    INTEGER NOT NULL
                      REFERENCES concern_routing_proposals(id) ON DELETE RESTRICT,
    ordinal       INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_ref  TEXT NOT NULL CHECK (length(trim(evidence_ref)) > 0),
    PRIMARY KEY (routing_id, ordinal),
    UNIQUE (routing_id, evidence_ref)
) WITHOUT ROWID;

CREATE TABLE concern_routing_limitations (
    routing_id  INTEGER NOT NULL
                    REFERENCES concern_routing_proposals(id) ON DELETE RESTRICT,
    ordinal     INTEGER NOT NULL CHECK (ordinal >= 0),
    limitation  TEXT NOT NULL CHECK (length(trim(limitation)) > 0),
    PRIMARY KEY (routing_id, ordinal)
) WITHOUT ROWID;

CREATE TRIGGER concern_routing_targets_immutable_update
BEFORE UPDATE ON concern_routing_targets BEGIN
    SELECT RAISE(ABORT, 'routing targets are immutable');
END;
CREATE TRIGGER concern_routing_targets_immutable_delete
BEFORE DELETE ON concern_routing_targets BEGIN
    SELECT RAISE(ABORT, 'routing targets are immutable');
END;
CREATE TRIGGER concern_routing_unifications_immutable_update
BEFORE UPDATE ON concern_routing_unifications BEGIN
    SELECT RAISE(ABORT, 'routing unification is immutable');
END;
CREATE TRIGGER concern_routing_unifications_immutable_delete
BEFORE DELETE ON concern_routing_unifications BEGIN
    SELECT RAISE(ABORT, 'routing unification is immutable');
END;
CREATE TRIGGER concern_routing_boundaries_immutable_update
BEFORE UPDATE ON concern_routing_boundaries BEGIN
    SELECT RAISE(ABORT, 'routing boundaries are immutable');
END;
CREATE TRIGGER concern_routing_boundaries_immutable_delete
BEFORE DELETE ON concern_routing_boundaries BEGIN
    SELECT RAISE(ABORT, 'routing boundaries are immutable');
END;
CREATE TRIGGER concern_routing_boundary_sources_immutable_update
BEFORE UPDATE ON concern_routing_boundary_sources BEGIN
    SELECT RAISE(ABORT, 'routing boundary sources are immutable');
END;
CREATE TRIGGER concern_routing_boundary_sources_immutable_delete
BEFORE DELETE ON concern_routing_boundary_sources BEGIN
    SELECT RAISE(ABORT, 'routing boundary sources are immutable');
END;
CREATE TRIGGER concern_routing_evidence_immutable_update
BEFORE UPDATE ON concern_routing_evidence BEGIN
    SELECT RAISE(ABORT, 'routing evidence is immutable');
END;
CREATE TRIGGER concern_routing_evidence_immutable_delete
BEFORE DELETE ON concern_routing_evidence BEGIN
    SELECT RAISE(ABORT, 'routing evidence is immutable');
END;
CREATE TRIGGER concern_routing_limitations_immutable_update
BEFORE UPDATE ON concern_routing_limitations BEGIN
    SELECT RAISE(ABORT, 'routing limitations are immutable');
END;
CREATE TRIGGER concern_routing_limitations_immutable_delete
BEFORE DELETE ON concern_routing_limitations BEGIN
    SELECT RAISE(ABORT, 'routing limitations are immutable');
END;

CREATE TABLE todo_direction_revisions (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    todo_id            INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
    revision           INTEGER NOT NULL CHECK (revision > 0),
    title              TEXT NOT NULL CHECK (
                           length(trim(title)) > 0
                           AND instr(title, char(10)) = 0
                           AND instr(title, char(13)) = 0
                       ),
    body               TEXT NOT NULL CHECK (length(trim(body)) > 0),
    source_concern_id  INTEGER REFERENCES concerns(id) ON DELETE RESTRICT,
    source_routing_id  INTEGER UNIQUE
                           REFERENCES concern_routing_proposals(id)
                           ON DELETE RESTRICT,
    provenance_kind    TEXT NOT NULL CHECK (
                           provenance_kind IN ('explicit', 'legacy_v1')
                       ),
    created_at         TEXT NOT NULL DEFAULT (
                           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                       ),
    UNIQUE (todo_id, revision),
    CHECK (
        (provenance_kind = 'legacy_v1'
         AND source_concern_id IS NOT NULL AND source_routing_id IS NULL)
        OR
        (provenance_kind = 'explicit'
         AND source_concern_id IS NOT NULL AND source_routing_id IS NOT NULL)
    )
);

CREATE INDEX todo_direction_latest
    ON todo_direction_revisions(todo_id, revision DESC);

CREATE TABLE todo_direction_boundaries (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    direction_revision_id  INTEGER NOT NULL
                               REFERENCES todo_direction_revisions(id)
                               ON DELETE RESTRICT,
    local_ref              TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    kind                   TEXT NOT NULL CHECK (
                               kind IN (
                                   'required', 'forbidden', 'authority',
                                   'non_goal', 'unresolved'
                               )
                           ),
    statement              TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    attribution            TEXT NOT NULL CHECK (
                               attribution IN (
                                   'explicit_user', 'governing_instruction',
                                   'accepted_inference', 'legacy_unknown'
                               )
                           ),
    UNIQUE (direction_revision_id, local_ref)
);

CREATE TABLE todo_direction_boundary_sources (
    boundary_id  INTEGER NOT NULL
                     REFERENCES todo_direction_boundaries(id) ON DELETE RESTRICT,
    ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
    source_ref   TEXT NOT NULL CHECK (length(trim(source_ref)) > 0),
    PRIMARY KEY (boundary_id, ordinal),
    UNIQUE (boundary_id, source_ref)
) WITHOUT ROWID;

CREATE TRIGGER todo_direction_revisions_immutable_update
BEFORE UPDATE ON todo_direction_revisions BEGIN
    SELECT RAISE(ABORT, 'direction revisions are immutable');
END;
CREATE TRIGGER todo_direction_revisions_immutable_delete
BEFORE DELETE ON todo_direction_revisions BEGIN
    SELECT RAISE(ABORT, 'direction revisions are immutable');
END;
CREATE TRIGGER todo_direction_boundaries_immutable_update
BEFORE UPDATE ON todo_direction_boundaries BEGIN
    SELECT RAISE(ABORT, 'direction boundaries are immutable');
END;
CREATE TRIGGER todo_direction_boundaries_immutable_delete
BEFORE DELETE ON todo_direction_boundaries BEGIN
    SELECT RAISE(ABORT, 'direction boundaries are immutable');
END;
CREATE TRIGGER todo_direction_boundary_sources_immutable_update
BEFORE UPDATE ON todo_direction_boundary_sources BEGIN
    SELECT RAISE(ABORT, 'direction boundary sources are immutable');
END;
CREATE TRIGGER todo_direction_boundary_sources_immutable_delete
BEFORE DELETE ON todo_direction_boundary_sources BEGIN
    SELECT RAISE(ABORT, 'direction boundary sources are immutable');
END;

CREATE TABLE todo_concerns (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    todo_id                INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
    concern_id             INTEGER NOT NULL UNIQUE
                               REFERENCES concerns(id) ON DELETE RESTRICT,
    authorized_routing_id  INTEGER UNIQUE
                               REFERENCES concern_routing_proposals(id)
                               ON DELETE RESTRICT,
    attached_at            TEXT NOT NULL DEFAULT (
                               strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                           ),
    UNIQUE (todo_id, concern_id)
);

CREATE INDEX todo_concerns_by_todo
    ON todo_concerns(todo_id, id);

CREATE TRIGGER todo_concerns_immutable_update
BEFORE UPDATE ON todo_concerns BEGIN
    SELECT RAISE(ABORT, 'concern attachments are immutable');
END;
CREATE TRIGGER todo_concerns_immutable_delete
BEFORE DELETE ON todo_concerns BEGIN
    SELECT RAISE(ABORT, 'concern attachments are immutable');
END;

CREATE TABLE todo_supersessions (
    superseded_todo_id    INTEGER PRIMARY KEY
                              REFERENCES todos(id) ON DELETE RESTRICT,
    surviving_todo_id     INTEGER NOT NULL
                              REFERENCES todos(id) ON DELETE RESTRICT,
    authorized_routing_id INTEGER NOT NULL UNIQUE
                              REFERENCES concern_routing_proposals(id)
                              ON DELETE RESTRICT,
    created_at            TEXT NOT NULL DEFAULT (
                              strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                          ),
    CHECK (superseded_todo_id <> surviving_todo_id)
);

CREATE INDEX todo_supersessions_survivor
    ON todo_supersessions(surviving_todo_id, superseded_todo_id);

CREATE TRIGGER todo_supersessions_immutable_update
BEFORE UPDATE ON todo_supersessions BEGIN
    SELECT RAISE(ABORT, 'todo supersessions are immutable');
END;
CREATE TRIGGER todo_supersessions_immutable_delete
BEFORE DELETE ON todo_supersessions BEGIN
    SELECT RAISE(ABORT, 'todo supersessions are immutable');
END;

CREATE TABLE todo_notes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    todo_id     INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
    text        TEXT NOT NULL CHECK (length(trim(text)) > 0),
    created_at  TEXT NOT NULL DEFAULT (
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                )
);

CREATE INDEX todo_notes_parent_order
    ON todo_notes(todo_id, created_at, id);

CREATE TRIGGER todo_notes_immutable_update
BEFORE UPDATE ON todo_notes BEGIN
    SELECT RAISE(ABORT, 'working notes are append-only');
END;
CREATE TRIGGER todo_notes_immutable_delete
BEFORE DELETE ON todo_notes BEGIN
    SELECT RAISE(ABORT, 'working notes are append-only');
END;

CREATE TABLE todo_situation_assessments (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    todo_id                INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
    agent_job_id           INTEGER NOT NULL UNIQUE
                               REFERENCES todo_agent_jobs(id) ON DELETE RESTRICT,
    direction_revision_id  INTEGER NOT NULL
                               REFERENCES todo_direction_revisions(id)
                               ON DELETE RESTRICT,
    concern_set_digest     TEXT NOT NULL CHECK (length(trim(concern_set_digest)) > 0),
    notes_through_id       INTEGER REFERENCES todo_notes(id) ON DELETE RESTRICT,
    based_on_design_id     INTEGER,
    disposition            TEXT NOT NULL CHECK (
                               disposition IN (
                                   'ready', 'needs_user_choice', 'inconclusive'
                               )
                           ),
    summary                TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    subject_label          TEXT NOT NULL CHECK (length(trim(subject_label)) > 0),
    observed_at            TEXT NOT NULL,
    producer_tool_call_id  TEXT NOT NULL
                               CHECK (length(trim(producer_tool_call_id)) > 0),
    created_at             TEXT NOT NULL DEFAULT (
                               strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                           ),
    UNIQUE (agent_job_id, producer_tool_call_id),
    FOREIGN KEY (based_on_design_id)
        REFERENCES todo_designs(id) ON DELETE RESTRICT
);

CREATE INDEX todo_situation_latest
    ON todo_situation_assessments(todo_id, id DESC);

CREATE TABLE todo_assessment_identity_refs (
    assessment_id  INTEGER NOT NULL
                       REFERENCES todo_situation_assessments(id)
                       ON DELETE RESTRICT,
    ordinal        INTEGER NOT NULL CHECK (ordinal >= 0),
    identity_ref   TEXT NOT NULL CHECK (length(trim(identity_ref)) > 0),
    PRIMARY KEY (assessment_id, ordinal),
    UNIQUE (assessment_id, identity_ref)
) WITHOUT ROWID;

CREATE TABLE todo_assessment_bases (
    id             INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    assessment_id  INTEGER NOT NULL
                       REFERENCES todo_situation_assessments(id)
                       ON DELETE RESTRICT,
    source_ref     TEXT NOT NULL CHECK (length(trim(source_ref)) > 0),
    kind           TEXT NOT NULL CHECK (
                       kind IN (
                           'git', 'document', 'database', 'installed_version',
                           'todo', 'annals', 'external'
                       )
                   ),
    locator        TEXT NOT NULL CHECK (length(trim(locator)) > 0),
    revision       TEXT NOT NULL CHECK (length(trim(revision)) > 0),
    observed_at    TEXT NOT NULL,
    UNIQUE (assessment_id, source_ref),
    UNIQUE (assessment_id, kind, locator)
);

CREATE TABLE todo_assessment_findings (
    id             INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    assessment_id  INTEGER NOT NULL
                       REFERENCES todo_situation_assessments(id)
                       ON DELETE RESTRICT,
    local_ref      TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    kind           TEXT NOT NULL CHECK (
                       kind IN ('current_state', 'constraint', 'dependency', 'gap')
                   ),
    claim          TEXT NOT NULL CHECK (length(trim(claim)) > 0),
    UNIQUE (assessment_id, local_ref)
);

CREATE TABLE todo_assessment_finding_evidence (
    finding_id    INTEGER NOT NULL
                      REFERENCES todo_assessment_findings(id) ON DELETE RESTRICT,
    ordinal       INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_ref  TEXT NOT NULL CHECK (length(trim(evidence_ref)) > 0),
    PRIMARY KEY (finding_id, ordinal),
    UNIQUE (finding_id, evidence_ref)
) WITHOUT ROWID;

CREATE TABLE todo_assessment_jurisdictions (
    id                INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    assessment_id     INTEGER NOT NULL
                          REFERENCES todo_situation_assessments(id)
                          ON DELETE RESTRICT,
    jurisdiction_key  TEXT NOT NULL CHECK (length(trim(jurisdiction_key)) > 0),
    concern            TEXT NOT NULL CHECK (length(trim(concern)) > 0),
    UNIQUE (assessment_id, jurisdiction_key)
);

CREATE TABLE todo_assessment_jurisdiction_assignments (
    jurisdiction_id  INTEGER NOT NULL
                         REFERENCES todo_assessment_jurisdictions(id)
                         ON DELETE RESTRICT,
    party            TEXT NOT NULL CHECK (length(trim(party)) > 0),
    role             TEXT NOT NULL CHECK (role IN ('owner', 'participant', 'consumer')),
    responsibility   TEXT NOT NULL CHECK (length(trim(responsibility)) > 0),
    PRIMARY KEY (jurisdiction_id, party, role)
) WITHOUT ROWID;

CREATE UNIQUE INDEX assessment_one_owner_per_jurisdiction
    ON todo_assessment_jurisdiction_assignments(jurisdiction_id)
    WHERE role = 'owner';

CREATE TABLE todo_assessment_jurisdiction_evidence (
    jurisdiction_id  INTEGER NOT NULL
                         REFERENCES todo_assessment_jurisdictions(id)
                         ON DELETE RESTRICT,
    ordinal          INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_ref     TEXT NOT NULL CHECK (length(trim(evidence_ref)) > 0),
    PRIMARY KEY (jurisdiction_id, ordinal),
    UNIQUE (jurisdiction_id, evidence_ref)
) WITHOUT ROWID;

CREATE TABLE todo_assessment_direction_mappings (
    id             INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    assessment_id  INTEGER NOT NULL
                       REFERENCES todo_situation_assessments(id)
                       ON DELETE RESTRICT,
    boundary_id    INTEGER NOT NULL
                       REFERENCES todo_direction_boundaries(id)
                       ON DELETE RESTRICT,
    disposition    TEXT NOT NULL CHECK (
                       disposition IN (
                           'satisfied', 'unsatisfied',
                           'constrains_design', 'unknown'
                       )
                   ),
    explanation    TEXT NOT NULL CHECK (length(trim(explanation)) > 0),
    UNIQUE (assessment_id, boundary_id)
);

CREATE TABLE todo_assessment_mapping_findings (
    mapping_id  INTEGER NOT NULL
                    REFERENCES todo_assessment_direction_mappings(id)
                    ON DELETE RESTRICT,
    finding_id  INTEGER NOT NULL
                    REFERENCES todo_assessment_findings(id) ON DELETE RESTRICT,
    PRIMARY KEY (mapping_id, finding_id)
) WITHOUT ROWID;

CREATE TABLE todo_assessment_unresolved (
    id             INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    assessment_id  INTEGER NOT NULL
                       REFERENCES todo_situation_assessments(id)
                       ON DELETE RESTRICT,
    local_ref      TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    kind           TEXT NOT NULL CHECK (
                       kind IN (
                           'user_choice', 'evidence_gap',
                           'jurisdiction_conflict'
                       )
                   ),
    description    TEXT NOT NULL CHECK (length(trim(description)) > 0),
    materiality    TEXT NOT NULL CHECK (length(trim(materiality)) > 0),
    UNIQUE (assessment_id, local_ref)
);

CREATE TABLE todo_assessment_unresolved_evidence (
    unresolved_id  INTEGER NOT NULL
                       REFERENCES todo_assessment_unresolved(id) ON DELETE RESTRICT,
    ordinal        INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_ref   TEXT NOT NULL CHECK (length(trim(evidence_ref)) > 0),
    PRIMARY KEY (unresolved_id, ordinal),
    UNIQUE (unresolved_id, evidence_ref)
) WITHOUT ROWID;

CREATE TRIGGER todo_situation_assessments_immutable_update
BEFORE UPDATE ON todo_situation_assessments BEGIN
    SELECT RAISE(ABORT, 'situation assessments are immutable');
END;
CREATE TRIGGER todo_situation_assessments_immutable_delete
BEFORE DELETE ON todo_situation_assessments BEGIN
    SELECT RAISE(ABORT, 'situation assessments are immutable');
END;
CREATE TRIGGER todo_assessment_identity_refs_immutable_update
BEFORE UPDATE ON todo_assessment_identity_refs BEGIN
    SELECT RAISE(ABORT, 'assessment identity references are immutable');
END;
CREATE TRIGGER todo_assessment_identity_refs_immutable_delete
BEFORE DELETE ON todo_assessment_identity_refs BEGIN
    SELECT RAISE(ABORT, 'assessment identity references are immutable');
END;
CREATE TRIGGER todo_assessment_bases_immutable_update
BEFORE UPDATE ON todo_assessment_bases BEGIN
    SELECT RAISE(ABORT, 'assessment bases are immutable');
END;
CREATE TRIGGER todo_assessment_bases_immutable_delete
BEFORE DELETE ON todo_assessment_bases BEGIN
    SELECT RAISE(ABORT, 'assessment bases are immutable');
END;
CREATE TRIGGER todo_assessment_findings_immutable_update
BEFORE UPDATE ON todo_assessment_findings BEGIN
    SELECT RAISE(ABORT, 'assessment findings are immutable');
END;
CREATE TRIGGER todo_assessment_findings_immutable_delete
BEFORE DELETE ON todo_assessment_findings BEGIN
    SELECT RAISE(ABORT, 'assessment findings are immutable');
END;
CREATE TRIGGER todo_assessment_finding_evidence_immutable_update
BEFORE UPDATE ON todo_assessment_finding_evidence BEGIN
    SELECT RAISE(ABORT, 'assessment evidence is immutable');
END;
CREATE TRIGGER todo_assessment_finding_evidence_immutable_delete
BEFORE DELETE ON todo_assessment_finding_evidence BEGIN
    SELECT RAISE(ABORT, 'assessment evidence is immutable');
END;
CREATE TRIGGER todo_assessment_jurisdictions_immutable_update
BEFORE UPDATE ON todo_assessment_jurisdictions BEGIN
    SELECT RAISE(ABORT, 'assessment jurisdictions are immutable');
END;
CREATE TRIGGER todo_assessment_jurisdictions_immutable_delete
BEFORE DELETE ON todo_assessment_jurisdictions BEGIN
    SELECT RAISE(ABORT, 'assessment jurisdictions are immutable');
END;
CREATE TRIGGER todo_assessment_jurisdiction_assignments_immutable_update
BEFORE UPDATE ON todo_assessment_jurisdiction_assignments BEGIN
    SELECT RAISE(ABORT, 'assessment jurisdiction assignments are immutable');
END;
CREATE TRIGGER todo_assessment_jurisdiction_assignments_immutable_delete
BEFORE DELETE ON todo_assessment_jurisdiction_assignments BEGIN
    SELECT RAISE(ABORT, 'assessment jurisdiction assignments are immutable');
END;
CREATE TRIGGER todo_assessment_jurisdiction_evidence_immutable_update
BEFORE UPDATE ON todo_assessment_jurisdiction_evidence BEGIN
    SELECT RAISE(ABORT, 'assessment jurisdiction evidence is immutable');
END;
CREATE TRIGGER todo_assessment_jurisdiction_evidence_immutable_delete
BEFORE DELETE ON todo_assessment_jurisdiction_evidence BEGIN
    SELECT RAISE(ABORT, 'assessment jurisdiction evidence is immutable');
END;
CREATE TRIGGER todo_assessment_direction_mappings_immutable_update
BEFORE UPDATE ON todo_assessment_direction_mappings BEGIN
    SELECT RAISE(ABORT, 'assessment direction mappings are immutable');
END;
CREATE TRIGGER todo_assessment_direction_mappings_immutable_delete
BEFORE DELETE ON todo_assessment_direction_mappings BEGIN
    SELECT RAISE(ABORT, 'assessment direction mappings are immutable');
END;
CREATE TRIGGER todo_assessment_mapping_findings_immutable_update
BEFORE UPDATE ON todo_assessment_mapping_findings BEGIN
    SELECT RAISE(ABORT, 'assessment mapping findings are immutable');
END;
CREATE TRIGGER todo_assessment_mapping_findings_immutable_delete
BEFORE DELETE ON todo_assessment_mapping_findings BEGIN
    SELECT RAISE(ABORT, 'assessment mapping findings are immutable');
END;
CREATE TRIGGER todo_assessment_unresolved_immutable_update
BEFORE UPDATE ON todo_assessment_unresolved BEGIN
    SELECT RAISE(ABORT, 'assessment unresolved items are immutable');
END;
CREATE TRIGGER todo_assessment_unresolved_immutable_delete
BEFORE DELETE ON todo_assessment_unresolved BEGIN
    SELECT RAISE(ABORT, 'assessment unresolved items are immutable');
END;
CREATE TRIGGER todo_assessment_unresolved_evidence_immutable_update
BEFORE UPDATE ON todo_assessment_unresolved_evidence BEGIN
    SELECT RAISE(ABORT, 'assessment unresolved evidence is immutable');
END;
CREATE TRIGGER todo_assessment_unresolved_evidence_immutable_delete
BEFORE DELETE ON todo_assessment_unresolved_evidence BEGIN
    SELECT RAISE(ABORT, 'assessment unresolved evidence is immutable');
END;

CREATE TABLE todo_designs (
    id                         INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    todo_id                    INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
    revision                   INTEGER NOT NULL CHECK (revision > 0),
    assessment_id              INTEGER
                                   REFERENCES todo_situation_assessments(id)
                                   ON DELETE RESTRICT,
    based_on_design_id         INTEGER REFERENCES todo_designs(id) ON DELETE RESTRICT,
    agent_job_id               INTEGER UNIQUE
                                   REFERENCES todo_agent_jobs(id) ON DELETE RESTRICT,
    draft_version              INTEGER NOT NULL DEFAULT 1 CHECK (draft_version > 0),
    state                      TEXT NOT NULL CHECK (
                                   state IN (
                                       'open', 'ready', 'authorized', 'rejected',
                                       'discarded', 'abandoned', 'invalidated',
                                       'legacy_unreviewed'
                                   )
                               ),
    summary                    TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    canonical_digest           TEXT,
    decision_source_path       TEXT,
    decision_thread_id         TEXT,
    decision_turn_id           TEXT,
    decision_reason            TEXT,
    decided_at                 TEXT,
    producer_tool_call_id      TEXT,
    created_at                 TEXT NOT NULL DEFAULT (
                                   strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                               ),
    UNIQUE (todo_id, revision),
    UNIQUE (agent_job_id, producer_tool_call_id),
    CHECK (
        (state = 'legacy_unreviewed'
         AND assessment_id IS NULL AND agent_job_id IS NULL
         AND producer_tool_call_id IS NULL
         AND canonical_digest IS NULL
         AND decision_source_path IS NULL AND decision_reason IS NULL
         AND decided_at IS NULL)
        OR
        (state = 'open'
         AND assessment_id IS NOT NULL AND agent_job_id IS NOT NULL
         AND producer_tool_call_id IS NOT NULL
         AND length(trim(producer_tool_call_id)) > 0
         AND canonical_digest IS NULL
         AND decision_source_path IS NULL AND decision_reason IS NULL
         AND decided_at IS NULL)
        OR
        (state = 'ready'
         AND assessment_id IS NOT NULL AND agent_job_id IS NOT NULL
         AND producer_tool_call_id IS NOT NULL
         AND canonical_digest IS NOT NULL
         AND length(trim(canonical_digest)) > 0
         AND decision_source_path IS NULL AND decision_reason IS NULL
         AND decided_at IS NULL)
        OR
        (state = 'authorized'
         AND assessment_id IS NOT NULL AND canonical_digest IS NOT NULL
         AND decision_source_path IS NOT NULL
         AND length(decision_source_path) > 0
         AND decision_reason IS NULL AND decided_at IS NOT NULL)
        OR
        (state = 'rejected'
         AND assessment_id IS NOT NULL AND canonical_digest IS NOT NULL
         AND decision_source_path IS NOT NULL
         AND length(decision_source_path) > 0
         AND decision_reason IS NOT NULL
         AND length(trim(decision_reason)) > 0
         AND decided_at IS NOT NULL)
        OR
        (state IN ('discarded', 'abandoned', 'invalidated')
         AND assessment_id IS NOT NULL
         AND decision_reason IS NOT NULL
         AND length(trim(decision_reason)) > 0
         AND decided_at IS NOT NULL)
    )
);

CREATE INDEX todo_design_latest
    ON todo_designs(todo_id, revision DESC);

CREATE UNIQUE INDEX todo_one_authorized_design_per_assessment
    ON todo_designs(todo_id, assessment_id)
    WHERE state = 'authorized';

CREATE TABLE todo_design_corrections (
    agent_job_id        INTEGER PRIMARY KEY
                            REFERENCES todo_agent_jobs(id) ON DELETE RESTRICT,
    based_on_design_id  INTEGER NOT NULL
                            REFERENCES todo_designs(id) ON DELETE RESTRICT,
    feedback            TEXT NOT NULL CHECK (length(trim(feedback)) > 0),
    basis_ref           TEXT NOT NULL UNIQUE
                            CHECK (
                                length(trim(basis_ref)) > 0
                                AND basis_ref = 'correction:' || agent_job_id
                            ),
    created_at          TEXT NOT NULL DEFAULT (
                            strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                        )
);

CREATE TRIGGER todo_design_corrections_valid_insert
BEFORE INSERT ON todo_design_corrections
WHEN NOT EXISTS (
    SELECT 1
    FROM todo_agent_jobs AS job
    JOIN todo_designs AS predecessor ON predecessor.id = NEW.based_on_design_id
    WHERE job.id = NEW.agent_job_id
      AND job.stage = 'design_reconciliation'
      AND job.todo_id = predecessor.todo_id
      AND predecessor.state IN ('ready', 'rejected', 'abandoned')
) BEGIN
    SELECT RAISE(ABORT, 'invalid design correction provenance');
END;

CREATE TRIGGER todo_design_corrections_immutable_update
BEFORE UPDATE ON todo_design_corrections BEGIN
    SELECT RAISE(ABORT, 'design corrections are immutable');
END;

CREATE TRIGGER todo_design_corrections_immutable_delete
BEFORE DELETE ON todo_design_corrections BEGIN
    SELECT RAISE(ABORT, 'design corrections are immutable');
END;

CREATE TABLE todo_design_jurisdiction_changes (
    id                INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    design_id         INTEGER NOT NULL REFERENCES todo_designs(id) ON DELETE RESTRICT,
    slot              TEXT NOT NULL CHECK (length(trim(slot)) > 0),
    local_ref         TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    jurisdiction_key  TEXT NOT NULL CHECK (length(trim(jurisdiction_key)) > 0),
    action            TEXT NOT NULL CHECK (action IN ('keep', 'move', 'add', 'retire')),
    rationale         TEXT NOT NULL CHECK (length(trim(rationale)) > 0),
    status            TEXT NOT NULL DEFAULT 'active'
                          CHECK (status IN ('active', 'dropped')),
    UNIQUE (design_id, slot)
);

CREATE TABLE todo_design_responsibilities (
    jurisdiction_change_id  INTEGER NOT NULL
                                REFERENCES todo_design_jurisdiction_changes(id)
                                ON DELETE RESTRICT,
    side                    TEXT NOT NULL CHECK (side IN ('expected', 'proposed')),
    party                   TEXT NOT NULL CHECK (length(trim(party)) > 0),
    role                    TEXT NOT NULL CHECK (
                                role IN ('owner', 'participant', 'consumer')
                            ),
    responsibility          TEXT NOT NULL CHECK (length(trim(responsibility)) > 0),
    PRIMARY KEY (jurisdiction_change_id, side, party, role)
) WITHOUT ROWID;

CREATE UNIQUE INDEX design_one_owner_per_side
    ON todo_design_responsibilities(jurisdiction_change_id, side)
    WHERE role = 'owner';

CREATE TABLE todo_design_jurisdiction_bases (
    jurisdiction_change_id  INTEGER NOT NULL
                                REFERENCES todo_design_jurisdiction_changes(id)
                                ON DELETE RESTRICT,
    ordinal                 INTEGER NOT NULL CHECK (ordinal >= 0),
    basis_ref               TEXT NOT NULL CHECK (length(trim(basis_ref)) > 0),
    PRIMARY KEY (jurisdiction_change_id, ordinal),
    UNIQUE (jurisdiction_change_id, basis_ref)
) WITHOUT ROWID;

CREATE TABLE todo_design_clauses (
    id                INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    design_id         INTEGER NOT NULL REFERENCES todo_designs(id) ON DELETE RESTRICT,
    slot              TEXT NOT NULL CHECK (length(trim(slot)) > 0),
    local_ref         TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    kind              TEXT NOT NULL CHECK (
                          kind IN (
                              'ownership', 'boundary', 'state', 'interface',
                              'lifecycle', 'failure', 'compatibility',
                              'acceptance', 'non_goal'
                          )
                      ),
    subject           TEXT NOT NULL CHECK (length(trim(subject)) > 0),
    statement         TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    jurisdiction_key  TEXT,
    status            TEXT NOT NULL DEFAULT 'active'
                          CHECK (status IN ('active', 'dropped')),
    UNIQUE (design_id, slot)
);

CREATE TABLE todo_design_clause_bases (
    clause_id   INTEGER NOT NULL REFERENCES todo_design_clauses(id) ON DELETE RESTRICT,
    ordinal     INTEGER NOT NULL CHECK (ordinal >= 0),
    basis_ref   TEXT NOT NULL CHECK (length(trim(basis_ref)) > 0),
    PRIMARY KEY (clause_id, ordinal),
    UNIQUE (clause_id, basis_ref)
) WITHOUT ROWID;

CREATE TABLE todo_design_choices (
    id            INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
    design_id     INTEGER NOT NULL REFERENCES todo_designs(id) ON DELETE RESTRICT,
    slot          TEXT NOT NULL CHECK (length(trim(slot)) > 0),
    local_ref     TEXT NOT NULL CHECK (length(trim(local_ref)) > 0),
    question      TEXT NOT NULL CHECK (length(trim(question)) > 0),
    materiality   TEXT NOT NULL CHECK (length(trim(materiality)) > 0),
    status        TEXT NOT NULL DEFAULT 'active'
                      CHECK (status IN ('active', 'dropped')),
    UNIQUE (design_id, slot)
);

CREATE TABLE todo_design_choice_bases (
    choice_id  INTEGER NOT NULL REFERENCES todo_design_choices(id) ON DELETE RESTRICT,
    ordinal    INTEGER NOT NULL CHECK (ordinal >= 0),
    basis_ref  TEXT NOT NULL CHECK (length(trim(basis_ref)) > 0),
    PRIMARY KEY (choice_id, ordinal),
    UNIQUE (choice_id, basis_ref)
) WITHOUT ROWID;

CREATE TABLE todo_design_operation_drops (
    design_id     INTEGER NOT NULL REFERENCES todo_designs(id) ON DELETE RESTRICT,
    operation_id  TEXT NOT NULL CHECK (length(trim(operation_id)) > 0),
    reason        TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    dropped_at    TEXT NOT NULL DEFAULT (
                      strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  ),
    PRIMARY KEY (design_id, operation_id)
) WITHOUT ROWID;

CREATE TABLE todo_design_operation_drop_bases (
    design_id     INTEGER NOT NULL,
    operation_id  TEXT NOT NULL,
    ordinal       INTEGER NOT NULL CHECK (ordinal >= 0),
    basis_ref     TEXT NOT NULL CHECK (length(trim(basis_ref)) > 0),
    PRIMARY KEY (design_id, operation_id, ordinal),
    UNIQUE (design_id, operation_id, basis_ref),
    FOREIGN KEY (design_id, operation_id)
        REFERENCES todo_design_operation_drops(design_id, operation_id)
        ON DELETE RESTRICT
) WITHOUT ROWID;

CREATE TABLE todo_design_assessment_returns (
    agent_job_id          INTEGER PRIMARY KEY
                              REFERENCES todo_agent_jobs(id) ON DELETE RESTRICT,
    assessment_id         INTEGER NOT NULL
                              REFERENCES todo_situation_assessments(id)
                              ON DELETE RESTRICT,
    design_id             INTEGER UNIQUE REFERENCES todo_designs(id) ON DELETE RESTRICT,
    reason                TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    producer_tool_call_id TEXT NOT NULL CHECK (length(trim(producer_tool_call_id)) > 0),
    created_at            TEXT NOT NULL DEFAULT (
                              strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                          )
);

CREATE TABLE todo_design_assessment_return_refs (
    agent_job_id         INTEGER NOT NULL
                             REFERENCES todo_design_assessment_returns(agent_job_id)
                             ON DELETE RESTRICT,
    ordinal              INTEGER NOT NULL CHECK (ordinal >= 0),
    missing_or_stale_ref TEXT NOT NULL CHECK (length(trim(missing_or_stale_ref)) > 0),
    PRIMARY KEY (agent_job_id, ordinal),
    UNIQUE (agent_job_id, missing_or_stale_ref)
) WITHOUT ROWID;

CREATE TRIGGER todo_design_assessment_returns_valid_subject
BEFORE INSERT ON todo_design_assessment_returns
WHEN NOT EXISTS (
    SELECT 1
    FROM todo_agent_jobs AS job
    JOIN todo_situation_assessments AS assessment
      ON assessment.id = NEW.assessment_id
    WHERE job.id = NEW.agent_job_id
      AND job.stage = 'design_reconciliation'
      AND job.todo_id = assessment.todo_id
) BEGIN
    SELECT RAISE(ABORT, 'assessment return does not match its design job');
END;

CREATE TRIGGER todo_design_assessment_returns_valid_design
BEFORE INSERT ON todo_design_assessment_returns
WHEN NOT (
    (NEW.design_id IS NULL AND NOT EXISTS (
        SELECT 1 FROM todo_designs WHERE agent_job_id = NEW.agent_job_id
    ))
    OR
    (NEW.design_id IS NOT NULL AND EXISTS (
        SELECT 1
        FROM todo_designs
        WHERE id = NEW.design_id
          AND agent_job_id = NEW.agent_job_id
          AND assessment_id = NEW.assessment_id
          AND state = 'abandoned'
    ))
) BEGIN
    SELECT RAISE(ABORT, 'assessment return has an invalid design outcome');
END;

CREATE TRIGGER todo_design_assessment_returns_immutable_update
BEFORE UPDATE ON todo_design_assessment_returns BEGIN
    SELECT RAISE(ABORT, 'assessment returns are immutable');
END;

CREATE TRIGGER todo_design_assessment_returns_immutable_delete
BEFORE DELETE ON todo_design_assessment_returns BEGIN
    SELECT RAISE(ABORT, 'assessment returns are immutable');
END;

CREATE TRIGGER todo_design_assessment_return_refs_immutable_update
BEFORE UPDATE ON todo_design_assessment_return_refs BEGIN
    SELECT RAISE(ABORT, 'assessment return references are immutable');
END;

CREATE TRIGGER todo_design_assessment_return_refs_immutable_delete
BEFORE DELETE ON todo_design_assessment_return_refs BEGIN
    SELECT RAISE(ABORT, 'assessment return references are immutable');
END;

CREATE TRIGGER todo_designs_no_insert_after_assessment_return
BEFORE INSERT ON todo_designs
WHEN NEW.agent_job_id IS NOT NULL AND EXISTS (
    SELECT 1
    FROM todo_design_assessment_returns
    WHERE agent_job_id = NEW.agent_job_id
) BEGIN
    SELECT RAISE(ABORT, 'design job already returned for assessment');
END;

CREATE TRIGGER todo_designs_identity_immutable
BEFORE UPDATE OF id, todo_id, revision, assessment_id, based_on_design_id,
                 agent_job_id, producer_tool_call_id, created_at
ON todo_designs BEGIN
    SELECT RAISE(ABORT, 'design identity and provenance are immutable');
END;

CREATE TRIGGER todo_designs_terminal_immutable
BEFORE UPDATE ON todo_designs
WHEN OLD.state NOT IN ('open', 'ready') BEGIN
    SELECT RAISE(ABORT, 'terminal designs are immutable');
END;

CREATE TRIGGER todo_designs_valid_transition
BEFORE UPDATE OF state ON todo_designs
WHEN NEW.state <> OLD.state
     AND NOT (
         (OLD.state = 'open'
          AND NEW.state IN ('ready', 'discarded', 'abandoned', 'invalidated'))
         OR
         (OLD.state = 'ready'
          AND NEW.state IN ('authorized', 'rejected', 'invalidated'))
     ) BEGIN
    SELECT RAISE(ABORT, 'invalid design transition');
END;

CREATE TRIGGER todo_designs_ready_content_sealed
BEFORE UPDATE OF draft_version, summary, canonical_digest ON todo_designs
WHEN OLD.state = 'ready' BEGIN
    SELECT RAISE(ABORT, 'ready design content is sealed');
END;

CREATE TRIGGER todo_designs_cannot_be_deleted
BEFORE DELETE ON todo_designs BEGIN
    SELECT RAISE(ABORT, 'designs cannot be deleted');
END;

CREATE TRIGGER todo_design_jurisdiction_changes_sealed_insert
BEFORE INSERT ON todo_design_jurisdiction_changes
WHEN (SELECT state FROM todo_designs WHERE id = NEW.design_id) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_jurisdiction_changes_sealed_update
BEFORE UPDATE ON todo_design_jurisdiction_changes
WHEN (SELECT state FROM todo_designs WHERE id = OLD.design_id) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_jurisdiction_changes_no_delete
BEFORE DELETE ON todo_design_jurisdiction_changes BEGIN
    SELECT RAISE(ABORT, 'design content cannot be deleted');
END;
CREATE TRIGGER todo_design_responsibilities_sealed_insert
BEFORE INSERT ON todo_design_responsibilities
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_jurisdiction_changes AS j ON j.design_id = d.id
    WHERE j.id = NEW.jurisdiction_change_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_responsibilities_sealed_update
BEFORE UPDATE ON todo_design_responsibilities
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_jurisdiction_changes AS j ON j.design_id = d.id
    WHERE j.id = OLD.jurisdiction_change_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_responsibilities_no_delete
BEFORE DELETE ON todo_design_responsibilities
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_jurisdiction_changes AS j ON j.design_id = d.id
    WHERE j.id = OLD.jurisdiction_change_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_jurisdiction_bases_sealed_insert
BEFORE INSERT ON todo_design_jurisdiction_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_jurisdiction_changes AS j ON j.design_id = d.id
    WHERE j.id = NEW.jurisdiction_change_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_jurisdiction_bases_immutable_update
BEFORE UPDATE ON todo_design_jurisdiction_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_jurisdiction_changes AS j ON j.design_id = d.id
    WHERE j.id = OLD.jurisdiction_change_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_jurisdiction_bases_no_delete
BEFORE DELETE ON todo_design_jurisdiction_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_jurisdiction_changes AS j ON j.design_id = d.id
    WHERE j.id = OLD.jurisdiction_change_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_clauses_sealed_insert
BEFORE INSERT ON todo_design_clauses
WHEN (SELECT state FROM todo_designs WHERE id = NEW.design_id) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_clauses_sealed_update
BEFORE UPDATE ON todo_design_clauses
WHEN (SELECT state FROM todo_designs WHERE id = OLD.design_id) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_clauses_no_delete
BEFORE DELETE ON todo_design_clauses BEGIN
    SELECT RAISE(ABORT, 'design clauses cannot be deleted');
END;
CREATE TRIGGER todo_design_clause_bases_sealed_insert
BEFORE INSERT ON todo_design_clause_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_clauses AS c ON c.design_id = d.id
    WHERE c.id = NEW.clause_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_clause_bases_immutable_update
BEFORE UPDATE ON todo_design_clause_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_clauses AS c ON c.design_id = d.id
    WHERE c.id = OLD.clause_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_clause_bases_no_delete
BEFORE DELETE ON todo_design_clause_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_clauses AS c ON c.design_id = d.id
    WHERE c.id = OLD.clause_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_choices_sealed_insert
BEFORE INSERT ON todo_design_choices
WHEN (SELECT state FROM todo_designs WHERE id = NEW.design_id) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_choices_sealed_update
BEFORE UPDATE ON todo_design_choices
WHEN (SELECT state FROM todo_designs WHERE id = OLD.design_id) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_choices_no_delete
BEFORE DELETE ON todo_design_choices BEGIN
    SELECT RAISE(ABORT, 'design choices cannot be deleted');
END;
CREATE TRIGGER todo_design_choice_bases_sealed_insert
BEFORE INSERT ON todo_design_choice_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_choices AS c ON c.design_id = d.id
    WHERE c.id = NEW.choice_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_choice_bases_immutable_update
BEFORE UPDATE ON todo_design_choice_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_choices AS c ON c.design_id = d.id
    WHERE c.id = OLD.choice_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_choice_bases_no_delete
BEFORE DELETE ON todo_design_choice_bases
WHEN (
    SELECT d.state FROM todo_designs AS d
    JOIN todo_design_choices AS c ON c.design_id = d.id
    WHERE c.id = OLD.choice_id
) <> 'open' BEGIN
    SELECT RAISE(ABORT, 'design content is sealed');
END;
CREATE TRIGGER todo_design_operation_drops_immutable_update
BEFORE UPDATE ON todo_design_operation_drops BEGIN
    SELECT RAISE(ABORT, 'design drop records are immutable');
END;
CREATE TRIGGER todo_design_operation_drops_immutable_delete
BEFORE DELETE ON todo_design_operation_drops BEGIN
    SELECT RAISE(ABORT, 'design drop records are immutable');
END;
CREATE TRIGGER todo_design_operation_drop_bases_immutable_update
BEFORE UPDATE ON todo_design_operation_drop_bases BEGIN
    SELECT RAISE(ABORT, 'design drop bases are immutable');
END;
CREATE TRIGGER todo_design_operation_drop_bases_immutable_delete
BEFORE DELETE ON todo_design_operation_drop_bases BEGIN
    SELECT RAISE(ABORT, 'design drop bases are immutable');
END;

CREATE VIEW todo_heads AS
SELECT t.id AS todo_id,
       (SELECT id FROM todo_direction_revisions AS d
        WHERE d.todo_id = t.id ORDER BY d.revision DESC LIMIT 1) AS direction_id,
       (SELECT max(id) FROM todo_concerns AS c
        WHERE c.todo_id = t.id) AS concern_cursor,
       (SELECT max(id) FROM todo_notes AS n
        WHERE n.todo_id = t.id) AS note_cursor,
       (SELECT id FROM todo_situation_assessments AS a
        WHERE a.todo_id = t.id ORDER BY a.id DESC LIMIT 1) AS assessment_id,
       (SELECT id FROM todo_designs AS d
        WHERE d.todo_id = t.id AND d.state = 'authorized'
        ORDER BY d.revision DESC LIMIT 1) AS design_id
FROM todos AS t;

PRAGMA user_version = 2;
