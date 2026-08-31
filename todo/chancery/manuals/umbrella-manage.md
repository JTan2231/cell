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

The routing liaison can propose but never decide. After inspecting one pending
proposal, explicit deterministic commands are:

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

Notes are immutable working annotations. Done and reopen are idempotent status
transitions on the umbrella. They do not prove that an implementation plan ran
or that external work is complete. Todo deliberately does not model execution,
work items, or a general project graph.

These reads and deterministic writes use Todo SQLite directly and do not
invoke Nucleus. They can expose private directions, paths, notes, assessment
findings, and design content. Model completion or prose never substitutes for
the explicit decision commands.
