PRAGMA user_version = 1;

CREATE TABLE pratica_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

INSERT INTO pratica_meta (key, value) VALUES ('schema_version', '1');

CREATE TABLE steward_scopes (
    scope_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    steward_party TEXT NOT NULL CHECK (length(steward_party) > 0),
    title TEXT NOT NULL CHECK (length(title) > 0),
    charter_markdown BLOB NOT NULL CHECK (length(charter_markdown) > 0),
    charter_sha256 TEXT NOT NULL CHECK (length(charter_sha256) = 64),
    descriptor_sha256 TEXT NOT NULL CHECK (length(descriptor_sha256) = 64),
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY (scope_id, version)
) STRICT;

CREATE TABLE frozen_bases (
    basis_id TEXT PRIMARY KEY,
    basis_kind TEXT NOT NULL CHECK (basis_kind IN ('steward', 'candidate')),
    label TEXT NOT NULL CHECK (length(label) > 0),
    scope_id TEXT,
    scope_version INTEGER,
    verifier_version TEXT NOT NULL CHECK (length(verifier_version) > 0),
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64),
    observed_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    CHECK (
        (basis_kind = 'steward' AND scope_id IS NOT NULL AND scope_version IS NOT NULL)
        OR (basis_kind = 'candidate' AND scope_id IS NULL AND scope_version IS NULL)
    ),
    FOREIGN KEY (scope_id, scope_version)
        REFERENCES steward_scopes(scope_id, version)
) STRICT;

CREATE UNIQUE INDEX frozen_bases_one_per_steward_version
    ON frozen_bases(scope_id, scope_version) WHERE basis_kind = 'steward';

CREATE TABLE frozen_basis_sources (
    basis_id TEXT NOT NULL REFERENCES frozen_bases(basis_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    source_id TEXT NOT NULL CHECK (length(source_id) > 0),
    kind TEXT NOT NULL CHECK (length(kind) > 0),
    locator TEXT NOT NULL CHECK (length(locator) > 0),
    origin_path TEXT,
    revision TEXT,
    content BLOB NOT NULL,
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (basis_id, ordinal),
    UNIQUE (basis_id, source_id)
) STRICT;

CREATE TABLE basis_verifications (
    verification_id TEXT PRIMARY KEY,
    basis_id TEXT NOT NULL REFERENCES frozen_bases(basis_id),
    outcome TEXT NOT NULL CHECK (outcome IN ('fresh', 'stale', 'unknown')),
    observed_manifest_sha256 TEXT
        CHECK (observed_manifest_sha256 IS NULL OR length(observed_manifest_sha256) = 64),
    detail_markdown BLOB,
    checked_at INTEGER NOT NULL
) STRICT;

CREATE INDEX basis_verifications_latest
    ON basis_verifications(basis_id, checked_at DESC, verification_id DESC);

CREATE TABLE integrations (
    integration_id TEXT PRIMARY KEY,
    entrant_party TEXT NOT NULL CHECK (length(entrant_party) > 0),
    title TEXT NOT NULL CHECK (length(title) > 0),
    context_markdown BLOB,
    context_sha256 TEXT CHECK (context_sha256 IS NULL OR length(context_sha256) = 64),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE integration_tracks (
    track_id TEXT PRIMARY KEY,
    integration_id TEXT NOT NULL REFERENCES integrations(integration_id),
    scope_id TEXT NOT NULL,
    scope_version INTEGER NOT NULL,
    steward_party TEXT NOT NULL CHECK (length(steward_party) > 0),
    created_at INTEGER NOT NULL,
    FOREIGN KEY (scope_id, scope_version)
        REFERENCES steward_scopes(scope_id, version)
) STRICT;

CREATE TABLE integration_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    integration_id TEXT NOT NULL REFERENCES integrations(integration_id),
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    kind TEXT NOT NULL CHECK (kind IN ('opened', 'track_added', 'track_retired')),
    track_id TEXT REFERENCES integration_tracks(track_id),
    reason TEXT,
    recorded_at INTEGER NOT NULL,
    UNIQUE (integration_id, ordinal)
) STRICT;

CREATE TABLE negotiations (
    negotiation_id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL REFERENCES integration_tracks(track_id),
    kind TEXT NOT NULL CHECK (kind IN ('initial', 'amendment')),
    predecessor_agreement_id TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (predecessor_agreement_id) REFERENCES agreements(agreement_id)
) STRICT;

CREATE TABLE offers (
    offer_id TEXT PRIMARY KEY,
    negotiation_id TEXT NOT NULL REFERENCES negotiations(negotiation_id),
    author_role TEXT NOT NULL CHECK (author_role IN ('entrant', 'steward')),
    terms_markdown BLOB NOT NULL CHECK (length(terms_markdown) > 0),
    terms_sha256 TEXT NOT NULL CHECK (length(terms_sha256) = 64),
    basis_id TEXT REFERENCES frozen_bases(basis_id),
    recorded_at INTEGER NOT NULL
) STRICT;

CREATE TABLE negotiation_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    negotiation_id TEXT NOT NULL REFERENCES negotiations(negotiation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    kind TEXT NOT NULL CHECK (kind IN (
        'opened', 'offer_submitted', 'assent', 'assent_withdrawn',
        'steward_blocked', 'cancelled', 'agreement_sealed'
    )),
    party_role TEXT CHECK (party_role IS NULL OR party_role IN ('entrant', 'steward')),
    offer_id TEXT REFERENCES offers(offer_id),
    basis_id TEXT REFERENCES frozen_bases(basis_id),
    review_markdown BLOB,
    reason TEXT,
    attempt_id TEXT REFERENCES agent_attempts(attempt_id),
    recorded_at INTEGER NOT NULL,
    UNIQUE (negotiation_id, ordinal)
) STRICT;

CREATE INDEX negotiation_events_projection
    ON negotiation_events(negotiation_id, ordinal DESC);

CREATE TABLE agreements (
    agreement_id TEXT PRIMARY KEY,
    negotiation_id TEXT NOT NULL UNIQUE REFERENCES negotiations(negotiation_id),
    offer_id TEXT NOT NULL REFERENCES offers(offer_id),
    basis_id TEXT NOT NULL REFERENCES frozen_bases(basis_id),
    sealed_event_id INTEGER NOT NULL UNIQUE REFERENCES negotiation_events(event_id),
    sealed_at INTEGER NOT NULL
) STRICT;

CREATE INDEX agreements_offer ON agreements(offer_id);

CREATE TABLE agent_attempts (
    attempt_id TEXT PRIMARY KEY,
    predecessor_attempt_id TEXT UNIQUE REFERENCES agent_attempts(attempt_id),
    kind TEXT NOT NULL CHECK (kind IN (
        'steward_response', 'composition_review', 'conformance_review'
    )),
    subject_id TEXT NOT NULL CHECK (length(subject_id) > 0),
    requester_id TEXT NOT NULL CHECK (length(requester_id) > 0),
    nucleus_job_id TEXT NOT NULL UNIQUE CHECK (length(nucleus_job_id) > 0),
    request_bytes BLOB NOT NULL,
    request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
    toolset_name TEXT NOT NULL CHECK (length(toolset_name) > 0),
    toolset_version INTEGER NOT NULL CHECK (toolset_version > 0),
    expected_offer_id TEXT REFERENCES offers(offer_id),
    expected_roster_digest TEXT,
    basis_id TEXT REFERENCES frozen_bases(basis_id),
    basis_digest TEXT NOT NULL CHECK (length(basis_digest) = 64),
    catalog_scope TEXT NOT NULL,
    catalog_version INTEGER NOT NULL CHECK (catalog_version > 0),
    catalog_verifier_version TEXT NOT NULL CHECK (length(catalog_verifier_version) > 0),
    catalog_observed_at INTEGER NOT NULL,
    catalog_party TEXT NOT NULL,
    catalog_title TEXT NOT NULL,
    catalog_charter_markdown BLOB NOT NULL,
    catalog_charter_sha256 TEXT NOT NULL CHECK (length(catalog_charter_sha256) = 64),
    catalog_sha256 TEXT NOT NULL CHECK (length(catalog_sha256) = 64),
    tool_after INTEGER NOT NULL DEFAULT 0 CHECK (tool_after >= 0),
    admitted INTEGER NOT NULL DEFAULT 0 CHECK (admitted IN (0, 1)),
    accepted_job_id TEXT,
    accepted_request_sha256 TEXT,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    runtime_state TEXT NOT NULL DEFAULT 'prepared' CHECK (runtime_state IN (
        'prepared', 'admitted', 'running', 'completed', 'failed',
        'cancelled', 'lost', 'timed_out'
    )),
    runtime_detail TEXT,
    domain_result_kind TEXT,
    domain_result_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX agent_attempts_one_active
    ON agent_attempts(kind, subject_id) WHERE active = 1;

CREATE TABLE attempt_sources (
    attempt_id TEXT NOT NULL REFERENCES agent_attempts(attempt_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    source_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    locator TEXT NOT NULL,
    origin_path TEXT NOT NULL,
    revision TEXT,
    content BLOB NOT NULL,
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (attempt_id, ordinal),
    UNIQUE (attempt_id, source_id)
) STRICT;

CREATE TABLE tool_receipts (
    receipt_id TEXT PRIMARY KEY,
    attempt_id TEXT NOT NULL REFERENCES agent_attempts(attempt_id),
    nucleus_job_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    arguments_sha256 TEXT NOT NULL CHECK (length(arguments_sha256) = 64),
    result_json BLOB NOT NULL,
    is_error INTEGER NOT NULL CHECK (is_error IN (0, 1)),
    domain_result_kind TEXT,
    domain_result_id TEXT,
    recorded_at INTEGER NOT NULL,
    UNIQUE (nucleus_job_id, call_id)
) STRICT;

CREATE TABLE tool_receipt_source_refs (
    receipt_id TEXT NOT NULL REFERENCES tool_receipts(receipt_id),
    source_ref TEXT NOT NULL,
    PRIMARY KEY (receipt_id, source_ref)
) STRICT;

CREATE TABLE composition_reviews (
    review_id TEXT PRIMARY KEY,
    integration_id TEXT NOT NULL REFERENCES integrations(integration_id),
    roster_revision INTEGER NOT NULL CHECK (roster_revision > 0),
    roster_digest TEXT NOT NULL CHECK (length(roster_digest) = 64),
    outcome TEXT NOT NULL CHECK (outcome IN ('compatible', 'conflicts', 'blocked')),
    review_markdown BLOB NOT NULL,
    review_sha256 TEXT NOT NULL CHECK (length(review_sha256) = 64),
    attempt_id TEXT REFERENCES agent_attempts(attempt_id),
    recorded_at INTEGER NOT NULL
) STRICT;

CREATE TABLE composition_review_agreements (
    review_id TEXT NOT NULL REFERENCES composition_reviews(review_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    track_id TEXT NOT NULL REFERENCES integration_tracks(track_id),
    agreement_id TEXT NOT NULL REFERENCES agreements(agreement_id),
    terms_sha256 TEXT NOT NULL CHECK (length(terms_sha256) = 64),
    basis_id TEXT NOT NULL REFERENCES frozen_bases(basis_id),
    PRIMARY KEY (review_id, ordinal),
    UNIQUE (review_id, track_id)
) STRICT;

CREATE TABLE conformance_reviews (
    review_id TEXT PRIMARY KEY,
    agreement_id TEXT NOT NULL REFERENCES agreements(agreement_id),
    candidate_basis_id TEXT NOT NULL REFERENCES frozen_bases(basis_id),
    outcome TEXT NOT NULL CHECK (outcome IN ('conforms', 'does_not_conform', 'blocked')),
    review_markdown BLOB NOT NULL,
    review_sha256 TEXT NOT NULL CHECK (length(review_sha256) = 64),
    attempt_id TEXT REFERENCES agent_attempts(attempt_id),
    recorded_at INTEGER NOT NULL
) STRICT;

CREATE TRIGGER steward_scopes_no_update BEFORE UPDATE ON steward_scopes
BEGIN SELECT RAISE(ABORT, 'steward_scopes are immutable'); END;
CREATE TRIGGER steward_scopes_no_delete BEFORE DELETE ON steward_scopes
BEGIN SELECT RAISE(ABORT, 'steward_scopes are immutable'); END;
CREATE TRIGGER frozen_bases_no_update BEFORE UPDATE ON frozen_bases
BEGIN SELECT RAISE(ABORT, 'frozen_bases are immutable'); END;
CREATE TRIGGER frozen_bases_no_delete BEFORE DELETE ON frozen_bases
BEGIN SELECT RAISE(ABORT, 'frozen_bases are immutable'); END;
CREATE TRIGGER frozen_basis_sources_no_update BEFORE UPDATE ON frozen_basis_sources
BEGIN SELECT RAISE(ABORT, 'frozen_basis_sources are immutable'); END;
CREATE TRIGGER frozen_basis_sources_no_delete BEFORE DELETE ON frozen_basis_sources
BEGIN SELECT RAISE(ABORT, 'frozen_basis_sources are immutable'); END;
CREATE TRIGGER basis_verifications_no_update BEFORE UPDATE ON basis_verifications
BEGIN SELECT RAISE(ABORT, 'basis_verifications are immutable'); END;
CREATE TRIGGER basis_verifications_no_delete BEFORE DELETE ON basis_verifications
BEGIN SELECT RAISE(ABORT, 'basis_verifications are immutable'); END;
CREATE TRIGGER integrations_no_update BEFORE UPDATE ON integrations
BEGIN SELECT RAISE(ABORT, 'integrations are immutable'); END;
CREATE TRIGGER integrations_no_delete BEFORE DELETE ON integrations
BEGIN SELECT RAISE(ABORT, 'integrations are immutable'); END;
CREATE TRIGGER integration_tracks_no_update BEFORE UPDATE ON integration_tracks
BEGIN SELECT RAISE(ABORT, 'integration_tracks are immutable'); END;
CREATE TRIGGER integration_tracks_no_delete BEFORE DELETE ON integration_tracks
BEGIN SELECT RAISE(ABORT, 'integration_tracks are immutable'); END;
CREATE TRIGGER integration_events_no_update BEFORE UPDATE ON integration_events
BEGIN SELECT RAISE(ABORT, 'integration_events are immutable'); END;
CREATE TRIGGER integration_events_no_delete BEFORE DELETE ON integration_events
BEGIN SELECT RAISE(ABORT, 'integration_events are immutable'); END;
CREATE TRIGGER negotiations_no_update BEFORE UPDATE ON negotiations
BEGIN SELECT RAISE(ABORT, 'negotiations are immutable'); END;
CREATE TRIGGER negotiations_no_delete BEFORE DELETE ON negotiations
BEGIN SELECT RAISE(ABORT, 'negotiations are immutable'); END;
CREATE TRIGGER offers_no_update BEFORE UPDATE ON offers
BEGIN SELECT RAISE(ABORT, 'offers are immutable'); END;
CREATE TRIGGER offers_no_delete BEFORE DELETE ON offers
BEGIN SELECT RAISE(ABORT, 'offers are immutable'); END;
CREATE TRIGGER negotiation_events_no_update BEFORE UPDATE ON negotiation_events
BEGIN SELECT RAISE(ABORT, 'negotiation_events are immutable'); END;
CREATE TRIGGER negotiation_events_no_delete BEFORE DELETE ON negotiation_events
BEGIN SELECT RAISE(ABORT, 'negotiation_events are immutable'); END;
CREATE TRIGGER agreements_no_update BEFORE UPDATE ON agreements
BEGIN SELECT RAISE(ABORT, 'agreements are immutable'); END;
CREATE TRIGGER agreements_no_delete BEFORE DELETE ON agreements
BEGIN SELECT RAISE(ABORT, 'agreements are immutable'); END;
CREATE TRIGGER attempt_sources_no_update BEFORE UPDATE ON attempt_sources
BEGIN SELECT RAISE(ABORT, 'attempt_sources are immutable'); END;
CREATE TRIGGER attempt_sources_no_delete BEFORE DELETE ON attempt_sources
BEGIN SELECT RAISE(ABORT, 'attempt_sources are immutable'); END;
CREATE TRIGGER tool_receipts_no_update BEFORE UPDATE ON tool_receipts
BEGIN SELECT RAISE(ABORT, 'tool_receipts are immutable'); END;
CREATE TRIGGER tool_receipts_no_delete BEFORE DELETE ON tool_receipts
BEGIN SELECT RAISE(ABORT, 'tool_receipts are immutable'); END;
CREATE TRIGGER tool_receipt_source_refs_no_update BEFORE UPDATE ON tool_receipt_source_refs
BEGIN SELECT RAISE(ABORT, 'tool_receipt_source_refs are immutable'); END;
CREATE TRIGGER tool_receipt_source_refs_no_delete BEFORE DELETE ON tool_receipt_source_refs
BEGIN SELECT RAISE(ABORT, 'tool_receipt_source_refs are immutable'); END;
CREATE TRIGGER composition_reviews_no_update BEFORE UPDATE ON composition_reviews
BEGIN SELECT RAISE(ABORT, 'composition_reviews are immutable'); END;
CREATE TRIGGER composition_reviews_no_delete BEFORE DELETE ON composition_reviews
BEGIN SELECT RAISE(ABORT, 'composition_reviews are immutable'); END;
CREATE TRIGGER composition_review_agreements_no_update BEFORE UPDATE ON composition_review_agreements
BEGIN SELECT RAISE(ABORT, 'composition_review_agreements are immutable'); END;
CREATE TRIGGER composition_review_agreements_no_delete BEFORE DELETE ON composition_review_agreements
BEGIN SELECT RAISE(ABORT, 'composition_review_agreements are immutable'); END;
CREATE TRIGGER conformance_reviews_no_update BEFORE UPDATE ON conformance_reviews
BEGIN SELECT RAISE(ABORT, 'conformance_reviews are immutable'); END;
CREATE TRIGGER conformance_reviews_no_delete BEFORE DELETE ON conformance_reviews
BEGIN SELECT RAISE(ABORT, 'conformance_reviews are immutable'); END;

CREATE TRIGGER agent_attempts_identity_immutable BEFORE UPDATE ON agent_attempts
WHEN NEW.attempt_id != OLD.attempt_id
  OR NEW.predecessor_attempt_id IS NOT OLD.predecessor_attempt_id
  OR NEW.kind != OLD.kind
  OR NEW.subject_id != OLD.subject_id
  OR NEW.requester_id != OLD.requester_id
  OR NEW.nucleus_job_id != OLD.nucleus_job_id
  OR NEW.request_bytes != OLD.request_bytes
  OR NEW.request_sha256 != OLD.request_sha256
  OR NEW.toolset_name != OLD.toolset_name
  OR NEW.toolset_version != OLD.toolset_version
  OR NEW.expected_offer_id IS NOT OLD.expected_offer_id
  OR NEW.expected_roster_digest IS NOT OLD.expected_roster_digest
  OR NEW.basis_id IS NOT OLD.basis_id
  OR NEW.basis_digest != OLD.basis_digest
  OR NEW.catalog_scope != OLD.catalog_scope
  OR NEW.catalog_version != OLD.catalog_version
  OR NEW.catalog_verifier_version != OLD.catalog_verifier_version
  OR NEW.catalog_observed_at != OLD.catalog_observed_at
  OR NEW.catalog_party != OLD.catalog_party
  OR NEW.catalog_title != OLD.catalog_title
  OR NEW.catalog_charter_markdown != OLD.catalog_charter_markdown
  OR NEW.catalog_charter_sha256 != OLD.catalog_charter_sha256
  OR NEW.catalog_sha256 != OLD.catalog_sha256
  OR NEW.created_at != OLD.created_at
BEGIN SELECT RAISE(ABORT, 'attempt request identity is immutable'); END;

CREATE TRIGGER agent_attempts_no_delete BEFORE DELETE ON agent_attempts
BEGIN SELECT RAISE(ABORT, 'agent_attempts are durable'); END;
