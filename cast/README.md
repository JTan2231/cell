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

## Check

From the Cell root:

```sh
./ci.sh cast
```

## Further documentation

- [Collect jobs](chancery/manuals/discovery-collect.md)
- [Read and export records](chancery/manuals/discovery-explore.md)
- [Installation, configuration, and recovery](chancery/manuals/install-operate.md)
- [Record terminology](docs/vocabulary.md)
