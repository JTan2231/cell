# Cell

A collection of multi-agent and knowledge infrastructural tools and experiments.

I find that a lot of human-shaped problems resist the fixedness of relational
database schemas, so I'm trying a more bureaucratic approach. Accordingly, most
of the workflows here involve Markdown paperwork shuffling between agents and
their respective realms of concern.

## Check

```sh
./ci.sh
```

To check one product while iterating, for example:

```sh
./ci.sh nucleus
```

Product gates are synchronous clients of one host-wide CI broker. Linked Git
worktrees share a single Cargo target and one heavy execution lane, so any
number of agents may request CI without multiplying compiler work or writable
targets. Requests queue fairly; an exact clean candidate may join identical
work already in flight, and a result is rejected as stale if its source changes
during execution. CI requires Python 3.10 or newer. See
[the broker contract](ci_broker/README.md).

Repeated product CI and release mechanics are declared in checked-in
[pipeline descriptors](pipeline/README.md). Selector-only deployment mechanics
and optimistic `current` checks are generated from the
[deployment profile](deployment/README.md); stateful products retain their own
lifecycle logic.

[Usher](usher/README.md) checks declared Cell membership: product identity,
Semantics participation, and Chancery presence. Every root CI invocation runs
the check. After building, `target/release/usher report .` shows each product's
evidence and any missing introductions.

## Further documentation

Each product directory has its own README and more specific documentation. For
shared topology, compatibility, and safe change ordering, start with the
[Nucleus ecosystem operator manual](nucleus/docs/operator-manual.md).
