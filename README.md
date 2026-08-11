# Annals

Annals is a local-first CLI for maintaining and searching trees of textual
topics. Each child is a more detailed view of its parent; completed trees end
in source material. The implementation follows the version-one design in the
[documentation index](docs/README.md).

The initial design deliberately uses:

- Rust for the CLI;
- SQLite as the authoritative store;
- SQLite FTS5 for search;
- no embeddings, generated topics, or external services.

## Build and use

The repository pins its validation toolchain in `ci.sh`:

```sh
./ci.sh
cargo build --release
```

Create a library, add a tree, and search it:

```sh
annals init --library ./annals.db
annals --library ./annals.db tree create \
  --title "Databases" --body "Notes about database systems."
annals --library ./annals.db node add \
  --parent 1 --kind source --title "Reference" \
  --body "Serializable transactions prevent several anomalies."
annals --library ./annals.db search "serializable transactions"
```

Run `annals --help`, `annals tree --help`, or `annals node --help` for the full
command surface. Every command also supports `--json`; the library path may be
provided by `--library`, `ANNALS_LIBRARY`, or the default `./annals.db`.

The checked-in relevance, recovery, concurrency, and contract suites run with
`./ci.sh`. The larger ignored benchmark harness and its recorded results are
documented in [performance results](docs/performance-results.md).
