# Manage configs and expense items

Mantic preserves current calculation definitions in one private SQLite database.
A config is a named container. Each expense item belongs to exactly one config
and describes a fixed amount and its expected due dates. There is no config
history, saved active config, balance, run, or payment state.

## Interfaces

```text
mantic [--database ABSOLUTE_PATH] [--json] init
mantic [--database ABSOLUTE_PATH] [--json] config create NAME
mantic [--database ABSOLUTE_PATH] [--json] config list
mantic [--database ABSOLUTE_PATH] [--json] config show NAME
mantic [--database ABSOLUTE_PATH] [--json] config rename NAME NEW_NAME
mantic [--database ABSOLUTE_PATH] [--json] config delete NAME
mantic [--database ABSOLUTE_PATH] [--json] item add CONFIG NAME AMOUNT --first DATE [--every UNIT] [--interval N] [--end DATE]
mantic [--database ABSOLUTE_PATH] [--json] item list CONFIG
mantic [--database ABSOLUTE_PATH] [--json] item update ID [--name NAME] [--amount AMOUNT] [--first DATE] [--every UNIT] [--interval N] [--end DATE | --clear-end] [--once]
mantic [--database ABSOLUTE_PATH] [--json] item delete ID
```

The default path is `~/Library/Application Support/Mantic/mantic.db`.
`--database` is an invocation-wide absolute path override. No config-specific
database, environment override, or retained database selection exists.

Run `init` before ordinary commands. It creates missing parent directories and
initializes an empty database. Repeating it on supported state preserves configs
and items. Missing, foreign, invalid, or unsupported state stops ordinary
commands. They do not initialize or migrate it implicitly.

## Definition model

Persistent schema one contains two domain tables. Schema metadata identifies
the database version; it is not a calculation config.

| Table | Fields and meaning |
| --- | --- |
| `config` | `id`, `name`: a stable local config identity and unique display/selecting name. |
| `config_item` | `id`, `config_id`, `name`, `amount_cents`, `first_due`, `repeat_unit`, `repeat_every`, `end_date`: one expense rule owned by its config. |

`config_item.config_id` references `config.id`. Deleting a config deletes its
items in the same transaction. Items cannot be shared among configs or moved
by the CLI. Item names are labels; use the item ID for updates and deletes.
IDs belong to the selected database and do not identify external objects.
Names are trimmed, nonempty, and contain no control characters. Config names
are unique and case-sensitive. Item names need not be unique.

Amounts accept whole units or one or two decimal places and use integer cents.
Expense amounts must be positive. All amounts use the same caller-chosen
currency with two decimal places. Mantic has no currency conversion or retained
currency setting.

Dates use `YYYY-MM-DD` from `0001-01-01` through `9999-12-31`.
`first_due` is both the first occurrence and the fixed
schedule anchor. `repeat_unit` is `day`, `week`, `month`, or `year`.
`repeat_every` is the positive interval, default one when recurrence is supplied.
Both recurrence fields are absent for a one-off item. An optional `end_date` is
an inclusive cutoff on or after `first_due`; it need not match an occurrence.
Intervals cannot exceed 4,294,967,295.

Each update validates and writes the complete resulting item atomically.
Unspecified fields retain their values. `--end` sets a cutoff, `--clear-end`
removes it, and `--once` removes recurrence. Changing the first due date changes
the anchor. Calculations never alter an anchor or advance a schedule.
Changing only `--every` preserves an existing interval or uses one for a one-off
item. `--interval` alone requires an already recurring item. `--once` conflicts
with recurrence changes, and setting and clearing a cutoff together is invalid.

## Consistency and recovery

Config and item changes use SQLite transactions. Reads return committed current
definitions; config show includes its items. Lists are complete for the selected
database or config. There is no revision or as-of timestamp. Later edits affect
later forecasts and do not rewrite any retained forecast because none exists.
SQLite contention uses a five-second busy timeout. No hard wall-clock latency
or automatic write-retry guarantee is promised.

Successful writes report the resulting definition or deletion. Invalid amounts,
dates, recurrence combinations, missing IDs, unknown config names, and duplicate
config names fail. The failed transaction leaves the prior definitions intact.
After an interrupted command, inspect the selected config or item before retrying
a create or add; writes have no caller-supplied idempotency key.

Stop on unsupported or damaged state. Preserve the database and select a
compatible program. Do not repair definitions through SQL, replace populated
state with an empty database, or treat a program rollback as data recovery.

## Privacy and supported boundary

Names, amounts, and schedules are private local state. The local user's filesystem
access is the trust boundary. The CLI is the supported editing interface; direct
SQLite integration is unsupported. Mantic sends no config data to a model or
network service. The caller owns disclosure of displayed or redirected output.

`--json` selects output schema one. Success uses
`{"schema_version":1,"ok":true,"result":...}` on stdout. Operation errors use
`{"schema_version":1,"ok":false,"error":{"code":"mantic_failed","message":"..."}}`
on stderr and exit 1. CLI syntax errors exit 2. Command usage can separately
append command identity,
observation time, and `CODEX_THREAD_ID` in Chancery's journal. It retains no
arguments, configuration bodies, output, or success result. Recording failures
preserve Mantic results. Missing thread attribution and internal calls are skipped.

Read `mantic.forecast.calculate` for recurrence expansion and calculations.
Read `mantic.installation` for program and private-state lifecycle.
