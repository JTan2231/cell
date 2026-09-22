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

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Collect jobs](chancery/manuals/discovery-collect.md)
- [Read and export records](chancery/manuals/discovery-explore.md)
- [Installation, configuration, and recovery](chancery/manuals/install-operate.md)
- [Record terminology](docs/vocabulary.md)
