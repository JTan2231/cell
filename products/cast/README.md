# Cast

Cast reads retained employers and public job postings and exports a consistent
snapshot for job selection. Collection has been removed.

## Example

With an initialized installation:

```sh
cast jobs list
cast export --json
```

New state uses accepted companies, jobs, workplaces, and source appearances.
The existing snapshot format remains available. Existing discovery state
remains readable without migration.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Product overview and feature inventory](chancery/overview.md)
- [Records and read handoff](chancery/manuals/discovery-explore.md)
- [State and current records](chancery/manuals/state.md)
- [Installation contract](chancery/manuals/installation.md)
- [Installation, configuration, and recovery procedure](chancery/manuals/install-operate.md)

Use `chancery product cast` for the installed overview, `chancery show ID` for
one contract, and `chancery resolve cast.install.operate` for the procedure and
its required feature contracts.
