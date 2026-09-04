# Exchange Krisis decision accounts

Use one separately provisioned Annals decisions library. Its explicit config
must select its own database and spool and must pin the persistent identity
returned by `annals init --kind decisions`:

```toml
library = "/absolute/path/to/Annals/decisions/annals.db"

[inbox]
root = "/absolute/path/to/Annals/decisions/spool"
minimum_available_bytes = 7000000000

[decision_feed]
expected_library_id = "32-lowercase-hex-characters"
```

Neither acceptance nor feed reads permit `--library` or config fallback.
The selected database must also carry the immutable `decisions` role; an exact
expected ID cannot turn a `general` database into a decisions library.
For this config, direct work add or integration, generic `inbox enqueue`,
`inbox register`, and backlog import are rejected. Scheduled `inbox run` binds
an empty fresh spool or verifies its existing binding and does not register `incoming/`; it
dispatches committed accepted originals and their explicit Annals retry
children only. A generic config cannot admit to or run the bound spool or the
decisions database through an alternate spool or direct library selector.

## Accept one account

```sh
annals --config /absolute/path/to/decisions/config.toml --json \
  inbox accept --producer krisis --key DECISION_ID ACCOUNT.md
```

The file is one regular non-symlink UTF-8 Markdown source no larger than 1 MiB.
Annals validates its fixed section and Source-metadata shape, computes SHA-256,
and derives label `Krisis decision DECISION_ID`. A first call reports
`acceptance: "created"`; the same key and bytes report `"replayed"` with the
original job and time. Different bytes conflict. Keep the Krisis outbox copy
until its caller has durably recorded that exact receipt.

Acceptance means ownership, not processing. It creates no delivery row or
model attempt and can succeed while dispatch is paused. The separately operated
`inbox run` later integrates and immediately applies this decisions library's
jobs under that command's Nucleus contract. Never resubmit an account because
its job failed.

## Consume a fixed prefix

Capture a watermark, then page strictly after a previously retained watermark
or item cursor:

```sh
annals --config /absolute/path/to/decisions/config.toml --json \
  decision-feed watermark
annals --config /absolute/path/to/decisions/config.toml --json \
  decision-feed page --watermark NEW_WATERMARK --after OLD_CURSOR --limit 100
```

Persist each returned event and cursor atomically in the consumer. After an
uncertain consumer commit, request the same page again. Events are immutable
and have stable IDs, so replay is safe. Continue from `next_cursor`; when a page
is empty it is byte-for-byte the submitted `--after` value. Annals records no
consumer acknowledgement.

Do not decode cursor contents or fetch the authority anchor. The feed omits raw
Markdown and general-library content. It does not express confidence, review,
disposition, supersession, relevance, relationships, truth, or current force.

## Recover safely

On an ambiguous acceptance, retry only the same key and bytes. Annals recovers
a published envelope whose database commit was interrupted. Identity, digest,
or envelope mismatch is a stop: inspect supported status and restore the exact
library/spool pair rather than editing SQLite or producer receipts. Low storage
is also a stop, never cleanup authority.
