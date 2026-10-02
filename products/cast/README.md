# Cast

Cast discovers employers, collects public job postings, and exports a consistent
snapshot for job selection.

## Example

With an initialized, configured installation:

```sh
cast run
cast job collect 'https://jobs.ashbyhq.com/company/job-id'
cast export --json
```

Collection uses the configured providers and request budgets. The export
contains stored companies, jobs, collection outcomes, and coverage.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Product overview and feature inventory](chancery/overview.md)
- [Collection contract](chancery/manuals/discovery-collect.md)
- [Records and read handoff](chancery/manuals/discovery-explore.md)
- [State and collection policy](chancery/manuals/state.md)
- [Installation contract](chancery/manuals/installation.md)
- [Installation, configuration, and recovery procedure](chancery/manuals/install-operate.md)

Use `chancery product cast` for the installed overview, `chancery show ID` for
one contract, and `chancery resolve cast.install.operate` for the procedure and
its required feature contracts.
