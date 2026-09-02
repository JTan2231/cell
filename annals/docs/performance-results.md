# Runtime characteristics

No latency, throughput, percentile, or database-size benchmark is claimed for
this implementation.

## Repository quality gate

`./ci.sh` is the complete checked-in gate. It runs formatting, Clippy, tests,
documentation, and a release build for the `annals` and `annals-usage`
packages under Rust 1.97.1. Other Cell product gates do not replace it.

Focused graph tests also inspect SQLite query plans for parent, child, and
evidence pages. They require the connection-local replay projection's indexes
and reject temporary sorts on those local selectors.

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

For a scheduled inbox job, reaching the unchanged 60-minute timeout without
durable success is the job's terminal processing failure. Annals archives the
job, exits the current activation nonzero, leaves successors for the next
activation, and does not start a second liaison for the timed-out job.

CLI root, relationship, evidence, and search lists are cursor-paged. CLI graph
views are bounded by explicit depth and node-count arguments. Limits must be
valid positive values where the command represents a page size.

`lately` selects source-delivery receipts by one explicit timestamp basis and
returns every delivery in the requested UTC interval. Its work join includes
only labels and content digests; it never loads retained source text or corpus
snapshots.

## Cost shape

The selected revision's `CorpusState` is held in memory while resolving or
validating a reconciliation, browsing, diffing, reverting, or planning a shake.
It contains concepts, explicit edges, and evidence, so state size and
whole-state invariant checks grow with all three. Reaching revision N also
reduces the typed effects from revisions 1 through N; there is intentionally no
trusted snapshot cache.

Applying a pending transition or confirmed shake validates the complete
projected state, then stores only canonical typed differences. Mutation work
includes replay plus state comparison, while history storage grows with actual
effects rather than total corpus size per revision. A mechanically equal result
is stored as `recorded` without a commit; an empty shake likewise creates no
revision.

Graph reads replay the requested state and load it into connection-local
temporary query tables. Root, relationship, evidence, and search pages apply
`LIMIT` before response hydration. Evidence loads only returned byte ranges,
each capped at 8 KiB, and graph edges carry IDs so labels are owned once per
selected concept. Local expansion stops at its depth, node, or internal edge
bound. The returned response is bounded, although the replay and temporary
query projection are whole-state work.

Exact-context examination reuse can avoid external model latency when work,
base revision, prompt version, model, and reasoning effort match a prior
successful run. `--reexamine` bypasses that lookup. No latency claim is made
for either path.

External model latency is not part of CI. Tests exercise an injected runner
and the direct dynamic-tool bridge in isolation; they do not invoke the
external model.
