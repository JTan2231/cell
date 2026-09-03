# Geste

Geste is a private local episode casebook. It records bounded accounts of work
against stable source anchors, then lets an agent search prior cases by the
shape of a new request and inspect a candidate's report and provenance graph.

Geste owns the episode account, not the records it cites. A search result is a
precedent candidate, not a policy or a claim that the earlier solution applies
unchanged.

## Build and check

```sh
./geste/ci.sh
cargo build --release --locked --package geste
```

## Isolated smoke

```sh
temporary=$(mktemp -d)
target/release/geste --database "$temporary/geste.db" init
target/release/geste --database "$temporary/geste.db" episode create episode.json
target/release/geste --database "$temporary/geste.db" search "request shape"
```

The installed database defaults to
`~/Library/Application Support/Geste/geste.db`. Use `--database` or
`GESTE_DATABASE` for an isolated casebook. Geste never falls back to a database
in the current directory.

Start with [the documentation index](docs/README.md). The
[Geste provider bundle](chancery/provider.json) is the release-matched index of
supported outward promises; after selecting an exact entry, use
`chancery resolve geste.episode.explore` (or the selected ID) for its normalized
boundary and explicit gaps. `release.sh` publishes a Git release and is not a
build or installation command.
