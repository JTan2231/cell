# Failure policy, incidents, and continuation

Use this feature to understand why a binding is halted, report a product
failure, read incident evidence, or approve future admission. The product owns
failure configuration and domain interpretation. Clockwork enforces the
selected scheduling response and retains failure identity. Approval of future
admission does not authorize retry of failed product work.

## Interfaces

```text
clockwork [--json] abend ACTIVATION_ID --code CODE --occurrence ID
clockwork [--json] binding halt KEY --code CODE --occurrence ID
clockwork [--json] binding resume KEY INCIDENT_ID
clockwork [--json] incident list [KEY] [--limit N]
clockwork [--json] incident show INCIDENT_ID
clockwork [--json] incident feed --after CURSOR --limit N
```

Commands print plain text by default. `--json` selects the existing compact
`ok:true` / `data` JSON envelope. With that flag, coded `ok:false` / `error`
failures go to stderr with exit one. Other failures are human-readable on
stderr. Machine callers must pass `--json`.

Manifest schema two adds a product-owned failure configuration:

```toml
[failure]
on_abend = "halt-until-approved"
# email_cli = "/Users/operator/.local/bin/email"
```

Omission means `halt-until-approved`. The only exception is
`continue-next-activation`, which retains the abend and permits the next timer
activation. It does not retry the failed occurrence. The product declares and
explains any exception. `email_cli` selects one absolute installed Email
wrapper; omission selects `$HOME/.local/bin/email` for the current Clockwork
user. This is a separately contracted Email transport, not an extension of the
attested product launch image.

A schema-two startup failure, nonzero direct-child exit, signal, timeout,
lost activation, or broker supervision failure applies the selected policy.
The product owns the interpretation of expected empty, waiting, and deferred
outcomes and returns zero for those outcomes. An overlap is normal and never
halts scheduling. Zero remains runtime evidence, not domain success.

A product can report a terminal domain failure while its supervised process
still runs, including after it durably recorded that failure:

```rust
clockwork::api::report_abend("model_failed", "job/immutable-job-id")?;
```

The helper returns `false` outside Clockwork and rejects a partial context. A
schema-two child receives `CLOCKWORK_BROKER_PATH`, `CLOCKWORK_STATE_ROOT`, and
`CLOCKWORK_ACTIVATION_ID` in addition to its registered environment. Preserve
those values across a product-owned environment scrub. Registered environment
names beginning `CLOCKWORK_` are reserved. The helper invokes the exact broker
with `abend ACTIVATION_ID --code CODE --occurrence ID`; it does not access
Clockwork storage directly. The activation must still be running.

A report contains only a bounded machine code (64 bytes) and immutable product
occurrence ID (256 bytes). Both permit ASCII letters, digits, `-`, `_`, `/`,
`.`, and `:`. Do not supply source text, an email body, a credential, or an
unbounded error message. Clockwork retains each `(key, occurrence)` once.
Reporting that same failed occurrence after approval does not create a new
halt. A distinct failed product attempt needs a distinct occurrence ID. Stop
claiming successor work as soon as an abend is encountered; a report cannot
cancel already admitted product work or make product commits atomic with the
Clockwork incident.

A default-policy abend durably records immutable failure evidence and starts
one pending episode for its stable key. The first abnormal attempt ends its
current run; later activations remain admissible before the shared service-health
threshold. No incident or halt is created solely by that first failure.

By default, five consecutive failed read-only checks, at least 60 seconds apart,
confirm the episode. Clockwork then opens one incident, closes future admission,
and makes the alert eligible together. It reads bounded installed Iatreion
reports; unknown health counts as failed with an explicit unknown condition.
A healthy worker observation, a later successful activation without an abend,
or explicit inactive intent clears the pending episode. Operator pause is
inactive intent. Checks do not run products or retry failed work. Existing
broker visits and the EMT worker advance due checks; no daemon or timer is added.

The immutable abend ledger remains in SQLite schema two. The private
schema-one `failure-checks.json` sidecar retains its ledger cursor and per-key
pending event and check progress. Clearing a pending episode preserves the
abend evidence. Incident list and feed expose confirmed halts, not pending
episodes. Use `notification check` to inspect pending check progress.

An established halt is independent of `enabled`, selected definition, product
maintenance, and deployment. Switching, disabling, compensation, restart, and
reinstall never clear it. The loaded timer may still invoke the broker, which
starts no product child. A halted launchd invocation returns its binding receipt
without another activation or incident. A manual run returns `binding_halted`.
A check does not cancel a child that was already admitted.

Inspect or explicitly approve continuation:

```sh
clockwork binding show owner/name
clockwork incident list owner/name --limit 20
clockwork incident show INCIDENT_ID
clockwork binding resume owner/name INCIDENT_ID
```

`halted_incident` identifies the current incident. `failure_policy_active`
indicates a selected schema-two definition. Incident timestamps are whole Unix
seconds; notification timestamps and attempts describe only that incident's
email submission attempts. `resume` requires the exact open incident, no
running activation, and no pending binding transition. It records approval and
opens future admission. It does not enable a disabled binding, activate a timer,
retry a failed product occurrence, or undo committed work. Only an explicit
user go-ahead authorizes this command; product installers must not invoke it.

When a product already owns a failure pause, transfer that gate under product
maintenance before permitting scheduled work:

```sh
clockwork binding halt owner/name --code legacy_failure --occurrence legacy/ID
```

This explicit halt bypasses the service-health delay and immediately retains
the failure incident without enabling or selecting work. An absent key becomes
a disabled tombstone. After the durable receipt,
the product may remove only its failure-owned scheduling gate. Keep product
failure evidence, item recovery state, user pauses, and maintenance holds.
Configuration remains product-owned; future scheduling failure enforcement is
Clockwork-owned. The migration does not infer old pauses by reading product
files or scanning old activation history.

## Retained evidence and feed

An abend retains `(key, occurrence)`, machine code, optional activation and
incident IDs, and recording time. Runtime failures use their activation as the
occurrence. The recorded abend remains immutable; later confirmation can create
an incident without rewriting that abend's optional incident ID. An incident
retains its UUID, key, first activation and selected
definition when available, code, occurrence, creation and resumption times.
There is at most one open incident per key. Admission tests that condition in
the same SQLite statement that records admission.

Incident list uses `items` and `has_more`, defaults to 20 rows, and accepts a
positive `--limit`. Show returns the selected retained incident. Its
notification status (`pending`, `accepted`, or `uncertain`), attempt times and
count describe Clockwork Email transport. Delegated acceptance belongs to EMT.
These are current local reads, not product readiness or final-delivery checks.

Incident feed reads insertion order with `items`, `next_cursor`, and
`has_more`; limits are 1 through 1000. Use `--after 0` to start at the beginning
of retained history. Save a cursor only after retaining all
page items. Cursors belong to this retained database history rather than
wall-clock time and must be re-established after restoring a different
history. Feed reads do not approve continuation or run products. Retained
incident rows are not deleted. No retention duration is promised.

## Privacy, recovery, and compatibility

Reports contain bounded identifiers only. Do not put source text, email body,
credentials, or an unbounded error in them. State and backups remain private
because keys, identifiers, timestamps, and launch metadata can identify work.
Direct storage mutation is unsupported. Back up `failure-checks.json` with the
incident database and notification sidecars. Preserve product evidence, pending
episodes and newer halt records during recovery; a pre-halt backup cannot
authorize resumed work. Do not run an older broker while the new sidecar exists.

Schema-one definitions retain their original digest and legacy failure
behavior even in the schema-two store. `failure_policy_active: false` exposes
that rollout gap until a schema-two definition is explicitly selected. Feature
contract, store schema, manifest schema, incident UUID, provider release, and
product occurrence identity remain separate. Existing incidents remain halted
and retain their current notification-check and approval rules. The new delay
applies to newly observed schema-two failures. No universal deprecation or
migration window is promised.

Read `clockwork.notifications` for alert eligibility, transport, and delegated
ownership; `clockwork.installation` for explicit schema migration; and
`clockwork.schedule.operate` for the incident operation procedure.
