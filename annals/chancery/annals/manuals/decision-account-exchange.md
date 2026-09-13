# Exchange Krisis decision documents

Use one separately provisioned Annals decisions library. Its explicitly selected config
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
a fresh empty spool or verifies its existing binding. It does not register
`incoming/` files. It dispatches only committed accepted originals and their
explicit Annals retry children. A generic config cannot admit to or run the bound spool or the
decisions database through an alternate spool or direct library selector.

A registered decisions library may instead use
`annals library NAME inbox accept ...` or `annals library NAME decision-feed ...`.
Named scope selects its registered identity-bound config and preserves the same
document intake checks, admission kind, spool binding, and feed contract. The raw
operator-path form still requires explicit `--config` and rejects `--library`.
Selecting a name or changing librarian instructions cannot relax admission.

## Accept one document

```sh
annals --config /absolute/path/to/decisions/config.toml --json \
  inbox accept --producer krisis --key DOCUMENT_KEY DOCUMENT.md
```

The file is one regular non-symlink, nonblank UTF-8 text source no larger than
1 MiB. Annals needs accessible text. It does not require a filename extension,
Markdown structure, headings, a content schema, source metadata, source lookup,
or a decision-fitness check. Krisis supplies the document; interpretation is
handled by the librarian agent under the configured library instructions.

Annals computes SHA-256 from the exact bytes. The ordinary work label comes
from the filename stem; file metadata comes from the filesystem. Annals creates
the job identity, queue order, acceptance time, and later delivery metadata.
A first call reports `acceptance: "created"`; the same key and bytes report
`"replayed"` with the original job and time. Different bytes conflict. Keep the
Krisis outbox copy until its caller has durably recorded that exact receipt.

Acceptance transfers ownership to Annals. It creates no delivery row or model
attempt and can succeed while dispatch is paused. The separately operated
`inbox run` later integrates and immediately applies this decisions library's
jobs under that command's Nucleus contract. Never resubmit a document because
its job failed.

## Consume a fixed prefix

Get a starting cursor with `decision-feed start` when reading existing documents
for the first time. It returns the same `Watermark` shape with an opaque cursor
before the first acceptance. It reads the selected library without changing it.
Capture a current watermark, then page after the starting cursor, a previously
retained watermark, or an item cursor:

```sh
annals --config /absolute/path/to/decisions/config.toml --json \
  decision-feed start
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

The exchange wire contract is version 2. Each event contains `cursor`,
`event_id`, `document_id` (the producer key), `source_name`, `source_sha256`,
`accepted_at` (RFC3339 acceptance time), and `document` (complete unchanged UTF-8 text). These are Annals
transport and storage facts, not required fields inside the document. Accepted
text is available before inbox dispatch and after later success or failure.
The feed makes no claim about the agent's interpretation or retention outcome.

A request accepts limits 1 through 200. A page also contains at most 4 MiB of
document bytes, excluding JSON encoding and transport fields. A nonempty page
can therefore have fewer events than requested. Continue until an empty page;
do not infer completion from a short page. Do not decode opaque cursor contents.

Feed output contains the full private document, including conversation text
when Krisis supplied it. Restrict access and logging accordingly. General-library
content does not appear. Annals records no consumer acknowledgements.

## Recover safely

On an ambiguous acceptance, retry only the same key and bytes. Annals recovers
a published envelope whose database commit was interrupted. Identity, digest,
or envelope mismatch stops acceptance. Inspect supported status and restore the
exact library/spool pair. Do not edit SQLite or producer receipts. Low storage
also stops acceptance and does not authorize cleanup.

## Rust access

Local Rust callers use `annals_api::Client::new(binary, decisions_config)` with
`accept`, `start`, `watermark`, and `read_page`. The client and Annals share the exported
receipt, watermark, page, and `AcceptedDocumentEvent` structs. Annals owns
admission and accepted-content access; no Krisis content codec is involved. The client invokes
the same CLI with the same effects and does not bypass its identity or storage
boundaries. Callers retain exact target binding, outbox, and cursor policy.

## Existing libraries

Library schema 7 preserves earlier acceptance sequences, event IDs, producer
keys, digests, jobs, times, and cursor meaning. Old structured projections remain
in a historical table. The current feed reads each original document from its
Annals-owned envelope. Historical receipt labels remain valid; new jobs use
ordinary filename labels. Exchange version, library schema, producer receipt,
and cursor version are separate compatibility axes. Consumers must use exchange
contract 2; the content itself has no schema version.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
