# Consumption telemetry

`annals-usage` is a separate workspace package and companion CLI that measures
the Codex consumption caused by Annals examinations. It is also Annals' default
Codex proxy. Its purpose is to make the measurable boundary explicit: a report
states both the recorded token totals and the quality of the observation rather
than filling historical or protocol gaps with estimates.

The normal installed path is:

```text
Annals examination
        |
        | model-run token
        v
annals-usage proxy ---- exact and cumulative events ----> usage.db
        |
        v
   real Codex
```

The proxy preserves the Codex arguments, standard streams, and exit status.
For `app-server --stdio`, it observes token and rate-limit events and enables
per-response usage events on `thread/start`. It does not change the liaison
prompt, model, reasoning effort, or tool set. The model-run token supplied by
Annals correlates an observed run with the library's model-run record and, for
an inbox examination, its source delivery and job receipt.

Telemetry is fail-open for an examination. If the companion database cannot be
opened or an event cannot be recorded, the proxy reports that telemetry is
unavailable on stderr and continues forwarding Codex traffic. It records a
turn's completion before forwarding that completion to Annals, because Annals
may close the isolated app-server immediately afterward.

## CLI

```text
annals-usage report [--json] [--limit N] [--config PATH]
annals-usage budget [--json] [--config PATH]
annals-usage doctor [--config PATH]
```

`report` joins recent delivery records from the Annals library with observed
runs and inbox job receipts. It defaults to the newest 20 deliveries. It shows
each delivery's attempts, coverage, token categories, and any calculable known
credit-equivalent. JSON additionally exposes the complete attempt records,
unattributed runs, and latest recorded budget snapshot. Job-receipt discovery
includes processing, done, duplicates, failed, and skipped envelopes; a
skipped job remains a failed source delivery for reporting purposes.

`budget` makes live `account/rateLimits/read` and `account/usage/read` requests
through the configured real Codex. It stores the allowance snapshot and prints
the account plan, available limit windows, used percentages, reset times, and
credit fields. It also shows lifetime, peak-day, and latest-day account token
activity as a clearly labeled cross-check; those global activity tokens are not
allowance units. `--json` retains the complete allowance result and, when that
separate endpoint is available, the complete activity result alongside the
observation time and scope. Activity-read failure is reported but does not hide
a successful allowance read.

`doctor` checks the referenced Annals library, spool, state-local Codex home,
and real Codex path; opens or creates the telemetry database; reads the Codex
version; and performs an authenticated account-limit request.

Help and version flags belong to `annals-usage`. Any other invocation is passed
through to the configured real Codex. This lets Annals select one executable as
its Codex command while retaining ordinary app-server behavior.

## Token accounting

Every observed usage value has six upstream categories:

- `inputTokens` includes ordinary, cached, and cache-write input;
- `cachedInputTokens` is the input served from a cache;
- `cacheWriteInputTokens` is the input written to a cache;
- `outputTokens` includes reasoning output;
- `reasoningOutputTokens` is the reasoning subset of output; and
- `totalTokens` is input plus output.

These are overlapping measurements, not six additive buckets. For a consistent
record:

```text
ordinary input = input - cached input - cache-write input
total          = input + output
reasoning      <= output
```

Consequently, never add cached or cache-write input to `inputTokens`, and never
add reasoning output to `outputTokens`. The human report indents the subset
categories to make this relationship visible.

An exact run total is the sum of the distinct upstream response-usage records
observed during that run. The ledger also retains every
`thread/tokenUsage/updated` event, including its last-response and cumulative
totals. A final consistent cumulative total is a fallback when exact response
events are unavailable.

Every examination attempt consumes tokens. A delivery report adds every
observed attempt associated with that delivery, including any historical
multi-attempt records, instead of reporting only a terminal attempt.

### Coverage

The delivery-level `coverage` value explains what the number means:

| Value | Meaning |
| --- | --- |
| `exact` | Every observed attempt is the sum of distinct per-response usage events. |
| `cumulative` | A consistent final cumulative thread snapshot is used for at least one attempt, with no known gap. |
| `gap` | At least one required attempt has missing or unusable telemetry, so no complete delivery total is claimed. |
| `no-model` | The delivery invoked no liaison, as for a fresh exact-byte duplicate or a permanent failure before work retention; usage is zero. |
| `pending` | The delivery is still processing and has no completed accounting result. |
| `reused-no-new-usage` | Annals reused an exact-context examination, so this delivery caused no new model usage. |
| `legacy-unobserved` | The delivery predates the proxy or otherwise has no observation from which usage can be reconstructed. |

An individual attempt can also have `invalid` coverage when upstream totals
violate the category relationships or the exact event stream is incomplete.
The delivery report treats that as a gap. Runs without a source-delivery
correlation, including manual examinations, remain visible under
`unattributedRuns` instead of being silently assigned.

`knownCreditEquivalent` applies the supported model's published ChatGPT rate
card to ordinary input, cached input, and output. Reasoning is already included
in output and is not charged twice. The current rate card does not separately
price cache-write input, so those tokens are excluded and reported as
`unpricedCacheWriteTokens`. A missing model rate makes the credit-equivalent
unknown. This value is a rate-card comparison, not an invoice and not a share
of a subscription allowance.

## Subscription budget boundary

The account-limit response is a coarse, account-global snapshot. It may contain
primary and secondary rolling windows, percent used, reset times, plan type,
purchased-credit state, and reset credits. It does not expose the token
denominator behind a subscription window, and it does not identify which
source delivery consumed a percentage point.

Other Codex activity on the same account contributes to the same snapshot.
Differences between two percent-used readings are therefore not reliable
per-delivery accounting: they may include concurrent activity and rounding.
`annals-usage report` is authoritative for Annals token consumption when its
coverage is `exact`; `annals-usage budget` is authoritative only for the
account-wide state at its `observedAt` timestamp. There is no supported exact
conversion between those two measurements.

The report includes the most recent explicit account-limit read, with its
observation timestamp, for context while preserving this account-wide
attribution warning. Current ChatGPT credit rates are maintained against the
[official rate card](https://learn.chatgpt.com/docs/pricing).

## Configuration and state

The installed macOS deployment writes
`$HOME/Library/Application Support/Annals/usage.toml`:

```toml
codex = "/absolute/path/to/the/real/codex"
database = "/absolute/path/to/usage.db"
library = "/absolute/path/to/annals.db"
spool = "/absolute/path/to/spool"
codex_home = "/absolute/path/to/codex-home"
```

An explicit `--config` selects the report, budget, or doctor configuration.
Otherwise the path resolves from `ANNALS_USAGE_CONFIG`, then to `usage.toml`
beside a nonempty `ANNALS_CONFIG`, then to the macOS state path under `HOME`.
Relative values are resolved from the selected configuration's directory, and
unknown keys are rejected.

`usage.db` is a companion SQLite ledger in the same installation state as the
Annals library and is accessible to the same user. It is intentionally not part
of the Annals library or corpus schema: telemetry cannot change retained works,
delivery records, reconciliations, or corpus revisions. The ledger retains:

- one correlated record for each observed model run;
- cumulative token snapshots;
- distinct exact response-usage records; and
- account quota snapshots.

Deployments retain `usage.toml` and `usage.db` alongside the library and spool.
The macOS deployer switches Annals and `annals-usage` together, so a scheduled
worker never needs a manual proxy update. See [System installation and
scheduled inbox](system-installation.md#macos-user-launchagent).

## Authority and limits

For source deliveries processed through the proxy, the telemetry ledger plus
the Annals library and job receipts is the supported accounting surface. The
coverage value is part of that authority: `exact` is a measured total, while a
gap or legacy value explicitly means that no authoritative total exists.

The tool does not infer historical usage, scrape a UI, or estimate a hidden
subscription denominator. Retain the telemetry database with the Annals state
when historical consumption reports must remain available.
