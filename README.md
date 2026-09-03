# Cell

Cell is the source monorepo for the local Nucleus ecosystem. It coordinates
changes across the shared execution substrate and its first-party requesters
without combining their runtime or domain authorities.

The repository contains thirteen independently versioned release units in
twelve product directories:

- `nucleus/`: the Nucleus CLI, daemon, typed client, portable contract, store,
  and exact Codex adapter;
- `annals/`: Annals and the independently versioned Annals Usage reporting
  projection;
- `todo/`: the synchronous Todo CLI and its database;
- `chancery/`: the read-only installed capability catalog, contract reader,
  and exact-ID outward-promise resolver;
- `weaver/`: the durable five-stage public-facing narrative requester;
- `email/`: the synchronous fixed-recipient plain-text Email CLI, which sends
  directly through Resend;
- `conversations/`: the read-only machine-wide Codex task-history CLI and
  reusable App Server adapter;
- `decisions/`: the continuous enacted-decision observer, durable review
  projection, and daily decision-email scheduler;
- `semantics/`: the registered-folder semantic repository service and
  constrained decision-reconciliation requester;
- `geste/`: the manual episode casebook and precedent-reporting CLI;
- `pratica/`: the durable system-integration terms negotiation and agreement
  CLI; and
- `clockwork/`: the current-user broker for immutable scheduled non-agent
  activations and their runtime history.

Nucleus owns agent admission, execution, authentication, job history, and the
durable requester-tool mailbox. Annals, Todo, Weaver, Decisions, Semantics,
Geste, and Pratica continue to own their domain state and success rules.
Clockwork owns stable schedule bindings, direct-child supervision, and runtime
history, while each scheduled product retains its work, retries, secrets,
logs, and domain-success rule.
Conversations reads the normal-user Codex App Server and owns no projection
state. Email is independent of Nucleus and calls Resend directly. Codex remains
an external harness rather than source vendored into Cell. Start with
[`nucleus/docs/operator-manual.md`](nucleus/docs/operator-manual.md) for shared
topology, compatibility, and safe change ordering.

## Build and check

Cell is one Cargo workspace with one lockfile:

```sh
cargo build --workspace --locked
```

Each product retains its own complete CI gate. The root dispatcher invokes a
selected product gate or the full ecosystem gate:

```sh
./ci.sh
./ci.sh nucleus
./ci.sh annals
./ci.sh todo
./ci.sh chancery
./ci.sh weaver
./ci.sh email
./ci.sh conversations
./ci.sh decisions
./ci.sh semantics
./ci.sh geste
./ci.sh pratica
./ci.sh clockwork
```

Running `./ci.sh` without selectors runs the twelve product gates sequentially.
A shared Nucleus contract or client change must pass all twelve gates. A purely
domain-local change may use its product gate while iterating, but the complete
root gate is the final repository check. A complete run also assembles all
thirteen source provider bundles and requires Chancery doctor and list to accept
their combined dependency graph. It requires every provider to publish a
schema-3 promise scope and every one of the 47 indexed entries to publish a
complete normalized promise without undeclared facets. It also proves the
Decisions lifecycle pilot resolves as `resolved_not_ready` while retaining its
explicit unspecified guarantees, and that the Annals Usage stress pilot
reports its known uncontracted upstream reliances rather than hiding them.

## Versions and releases

Nucleus's six crates share one Nucleus version. Annals, Annals Usage, Todo,
Chancery, Weaver, Email, Conversations, Decisions, Semantics, Geste, Pratica,
and Clockwork keep independent versions and release scripts. Source colocation does not imply
lockstep release or deployment, a shared state directory, or a shared database.
New releases use
product-qualified tags (`nucleus-v*`, `annals-v*`, `annals-usage-v*`,
`todo-v*`, `chancery-v*`, `weaver-v*`, `email-v*`, `conversations-v*`,
`decisions-v*`, `semantics-v*`, `geste-v*`, `pratica-v*`, and
`clockwork-v*`). Pre-Cell release tags remain available in the original product
repositories.
