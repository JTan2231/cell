# Performance results

These measurements exercise the deterministic release-mode corpus in
`tests/operational.rs`. The fixture uses ten broad branches, a 20-level deep
branch, a 1,600-word source that produces two overlapping passages, a common
term in every source, and stable rare markers. It contains exactly the stated
number of search units; no random seed is used.

## Reproduce

```sh
cargo build --release --locked
cargo test --release --locked --test operational \
  deterministic_1k_and_10k_scale_samples -- --ignored --nocapture
cargo test --release --locked --test operational \
  deterministic_100k_unit_reindex_and_search_smoke -- --ignored --nocapture
```

Search and mutation measurements include release-binary startup, SQLite open,
JSON serialization, and process shutdown. Each p50/p95 value comes from 20
samples after a warm-up query with a warm filesystem cache. The bulk seed is a
single test-helper transaction.

## Recorded environment

- Date: 2026-08-11
- Host: Apple M4 Mac mini, 10 cores, 16 GB memory, arm64 macOS
- Bundled SQLite: 3.50.2 with FTS5
- Fixture: `deterministic-v1`, no randomness

## Scale and rebuild

| Search units | Bulk seed | Seed throughput | Full reindex | Database size |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 4.646 ms | 215,221 units/s | 38.289 ms | not recorded |
| 10,000 | 13.978 ms | 715,412 units/s | 331.214 ms | not recorded |
| 100,000 | 160.226 ms | 624,118 units/s | 3.711 s | 76,713,984 bytes |

## Warm 100k-unit latency

| Operation | p50 | p95 | Gate |
| --- | ---: | ---: | ---: |
| Global rare-term search, 10 results | 22.39 ms | 22.67 ms | <150 ms |
| Global common-term search, 10 results | 75.07 ms | 76.82 ms | <150 ms |
| Scoped rare-term search, 10 results | 22.37 ms | 24.00 ms | <150 ms |
| Scoped common-term search, 10 results | 105.80 ms | 108.32 ms | <150 ms |
| Shallow breadcrumb lookup | 76.15 ms | 77.70 ms | recorded |
| Deep breadcrumb lookup | 77.66 ms | 79.46 ms | recorded |
| Indexed node append | 22.99 ms | 23.51 ms | <50 ms |
| Indexed body edit | 22.85 ms | 23.51 ms | <50 ms |
| Indexed no-position move | 23.09 ms | 29.90 ms | <50 ms |
| Indexed leaf delete | 22.82 ms | 23.24 ms | <50 ms |

All provisional 100k-unit gates pass. The non-ignored operational tests also
exercise a stable reader snapshot while a CLI writer commits under WAL,
transaction rollback after forced indexing failures, and backup-copy integrity
recovery through `validate` plus `reindex`.
