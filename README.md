# Cell

A collection of tools and experiments for multi-agent systems and knowledge management.

I find that fixed relational database schemas do not fit many human problems.
I am trying a more bureaucratic approach. Most workflows here pass Markdown
documents between agents with different responsibilities.

## Check

```sh
./ci.sh
```

By default, root CI checks products with staged, unstaged, or nonignored
untracked changes relative to `HEAD`. It uses the product roots in the pipeline
descriptors. A changed product descriptor also selects that product. Deletions
and both paths of a rename count.

To check a product even when it has no outstanding changes:

```sh
./ci.sh nucleus
```

Use the default command for routine validation. Run full CI only when the user
explicitly requests it; agents must not add `--all` on their own.

When full CI is requested, run every product gate and integrated catalog validation:

```sh
./ci.sh --all
```

`--all` cannot be combined with product names. Add `--verbose` to any mode for
detailed output. Every mode runs the common pipeline preflight and Usher
recognition check, even when no products are selected. Shared or unowned
changes are reported but do not select more products. Root CI does not add
dependent products. Its result reports the selected scope; use `--all` for
full validation.

Product gates use one host-wide CI broker and wait for its result. Linked Git
worktrees share one Cargo target and one heavy execution lane. Agents can
request CI without creating separate compiler work or writable targets.
Requests use a fair queue. An exact clean candidate can join identical work
already in progress. Source or Git status changes during planning or execution
are rejected as stale. CI requires Python 3.10 or newer. See
[the broker contract](ci_broker/README.md).

The checked-in [pipeline descriptors](pipeline/README.md) define shared product
CI and release operations. Usher uses a separate Rust
`usher-install` executable backed by the shared `cell-install` library.
Conversations and CRM use generated selector-only installers;
stateful products retain their own lifecycle logic. See
[deployment](deployment/README.md) for their installation boundaries.

`./deploy.sh SYSTEM...` prepares and deploys selected systems from one committed
local `main` snapshot. The coordinator runs in the foreground. It stages tested
binaries, holds and drains affected products, and invokes their installers and
readiness checks. It uses product-owned recovery before it removes temporary
run state. See
[deployment and initial migration](deployment/README.md)
and the [shared operator manual](nucleus/docs/operator-manual.md).

[Usher](usher/README.md) checks declared Cell membership: product identity,
Semantics participation, and Chancery presence. Every root CI invocation runs
the check. After building, `target/release/usher report .` shows each product's
evidence and any missing introductions.

## Further documentation

Each product directory has a README and detailed documentation. For shared
topology, compatibility, and the sequence of changes, start with the
[Nucleus ecosystem operator manual](nucleus/docs/operator-manual.md).
