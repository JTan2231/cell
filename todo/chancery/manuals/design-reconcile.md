# Reconcile a Todo desired-state design

Design reconciliation starts from the latest **current ready** situation
assessment for one open canonical todo. It specifies normative desired state;
it is deliberately not an implementation plan or execution record.

```sh
/Users/joey/.local/bin/todo design propose <TODO_ID>
/Users/joey/.local/bin/todo design show <DESIGN_ID>
```

Todo resolves and rechecks the exact `aN`; the caller does not select an older
assessment. The design liaison receives that assessment, current direction,
and accepted predecessor design if any. There are no external research tools:
new facts belong in a new assessment.

## Design contents

Every proposed operation cites the closed catalog derived from direction,
assessment, predecessor, and correction records. Jurisdiction transitions are
explicitly `keep`, `move`, `add`, or `retire`, with exactly one owner in each
nonempty responsibility set.

Clauses cover all nine kinds:

- ownership;
- boundary;
- state;
- interface;
- lifecycle;
- failure;
- compatibility;
- acceptance; and
- non-goal.

A design is ready only when no active choices remain, every clause kind is
present, all direction boundaries are covered, and all active predecessor
operations are addressed. It cannot contain implementation tasks, file edits,
commands, sequencing, estimates, deployment actions, or execution claims.

If the assessment is insufficient or stale, the liaison records a durable
return for assessment rather than inventing a design. If a run ends with an
open draft, Todo marks it abandoned; it remains inspectable and may be
corrected but is not ready.

## Correction and decision

```sh
/Users/joey/.local/bin/todo design correct <DESIGN_ID> <FEEDBACK>
/Users/joey/.local/bin/todo design accept <DESIGN_ID> \
  --source <READABLE_UTF8_PATH>
/Users/joey/.local/bin/todo design reject <DESIGN_ID> \
  --reason <TEXT> --source <READABLE_UTF8_PATH>
```

Correction stores exact feedback and predecessor provenance and creates a
successor; it never edits the named design. It is supported for ready,
rejected, or abandoned proposals whose exact assessment remains current.

Accept and reject bypass Nucleus. They require readable UTF-8 authorization
provenance. Acceptance atomically rechecks the ready design, open canonical
umbrella, direction, attachments, and assessment currentness. A newer
assessment blocks both correction and acceptance against the older basis.

Acceptance stops at desired state. It creates no task, implementation job,
deployment, or completion claim. Todo state is authoritative; model prose and
Nucleus completion cannot decide the design.
