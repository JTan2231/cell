# Semantics

Semantics maintains one authoritative, append-only vocabulary for each
participating project folder. It consumes durable Decisions lifecycle events,
uses Conversations only to resolve an event's exact thread working directory,
and asks Nucleus to propose one typed reconciliation when a decision is
effective. Semantics—not the model and not Nucleus—validates and commits the
result.

Participation is explicit. A registered folder must contain an exact line in
its root `AGENTS.md`:

```text
Semantics-Project: project-id
```

The central SQLite database stores the project registry, immutable semantic
revisions, intake state, and durable Nucleus correlations. Project files are
never rewritten by the worker.

Start with [the documentation map](docs/README.md), then see the
[CLI reference](docs/cli.md) or [user installation guide](docs/system-installation.md).

```sh
./ci.sh
```

Product CI is offline and uses synthetic state and fake service boundaries.

`./release.sh --patch|--minor|--major` is the separately authorized Git
publication path. It requires clean synchronized `main`, runs product CI,
commits the version bump, creates `semantics-v*`, and atomically pushes the
commit and tag. It does not deploy the installed service.
