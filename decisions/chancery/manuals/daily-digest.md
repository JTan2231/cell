# Daily enacted-decision digest

The continuous observer starts at its write-once activation baseline. A
completed root turn is eligible only when it has a post-baseline authoritative
user message and at least one successfully completed, nonempty App Server
`fileChange` item. The effect establishes enacted scope, not meaning: only the
user's explicit settlement can authorize a decision. Assistant text may resolve
“yes,” “this works,” or “do that,” but assistant behavior, writes, silence,
plans, and status reports are never authority.

The synchronous Codex `Stop` hook only enqueues session/turn correlation. The
60-second observer resolves that one completed turn and processes one durable
observation at a time. Its bounded first invocation receives every eligible
current-turn user authority, the immediately preceding assistant proposal when
one exists, at most the current turn's final assistant result, and a
content-free file-change count—not the whole turn or thread. It expands once to
prior normalized conversation context only after a validated `needs_context`
result, while keeping authority in the original turn. There is no parallel
whole-history classification.

Decisions does not copy that source text into its own database. Nucleus does
durably retain each exact request prompt and tool exchange in its private local
database and does not prune those records automatically. Retention is the
bounded level-0 slice for the first request and the normalized full thread
prefix when level 1 is used; Nucleus owns recovery and eventual manual pruning.

Start inspection with:

```sh
decisions observe status --date YYYY-MM-DD
decisions observe reconcile --date YYYY-MM-DD
decisions observe process
```

Reconcile only enqueues missed post-baseline effectful turns. Process resumes a
processing observation first, skips queued rows before their retry time, and
orders ready queued work by retry-ready time. A deferred source therefore yields
to other ready work while processing remains serial and can invoke Nucleus.
Queued status includes rows waiting for their retry time. Date-scoped status
conservatively includes unresolved failures that do not yet have an authority
time. A terminally failed observation is not automatically requeued and blocks
its daily projection; do not manufacture a verdict or reset the baseline. Only
after explicit recovery authorization and correction of the cause, `decisions
observe retry OBSERVATION_ID` increments its attempt epoch and requeues it while
retaining prior jobs and receipts.

Abandonment is a different recovery. Use `decisions observe abandon
OBSERVATION_ID --source-unavailable` only after diagnosis proves that the exact
Stop-hook source is permanently unavailable. It waits for the serial processing
lock and accepts only a previously observer-deferred pending level-0 row with a
retry time, no not-completed marker, and no bound source, job, authority,
verdict, or candidate. It records audited `complete` / `not_eligible` state with
the fixed `conversation_source_abandoned` marker. It stores no caller-provided
reason, creates no verdict, candidate, or lifecycle event, and leaves the
baseline unchanged. A repeated exact command is idempotent recovery from
uncertain command completion. Never abandon a merely unfinished turn; if
completed-root reconciliation later discovers an abandoned source, it fails
closed.

`daily build` records a durable completion cutoff, reconciles and drains to a
fixed point, then captures a SQLite-rowid observation-admission watermark. It
projects only observations completed by the cutoff and admitted through that
watermark; later hook rows remain available to a later build instead of racing
the immutable run. A healthy 09:00 run therefore performs no model work;
exceptional catch-up remains serial. Build is local and does not send. A turn
that completes later can enter a manual rebuild of its authority date, but an
accepted scheduled delivery is not automatically amended and the turn is not
carried into another date. Preview freezes and displays the exact current
revision. Manual send requires explicit authorization and uses an ad-hoc key.
The installed recurring service may invoke `daily run --scheduled` under its
separately established authorization.

Only explicit authoritative user settlements qualify. Inspect medium candidates
with `decisions show`, then confirm or dismiss them before preview when desired.
Never send a normal or all-clear digest after any observation, source,
classification, or projection failure. Email acceptance is not Gmail receipt;
validate receipt separately when that is the requested terminal outcome.

Schema-version-one `legacy_scan` runs and their requester receipts remain
readable. A matching interrupted legacy build resumes its original deterministic
attempt; only a positively observed terminal failure permits retry 1 or retry 2.
If its exact source can no longer be reproduced, `decisions daily abandon` is
the explicit recovery path. It requires observed Nucleus terminality and never
mutates continuous observations; it is separate from unavailable-source
observation abandonment.
