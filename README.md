# Cell

Cell is the source monorepo for the local Nucleus ecosystem. It coordinates
changes across the shared execution substrate and its first-party requesters
without combining their runtime or domain authorities.

The repository contains four independently versioned release units in three
product directories:

- `nucleus/`: the Nucleus CLI, daemon, typed client, portable contract, store,
  and exact Codex adapter;
- `annals/`: Annals and the independently versioned Annals Usage reporting
  projection; and
- `todo/`: the synchronous Todo CLI and its database.

Nucleus owns agent admission, execution, authentication, job history, and the
durable requester-tool mailbox. Annals and Todo continue to own their domain
state and success rules. Codex remains an external harness rather than source
vendored into Cell. Start with [`nucleus/docs/operator-manual.md`](nucleus/docs/operator-manual.md)
for shared topology, compatibility, and safe change ordering.

## Build and check

Cell is one Cargo workspace with one lockfile:

```sh
cargo build --workspace --locked
```

Each product retains its own complete CI gate and 60-second budget. The root
dispatcher has no ecosystem-wide 60-second deadline:

```sh
./ci.sh
./ci.sh nucleus
./ci.sh annals
./ci.sh todo
```

Running `./ci.sh` without selectors runs the three product gates sequentially.
A shared Nucleus contract or client change must pass all three gates. A purely
domain-local change may use its product gate while iterating, but the complete
root gate is the final repository check.

## Versions and releases

Nucleus's six crates share one Nucleus version. Annals, Annals Usage, and Todo
keep independent versions and release scripts. Source colocation does not imply
lockstep release or deployment, a shared state directory, or a shared database.
New releases use product-qualified tags (`nucleus-v*`, `annals-v*`,
`annals-usage-v*`, and `todo-v*`). Pre-Cell release tags remain available in
the original product repositories.
