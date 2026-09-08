# Krisis data model

Krisis uses SQLite schema 6 at the migration-compatible Decisions path. The
database and private document run directories together hold durable observer
state. Krisis does not store the authoritative accepted library copy.

`observations` stores hook correlation, canonical source identity, frozen-source
digest, completion time, activation eligibility, processing state, attempt epoch,
and target Annals config/library binding. Each eligible completed exchange has
one `decision` or `no_decision` outcome. A negative result needs no document.

`decision_documents` stores that outcome's stable document key and delivery
state. A pending row contains exact Markdown, SHA-256, and target identity. An
accepted row retains the digest and exact Annals receipt and clears its Markdown.
A negative row has no body, digest, or receipt. Coverage and a positive outbox
entry commit in one transaction. Receipt persistence verifies the same key,
bytes, and target before clearing the body.

Private `document-runs/OBSERVATION-ATTEMPT/` directories contain the frozen
normalized conversation, immutable Nucleus request, job identity, tool receipts,
and optional `decision.md`. They allow classification and rendering to resume
without rebuilding source or changing an ambiguous request. They remain after
Annals acceptance and must be included in private state protection and backup.

The document key is SHA-256 over the serialized host/thread/turn identity tuple
with a `document_` prefix. The content digest is SHA-256 of the exact UTF-8
Markdown bytes. Neither identity is a required field within the document.

Historical `decision_accounts`, `decision_account_sources`,
`decision_account_outbox`, authority verdicts, classifier receipts, and Decisions
lifecycle tables retain their original records and interpretation. New observer
processing does not write account projections or lifecycle events.

Migration 4-to-5 adds document delivery state. It refuses pending old account
handoffs, processing observations, or planned/submitted old observation jobs
with `legacy_account_cutover_required`. Settle those with the compatible old
runtime before migration. No account contents are inferred, rewritten, or lost
by the migration. Earlier migration guards remain in effect.

`observer_worker` retains the latest run's start and finish, an unfinished-run
marker, the start of continuous idle time, and the last worker error. `health`
combines these records with the serial lock. Empty polls preserve idle time.

`observation_failures` retains each failed attempt's observation identity, attempt
epoch, time, code, and private diagnostic detail. Retry clears the observation's
current failure but preserves these rows. Migration 5-to-6 adds these tables and
copies currently failed observations into the failure history. It does not retry
work or change the baseline. Earlier attempts already overwritten by older
versions cannot be reconstructed.

A delivery error marks its observation failed and keeps the document pending.
Pending counts include these retained deliveries, even though automatic delivery
excludes them. Explicit observation retry releases the same document for delivery;
it does not create another classification or producer key.
