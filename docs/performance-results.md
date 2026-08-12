# Performance results

The current homogeneous-node schema and one-row-per-node index do not use the
older passage-based benchmark corpus. No p50, p95, throughput, or database-size
numbers are claimed for this implementation.

## Enforced repository limit

`./ci.sh` is the complete checked-in quality gate and has a hard 60-second
wall-clock limit. It runs, under Rust 1.97.1:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked ...
cargo test --all-features --locked --no-fail-fast
cargo doc --all-features --no-deps --locked
cargo build --all-features --release --locked
```

Exceeding 60 seconds fails the gate.

## Bounded operations

The implementation has these explicit runtime bounds:

- model process-group execution timeout: 30 minutes;
- captured model stdout: at most 16 MiB;
- retained model-error tail: at most 64 KiB;
- search query: at most 4,096 UTF-8 bytes and 32 terms;
- returned search results: 1 through 100;
- lexical candidate pool: 50 through 500 rows.

Model invocation is exercised in tests with local fake executables over the
same standard-I/O boundary. The CI runtime therefore does not measure external
model latency.

## Cost shape

Ingestion holds the raw UTF-8 input, its 8,192-byte transport windows, the
complete prompt, and the accepted proposal in memory. Inference completes
before an immediate SQLite write transaction begins.

Every canonical mutation currently rebuilds one search row per node plus the
FTS5 table. Mutation cost therefore grows with total node count. Search reads a
bounded candidate set and returns one result per matching node.
