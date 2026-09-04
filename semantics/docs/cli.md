# CLI

The installed command is `~/.local/bin/semantics`. Output is JSON; `--json`
selects compact JSON. Errors sent with `--json` have
`{"ok":false,"error":{"code":"...","message":"..."}}` on stderr.

The default database is
`~/Library/Application Support/Semantics/semantics.db`. Use
`--database ABSOLUTE_PATH` or `SEMANTICS_DATABASE` for an isolated database.
Semantics invokes Annals with an explicit decisions-library config. The
installed default is
`~/Library/Application Support/Annals/decisions/config.toml`; set
`SEMANTICS_ANNALS_CONFIG` only to an absolute alternate path and
`SEMANTICS_ANNALS` only to an alternate executable.

## Projects

```text
semantics project register ID ROOT
semantics project list
semantics project show ID
semantics project move ID NEW_ROOT
semantics project pause ID
semantics project resume ID
semantics project retire ID
```

IDs start with a lowercase letter and contain only lowercase ASCII letters,
digits, and `-`. Register and move canonicalize a directory and require the
exact root marker `Semantics-Project: ID` in a regular `AGENTS.md`. Registration
captures the current accepted-account watermark from the exact configured
Annals decisions library. Move preserves the stable project identity and both
the Annals and legacy Decisions cursor histories.

Pause prevents new semantic commits and changes assigned pending intake to
paused. Resume revalidates the marker. Retirement is permanent, is allowed
only from paused, and refuses unresolved assigned intake.

For a retained schema-one database, activation is requested only through the
deployer's explicit `--final-decisions-watermark OPAQUE_CURSOR` option after
legacy append is stopped and external cutover gates are held. The internal
candidate command requires that exact value, proves all non-retired legacy
scan cursors match it, rejects pending/processing legacy work and active or
ambiguous legacy Nucleus jobs, captures one Annals watermark, and commits all
new cursors atomically. It has no default activation mode. Already activated
schema-two updates omit the option and retain their identity and cursors.
The deployer requires `--keep-maintenance` with the one-time watermark so the
local commit ends in an authenticated Semantics-owned hold; a later successful
invocation of the same release without either option releases only that hold.
Never manufacture or parse either cursor kind. Hidden Annals registration
overrides exist only for isolated tests.

## Repository

```text
semantics repository show PROJECT [--revision N]
semantics repository search PROJECT QUERY [--revision N]
semantics repository log PROJECT [--from N] [--to N]
semantics repository diff PROJECT FROM TO
semantics repository seed PROJECT --label LABEL --meaning MEANING [--grounding STATEMENT]
semantics repository seed-markdown PROJECT PATH
```

`show` replays HEAD unless a revision is selected. `search` matches
case-insensitive label or meaning text. `log` and `diff` return immutable
revision records rather than a mutable projection.

Seed commands are bootstrap-only and refuse a project that already has a
revision. `seed-markdown` accepts a project-local definition-list source,
records its project-relative source label and SHA-256 digest, and commits one
atomic revision. The source is needed only for that command: after verifying
repository HEAD, it may be removed under the project's normal file-change
authority. Replay uses the committed effects and never reopens the seed file.

## Intake

```text
semantics intake status [--status STATUS]
semantics intake assign EVENT_ID PROJECT
semantics intake retry EVENT_ID
```

Statuses are `unassigned`, `pending`, `awaiting_review`, `paused`,
`processing`, `applied`, `ignored`, and `failed`. Assignment is an audited
manual routing correction and revalidates the target marker. Retry applies to
failed intake and refuses an active or ambiguous prior Nucleus job.
`intake status` returns separate `annals_decision_accounts` and
`legacy_decisions` collections. New accounts never use `awaiting_review`;
legacy rows retain all old states and decoding. New account rows expose a
fixed `routing_outcome` and project assignment, never the transient resolved
cwd or raw dependency diagnostics.

`semantics --json intake run` is the private one-shot worker interface selected
by the installed Clockwork `semantics/worker` definition. It is intentionally
hidden from normal help. Clockwork owns activation and process history only;
this report and durable Semantics state own the domain result.

## Readiness

```text
semantics doctor
semantics --json doctor
```

Doctor checks SQLite schema 2, every non-retired project's exact marker, the
explicit Annals decisions config and feed/library identity, Conversations
exact-cwd readiness, and Nucleus health, required capabilities, preserved
legacy schemas, and the successor immutable account toolset. It captures one
fixed Annals watermark and, from every distinct installed scan cursor, reads
and identically replays each bounded page until an unchanged empty page. It
rejects page cycles, nonadvancement, duplicate identities, changed replay, and
more than 1,000 pages from one cursor. A migrated
database with an active or paused project fails readiness until every such
project has the selected Annals identity and activation/scan cursors. An empty
database may report `activation pending; no active or paused projects` because
there is no consumer cursor to skip; its first project registration captures
the then-current watermark. Doctor exits nonzero if any check fails.
