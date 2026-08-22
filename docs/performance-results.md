# Runtime characteristics

No latency, throughput, percentile, or database-size benchmark is claimed for
this implementation.

## Repository quality gate

`./ci.sh` is the complete checked-in gate and has a hard 60-second wall-clock
limit. It runs formatting, Clippy, tests, documentation, and a release build
under Rust 1.97.1. Exceeding 60 seconds is a CI failure.

Focused graph tests also inspect SQLite query plans for parent, child, and
evidence pages. They require revision-leading indexes and reject temporary
sorts on those local selectors.

## Explicit liaison bounds

- a liaison process has a 60-minute timeout;
- the app-server JSON-RPC transcript is limited to 64 MiB;
- the retained model-error tail is limited to 64 KiB;
- one `work_read` call accepts 1 through 20 regions;
- a region returns at most 12,000 characters and defaults to 4,000;
- a work overview returns at most 16,000 heading-label characters and reports
  truncation;
- one work or corpus search call accepts 1 through 20 nonempty queries;
- a work-search query returns 1 through 10 matches and defaults to 5;
- each work-search match contains at most 1,000 excerpt characters;
- a corpus-search query returns 1 through 50 matches and defaults to 10, with
  its own continuation cursor;
- `corpus_inspect` accepts 1 through 20 inspection requests in one call;
- direct parent, child, evidence, and root pages contain 1 through 100 items
  and default to 25;
- a concept inspection previews at most 20 items per relation and defaults to
  5;
- a local graph inspection has depth 0 through 5, returns at most 500 concepts,
  and reports a frontier when truncated; and
- model-facing evidence excerpts contain at most 2,000 characters and report
  when the exact quotation is truncated.

CLI root, relationship, evidence, and search lists are cursor-paged. CLI graph
views are bounded by explicit depth and node-count arguments. Limits must be
valid positive values where the command represents a page size.

`lately` selects source-delivery receipts by one explicit timestamp basis and
returns every delivery in the requested UTC interval. Its work join includes
only labels and content digests; it never loads retained source text or corpus
snapshots.

## Cost shape

Works and selected corpus snapshots are held in memory while resolving and
validating a reconciliation. A corpus snapshot contains concepts, explicit
edges, and evidence, so basic materialization grows with all three. Cycle
checks and derived root/leaf classification also traverse the edge set.

Applying a pending transition or confirmed shake validates and materializes the
complete projected corpus state and retains complete corpus snapshots in
history. Mutation time and history storage therefore grow with corpus size. A
projected corpus state mechanically equal to its base is stored as a `recorded`
reconciliation without rebuilding materialized state or creating a commit or
revision; an empty shake likewise creates no commit.

Ordinary graph reads use a revision-scoped database facade and allocate only a
bounded view between SQLite and presentation. Root, relationship,
evidence, and search pages apply `LIMIT` before response hydration. Evidence
loads only returned byte ranges, each capped at 8 KiB, and graph edges carry IDs
throughout so labels are owned once per selected concept. Local expansion stops
at its depth, node, or internal edge bound. Its exact frontier is derived from
stored revision degrees and the bounded returned edge set rather than
additional edge scans.

Whole snapshots are still intentionally loaded by reconciliation resolution,
validation, diff, reversion, and shake planning. Immutable relational revision
rows retain the existing full-snapshot history storage asymptotics while making
interactive historical reads subset-addressable.

Exact-context examination reuse can avoid external model latency when work,
base revision, prompt version, model, and reasoning effort match a prior
successful run. `--reexamine` bypasses that lookup. No latency claim is made
for either path.

External model latency is not part of CI. Tests exercise an injected runner
and the direct dynamic-tool bridge in isolation; they do not invoke the
external model.
