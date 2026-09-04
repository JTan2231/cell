# Pratica

Pratica is a private local system-integration terms broker. An entrant submits
complete Markdown expectations to a steward scope. Pratica preserves each exact
offer, obtains a bounded steward response through Nucleus, tracks current
assent, and seals an agreement only when every required party assents to the
same current offer on the recorded system basis.

Top-level manifests and Markdown can arrive from borrowed files or standard
input. Once accepted, Pratica retains the normalized descriptor, exact contract
or source snapshots, and replay identity needed to continue without a
caller-managed scratch file; it never deletes supplied or referenced files.

An integration can contain several independent bilateral tracks. Pratica can
review their composition without silently merging or rewriting them, and can
later compare a sealed design agreement with a candidate implementation basis.
Terms remain opaque Markdown; Pratica enforces protocol mechanics rather than a
product-contract schema.

## Build and check

```sh
./pratica/ci.sh
cargo build --release --locked --package pratica
```

## Isolated start

```sh
temporary=$(mktemp -d)
target/release/pratica --database "$temporary/pratica.db" init
target/release/pratica --database "$temporary/pratica.db" doctor
target/release/pratica --database "$temporary/pratica.db" steward list
target/release/pratica --database "$temporary/pratica.db" integration list
target/release/pratica --database "$temporary/pratica.db" agreement list
```

The installed database defaults to
`~/Library/Application Support/Pratica/pratica.db`. Use `--database` or
`PRATICA_DATABASE` for an isolated ledger. Pratica never falls back to a
database in the current directory.

New databases use schema two. Upgrade an existing schema-one database only
while Pratica is quiescent with `pratica migrate --backup ABSOLUTE_PATH`; keep
that private backup with the old binary for rollback.

Start with [the documentation index](docs/README.md). The
[Pratica provider bundle](chancery/provider.json) is the release-matched index
of supported outward promises; after selecting an exact entry, use
`chancery resolve pratica.integration.negotiate` (or the selected ID) for its
normalized boundary and explicit gaps. `release.sh` publishes a Git release and
is not a build or installation command.
