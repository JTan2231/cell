# Inspect Annals model consumption

Annals Usage calculates the model consumption caused by Annals examinations.
Use it to read recent source-delivery totals, attempt coverage, unattributed
runs, and any calculable rate-card equivalent. It is a separate companion CLI
with a versioned live projection. It owns no database or runtime state.

The report makes no model call and changes no Annals or Nucleus domain state.
It is not a report of arbitrary Nucleus requesters or all account activity.
It provides no invoice, subscription allocation, historical estimate, UI
scrape, or offline fallback.

## Interfaces and output

```text
annals-usage report [--json] [--details] [--limit N] [--config PATH]
```

For the installed command:

```sh
/Users/joey/.local/bin/annals-usage report
/Users/joey/.local/bin/annals-usage report --json --details --limit <COUNT>
```

Read `annals-usage.execution.operate` for the configuration schema, path
selection, and Nucleus access boundary. A report needs readable current Annals
library and spool attribution plus compatible retained Nucleus jobs, attempts,
and output through the configured socket.

The default scope is the newest 20 source deliveries. Output version 2 shows
delivery totals, attempt counts, coverage, and any calculable credit-equivalent.
It also shows unattributed run identity, status, totals, coverage, and errors.
`hasMore` and `unattributedHasMore` indicate further records. Increase positive
`--limit` to read more.

`--details` includes complete attempt and response projections in both text
and JSON. `--json` selects encoding only. Every result identifies the projection
version: human output prints it after the generation time, and JSON exposes
`projectionVersion`. Interpretation can change independently of retained
records. Nucleus observation times remain response-event times in JSON output.

## Attribution and source records

Each invocation joins current Annals delivery and model-run attribution with
Nucleus jobs, attempts, and ordered model-output records. The model-run token
correlates a Nucleus job with an Annals model run and, for an inbox examination,
its source delivery and job receipt.

```text
Annals examination ----> Annals library and inbox job receipt
        |
        | model-run token in requester identity
        v
     Nucleus ----------> job, attempt, and exact output records
                                      |
                                      v
                         annals-usage live projection
```

Job-receipt discovery covers `processing`, `done`, `duplicates`, `failed`, and
`skipped` envelopes. A skipped job counts as a failed source delivery. Manual
examinations and other runs without a source-delivery correlation remain under
`unattributedRuns`; they are never silently assigned to a delivery.

A retry child is a distinct source delivery. If it starts a new examination,
its Nucleus job and tokens belong to the child. Reusing the original attempt's
exact valid reconciliation creates no new model attempt. The original attempt
remains attributed to the original delivery. Use `annals inbox retry status`
to pair the two deliveries and their domain outcomes; this report does not
merge or reattribute them.

Nucleus output records are the atomic reporting source. Annals Usage decodes
token-usage, response-completion, and turn-completion messages and derives
totals, coverage, thread and turn identity, and credit-equivalent values in
memory. Non-output records, incompatible schemas, undecodable messages,
missing usage, inconsistent totals, and missing terminal output become explicit
coverage gaps.

## Token accounting

Every observed usage value has six upstream categories:

| Field | Meaning |
| --- | --- |
| `inputTokens` | Ordinary, cached, and cache-write input. |
| `cachedInputTokens` | The input served from a cache. |
| `cacheWriteInputTokens` | The input written to a cache. |
| `outputTokens` | Output, including reasoning. |
| `reasoningOutputTokens` | The reasoning subset of output. |
| `totalTokens` | Input plus output. |

These measurements overlap. For a consistent record:

```text
ordinary input = input - cached input - cache-write input
total          = input + output
reasoning      <= output
```

Do not add cached or cache-write input to `inputTokens`. Do not add reasoning
to `outputTokens`. Human output indents the subset categories.

An exact run total sums distinct upstream response-usage records observed
during that run. A consistent final cumulative `thread/tokenUsage/updated`
total is a fallback when exact response events are unavailable or disagree
with that total. A delivery report adds every associated Nucleus attempt,
including attempts other than the selected reconciliation's model run.

## Coverage and success

Every delivery has one `coverage` value:

| Value | Meaning |
| --- | --- |
| `exact` | Distinct response-usage records cover every observed attempt and agree with any final cumulative total. |
| `cumulative` | A consistent final thread total is used for at least one attempt. |
| `gap` | Required output is missing, incompatible, or unusable, so no complete delivery total is claimed. |
| `no-model` | No liaison ran, as for a fresh exact-byte duplicate or a permanent failure before work retention; usage is zero. |
| `pending` | Delivery or Nucleus runtime work remains active, so accounting is not terminal. |
| `reused-no-new-usage` | Annals reused an exact-context examination, so this delivery caused no new model usage. |

Each attempt reports `exact`, `cumulative`, `gap`, or `pending`. A delivery
with an unusable required attempt is a gap. Successful reporting keeps these
states distinct, qualifies every total, and shows unattributed work and output
errors. Do not replace `gap` with an estimate.

## Rate-card comparison

`knownCreditEquivalent` applies the supported model's published ChatGPT rate
card to measured ordinary input, cached input, and output. Reasoning is already
included in output. The current rate card does not separately price cache-write
input, so those tokens are excluded and reported as `unpricedCacheWriteTokens`.
A missing model rate makes the credit-equivalent unknown.

This value is a rate-card comparison, not an invoice, currency charge,
subscription share, or account allowance unit. Rates are maintained against
the [official rate card](https://learn.chatgpt.com/docs/pricing). This contract
does not pin the external rate card's version, digest, or installed contract ID.

## Rust interfaces

`annals-api::usage` owns the read-only library queries and partial inbox-receipt
decoding used for attribution. Annals Usage imports its typed delivery,
model-run, and receipt records. It contains no production SQL or spool-schema
decoder. The reader does not initialize or modify a library.

`annals_usage::api` exposes the consumption report types and a client for an
explicitly selected reporting executable. `Client::report` returns
`ConsumptionSummary`; `report_details` returns `ConsumptionReport`. The CLI
produces the same types. Nucleus/Codex account normalization, attribution,
coverage, and wire formats remain part of the current reporting behavior.

## Authority, failure, and privacy

Annals library state, delivery and model-run records, and job receipts plus
Nucleus job, attempt, and exact output records are the durable authorities.
Annals Usage owns only the disposable live interpretation. It stores no
telemetry database, aggregate, response projection, or account snapshot.

If an authority cannot be read, the command fails instead of returning stale
retained output. A reported gap identifies missing authoritative coverage;
inspect the underlying records through their owning interfaces. Historical
recalculation requires the underlying Annals and Nucleus records to remain
retained and compatible. Retain those authorities when reports
must remain available.

The report reads private local attribution and output metadata. Keep that
information inside its local security boundary. It reads no Nucleus credential
files and grants no Annals inbox, corpus, Nucleus job, authentication, or
service mutation authority.

Use `annals-usage budget` for a live account-global allowance snapshot. Other
Codex activity shares the account, and there is no supported exact conversion
between delivery tokens and a subscription percentage. The account feature
owns the full explanation.

## Compatibility and limits

The default report scope is the newest 20 deliveries. This contract does not
promise accepted `--limit` bounds, pagination behavior, stable recency ordering
or tie-breaking, throughput, wall-clock latency, an atomic snapshot across
Annals and Nucleus, a visibility bound, or an upstream retention horizon.
`projectionVersion` identifies interpretation, but no exact JSON compatibility,
projection migration rule, cross-release support window, or deprecation period
is promised.

Dependencies on `annals.corpus.explore` and `nucleus.execution.operate` establish
documentation compatibility. They are not dedicated contracts for the private
upstream data surfaces read here. Those Annals and Nucleus data reliances, and
the external rate card, remain explicit resolver gaps. The dependency on
`annals-usage.execution.operate` supplies the shared configuration explanation;
it does not authorize its login operation.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
