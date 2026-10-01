# Projects and bootstrap vocabulary

Semantics registers project folders under stable identities. A root marker
permits participation; it does not transfer runtime behavior authority to
Semantics. The participating product owns its root instructions, source, tests,
and current product behavior.

## Interfaces and records

```text
semantics project register ID ROOT
semantics project list
semantics project show ID
semantics project move ID NEW_ROOT
semantics project pause ID
semantics project resume ID
semantics project retire ID
semantics repository seed PROJECT --label LABEL --meaning MEANING [--grounding STATEMENT]
semantics repository seed-markdown PROJECT PATH
```

Use `semantics::api::Client` for the corresponding public operations and
provider-owned output types. The client preserves each command's effect and
authorization boundary. It does not invoke hidden cutover or worker operations.
Project list returns ID, canonical current path, lifecycle status, and HEAD.
Project show and operation receipts return the complete selected record.

IDs start with a lowercase letter and contain lowercase ASCII letters, digits,
and `-`. Register and move canonicalize a directory. They require the exact
line `Semantics-Project: ID` in that folder's regular root `AGENTS.md`.
Explain there that Semantics maintains terminology and history while source,
tests, and product documentation own runtime behavior.

The `semantics` CLI prints plain text by default. Pass `--json` for the existing
command-specific JSON schema. The typed client and pinned worker explicitly
request JSON. Output selection changes no records, effects, or exit statuses.

## Registration and activation

Registration binds the canonical root to one unique project identity and captures
the current opaque watermark of the configured Annals decisions library. It
excludes earlier accepted documents from automatic intake. Project identity,
path history, lifecycle state, and cursors are Semantics-owned durable records.
Annals owns library identity, source bytes, feed order, and opaque cursor meaning.

Every active or paused project must retain the selected library identity and its
activation and scan cursors. New projects capture their own watermark. Historical
Annals and Decisions cursor bytes are distinct and are never interchanged or
parsed by a caller. One-time migrated-database activation is an explicit
installation procedure in `semantics.project.operate`; ordinary registration or
an update does not choose a legacy watermark.

## Move, pause, resume, and retirement

A move changes the canonical root and appends path history. It preserves stable
project and concept IDs, repository history, and both Annals and legacy Decisions
activation and scan histories. The new root must carry the exact marker.

Pause prevents new semantic commits, including late proposals from an in-flight
job. It changes assigned pending intake to paused. It does not cancel a Nucleus
job, delete source content, or release maintenance or a Clockwork incident halt.
Resume revalidates the marker and permits eligible intake to proceed.

Retirement is permanent and requires a paused project. The former folder need
not exist. Retirement atomically marks pending or paused intake with zero attempts
and no retained Nucleus request as `ignored` with reason `project_retired`.
It retains source bytes, identities, repository history, and cursors and creates
no semantic revision or model receipt for that closure.

Attempted, processing, failed, correlated, or awaiting-review intake blocks
retirement. Refusal leaves the project and all intake unchanged. Resolve the
exact retained work through its recovery contract; do not erase a correlation
or force retirement through SQLite.

## Bootstrap seeding

Both seed forms are allowed only at revision 0. They commit one atomic revision
under the ordinary effect validation rules in `semantics.repository.explore`.
The Markdown form accepts a project-local definition-list source inside the
canonical root. It records a project-relative label and SHA-256 digest.

The seed file is required only during that command. After verifying repository
HEAD, it may be removed under the project's normal file-change authority.
Replay uses committed effects and does not reopen the file. Seeding is not a
way to overwrite or repair an existing repository.

## Success, failure, and privacy

A successful operation returns the selected durable project or seed result.
A marker, identity, lifecycle, or seed validation error leaves the refused
transition uncommitted. An unavailable Annals watermark blocks registration;
never invent one. Inspect project status and HEAD before repeating an uncertain
operation. Follow `semantics.project.operate` for procedures and verification.

These commands use local-user access and private Semantics state. Registration
reads the exact configured Annals feed. Seed reads the selected project-local
file. Reads and lifecycle operations do not themselves authorize a model job,
upstream mutation, or disclosure. Project paths, source labels, meanings, and
retained documents remain private.

Project and concept identity remain stable across moves and wording changes.
Provider release, persistent schema, and feature contracts have separate
versions. No registry capacity, activation latency, retention horizon, or future
migration/deprecation window is promised.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID, not arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.

## Related contracts

Read `semantics.repository.explore` for effect and replay meaning,
`semantics.reconciliation` for intake and jobs, and `semantics.service` for
readiness, admission, and installation.
