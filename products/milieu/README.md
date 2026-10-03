# Milieu

Milieu reads retained employers and public job postings and exports a consistent
snapshot for job selection. Collection has been removed.

## Example

With an initialized installation:

```sh
milieu jobs list
milieu export --json
```

New state uses accepted companies, jobs, workplaces, and source appearances.
The existing snapshot format remains available. Read the state contract for
supported storage and recovery.

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
- [Installation, initialization, and recovery procedure](chancery/manuals/install-operate.md)

Use `chancery product milieu` for the installed overview, `chancery show ID` for
one contract, and `chancery resolve milieu.install.operate` for the procedure and
its required feature contracts.
