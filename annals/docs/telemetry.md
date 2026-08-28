# Consumption telemetry

`annals-usage` is a separate workspace package and companion CLI that measures
the Codex consumption caused by Annals examinations. Nucleus, not
`annals-usage`, owns invocation and authentication. The companion's purpose is
to make the measurable boundary explicit: a report
states both the recorded token totals and the quality of the observation rather
than filling historical or protocol gaps with estimates.

The normal installed path is:

```text
Annals examination
        |
        | deterministic job + model-run token
        v
     Nucleus ---- exact raw Codex protocol records ----> annals-usage sync
        |
        v
 Nucleus-owned Codex                                usage.db
```

Nucleus stores the exact app-server protocol input and output independently of
the Annals process. The model-run token in the Nucleus requester identity
correlates a terminal job with the library's model-run record and, for an inbox
examination, its source delivery and job receipt. `annals-usage report`
materializes a pending row for an active Annals job and incrementally imports
previously unseen terminal jobs one at a time. It
replays raw token, response-usage, rate-limit, and turn-completion messages into
the companion ledger without changing the liaison prompt, model, reasoning
effort, or tool set. Replayed events retain their Nucleus observation times, so
historical ordering is not shifted to report time. Non-Codex schemas,
undecodable records, missing usage, and incomplete terminal streams are
disclosed as coverage gaps.

Each terminal import is one SQLite transaction: pending-row replacement, raw
event replay, source-delivery attribution, and terminalization either commit
together or leave the prior row unchanged for retry. A retained terminal
receipt can also repair a completed Nucleus run imported without delivery
attribution; the repair preserves the run identity and atomically replaces its
derived event rows from Nucleus's retained log. Completed tokens that do not
need attribution repair are filtered before fetching job details or logs. Attempt
start and completion times come from Nucleus. If Nucleus is temporarily
unavailable, `report` warns on stderr and reports the observations already
retained in `usage.db`; model execution itself is unaffected.

Nucleus owns one persistent Codex home and the cross-process authentication
lease. Annals and `annals-usage` never read, copy, or configure its credentials.
Interactive `budget` and `doctor` account reads use a nonblocking lease request
and preserve the existing busy error behavior. Inbox authentication preflight
may wait up to 30 seconds before leaving a new job unattempted.

## CLI

```text
annals-usage report [--json] [--limit N] [--config PATH]
annals-usage budget [--json] [--config PATH]
annals-usage doctor [--config PATH]
annals-usage login --device-auth
```

`report` joins recent delivery records from the Annals library with observed
runs and inbox job receipts. It defaults to the newest 20 deliveries. It shows
each delivery's attempts, coverage, token categories, and any calculable known
credit-equivalent. JSON additionally exposes the complete attempt records,
unattributed runs, and latest recorded budget snapshot. Job-receipt discovery
includes processing, done, duplicates, failed, and skipped envelopes; a
skipped job remains a failed source delivery for reporting purposes.

A retry child is a distinct source delivery and is reported separately from
its original failure. If it starts a new examination, that model run and its
tokens are attributed to the child. Reusing the original attempt's exact valid
reconciliation creates no new model attempt. `annals inbox retry status` is the
authoritative view that pairs the two deliveries and their outcomes; the
telemetry report does not merge them or reattribute the original attempt.

`budget` asks Nucleus to make live `account/rateLimits/read` and
`account/usage/read` requests. It stores the allowance snapshot and prints
the account plan, available limit windows, used percentages, reset times, and
credit fields. It also shows lifetime, peak-day, and latest-day account token
activity as a clearly labeled cross-check; those global activity tokens are not
allowance units. `--json` retains the complete allowance result and, when that
separate endpoint is available, the complete activity result alongside the
observation time and scope. Activity-read failure is reported but does not hide
a successful allowance read.

`doctor` checks the referenced Annals library, spool, Nucleus executable and
socket; opens or creates the telemetry database; reads the Nucleus and Codex
versions; validates readiness; and performs a nonblocking authenticated
account-limit request through Nucleus.

Help and version flags belong to `annals-usage`. `login --device-auth`
delegates to the configured `nucleus auth login --device-auth` command. Other
former proxy/passthrough invocations are rejected; Annals communicates only
with Nucleus.

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
nucleus = "/absolute/path/to/nucleus"
nucleus_socket = "/absolute/path/to/nucleus.sock"
database = "/absolute/path/to/usage.db"
library = "/absolute/path/to/annals.db"
spool = "/absolute/path/to/spool"
```

An explicit `--config` selects the report, budget, or doctor configuration.
Otherwise the path resolves from `ANNALS_USAGE_CONFIG`, then to `usage.toml`
beside a nonempty `ANNALS_CONFIG`, then to the macOS state path under `HOME`.
Relative values are resolved from the selected configuration's directory, and
unknown keys are rejected.

`nucleus_socket` may be omitted to use Nucleus's standard current-user socket.
The executable is used only for delegated login; reports, budget reads, and
doctor communicate over the socket. Use `annals-usage login --device-auth` or
`nucleus auth login --device-auth`; both leave credential storage and lease
ownership entirely inside Nucleus.

`usage.db` is a companion SQLite ledger in the same installation state as the
Annals library and is accessible to the same user. It is intentionally not part
of the Annals library or corpus schema: telemetry cannot change retained works,
delivery records, reconciliations, or corpus revisions. The ledger retains:

- one correlated record for each observed model run;
- cumulative token snapshots;
- distinct exact response-usage records; and
- account quota snapshots.

Deployments retain `usage.toml` and `usage.db` alongside the library and spool.
The macOS deployer switches Annals and `annals-usage` together and pins both
configs to the deployed Nucleus socket. See [System installation and
scheduled inbox](system-installation.md#macos-user-launchagent).

## Authority and limits

For source deliveries processed through Nucleus, the telemetry ledger plus
the Annals library and job receipts is the supported accounting surface. The
coverage value is part of that authority: `exact` is a measured total, while a
gap or legacy value explicitly means that no authoritative total exists.

The tool does not infer historical usage, scrape a UI, or estimate a hidden
subscription denominator. Retain the telemetry database with the Annals state
when historical consumption reports must remain available.
