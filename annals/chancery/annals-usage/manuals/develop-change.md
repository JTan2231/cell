# Change Annals Usage

Annals Usage is a live projection over two authorities: Annals owns delivery
and model-run attribution, while Nucleus owns jobs, attempts, exact model
output, account access, and credentials. Annals Usage must not create a second
telemetry authority.

Before development, read the complete canonical vocabulary and telemetry
contract:

```sh
cd /Users/joey/rust/cell/annals
less docs/vocabulary.md
less docs/telemetry.md
```

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

When derived interpretation changes, update the projection version and exact
tests while retaining the atomic records needed to recalculate history. If a
needed atomic fact is absent, expose a gap rather than synthesizing authority.

## Development workflow

1. Decide whether the requested meaning belongs to Annals Usage, Annals, or
   Nucleus.
2. Make the smallest change in `annals/crates/annals-usage` and its projection
   tests.
3. Update `annals/docs/telemetry.md` and configuration or installation
   contracts when affected.
4. Run:

   ```sh
   cd /Users/joey/rust/cell
   ./annals/ci.sh
   ```

   The Annals product gate has a hard 60-second deadline.
5. Run separately authorized live report, budget, doctor, login, and Annals
   requester canaries only when the changed boundary requires them.

Annals Usage is separately versioned but deployed with Annals. Its source
change does not authorize `annals/release.sh`, which commits, tags, and pushes,
or the installed deployment. Tests and live diagnostics may expose private
delivery attribution, output metadata, and account activity; keep them inside
the local security boundary.
