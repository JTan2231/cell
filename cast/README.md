# Cast

Cast discovers previously unknown employers and collects job postings using
ordinary Rust code and HTTP. It retains private SQLite evidence and exports a
consistent JSON snapshot for a separate job-selection workflow.

```sh
./ci.sh cast
<TESTED_CAST_INSTALL> install --binary <TESTED_CAST_BINARY> \
  --bundle /Users/joey/rust/cell/cast/chancery
cast init
cast run
cast export --json
```

Collection uses configured TheirStack, Brave and Hacker News searches, followed
by supported public careers sources. Query/source coverage, freshness, failures
and budget deferrals remain visible. There are no routine model calls,
computer-use calls, CRM updates, application packets or email sends.

The installed frontend reads only `THEIRSTACK_API_KEY` and
`BRAVE_SEARCH_API_KEY` from `~/.zshrc` and scrubs unrelated environment before
starting the payload. Explicit `CAST_STATE_DIR` is preserved. Discovery state
defaults to `~/.local/share/cast`; `--state-dir PATH` selects isolated state.
No recurring schedule is activated by installation or the commands above.

- [Collection contract](chancery/manuals/discovery-collect.md)
- [Read and export contract](chancery/manuals/discovery-explore.md)
- [Installation, config and recovery](chancery/manuals/install-operate.md)
- [Vocabulary](docs/vocabulary.md)

Product CI runs formatting, linting, Rust tests, Chancery validation and the
macOS installation/credential-wrapper regression. Root CI also checks Cell
membership and the integrated provider graph. Live provider checks use a
separately chosen private state directory and explicit request budget.
