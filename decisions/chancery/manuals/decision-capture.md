# Capture decision documents

Use `krisis health`, `observe status`, `observe process`, and `observe reconcile` to operate automatic
capture. Krisis classifies one eligible completed root exchange as decision or
no-decision. A positive result creates a summary heading followed by the full
normalized user/assistant conversation through that exchange. Code renders the
source; the classifier returns only `is_decision` and `summary`.

The selected turn must contain a nonblank user message because Krisis identifies
user decisions. Empty or whitespace-only assistant text and earlier turns with
no messages are valid context. The classifier decides whether the selected
exchange contains a user decision.

The observer uses `krisis/decision-document/1` as its sole production path.
`document build --thread-id THREAD --turn-id TURN --directory DIRECTORY` and
`document render --directory DIRECTORY` use the same construction engine for
local files. They neither deliver to Annals nor advance observer coverage.
Retired daily and review commands cannot provide another production path.

Processing requires explicit absolute Annals binary and config paths and the
expected persistent decisions-library ID. It first delivers the oldest pending
document whose observation has not failed, using the same producer key and bytes. Otherwise, it freezes and
classifies one queued observation. The full prompt has a 262144-byte bound and
is never truncated. A positive summary is a nonblank single line of at most
1000 Unicode scalar values; a negative summary is null. Decision interpretation
is governed by the classifier instructions, not a content-fitness validator.

The private run saves source, request, and tool receipts before acknowledgement.
Coverage and the target-bound document outbox then commit together. Explicit recovery of uncertain
Nucleus outcomes resumes the same request. A matching Annals receipt clears the
outbox body; the private run remains.

The first observation processing error marks that observation failed and
retains its error. This includes missing or incomplete sources, classification,
dependency, target, digest, and receipt errors. A delivery failure keeps exact
bytes pending and excludes the failed observation from automatic delivery.
A conversation read failure (`document_source_unavailable`), including a timeout
or protocol error, returns zero after its failed observation is saved. Later
scheduled work can proceed. Other observation failures return nonzero with
`observation_processing_failed` and halt the configured Clockwork schedule.
Errors that prevent a durable outcome also return nonzero.

Failed observations do not retry automatically. Repeated hooks and reconciliation
preserve their failed state. Use `krisis observe retry OBSERVATION_ID` after
inspection. A pending document reuses its exact key, bytes, and target. An
uncertain Nucleus job or saved accepted classification reuses the saved run.
A new classification attempt requires no existing run, or a saved terminal run
without an accepted classification. Retry preserves private failure history.
A known accepted document or committed result is never discarded by failure.

Annals exchange contract 2 accepts the handed document as accessible nonblank
UTF-8 text, subject to a 1 MiB file bound and normal integrity/storage checks.
No sections, source metadata, source lookup, or semantic interpretation are
required for acceptance. Annals owns storage and later processing. Search and
read accepted documents through Annals; its feed returns complete accepted text.

Schema 6 preserves historical account and Decisions records. Migration requires
old account handoffs and in-flight observation jobs to be settled first. Legacy
codecs retain their historical meaning; they do not govern new documents.

Rust callers use `decisions::api::Client` for hook, activation, processing,
status, reconciliation, and diagnostics, and `annals_api::Client` for delivery.
The existing `accounts_pending_annals` and `accounts_accepted_by_annals` status
fields now count document handoffs. Counts are complete; status displays at most
20 failure IDs and codes by default and reports `failures_has_more`. Use
`--limit` to select more. Date selection uses exchange completion time for new
observations. Delivery counts are global; pending includes documents held by
failed observations until explicit retry. No routine result includes full source.

## Worker health

`krisis health [--max-idle-seconds N] [--json]` reads worker activity separately
from failed-observation counts. It needs no Annals configuration and calls no
dependency. It obeys maintenance admission and can migrate the database.

`working` means the serial processing lock has a live owner. `idle` means the
worker has finished a run within the selected limit. Empty polls update the last
finish without resetting continuous idle time. A saved conversation read failure
leaves the worker idle; other newly failed observations are worker errors.
Historical failed observations do not create recurring incidents.

The idle limit defaults to 180 seconds for the installed 60-second schedule.
`stale` means the last finished run is older than that limit. It is a diagnostic
threshold, not a scheduling guarantee. `error` means the last worker run failed,
excluding saved conversation read failures. `interrupted` means a started run has no finish and
no lock owner. `unobserved` means no worker activity has been recorded yet.

JSON reports `ok`, `state`, `checked_at`, `state_since`,
`state_duration_seconds`, `last_started_at`, `last_finished_at`,
`max_idle_seconds`, and `error_code`. Times are Unix seconds; durations are seconds
at `checked_at`. Unknown transition times and durations are null. An interrupted
exit time is unknown; its last start remains visible. Stale state begins at the
last finish plus the selected limit. A live run duration proves ownership, not
classifier progress. Exit zero means working or idle. Other states print the
report and exit nonzero. `Client::health` returns unhealthy reports with `ok: false`.
Use `doctor` for dependency readiness and `observe status` for record counts.

Schema 5-to-6 migration adds worker activity and failure history, preserving
existing failures without retrying them. It cannot restore older overwritten
attempt errors. Back up the database and document runs before a schema upgrade;
older binaries cannot open schema 6.

## Scheduled failure policy

Krisis configures Clockwork definition schema 2 for `krisis/observer` with
`[failure] on_abend = "halt-until-approved"`. A conversation read failure
(`document_source_unavailable`), including a timeout or protocol error, is a
handled outcome after Krisis saves the failed observation. It returns zero,
permits later activations, and creates no pause alert. Other launch, dependency,
source-validation, classification, or Annals delivery failures halt future
activations. A failed or cancelled
Nucleus job after an accepted classification preserves the classification and
reports that exact job to Clockwork; it creates no successor attempt. An empty poll or valid
maintenance gate is a successful no-work result. Krisis owns these outcome
meanings and its configuration; Clockwork owns the durable scheduling incident,
admission gate, and one retained email notification through
`HOME/.local/bin/email`.

Inspect `clockwork incident list krisis/observer` and `clockwork incident show
INCIDENT_ID`. After explicit approval, use `clockwork binding resume
krisis/observer INCIDENT_ID`. This allows later scheduling and does not retry a
failed observation. Use the separate guarded `observe retry OBSERVATION_ID` when
that recovery is authorized. It retains saved requests, accepted classifications,
exact documents, target identity, and idempotent Annals acceptance.

Definition switches and deployment preserve the Clockwork incident. Existing
failed observations remain terminal history; cutover does not re-alert or retry
them. The retired Decisions schedules remain disabled. Schema-one definitions
keep their old policy until a schema-two definition is explicitly selected.
