# Mantic

Mantic is a local forecasting CLI. It subtracts expected expenses from a
supplied starting amount over a supplied calendar period. One private SQLite
database holds multiple named configs. Each config is a reusable expense bundle
that defines amounts and recurrence schedules. A forecast can exclude saved
expenses and include temporary expenses through ordinary CLI arguments.

Mantic preserves only the current config definitions. A forecast reads one
consistent snapshot, applies temporary adjustments in memory, prints its
calculated result, and exits. Saved definitions remain unchanged.
Starting amounts, runs, generated occurrences, balances, and payment records
are not retained. The caller can preserve stdout when needed.

## Features

| ID | Read this to understand |
| --- | --- |
| `mantic.config.manage` | Database initialization, named configs, item ownership, amounts, recurrence definitions, and config edits. |
| `mantic.forecast.calculate` | Temporary exclusions and inclusions, inclusive dates, anchored recurrence, composition, totals, details, shortfalls, and output. |
| `mantic.installation` | Private state, immutable program and provider selection, compatibility, and recovery boundaries. |

Use `mantic.install.operate` for installation and retained-release recovery.
Its required feature contract explains selection and state behavior.

The CLI prints readable text by default. Pass `--json` for machine output.
Forecast output uses schema two with complete effective and excluded definitions
and tagged occurrence sources. Config and item output remains schema one.
The default database is
`~/Library/Application Support/Mantic/mantic.db`. A global
`--database ABSOLUTE_PATH` selects a different database for that invocation.
It does not make each config a database or retain a default selection.

Mantic owns expense configuration and calculation rules. Callers supply amounts,
dates, and authority for edits. The forecast is a projection from those inputs;
it does not observe account balances, confirm payments, or access a bank.
No model, daemon, schedule, or network service participates in a forecast.

Installed Chancery documentation does not establish live readiness or authorize
an edit. Program and provider publication move together. Shared command usage
records command metadata separately from Mantic's configuration database.
