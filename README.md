# Annals

Annals is a local CLI for maintaining an evidence-grounded conceptual corpus.
Source works are retained unchanged. Corpus concepts belong to the library, may
be supported by many works, and change only through one validated proposal and
one atomic revision commit.

The public interface uses work labels, concept paths, and exact quotations.
SQLite identifiers, byte ranges, and sibling positions remain implementation
details.

## Requirements

- macOS or Linux and Rust 1.97.1 to build the repository;
- an installed and authenticated `codex` executable for `annals integrate`;
- no daemon or separate database server.

The liaison uses `gpt-5.6-terra` with medium reasoning. Annals gives it a short
pointer prompt and exactly six session-scoped tools through an isolated Codex
app-server; no shell, web, planning, user-input, or multi-agent tools are
available. The complete work is not placed in the prompt. Its recorded
`submit_change` tool call, rather than its final response, is the deliverable.

## Build and use

```sh
./ci.sh
cargo build --release
```

Create a library, examine a work, review the proposal, and apply it:

```sh
annals --library ./annals.db init
annals --library ./annals.db integrate ./serializable-execution.md \
  --name "Serializable execution"
annals --library ./annals.db change show
annals --library ./annals.db change apply
annals --library ./annals.db show
```

`integrate` retains the immutable work before model examination. It records a
proposal without changing the corpus unless `--apply` is supplied and the
proposal contains no uncertainties. An already retained work can be examined
again by label:

```sh
annals --library ./annals.db integrate --work "Serializable execution"
```

A human or another program can submit the same strict change contract without
invoking a model:

```sh
annals --library ./annals.db change submit request.json \
  --work "Serializable execution" --base 0
```

Inspect current and historical state with language-level output:

```sh
annals --library ./annals.db search "predicate locking"
annals --library ./annals.db log
annals --library ./annals.db diff 0 1
annals --library ./annals.db show --at 1
annals --library ./annals.db revert 1
```

Every command supports `--json`. The library path resolves from `--library`,
then `ANNALS_LIBRARY`, then `./annals.db`.

See the [documentation index](docs/README.md) for the command, protocol,
architecture, storage, search, and runtime contracts.
