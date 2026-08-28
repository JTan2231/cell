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
todo email preview
todo email send [--scheduled]
```

Global options work before or after the subcommand. Commands are
noninteractive. `note add ... -` reads one working note from standard input.

`list` and `search` return open todos by default. `--all` includes completed
todos; `all` is not a stored status. Results use a stable, deterministic order
and are bounded by `--limit`.

`done` and `reopen` are idempotent. Todo content and existing working notes
cannot be edited or deleted through the CLI.

## Outstanding-todo email

`email preview` renders the exact current digest without reading
`RESEND_API_KEY` or making a network request. Human output contains `From`,
`To`, and `Subject` headers followed by the plain-text body. JSON data contains
`from`, `to`, `todo_count`, `subject`, `text`, and `html`.

`email send` sends that digest immediately through Resend. It requires a
nonblank `RESEND_API_KEY` with no surrounding whitespace in the process
environment and uses
`todo-email/<UUIDv7>` as its idempotency key. `email send --scheduled` also
sends immediately; it changes only the key to
`todo-daily-email/<LOCAL YYYY-MM-DD>`, identifying the most recent local 09:00
occurrence. Neither mode submits a Resend `scheduled_at` value. One invocation
freezes the body and key for up to three total attempts on transport failures,
`429`, or `5xx` responses.

Human success is `Sent N outstanding todos to ADDRESS (RESEND_ID)` and is
suppressed by `--quiet`. JSON data contains `email_id`, `idempotency_key`,
`scheduled`, `to`, and `todo_count`.

The digest contains all open todos, newest first, as ID and title only. Its
subject is `Todo: N outstanding`; the text body starts with
`Outstanding todos: N` and lists `- tN — Title`. When none are open, the body
is `No outstanding todos.` rather than skipping the occurrence. Email commands
require `[email]` configuration; other commands do not.

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

Email delivery is optional. For example, the current user deployment may add:

```toml
[email]
from = "todo@joeytan.dev"
to = "j.tan2231@gmail.com"
```

Those addresses are deployment configuration, not product defaults. `from`
and `to` are required, nonblank single-line strings when `[email]` is present.
The sender domain must be verified in Resend. The API key is environment-only;
it is not a supported TOML field.

Unknown fields, incomplete email configuration, and invalid quality values are
errors. The deprecated
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
