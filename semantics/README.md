# Semantics

Semantics maintains one authoritative, append-only vocabulary for each
participating project folder. It consumes durable accepted-account events from
one dedicated Annals decisions library, uses Conversations only to resolve an
account's exact authority-thread working directory, and asks Nucleus to propose
one typed reconciliation. Semantics—not Annals, the model, or Nucleus—validates
and commits the result. Legacy Decisions intake remains preserved for replay
and recovery but is no longer the future feed.

On macOS, Clockwork owns the recurring process activation for the immutable
`semantics/worker` definition. Semantics still owns worker serialization,
intake state, recovery, validation, and every repository commit.

Participation is explicit. A registered folder must contain an exact line in
its root `AGENTS.md`:

```text
Semantics-Project: project-id
```

The central SQLite database stores the project registry, immutable semantic
revisions, intake state, and durable Nucleus correlations. Project files are
never rewritten by the worker.

Start with [the documentation map](docs/README.md), then see the
[CLI reference](docs/cli.md), [user installation guide](docs/system-installation.md),
or [Semantics provider bundle](chancery/provider.json). After selecting an exact
Semantics entry, use `chancery resolve semantics.repository.explore` (or the
selected ID) to inspect its normalized outward boundary and explicit gaps.

```sh
./ci.sh
```

Product CI is offline and uses synthetic state and fake service boundaries.

`./release.sh --patch|--minor|--major` is the separately authorized Git
publication path. It requires clean synchronized `main`, runs product CI,
commits the version bump, creates `semantics-v*`, and atomically pushes the
commit and tag. It does not deploy the installed service.
