# Record and inspect agent command usage

Chancery owns one private SQLite command-usage journal and the `chancery-usage`
Rust crate. There is no daemon or network transport. Products invoke the library
at their agent command boundary. Automatic workers, hooks and product-owned
dependency calls are excluded. The journal records observed invocation, not completion,
domain success, model consumption, arguments, or output.

## Registration interfaces

A participating program's exact `PROGRAM --register-usage` mode initializes
only an empty supported journal through Chancery's owning library and adds that
program's full declared inventory. It runs no product work and is not itself
observed. Existing identities and history remain. Unsupported or foreign state
fails; product binaries never migrate a journal. Registration is separate from
copying or selecting binaries and must be repeated when a release adds commands.

`chancery usage init` initializes supported empty state and registers Chancery's
commands. `chancery usage register SYSTEM COMMAND_ID...` requires initialized
state and adds the named system and commands. Each identity is idempotent. A
later error can leave earlier registrations committed. Historical commands
removed from a later release remain registered; this is not proof of current
installation or instrumentation. Read `chancery show chancery.usage.operate`
for registration and backup/restore procedures.

## Writer API and attribution

Rust callers import `chancery_usage`. `Store::initialize` is the owning
initialization interface; `Store::open` and ordinary recording never initialize
or migrate. `Store::read` opens supported state for read queries.
`register_system` and
`register_command` add identities separately. Free registration functions open
already initialized state through `default_path`; they do not initialize it.
`record(system, command)` reads `CODEX_THREAD_ID` and returns
`Result<Option<i64>>`: a committed row ID or `None` when skipped. Missing or empty
thread attribution,
`CHANCERY_USAGE_INTERNAL=1`, and `CHANCERY_USAGE_DISABLED=1` skip recording before
path resolution or database access. `record_with_thread(system, command, thread)`
and `Store::record` require an explicit nonempty `&str` thread and return a row
ID. They do not consult environment flags. Their caller owns agent attribution.
Services must pass
request-scoped correlation; a daemon's startup environment does not identify
its later callers. All these functions return a result. `observe` is the
command adapter: it reports a bounded error on stderr and preserves execution.

The public `chancery_usage::cli` adapter exposes `command_ids(command, prefix)`,
`parse_command_from(arguments, prefix)`, `registration_requested()`,
`register_ids(system, ids)`, `registration_exit(system, ids)`,
`register_if_requested(system, prefix)`, `try_parse(system, prefix)`, and
`parse(system, prefix)`. The generic parser helpers use the caller's Clap
`Parser` type. `command_ids` enumerates the full declared inventory.
`parse_command_from` parses without recording and returns the parsed value and
optional canonical command ID. Products that need further dispatch can use
this boundary before calling `observe`. `try_parse` handles registration mode,
parses, and observes a valid selected command once; `parse` retains Clap's exit
handling. Registration mode is recognized only when the complete argument list
is exactly `--register-usage`. `register_ids` uses the owning initialization
API and registers the supplied identities; the exit adapter returns exit 0 on
success or a bounded diagnostic and exit 1 on failure.

The Clap adapter uses canonical declared subcommand names joined with dots.
Aliases do not create identities. Arguments, flags, external subcommand values,
help, version and invalid command syntax produce no identities or usage rows.
Annals resolves named-library dispatch before recording the underlying command.
Annals Usage uses system `annals` and command prefix `usage.`. Krisis uses Cell's
stable product ID `decisions`. Email's positional send is command `send`.
Public `status-snapshot` calls count once. Internal helpers and direct library
calls are not implicitly observed; library consumers call the explicit API
when they own a command boundary. No parent-child execution tree is inferred.

Product-owned dependency processes set `CHANCERY_USAGE_INTERNAL=1` on the child.
A wrapper for the same command preserves thread attribution and this marker
through environment scrubbing. Clockwork excludes `__launchd` and `__exec`
from observation and marks its product child internal. The Krisis Stop hook
marks its invocation internal. Public worker commands remain observable when
an agent invokes them directly.

Nucleus clears inherited `CODEX_THREAD_ID` and `CHANCERY_USAGE_INTERNAL` when
launching a new agent. That agent supplies its own thread for its commands.
`CHANCERY_USAGE_DISABLED` remains in effect. Agent execution started by an
automation is eligible; its surrounding mechanical work is not. These are
cooperative instrumentation rules, not caller authentication.

## Records

The database has three application tables:

| Table | Fields and interpretation |
| --- | --- |
| `systems` | `id`: one explicitly registered stable system identity. |
| `commands` | `system_id`, `id`: a command identity unique within its system. |
| `usage` | `id`, `recorded_at`, `codex_thread_id`, `system_id`, `command_id`: one observed command-handler entry. |

A foreign key binds each usage row to its registered command and system.
Unknown identities fail recording. Repeated invocations append separate rows.
The API and database triggers prohibit updating or deleting usage or reassigning
registered identities. There is no retention cleanup or pruning command.
`recorded_at` is the writer's whole Unix-second timestamp. `id` orders committed
inserts within this database history; wall-clock time does not define ordering.
New writes require thread attribution; existing nullable rows remain readable.
A supplied ID must contain
1–128 ASCII letters, digits, dots, dashes or underscores; IDs are opaque metadata
and are not evidence of caller authorization or a verified Codex history record.

## Read

```text
chancery [--json] usage systems
chancery [--json] usage commands [--system SYSTEM] [--thread THREAD | --unattributed] [--since UNIX_SECONDS] [--until UNIX_SECONDS]
chancery [--json] usage events [--system SYSTEM] [--thread THREAD | --unattributed] [--since UNIX_SECONDS] [--until UNIX_SECONDS] [--after ID] [--limit COUNT]
```

```sh
chancery usage systems
chancery usage commands --system annals --since 1700000000
chancery usage commands --thread THREAD_ID
chancery usage events --thread THREAD_ID --after 0 --limit 100
chancery usage events --unattributed
chancery --json usage commands
```

`commands` returns each registered command, its recorded invocation count, and
its latest selected observation time. Unused commands have zero and a null
latest time. Counts select the requested system, thread and time range; zero
means no selected recorded invocations, not proof the command was never used.
`--since` is inclusive and `--until` exclusive, in whole Unix seconds. Bounds
must be nonnegative; when both are present, since must be less than until.
`--thread` and
`--unattributed` conflict. `events` returns insertion-ordered rows, `next_cursor`
and `has_more`. Limits are 1–1000, default 100; cursors are nonnegative and belong
to this database history. Re-establish them after restoring another history.
Report invocations are themselves observed when registration and storage permit.
Read queries do not modify journal rows; CLI dispatch can append its own usage.
Output uses Chancery's schema-three envelope; journal schema is independently one.
`usage systems` returns `items`, the registered ID list. `usage commands`
returns the selected `scope` and `items`. Each `CommandCount` has `system_id`,
`command_id`,
`invocations`, and nullable `last_recorded_at`. `usage events` returns the
selected `scope` and `page`. That `EventPage` has `items`,
`next_cursor`, and `has_more`; each `Event` has `id`, `recorded_at`, nullable
`codex_thread_id`, `system_id`, and `command_id`. The Rust read methods are
`Store::systems`, `Store::counts(&Filter)`, and `Store::events(&Filter, after,
limit)`. `Filter` has `system`, `thread`, `unattributed`, `since`, and `until`.
Read-only `Store::read` and ordinary reads do not observe themselves; CLI
dispatch is a separate boundary. Initialization returns `initialized: true`
and journal `schema_version: 1`. Explicit registration returns `system_id`
and `commands`. CLI syntax errors return exit 2,
while unavailable or invalid journal state returns `usage_failed` with exit 1.
Read `chancery.directory.discover` for the common output envelope.

## Storage, failure, and recovery

The default database is
`~/Library/Application Support/Chancery/usage.sqlite3`.
`CHANCERY_USAGE_DB` can select an absolute path; an empty value uses the default.
The database is a private regular file (0600), and initialization creates missing
parent directories privately. File permissions are the current-user trust
boundary. Never put prompts, credentials or bodies in any identity.

SQLite serializes brief writes. The busy timeout is 100 ms; there is no
background queue or automatic recording retry. OS scheduling and storage can
exceed nominal waits. A failed append leaves an observation gap and must not
fail the product operation. An uncertain write may have committed; do not
retry it as if the journal promised duplicate suppression. Registration errors
are returned separately. `CHANCERY_USAGE_DISABLED=1` disables environment-based
recording for isolated
tests or explicitly unobserved execution. It does not disable explicit
registration or request-scoped recording APIs.

The journal claims only recorded activity from instrumented boundaries. It
cannot enumerate missed observations or infer usage before instrumentation.
No completeness percentage, retention horizon, throughput, or hard latency
objective is promised. Rows contain no duration, outcome, token count or output.

Recovery requires a consistent compatible journal/schema pair and stopped
writers. Keep sidecars with any quiescent file backup and preserve the old
history. Program rollback does not erase new rows. Missing or unsupported state
remains an explicit error; never recreate a populated database or bypass its
triggers. Chancery recording does not operate product databases, authenticate
services, execute represented capabilities, or authorize domain actions. Read
`chancery.usage.operate` for backup, restore, and registration steps.

Rebuild participating binaries to adopt these rules; replacing Chancery alone
does not update their compiled recorder. Deploy matching wrappers and hooks,
and refresh pinned Clockwork brokers through their owning installation procedures.
