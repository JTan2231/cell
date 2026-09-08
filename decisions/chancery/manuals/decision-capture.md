# Capture decision documents

Use `krisis observe status`, `process`, and `reconcile` to operate automatic
capture. Krisis classifies one eligible completed root exchange as decision or
no-decision. A positive result creates a summary heading followed by the full
normalized user/assistant conversation through that exchange. Code renders the
source; the classifier returns only `is_decision` and `summary`.

The observer uses `krisis/decision-document/1` as its sole production path.
`document build --thread-id THREAD --turn-id TURN --directory DIRECTORY` and
`document render --directory DIRECTORY` use the same construction engine for
local files. They neither deliver to Annals nor advance observer coverage.
Retired daily and review commands cannot provide another production path.

Processing requires explicit absolute Annals binary and config paths and the
expected persistent decisions-library ID. It first retries the oldest pending
document with the same producer key and bytes. Otherwise, it freezes and
classifies one queued observation. The full prompt has a 262144-byte bound and
is never truncated. A positive summary is a nonblank single line of at most
1000 Unicode scalar values; a negative summary is null. Decision interpretation
is governed by the classifier instructions, not a content-fitness validator.

The private run saves source, request, and tool receipts before acknowledgement.
Coverage and the target-bound document outbox then commit together. Uncertain
Nucleus outcomes resume the same request. A matching Annals receipt clears the
outbox body; the private run remains. Annals outage leaves exact bytes pending.
Identity, source, digest, or receipt conflict stops processing.

Annals exchange contract 2 accepts the handed document as accessible nonblank
UTF-8 text, subject to a 1 MiB file bound and normal integrity/storage checks.
No sections, source metadata, source lookup, or semantic interpretation are
required for acceptance. Annals owns storage and later processing. Search and
read accepted documents through Annals; its feed returns complete accepted text.

Schema 5 preserves historical account and Decisions records. Migration requires
old account handoffs and in-flight observation jobs to be settled first. Legacy
codecs retain their historical meaning; they do not govern new documents.

Rust callers use `decisions::api::Client` for hook, activation, processing,
status, reconciliation, and diagnostics, and `annals_api::Client` for delivery.
The existing `accounts_pending_annals` and `accounts_accepted_by_annals` status
fields now count document handoffs. Counts are complete; status displays at most
20 failure IDs and codes by default and reports `failures_has_more`. Use
`--limit` to select more. Date selection uses exchange completion time for new
observations. Delivery counts are global. No routine result includes full source.
