# Semantics

Semantics maintains terminology and its history for registered project folders.
It reads accepted documents from Annals and uses Nucleus to propose changes.
Semantics validates each change before it appends a repository revision.

## Example

```sh
semantics project list
semantics repository show PROJECT
semantics repository search PROJECT TERM
```

Repository reads describe maintained meaning. Product source, tests, and
current product documentation define runtime behavior.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

Read `chancery product semantics` for the installed overview and inventory,
`chancery show ID` for one feature or procedure, and `chancery resolve ID` for
its required contract reading.

- [Product overview](chancery/overview.md)
- [Repository terminology and history](chancery/manuals/repository-explore.md)
- [Project participation and identity](chancery/manuals/projects.md)
- [Accepted-document reconciliation](chancery/manuals/reconciliation.md)
- [Service and installation guarantees](chancery/manuals/service.md)
- [Operation procedures](chancery/manuals/project-operate.md)
- [Development procedure](chancery/manuals/develop-change.md)
