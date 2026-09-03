# Inspect Annals model consumption

Annals Usage calculates model consumption caused by Annals examinations. It is
a separate companion CLI and projection version, not a database or runtime
authority.

```sh
/Users/joey/.local/bin/annals-usage report
/Users/joey/.local/bin/annals-usage report --json --limit <COUNT>
```

Each invocation joins current Annals source-delivery and model-run attribution
with Nucleus jobs, attempts, and ordered model-output records. Inbox receipt
discovery covers processing and every terminal archive. Manual examinations
without a source-delivery correlation remain visible as unattributed runs.

## Read coverage before totals

Every delivery has one coverage value:

- `exact`: distinct response-usage records cover every attempt and agree with
  any final cumulative total;
- `cumulative`: a consistent final thread total is used for at least one
  attempt;
- `gap`: required output is missing, incompatible, or unusable, so no complete
  total is claimed;
- `no-model`: no liaison ran, such as a fresh duplicate or pre-retention
  permanent failure;
- `pending`: delivery or runtime work remains active; or
- `reused-no-new-usage`: Annals reused an exact-context examination.

Do not replace `gap` with an estimate. A retry child is a distinct delivery;
its new model work belongs to the child while the original remains attributed
to the original. Use `annals inbox retry status` to understand their domain
relationship.

Token categories overlap. Cached and cache-write input are subsets of input;
reasoning is a subset of output; total is input plus output. Never add those
subsets a second time.

`knownCreditEquivalent` applies the supported model rate card to measured
ordinary input, cached input, and output. It is a comparison, not an invoice or
a subscription share. Cache-write input may remain explicitly unpriced.

## Authority, failure, and privacy

Nucleus output records and job state plus Annals delivery, model-run, and job
receipt records are the durable sources. The report is created in memory and
stores no aggregate, response projection, account snapshot, or telemetry
database. If an authority cannot be read, the command fails rather than
returning stale retained output.

The report reads private local attribution and output metadata. Historical
recalculation depends on retaining the underlying Annals and Nucleus
authorities. Use `annals-usage budget` for a live account-global allowance
snapshot; that snapshot cannot be converted into an exact per-delivery share.

The report defaults to the newest 20 source deliveries. This contract does not
promise the accepted `--limit` bounds, pagination, stable recency tie-breaking,
an atomic snapshot across Annals and Nucleus, a wall-clock visibility bound, or
an upstream retention horizon. `projectionVersion` identifies the live
interpretation, but no exact JSON compatibility, migration, or deprecation
window is promised.

The installed dependencies on `annals.corpus.explore` and
`nucleus.execution.operate` establish documentation-contract compatibility;
they are not dedicated contracts for the private upstream records this report
reads. Those Annals and Nucleus data reliances, and the external rate card with
no pinned contract version or digest, remain explicit resolver gaps rather
than inferred public data surfaces.
