# Calculate a forecast

Mantic calculates the expected remainder from a bundle of expense rules, a
starting amount, and a calendar period. Select one saved config as the bundle,
then optionally exclude saved items or include temporary expenses for this run.
Its result is a projection from supplied rules. It does not verify actual
balances, expense occurrence, or payment status.

## Interface and inputs

```text
mantic [--database ABSOLUTE_PATH] [--json] forecast CONFIG AMOUNT [--from DATE] --until DATE [--details] [--exclude ITEM_ID]... [--include NAME AMOUNT [--first DATE] [--every UNIT] [--interval N] [--end DATE]]...
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

## Exclude and include expenses

Use `--exclude ITEM_ID` to remove a saved expense rule from this run. Read
`mantic item list CONFIG` to find its ID. The ID must belong to the selected
config snapshot. An unknown ID or an item from another config fails the run.
Repeating the same ID excludes it once. Exclusion removes all occurrences of
that rule within the forecast period; it does not delete or edit the saved item.
Item names are nonunique labels. Mantic does not select exclusions by name or
interpret a natural-language description.

For example, if the default config's daily 60-unit Robinhood expense has ID 17:

```sh
mantic forecast default 5000 --from 2026-10-02 --until 2026-12-31 \
  --exclude 17
```

Use `--include NAME AMOUNT` to add a temporary expense rule. Pass ordinary CLI
values; JSON input is not used. Names and amounts follow the saved-item rules:
a trimmed nonempty name without control characters, and a positive amount in
whole units or one or two decimal places. Each inclusion remains a separate
charge, even when its name, amount, and schedule match another expense.

```sh
mantic forecast default 5000 --from 2026-10-02 --until 2026-12-31 \
  --include "Extra monthly" 500 --every month
```

Each `--include` starts a new expense. The scheduling modifiers `--first`,
`--every`, `--interval`, and `--end` apply to the most recent preceding inclusion.
Forecast flags such as `--from`, `--until`, `--details`, and `--exclude` do not
change that target. A scheduling modifier before any inclusion is a CLI syntax
error. Repeating a modifier within the same inclusion is also a syntax error.

```sh
mantic forecast default 5000 --from 2026-10-02 --until 2026-12-31 \
  --exclude 17 \
  --include "Extra monthly" 500 --every month \
  --include "One-time repair" 200 --first 2026-11-01
```

Omitting `--first` uses the final resolved forecast `from` date, including when
`--from` appears after the inclusion. Omitting `--every` makes the expense
one-off. `--every` accepts `day`, `week`, `month`, or `year`. Recurrence defaults
to interval one. `--interval` requires `--every` within the same inclusion and
accepts 1 through 4,294,967,295. `--end` is an inclusive cutoff on or after the
resolved first due date. Supply `--first` explicitly to preserve an ad hoc
schedule's anchor when changing the forecast start date.

The monthly example charges on October 2, November 2, and December 2 for a
total of 1,500 units. Saved expenses retain their original anchors.

Mantic loads one saved config snapshot, removes excluded rules, and appends
inclusions in CLI order. Saved rules are ordered by item ID. Repeated
exclusions have no additional effect, and exclusions cannot target inclusions.
Exclusion order and the position of forecast flags do not change composition.
The resulting expense list is the calculator input. No config write occurs.

## Calculation rules

For each effective expense, generate occurrences from its fixed `first_due`
anchor. Include only occurrences on or after `from`, on or before `until`, and
on or before an optional expense cutoff. A one-off expense contributes at most
once. Earlier occurrences are excluded; they do not reduce the supplied starting
amount.

Day and week recurrence advances by the configured number of calendar days or
weeks. Month and year recurrence selects each occurrence from the original
anchor, clamping its day to the last available day of the target month.
January 31 therefore gives February 28 and March 31 in an ordinary year.
A yearly February 29 rule uses February 28 in ordinary years and returns to
February 29 in leap years. Every N months or years preserves the same anchor.

Order occurrences by due date, then saved item ID, then inclusion ordinal.
Saved expenses precede inclusions on the same date. Subtract each expense's
fixed positive amount once per occurrence. Sum with checked integer arithmetic:

```text
remaining_cents = starting_amount_cents - expected_expenses_cents
```

A negative remainder is a successful forecast. The first shortfall date is
the first date on which the running balance is negative. A negative starting
amount makes the starting date the first shortfall date, even with no expenses.
Expenses with no selected occurrences contribute zero.

## Results and retention

The result includes the selected config, resolved starting amount and dates,
total expected expenses, remaining amount, and nullable first shortfall date.
Readable text always identifies excluded items and included expenses with their
resolved schedules, even when they have no occurrences in the selected period.
`--details` adds each selected occurrence to readable text output. JSON always
contains the complete effective expense definitions, complete excluded saved
definitions, and complete occurrence list. Each occurrence identifies its
source, date, amount, and running balance. The occurrence list covers only this
forecast period.

Forecasting opens the configuration read-only and loads one consistent snapshot.
Calculation then runs in memory. A concurrent committed edit can affect a later
forecast but cannot split this forecast across different definition versions.
The forecast does not create, migrate, repair, or write Mantic state.

Each invocation is independent. There is no run ID, run table, saved starting
amount, retained result, generated-occurrence table, payment record, or advancing
next due date. The caller can retain stdout. To reproduce a calculation after
edits, preserve the normalized effective definitions in the result, explicit
inputs, and compatible engine version.

Readable text is the default. `--json` selects forecast output schema two.
Machine callers must request JSON. Success uses
`{"schema_version":2,"ok":true,"result":...}`
on stdout. The result has `config_id`, `config_name`, `starting_amount_cents`,
`from`, `until`, `total_expenses_cents`, `remainder_cents`, `first_shortfall`,
`expenses`, `excluded_items`, and `occurrences`. `first_shortfall` is a date or
null.

Every effective expense and occurrence uses a `source` object:

```json
{"kind":"saved","item_id":17}
```

```json
{"kind":"included","ordinal":1}
```

A saved source identifies an item in the selected database. An included source
uses a one-based ordinal in CLI inclusion order, local to this invocation.
It is not a database ID. Names remain nonunique labels.

Each entry in `expenses` has `source`, `name`, `amount_cents`, `first_due`,
`repeat_unit`, `repeat_every`, and `end_date`. These are complete normalized
rules in effective calculation order. Dates use `YYYY-MM-DD`; recurrence unit
and interval are both null for a one-off expense, and a missing cutoff is null.
The first due date is always resolved, including its invocation default.

Each entry in `excluded_items` has `id`, `config_id`, `name`, `amount_cents`,
`first_due`, `repeat_unit`, `repeat_every`, and `end_date`. These complete saved
definitions appear once each, in item-ID order. They remain in the result even
if they would have had zero occurrences. Empty composition lists use `[]`.

Each occurrence has `date`, `source`, `item_name`, `amount_cents`, and
`remainder_cents`. Schema two replaces the old occurrence `item_id` field with
`source`; callers that parsed forecast schema one must update. Every forecast
uses schema two, including a run without exclusions or inclusions. Config and
item command output remains schema one. Persistent state remains schema one.

Operation errors use
`{"schema_version":2,"ok":false,"error":{"code":"mantic_failed","message":"..."}}`
on stderr and exit 1. CLI syntax errors exit 2. The calculation has no model,
network, bank, daemon, or schedule dependency.

## Failure, privacy, and compatibility

Invalid inputs, unknown configs or excluded IDs, missing or unsupported state,
calendar range errors, overflow, and occurrence limits stop the calculation.
No partial result is presented as a normal forecast. Correct the selected input
or restore a compatible program before retrying; failed forecasting writes no
config data.
Amounts must fit signed 64-bit cents. The combined effective expenses may
produce at most 100,000 selected occurrences. Excluded rules do not count toward
this limit. A larger selection fails instead of truncating the result. No
wall-clock calculation latency or database-capacity objective is promised.

Starting amounts, periods, names, schedules, and results are private. Mantic
retains only saved definitions. Temporary inclusions and exclusions are not
retained. Output can disclose private definitions to the caller's terminal or
chosen destination. Optional Chancery command usage retains only identity,
time, and thread correlation, and recording failures preserve the result.

Output schema, persistent schema, product release, and this contract version
are separate. Direct SQLite integration, income rules, variable amounts, holiday
adjustment, actual payment tracking, and automatic output retention are unsupported.
Read `mantic.config.manage` for stored definitions and update semantics.
