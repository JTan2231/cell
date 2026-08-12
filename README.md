# Annals

Annals is a local CLI that constructs and searches conceptual trees. Every
node contains one string. A child is a materially narrower refinement of its
parent; root, branch, and leaf are consequences of topology rather than node
kinds.

Ingestion accepts naive UTF-8 text, asks a pinned Codex subprocess to propose
one grounded tree, validates the proposal deterministically, and commits the
tree and its complete generation record to SQLite in one transaction. Semantic
quality is model-judged; Annals does not claim an objective conceptual metric.

## Requirements

- Rust 1.97.1 for building this repository;
- an installed and authenticated `codex` executable for `annals ingest`;
- no service process or separate database server.

The embedded Codex bundle invokes `gpt-5.6-terra` with medium reasoning through
standard I/O. The model and reasoning setting are not command-line options.

## Build and use

```sh
./ci.sh
cargo build --release
```

Create a library and generate a tree from a file:

```sh
annals init --library ./annals.db
annals --library ./annals.db ingest ./corpus.txt
annals --library ./annals.db tree list
annals --library ./annals.db search "serializable transactions"
```

Standard input is also accepted. The three resolution values are hard maxima,
not targets:

```sh
cat corpus.txt | annals --library ./annals.db ingest - \
  --node-budget 32 --max-depth 6 --max-children 6
```

Manual homogeneous trees remain available:

```sh
annals --library ./annals.db tree create --text "Database systems"
annals --library ./annals.db node add --parent 1 --text "Transactions"
```

Generated trees cannot be changed with individual node commands. Delete one as
a complete tree with `annals tree delete ROOT_ID`.

Run `annals --help` for the complete command surface. Every command supports
`--json`; the library path resolves from `--library`, then `ANNALS_LIBRARY`,
then `./annals.db`.

See the [documentation index](docs/README.md) for the implemented contracts.
