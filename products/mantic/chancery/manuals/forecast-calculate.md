# Calculate a forecast

Mantic calculates the expected remainder from one current config, a starting
amount, and a calendar period. Its result is a projection from supplied rules.
It does not verify actual balances, expense occurrence, or payment status.

## Interface and inputs

```text
mantic [--database ABSOLUTE_PATH] [--json] forecast CONFIG AMOUNT [--from DATE] --until DATE [--details]
```

Select a named config in the one database. The default database is
`~/Library/Application Support/Mantic/mantic.db`; the global absolute
`--database` override selects another database for that invocation only.
The selected database must already contain supported initialized state.

`AMOUNT` is the money available before the starting day's expenses. It accepts
whole units or one or two decimal places, including a negative starting amount,
and is converted to signed integer cents. Every item and the starting amount
must use the same currency with two decimal places.

`--from` defaults to the user's local calendar date. `--until` is required and
must be on or after the starting date. Both dates are included. The output always
reports the resolved dates. Use explicit dates to repeat a calculation without
depending on the local clock.
Dates use `YYYY-MM-DD` from `0001-01-01` through `9999-12-31`.

## Calculation rules

For each item, generate occurrences from its fixed `first_due` anchor. Include
only occurrences on or after `from`, on or before `until`, and on or before an
optional item cutoff. A one-off item contributes at most once. Earlier
occurrences are excluded; they do not reduce the supplied starting amount.

Day and week recurrence advances by the configured number of calendar days or
weeks. Month and year recurrence selects each occurrence from the original
anchor, clamping its day to the last available day of the target month.
January 31 therefore gives February 28 and March 31 in an ordinary year.
A yearly February 29 rule uses February 28 in ordinary years and returns to
February 29 in leap years. Every N months or years preserves the same anchor.

Order occurrences by due date and then item ID. Subtract each item's fixed
positive amount once per occurrence. Sum with checked integer arithmetic:

```text
remaining_cents = starting_amount_cents - expected_expenses_cents
```

A negative remainder is a successful forecast. The first shortfall date is
the first date on which the running balance is negative. A negative starting
amount makes the starting date the first shortfall date, even with no expenses.
Items with no selected occurrences contribute zero.

## Results and retention

The result includes the selected config, resolved starting amount and dates,
total expected expenses, remaining amount, and nullable first shortfall date.
`--details` adds each selected occurrence to readable text output. JSON always
contains the complete occurrence list, with date, item identity, amount, and
running balance. The list covers only this forecast period.

Forecasting opens the configuration read-only and loads one consistent snapshot.
Calculation then runs in memory. A concurrent committed edit can affect a later
forecast but cannot split this forecast across different definition versions.
The forecast does not create, migrate, repair, or write Mantic state.

Each invocation is independent. There is no run ID, run table, saved starting
amount, retained result, generated-occurrence table, payment record, or advancing
next due date. The caller can retain stdout. To reproduce a calculation after
edits, the caller must also preserve its definitions, explicit inputs, and
compatible engine version.

Readable text is the default. `--json` selects output schema one. Machine callers
must request JSON. Success uses `{"schema_version":1,"ok":true,"result":...}`
on stdout. The result has `config_id`, `config_name`, `starting_amount_cents`,
`from`, `until`, `total_expenses_cents`, `remainder_cents`, `first_shortfall`,
and `occurrences`. `first_shortfall` is a date or null. Each occurrence has
`date`, `item_id`, `item_name`, `amount_cents`, and `remainder_cents`.

Operation errors use
`{"schema_version":1,"ok":false,"error":{"code":"mantic_failed","message":"..."}}`
on stderr and exit 1. CLI syntax errors exit 2. The calculation has no model,
network, bank, daemon, or schedule dependency.

## Failure, privacy, and compatibility

Invalid inputs, unknown configs, missing or unsupported state, calendar range
errors, overflow, and occurrence limits stop the calculation. No partial result
is presented as a normal forecast. Correct the selected input or restore a
compatible program before retrying; failed forecasting writes no config data.
Amounts must fit signed 64-bit cents. A forecast permits at most 100,000 selected
occurrences. A larger selection fails instead of truncating the result. No
wall-clock calculation latency or database-capacity objective is promised.

Starting amounts, periods, names, schedules, and results are private. Mantic
retains only the definitions. Output can disclose them to the caller's terminal
or chosen destination. Optional Chancery command usage retains only identity,
time, and thread correlation, and recording failures preserve the result.

Output schema, persistent schema, product release, and this contract version
are separate. Direct SQLite integration, income rules, variable amounts, holiday
adjustment, actual payment tracking, and automatic output retention are unsupported.
Read `mantic.config.manage` for stored definitions and update semantics.
