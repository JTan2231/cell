# Cast

Cast discovers employers and collects job postings through Rust code and HTTP.
It stores records in a private SQLite database and exports a consistent JSON
snapshot for downstream job selection.

```sh
./ci.sh cast
<TESTED_CAST_INSTALL> install --binary <TESTED_CAST_BINARY> \
  --bundle /Users/joey/rust/cell/cast/chancery
cast init
cast run
cast export --json
```

Collection uses configured TheirStack, Brave and Hacker News searches, followed
by supported public careers sources. After collection, Cast stores incoming job
fields only when the title contains `engineer`, ignoring case. This includes
`Engineering Manager`. Company and source records, collection progress and
request charges are retained. Existing jobs are not removed by this filter.
Cast stores collection timestamps, query limits, failures and budget deferrals.
Collection does not call models, use computer controls, update CRM, prepare
applications or send email.

The installed frontend reads only `THEIRSTACK_API_KEY` and
`BRAVE_SEARCH_API_KEY` from `~/.zshrc`. It removes unrelated environment variables
before it starts the payload. It preserves an explicit `CAST_STATE_DIR`. Discovery state
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
