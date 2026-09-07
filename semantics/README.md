# Semantics

Semantics maintains an authoritative, append-only vocabulary for each
participating project folder. It reads durable accepted-account events from one
dedicated Annals decisions library. Conversations resolves each account's exact
authority-thread working directory. Nucleus proposes a typed reconciliation;
Semantics validates and commits it. Legacy Decisions intake remains available
for replay and recovery. New intake comes from Annals.

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
selected ID) to read its contract and declared gaps.

```sh
./ci.sh
```

Product CI is offline and uses synthetic state and fake service boundaries.

`./release.sh --patch|--minor|--major` is the separately authorized Git
publication path. It requires clean synchronized `main`, runs product CI,
commits the version bump, creates `semantics-v*`, and atomically pushes the
commit and tag. It does not deploy the installed service.
