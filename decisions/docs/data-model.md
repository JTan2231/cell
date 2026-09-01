# Data model

The current SQLite schema version is 3. `PRAGMA user_version` and
`schema_migrations` are checked at every open. Versions 1 and 2 migrate
sequentially in explicit transactions; newer and unsupported older versions
fail closed.

- `product_metadata` owns the write-once `observer_baseline_at`. Deployment
  creates it after candidate migration and release staging but before the live
  hook, command, plists, or services are published. Default activation stores
  the next whole Unix second. Processing, reconciliation, and daily projection
  require it; redeploy never advances it.
- `observations` is the durable completed-turn queue. Its stable ID derives from
  the hook session/turn correlation. After exact Conversations resolution it
  binds the canonical host/thread, a source digest, content-free completed-file
  change count, authority time, narrow/expanded scope, outcome, and bounded
  failure state. It never stores the hook body or a transcript.
- `observation_jobs` persists each scope's Nucleus attempt, request digest,
  admission state, and bounded terminal-only retry identity. An explicitly
  retried terminal observation increments its attempt epoch and retains every
  earlier job and receipt. There is at most one active processor and no
  parallel observation fan-out.
- `observation_classification_receipts` is the continuous requester's durable
  mailbox ledger. It stores exact bounded result bytes and a text-free validated
  DTO before acknowledgement, allowing uncertain in-flight processing to
  resume without a duplicate domain outcome.
- `authority_verdicts` records exactly one `decision` or `no_decision` result for
  each eligible user message in a completed observation. Negative verdicts are
  durable coverage, not inferred silence.
- `observation_candidates` associates validated stable candidates with the
  observation that established them.
- `runs` owns the local date window, durable `coverage_cutoff_at`, SQLite-rowid
  `observation_admission_watermark`, coverage manifest, terminal state, failure
  detail, review-driven content revision, and `run_kind`. The two frontiers bind
  the projection to turns completed by the cutoff and observation rows admitted
  through the watermark; later hook rows remain outside that immutable run. New
  runs are `observation_projection`; migrated version-one
  runs remain `legacy_scan`. Completeness is as of the cutoff, so a later-
  completing turn can change a manual rebuild of its authority date without
  amending an accepted scheduled delivery. A partial unique index permits only
  one `building` or `abandoning` run per report date.
- `run_jobs` and `classification_receipts` retain the original whole-thread
  requester correlation and pre-acknowledgement recovery state for legacy runs.
  They never store request or prompt bytes.
- `candidates` owns stable decisions, disposition, confidence, source event
  time and precision, exact authority byte span, optional rationale and
  supersession reference, and current review state.
- `run_candidates` lets stable candidates appear in repeat builds without
  changing identity.
- `candidate_sources` retains machine-local host/thread/turn/item anchors for
  the authoritative user message and the minimal supporting context.
- `reviews` is an append-only confirm/dismiss audit trail.
- `decision_events` is the append-only consumer outbox. Its SQLite sequence is
  exposed only through opaque cursors. Each row freezes an immutable
  version-one `decision_admitted` or `decision_reviewed` envelope containing
  normalized candidate lifecycle data and stable source anchors. Admission is
  transactionally coupled to first candidate persistence; each review event is
  coupled to its review row and current-state update. Partial unique indexes
  enforce one admission event per decision and one event per review. The
  version-one consumer contract activates at a current watermark and does not
  expose an origin cursor for historical replay.
- `digest_snapshots` freezes the subject and body per run revision.
- `deliveries` persists manual or scheduled idempotency, acceptance, and the
  external Email message ID. Admission is transactional, only one unresolved
  manual delivery may exist per run revision, and acceptance is monotonic.

Raw conversation text is sent to the bounded classifier but is not copied into
the Decisions database. Nucleus does durably retain the exact request prompt
and tool traffic in its own private local database, with no automatic pruning.
That retained text is the bounded level-0 slice initially and the normalized
full thread prefix when the single level-1 expansion is used; Nucleus owns its
retention and recovery. Initial observation classification receives only the
eligible current-turn user authorities, the immediately preceding assistant
proposal needed to interpret them when one exists, at most the current turn's
final assistant result, and a file-change count—never the whole turn/thread,
paths, diffs, commands, or tool output. Prior normalized context is disclosed
only after a validated request for the observation's single allowed expansion.
Persisted receipt candidates contain normalized statement/rationale fields,
disposition/confidence,
time/precision, validated span, and machine anchors only. Request digests
cannot reconstruct the prompt. Model-controlled invalid source IDs and upstream
error bodies are replaced by bounded product-owned errors before persistence.
The email contains normalized, deterministically disclosure-checked decision
statements and IDs, never excerpts, raw transcripts, rationales, credentials,
account/email identifiers, source IDs, tool traces, or local paths.

Back up the database together with `-wal` and `-shm` while the application is
quiescent, or use a SQLite-aware backup. Both the observer and daily-email
services must be stopped for a quiescent file copy. The user deployer does this
and preserves the database plus `-wal`, `-shm`, and `-journal` before candidate
doctor opens and migrates versions 1 or 2 to version 3. Version-3 migration
preserves existing domain rows and backfills retained candidate admissions
followed by append-only reviews before committing its new user version. Any
later schema change must add an explicit migration and preserve the same
rollback boundary before modification.

On Unix, a newly created state leaf is mode `0700`; the main database and any
existing SQLite `-wal`, `-shm`, or `-journal` sidecars are symlink-checked and
enforced at mode `0600`. A caller-selected existing parent is validated but its
mode is never changed. The content-free `.run.lock` sibling is likewise opened
without following symlinks and enforced at mode `0600`; it serializes only the
short admission-versus-abandonment critical section and is outside SQLite
backup state.
