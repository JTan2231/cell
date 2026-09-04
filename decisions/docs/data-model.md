# Krisis data model

Krisis uses SQLite schema version 4 at the migration-compatible Decisions path.
Schema versions 1 through 3 are upgraded sequentially; their rows and decoders
remain intact.

## Active schema-4 records

`observations` owns one correlated completed-turn work item, level, retry epoch,
source digest, exact Annals config-path/library target binding, status, and
terminal `decision`/`no_decision` outcome.
`observation_authority_items` is the admitted user authority set and
`authority_verdicts` supplies exact binary coverage.

`observation_jobs` and `observation_classification_receipts` retain Nucleus job,
tool-call, exact call-argument and request digests, accepted-result, retry, and
recovery correlation. New
Krisis receipt bodies are reduced to a minimal marker after Annals acceptance;
legacy Decisions receipt decoding is unchanged.

`decision_accounts` holds stable account identity, occurrence, precision,
authority span, and—only before acceptance—generated statement/context/action/
result and exact quote. `decision_account_sources` records authority and
supporting anchors; non-authority support rows may be removed after acceptance.
`observation_accounts` relates accounts to their producing coverage transaction.

`decision_account_outbox` is the durable handoff ledger. Pending rows contain
the deterministic Markdown, SHA-256, exact config path, and expected persistent
library ID. Accepted rows contain no Markdown and
retain contract/library/producer/key/digest/job/time/created-or-replayed receipt
evidence. Conflicting account identity, bytes, anchor, or receipt fails closed.

## Stable identity

Krisis derives decision ID from real host identity, canonical authority item
identity, and the validated exact UTF-8 byte span. Model aliases and normalized
wording cannot affect it. Occurrence is an `i64` Unix second and timestamp
precision is retained separately.

The account Source JSON schema version, classifier capture-rule version,
decision ID, Nucleus job/call IDs, Annals library/job IDs, digest, observation
ID, and legacy Decisions lifecycle IDs are separate compatibility axes.

## Retained legacy records

Runs, candidates, candidate sources, reviews, snapshots, email deliveries, and
lifecycle events remain so schema migration, interrupted recovery, existing
read-only `events`, and legacy `show` can decode prior state. Krisis creates no
new digest, review, email, candidate, or lifecycle state through its public
active path.

The database is an operational capture ledger, not the decision library. Once
Annals accepts an account, Krisis intentionally cannot reconstruct its prose
from retained state; browse and search through Annals.

Migration to schema 4 refuses any planned/submitted legacy
`decisions-observe-*` correlation and any accepted legacy classification whose
observation did not commit. Terminal legacy history remains readable.
