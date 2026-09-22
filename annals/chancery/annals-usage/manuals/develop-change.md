# Change Annals Usage

Annals Usage projects records from two authorities. Annals owns delivery and
model-run attribution. Nucleus owns jobs, attempts, exact model output, account
access, and credentials. Annals Usage must not create a second
telemetry authority.

Before development, read the complete canonical Semantics repository and
telemetry contract:

```sh
/Users/joey/.local/bin/chancery show semantics.repository.explore
/Users/joey/.local/bin/semantics repository show annals
cd /Users/joey/rust/cell/annals
less docs/telemetry.md
```

Semantics owns contributor terminology; telemetry documentation, code, and
tests remain authoritative for reporting behavior. Never put Semantics
repository output in an Annals liaison prompt.

Run the installed Nucleus manual before changing output decoding, account or
authentication behavior, compatibility, deployment, or any other shared
execution boundary:

```sh
/Users/joey/.local/bin/nucleus manual
```

## Invariants to preserve

- Reports are calculated live and fail when an authority is unavailable.
- No telemetry database, token aggregate, account snapshot, credential copy,
  UI scrape, or offline estimate is retained.
- Exact, cumulative, gap, no-model, pending, and reused coverage remain
  distinguishable.
- Input/output totals do not double-count cached, cache-write, or reasoning
  subsets.
- Unattributed runs and incompatible output remain visible.
- Rate-card equivalents remain comparisons, not invoices.
- Account allowance remains global and is never presented as an Annals
  delivery denominator.

When interpretation changes, update the projection version and relevant tests.
Retain the records needed to recalculate history. If a required record is
absent, report a gap.

## Development workflow

1. Decide whether the requested meaning belongs to Annals Usage, Annals, or
   Nucleus.
2. Make the smallest change in `annals/crates/annals-usage` and its projection
   tests.
3. Update `annals/docs/telemetry.md` and configuration or installation
   contracts when affected.
4. Commit the changes and submit them through the installed CI manager:

   ```sh
   cd /Users/joey/rust/cell
   ./ci.sh submit COMMIT
   ```

   The manager integrates, validates, attempts bounded repairs, deploys, and
   emails the outcome.
5. Verify the retained manager outcome. Login remains a separately authorized
   operation.

Annals Usage is separately versioned but deployed with Annals. Its source
change does not authorize `annals/release.sh`, which commits, tags, and pushes,
or a separate installed deployment. Tests and live diagnostics may expose private
delivery attribution, output metadata, and account activity; keep them inside
the local security boundary.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
