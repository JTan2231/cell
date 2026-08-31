# Capture and research a concern for later

Use Todo when an actionable concern or follow-up should be retained for later.
Do not use it merely because work is actionable: work requested for immediate
completion should normally be done now.

Todo preserves the place where the concern arose, then separately researches
which durable todo identity, if any, it belongs to. The standard command is:

```sh
/Users/joey/.local/bin/todo new \
  --source <READABLE_UTF8_PATH> \
  <DIRECTION>
```

`--source` must name an existing readable UTF-8 regular file relevant to where
the need arose. Use the actual originating artifact or transcript available in
the task context. Do not create a Markdown checkbox, arbitrary placeholder
file, or unrelated source merely to satisfy the option.

## Exact durable boundary

`todo new` is a convenience for two distinct steps:

```sh
/Users/joey/.local/bin/todo concern add \
  <DIRECTION> --source <READABLE_UTF8_PATH>
/Users/joey/.local/bin/todo concern assess <CONCERN_ID>
```

The first step validates and canonicalizes the source path and commits one
`cN` concern containing the caller's direction and provenance. It performs no
model work and makes no identity decision.

The second step freezes the concern and a bounded snapshot of plausible todo
umbrellas, then asks a constrained routing liaison to record one pending `rN`.
The proposed action is one of:

- attach the concern to an unchanged existing `tN`;
- create a new durable `tN`;
- revise the direction of one enduring `tN`;
- unify exactly two historical identities under one survivor;
- dismiss when positive evidence establishes no actionable outcome; or
- defer because evidence or a material user choice is insufficient.

Research never applies the action. `todo new` therefore does **not** create or
revise a `tN`, even when the proposal recommends it. Inspect the records with:

```sh
/Users/joey/.local/bin/todo concern show <CONCERN_ID>
/Users/joey/.local/bin/todo routing show <ROUTING_ID>
```

## Failure and authority

The `cN` commit happens before Nucleus submission and survives Nucleus,
authentication, model, or later liaison failure. If research fails, preserve
and inspect that concern rather than blindly running `todo new` again. Research
can be retried for the existing concern through `concern assess`.

Todo's validated tool result is the domain result. Nucleus owns runtime and raw
protocol state; model prose cannot accept routing. A person must later invoke
the provenance-bearing `routing accept` or `routing reject` command. Stale
proposals have no force path and must be researched again.

Todo stores source paths, not source bytes. The routing liaison can read
selected source content, and Nucleus can retain that content in raw job output.
Keep both systems inside the appropriate private boundary.
