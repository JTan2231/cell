# Inspect and manage Todo identities

Todo separates captured concerns (`cN`), routing proposals (`rN`), durable todo
umbrellas (`tN`), situation assessments (`aN`), designs (`dN`), and working
notes (`nN`). IDs are not interchangeable.

## Read current and historical state

```sh
/Users/joey/.local/bin/todo concern list
/Users/joey/.local/bin/todo concern show <CONCERN_ID>
/Users/joey/.local/bin/todo routing show <ROUTING_ID>
/Users/joey/.local/bin/todo list
/Users/joey/.local/bin/todo search <QUERY>
/Users/joey/.local/bin/todo show <TODO_ID>
```

Todo list and search return open canonical umbrellas by default. `--all`
includes applicable completed and superseded history. `show` reports current
direction revision, attached concerns, notes, current assessment state,
designs, decisions, staleness, and supersession.

## Decide routing explicitly

The routing liaison proposes; the caller decides. To decide a pending proposal
after inspecting it:

```sh
/Users/joey/.local/bin/todo routing accept <ROUTING_ID> \
  --source <READABLE_UTF8_PATH>
/Users/joey/.local/bin/todo routing reject <ROUTING_ID> \
  --reason <TEXT> --source <READABLE_UTF8_PATH>
```

The source is authorization provenance, not model input. Todo verifies it as a
readable UTF-8 regular file and stores the canonical path. Acceptance
atomically rechecks the complete frozen basis. If the concern, candidates,
directions, proposal, or umbrella status changed, the proposal is stale and
must be reassessed. There is no `--force`.

Attachment does not revise an umbrella. Revision preserves identity and does
not merge. Unification preserves concerns, revisions, notes, and superseded
identifiers while choosing one canonical survivor.

## Notes and lifecycle

```sh
/Users/joey/.local/bin/todo note add <TODO_ID> <TEXT>
/Users/joey/.local/bin/todo done <TODO_ID>
/Users/joey/.local/bin/todo reopen <TODO_ID>
```

Notes are immutable. `done` sets umbrella status to `done`; `reopen` sets it to
`open`. Both commands are idempotent. Todo stores concerns, direction, assessments,
designs, notes and this open/done lifecycle.

These reads and deterministic writes use Todo SQLite directly and do not
invoke Nucleus. They can expose private directions, paths, notes, assessment
findings, and design content. Model completion or prose never substitutes for
the explicit decision commands.

## Rust callers

The provider crate exports `todo::api`: supported request and response
types, provider-owned envelope decoding, and an explicit-executable CLI client.
Use these types at imports and convert only to caller-local domain values.
The client performs the same operations under this contract and never adds
retry or authorization. See `todo/docs/rust-api.md`; the Rust structs and
enums define the interface without a separate declaration layer.

## Output selection

Todo list and search return compact ID, title and lifecycle rows. They default
to 20 results with `has_more`; use `--limit` with a positive integer for more.
Show returns the selected umbrella, concerns, notes and current assessment and
design summaries. Lifecycle and note changes return durable receipts.
