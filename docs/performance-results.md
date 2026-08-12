# Runtime characteristics

No latency, throughput, percentile, or database-size benchmark is claimed for
this implementation.

## Repository quality gate

`./ci.sh` is the complete checked-in gate and has a hard 60-second wall-clock
limit. It runs formatting, Clippy, tests, documentation, and a release build
under Rust 1.97.1. Exceeding 60 seconds is a CI failure.

## Explicit bounds

- a liaison process has a 30-minute timeout;
- the app-server JSON-RPC transcript is limited to 64 MiB;
- the retained model-error tail is limited to 64 KiB;
- one `work_read` call accepts 1 through 20 regions;
- a region returns at most 12,000 characters and defaults to 4,000;
- a work overview returns at most 16,000 heading-label characters and reports
  when its structure is truncated;
- one work or corpus search tool call accepts 1 through 20 nonempty queries;
- a model search returns 1 through 10 matches per query and defaults to 5; and
- each `work_search` match contains at most 1,000 excerpt characters;
- `corpus_inspect` accepts 1 through 20 paths, returning at most 50 child labels
  and 10 evidence excerpts per concept;
- corpus-search matches include at most three evidence excerpts; and
- model-facing evidence excerpts contain at most 2,000 characters and report
  when the exact quotation is truncated.

The CLI search limit must be positive and defaults to 10.

## Cost shape

Works and current corpus snapshots are held in memory while resolving a
proposal. Applying a change validates and materializes the complete projected
corpus, rebuilds all derived search rows, and stores complete before-and-after
snapshots in the commit. Mutation time and history storage therefore grow with
corpus size.

Current CLI search scans materialized concept labels and paths, sorts matching
candidates, and returns the requested prefix. Historical `show` reads the
stored snapshot for one revision directly. `diff` compares two stored
snapshots. `revert` computes and validates an inverse against current HEAD.

External model latency is not part of CI. Tests exercise an injected runner and
the direct dynamic-tool bridge and private MCP adapter in isolation; they do
not invoke the external model.
