# CLI

The installed command is `~/.local/bin/semantics`. Output is JSON; `--json`
selects compact JSON. Errors sent with `--json` have
`{"ok":false,"error":{"code":"...","message":"..."}}` on stderr.

The default database is
`~/Library/Application Support/Semantics/semantics.db`. Use
`--database ABSOLUTE_PATH` or `SEMANTICS_DATABASE` for an isolated database.

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
captures the current Decisions lifecycle watermark. Move preserves the stable
project identity and both activation and scan cursors.

Pause prevents new semantic commits and changes assigned pending intake to
paused. Resume revalidates the marker. Retirement is permanent, is allowed
only from paused, and refuses unresolved assigned intake.

The hidden `--activation-cursor` registration option is for controlled tests or
recovery only. Never manufacture or parse a Decisions cursor.

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

`semantics --json intake run` is the private one-shot worker interface used by
launchd. It is intentionally hidden from normal help.

## Readiness

```text
semantics doctor
semantics --json doctor
```

Doctor checks SQLite schema 1, every non-retired project's exact marker, the
installed Decisions lifecycle watermark, Conversations exact-cwd readiness,
and Nucleus health, required capabilities, schemas, and the immutable Semantics
toolset. It exits nonzero if any check fails.
