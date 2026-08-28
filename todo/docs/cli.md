# CLI contract

## Command surface

```text
todo [--config PATH] [--database PATH] [--json] [--quiet] [-v...] COMMAND

todo init
todo new DIRECTION --source PATH [--quality low|medium|high] [--model MODEL]
todo list [--all] [--limit N]
todo search QUERY [--all] [--limit N]
todo show tN
todo note add tN TEXT|-
todo done tN
todo reopen tN
```

Global options work before or after the subcommand. Commands are
noninteractive. `note add ... -` reads one working note from standard input.

`list` and `search` return open todos by default. `--all` includes completed
todos; `all` is not a stored status. Results use a stable, deterministic order
and are bounded by `--limit`.

`done` and `reopen` are idempotent. Todo content and existing working notes
cannot be edited or deleted through the CLI.

## Creating a todo

`DIRECTION` is a short need or concern for the research liaison to
investigate. It need not be a title or complete specification.

`--source PATH` names the readable UTF-8 file where the need originated. It is
usually a Codex conversation transcript, especially when the caller is an
agent, but this is guidance rather than a file-format restriction. Todo stores
the resolved absolute source path on the resulting todo. It does not store,
copy, content-address, or later display the file contents. A real path is
required; standard input is therefore not accepted as a source.

The source and direction are immutable provenance. `show` reports both. It
does not reopen the source file.

`--quality` selects the liaison's reasoning preset and defaults to `high`.
`--model` overrides only the preset's model. Configuration may provide the
same defaults under `[liaison]`.

## Database and configuration selection

The development binary requires one database target. `--config PATH` selects a
config before `TODO_CONFIG`; then `--database PATH` selects a database before
`TODO_DATABASE`, which selects one before the configured database. A database
option may therefore override the database from a simultaneously selected
config while retaining that config's liaison settings. A relative database
path in a configuration file is resolved relative to that file. Todo never
silently creates `./todo.db`.

The installed macOS frontend supplies
`$HOME/Library/Application Support/Todo/config.toml` only when neither an
option nor an environment variable has selected a database or config.

A minimal strict configuration is:

```toml
database = "todo.db"

[liaison]
quality = "high"
# model = "an-exact-model-override"
```

Unknown fields and invalid quality values are errors. The deprecated
`liaison.codex` key remains parseable during the deployment rollback window but
is ignored; there is no direct-Codex fallback. Nucleus uses `NUCLEUS_SOCKET`
when set and its standard per-user socket otherwise.

## Output and errors

Human output is concise and terminal control characters from database, source,
or model text are escaped before rendering. `--quiet` suppresses successful
mutation acknowledgements but not query results or errors.

`--json` emits exactly one JSON response document: success goes to standard
output and failure goes to standard error. JSON is a CLI protocol; it is not
stored in SQLite. Successes have `ok: true` and a `data` value; failures have
`ok: false` and a stable error object. Diagnostics may still go to standard
error after a successful response; live liaison progress is suppressed when
JSON is selected.
