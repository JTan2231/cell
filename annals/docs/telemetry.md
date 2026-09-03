# Consumption telemetry

`annals-usage` is a separate workspace package and companion CLI that
calculates the Codex consumption caused by Annals examinations. Nucleus owns
model invocation, authentication, job and attempt state, and the exact raw
model-output records. Annals Usage owns only the live reporting projection.
It stores no telemetry database, token aggregate, response projection, or
account snapshot.

The reporting path is:

```text
Annals examination --------------------------> Annals library + job receipt
        |
        | model-run token in requester identity
        v
     Nucleus -------------------------------> exact model-output records
        |                                                |
        `---------------- job/attempt state -------------+
                                                         v
                                              annals-usage report
                                                         |
                                                         v
                                               in-memory projection
```

The model-run token correlates a Nucleus job with the library's model-run
record and, for an inbox examination, its source delivery and job receipt.
`report` reads those authorities on every invocation. It does not copy them
into another durable store. If Nucleus or an Annals attribution source cannot
be read, the command fails instead of returning a stale retained report.
Every result identifies its `annals-usage` projection version: human output
prints it after the generation time and JSON exposes it as
`projectionVersion`. Derived interpretations may therefore evolve without
freezing them beside the atomic records.

Nucleus output records are the atomic reporting source. Annals Usage decodes
token-usage, response-completion, and turn-completion messages and derives
totals, coverage, thread and turn identity, and credit-equivalent values in
memory. Non-output records, incompatible schemas, undecodable messages,
missing usage, inconsistent totals, or missing terminal output become explicit
coverage gaps. Nucleus observation times remain the response-event times in
JSON output.

Nucleus also owns one persistent Codex home and the cross-process
authentication lease. Annals and Annals Usage never read, copy, or configure
its credentials. Interactive `budget` and `doctor` account reads use a
nonblocking lease request and preserve the existing busy error behavior. Inbox
authentication preflight may wait up to 30 seconds before leaving a new job
unattempted.

## CLI

```text
annals-usage report [--json] [--limit N] [--config PATH]
annals-usage budget [--json] [--config PATH]
annals-usage doctor [--config PATH]
annals-usage login --device-auth
```

`report` joins recent delivery records from the Annals library with live
Nucleus jobs, their ordered model output, and inbox job receipts. It defaults
to the newest 20 deliveries. It shows each delivery's observed attempts,
coverage, token categories, and any calculable known credit-equivalent. JSON
additionally exposes the projection version, complete attempt and response
projections, plus runs that cannot be attributed to a source delivery.
Job-receipt discovery includes
processing, done, duplicates, failed, and skipped envelopes; a skipped job
remains a failed source delivery for reporting purposes.

A retry child is a distinct source delivery and is reported separately from
its original failure. If it starts a new examination, that Nucleus job and its
tokens are attributed to the child. Reusing the original attempt's exact valid
reconciliation creates no new model attempt. `annals inbox retry status` is the
authoritative view that pairs the two deliveries and their outcomes; the
consumption report does not merge them or reattribute the original attempt.

`budget` asks Nucleus to make live `account/rateLimits/read` and
`account/usage/read` requests. It prints the account plan, available limit
windows, used percentages, reset times, and credit fields. It also shows
lifetime, peak-day, and latest-day account token activity as a clearly labeled
cross-check; those global activity tokens are not allowance units. `--json`
contains the complete live allowance result and, when available, the complete
activity result alongside the command's observation time and scope. Nothing
from the read is retained by Annals Usage.

`doctor` checks the configured Nucleus executable, Annals library, and spool;
reads Nucleus and Codex versions; validates readiness; and performs a
nonblocking authenticated account request through Nucleus. It creates no
state.

Help and version flags belong to `annals-usage`. `login --device-auth`
delegates to the configured `nucleus auth login --device-auth` command. Other
invocations are rejected; Annals communicates with Codex only through Nucleus.

## Token accounting

Every observed usage value has six upstream categories:

- `inputTokens` includes ordinary, cached, and cache-write input;
- `cachedInputTokens` is the input served from a cache;
- `cacheWriteInputTokens` is the input written to a cache;
- `outputTokens` includes reasoning output;
- `reasoningOutputTokens` is the reasoning subset of output; and
- `totalTokens` is input plus output.

These are overlapping measurements, not six additive buckets. For a
consistent record:

```text
ordinary input = input - cached input - cache-write input
total          = input + output
reasoning      <= output
```

Consequently, never add cached or cache-write input to `inputTokens`, and never
add reasoning output to `outputTokens`. The human report indents the subset
categories to make this relationship visible.

An exact run total is the sum of the distinct upstream response-usage records
observed during that run. A consistent final cumulative
`thread/tokenUsage/updated` total is a fallback when exact response events are
unavailable or disagree with the cumulative total. Every examination attempt
consumes tokens, so a delivery report adds every Nucleus attempt associated
with that delivery rather than only the selected reconciliation's model run.

### Coverage

The delivery-level `coverage` value explains what the number means:

| Value | Meaning |
| --- | --- |
| `exact` | Every observed attempt is the sum of distinct per-response usage events and agrees with any final cumulative total. |
| `cumulative` | A consistent final cumulative thread total is used for at least one attempt. |
| `gap` | At least one required attempt has missing, incompatible, or unusable output, so no complete delivery total is claimed. |
| `no-model` | The delivery invoked no liaison, as for a fresh exact-byte duplicate or a permanent failure before work retention; usage is zero. |
| `pending` | The delivery or its Nucleus job is still active, so accounting is not terminal. |
| `reused-no-new-usage` | Annals reused an exact-context examination, so this delivery caused no new model usage. |

An individual attempt reports its own `exact`, `cumulative`, `gap`, or
`pending` coverage. A delivery with an unusable attempt is a gap. Jobs without
a source-delivery correlation, including manual examinations, remain visible
under `unattributedRuns` instead of being silently assigned.

`knownCreditEquivalent` applies the supported model's published ChatGPT rate
card to ordinary input, cached input, and output. Reasoning is already included
in output and is not charged twice. The current rate card does not separately
price cache-write input, so those tokens are excluded and reported as
`unpricedCacheWriteTokens`. A missing model rate makes the credit-equivalent
unknown. This value is a rate-card comparison, not an invoice or a share of a
subscription allowance.

## Subscription budget boundary

The account-limit response is a coarse, account-global snapshot. It may
contain rolling windows, percent used, reset times, plan type, and purchased
credit state. It does not expose the token denominator behind a subscription
window or identify which source delivery consumed a percentage point.

Other Codex activity on the same account contributes to the same snapshot.
Differences between two separately run `budget` commands are therefore not
reliable per-delivery accounting: they may include concurrent activity and
rounding. `annals-usage report` is authoritative for Annals token consumption
when its coverage is `exact`; `annals-usage budget` describes only the live
account-wide state at its `observedAt` timestamp. There is no supported exact
conversion between those measurements.

Current ChatGPT credit rates are maintained against the [official rate
card](https://learn.chatgpt.com/docs/pricing).

## Configuration and state

The installed macOS deployment writes
`$HOME/Library/Application Support/Annals/usage.toml`:

```toml
nucleus = "/absolute/path/to/nucleus"
nucleus_socket = "/absolute/path/to/nucleus.sock"
library = "/absolute/path/to/annals.db"
spool = "/absolute/path/to/spool"
```

An explicit `--config` selects the report, budget, or doctor configuration.
Otherwise the path resolves from `ANNALS_USAGE_CONFIG`, then to `usage.toml`
beside a nonempty `ANNALS_CONFIG`, then to the macOS state path under `HOME`.
Relative values are resolved from the selected configuration's directory, and
unknown keys are rejected.

`nucleus_socket` may be omitted to use Nucleus's standard current-user socket.
The executable is used only for delegated login; report, budget, and doctor
communicate over the socket. The Annals library and spool are opened only to
derive attribution and delivery context. Annals Usage has no state file of its
own and a valid configuration has no `database` key.

Deployments retain `usage.toml` with the Annals library and spool, switch
Annals and Annals Usage together, and pin both configurations to the deployed
Nucleus socket. An obsolete `usage.db` and its SQLite sidecars are discarded
after a successful deployment; they are retained only temporarily when needed
to roll back an uncommitted cutover. See [System installation and scheduled
inbox](system-installation.md#macos-user-clockwork-binding).

## Authority and limits

Nucleus raw model output, Nucleus job and attempt state, Annals library state,
and Annals job receipts are the durable authorities. The report is a
disposable projection over those atomic records. Its coverage value is part of
the result: `exact` is a measured total, while `gap` explicitly means that no
authoritative complete total exists.

The tool does not infer historical usage, scrape a UI, estimate a hidden
subscription denominator, or provide offline fallback. Retain and back up the
underlying Nucleus and Annals authorities when historical reports must remain
available.
