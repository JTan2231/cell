# Capture and research a concern for later

Use Todo to retain an actionable concern or follow-up for later. Normally
complete work now when the user requests immediate completion.

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
- dismiss with a supplied basis for retaining no actionable outcome; or
- defer with the missing routing input or unresolved user choice.

Research never applies the action. `todo new` therefore does **not** create or
revise a `tN`, even when the proposal recommends it. Inspect the records with:

```sh
/Users/joey/.local/bin/todo concern show <CONCERN_ID>
/Users/joey/.local/bin/todo routing show <ROUTING_ID>
```

## Failure and authority

Todo commits `cN` before Nucleus submission. It survives Nucleus, authentication,
model and later liaison failures. If research fails, inspect the retained
concern. Use `concern assess` to retry research without capturing it again.

Todo's validated tool result is the domain result. Nucleus owns runtime and raw
protocol state; model prose cannot accept routing. A person must later invoke
the provenance-bearing `routing accept` or `routing reject` command. Stale
proposals have no force path and must be researched again.

Todo stores source paths, not source bytes. The routing liaison can read
selected source content, and Nucleus can retain that content in raw job output.
Keep both systems inside the appropriate private boundary.

## Rust callers

Use `todo::api::Client` with provider-owned request and response types.
The client invokes an explicitly selected CLI and decodes its envelopes. It
preserves this operation's effects, failures, and authority requirements and
does not retry automatically. Convert results only to caller-local models.

## Output selection

Capture and routing preserve immutable concerns, sealed proposals and explicit
decisions. Concern list defaults to 20 rows with ID, status, recorded time and
a marked excerpt of at most 240 characters. Results include `has_more`; use
`--limit` with a positive integer for more. Concern show returns full provenance
and routing history.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
