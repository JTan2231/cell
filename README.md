# Annals

Annals is a local CLI for maintaining an evidence-grounded conceptual corpus.
Source works are retained unchanged. Corpus concepts belong to the library, may
be supported by many works, and change only when one validated reconciliation
produces an atomic revision commit.

The public interface uses work labels, durable concept IDs such as `c42`, and
exact quotations. Concepts form an unordered directed acyclic graph: an edge
points from a broader concept to a narrower one, and a concept may have several
parents. Labels may repeat. There is no canonical path, primary parent, sibling
order, or move operation.

Source byte ranges and non-concept SQLite identifiers remain implementation
details. Evidence belongs to a concept as a whole rather than to one of its
parent edges, and every derived leaf must remain evidence-grounded.

## Requirements

- macOS or Linux and Rust 1.97.1 to build the repository;
- an installed and authenticated `codex` executable for `annals integrate`;
- no daemon or separate database server.

The liaison defaults to high quality: `gpt-5.6-sol` with max reasoning.
`--quality low` selects `gpt-5.6-luna` with medium reasoning, and `--quality
medium` selects `gpt-5.6-terra` with medium reasoning. `--model` provides an
exact model override. Annals gives the liaison a short pointer prompt and
exactly six session-scoped tools through an isolated Codex app-server; no
shell, web, planning, user-input, or multi-agent tools are available. The
complete work is not placed in the prompt. Its recorded
`submit_reconciliation` tool call, rather than its final response, is the
deliverable.

## Build and use

```sh
./ci.sh
cargo build --release
```

Create a library, examine a work, review the reconciliation, and apply its
projected corpus transition:

```sh
annals --library ./annals.db init
annals --library ./annals.db integrate ./serializable-execution.md \
  --name "Serializable execution"
annals --library ./annals.db change show
annals --library ./annals.db change apply
annals --library ./annals.db overview
annals --library ./annals.db roots
```

`integrate` content-addresses the immutable work by its exact SHA-256 digest
before model examination. It records a provisional, best-current
reconciliation. With `--apply`, a projected state transition is committed; if
the projected corpus is mechanically equal to the base, the reconciliation is
stored with status `recorded` and the revision stays where it is.
Optional free-form annotations are retained with the reconciliation and have
no validation or application semantics.

An already retained work can be selected by label. Annals reuses a successful
examination only when the work, corpus revision, prompt version, model, and
reasoning effort all match:

```sh
annals --library ./annals.db integrate --work "Serializable execution"
```

Use `--reexamine` to bypass that exact-context reuse:

```sh
annals --library ./annals.db integrate --work "Serializable execution" \
  --reexamine
```

Choose a lower-cost preset or an exact model when needed:

```sh
annals --library ./annals.db integrate --work "Serializable execution" \
  --quality low
annals --library ./annals.db integrate --work "Serializable execution" \
  --quality medium --model gpt-5.6-sol
```

An explicit model changes only the model; `--quality` continues to select its
reasoning effort and defaults to `high`.

A human or another program can submit the same strict reconciliation contract
without invoking a model:

```sh
annals --library ./annals.db change submit request.json \
  --work "Serializable execution" --base 0
```

Browse current and historical state with bounded, language-level output:

```sh
annals --library ./annals.db search "predicate locking"
annals --library ./annals.db overview
annals --library ./annals.db roots --limit 25
annals --library ./annals.db concept show c42
annals --library ./annals.db graph c42 --direction both --depth 2
annals --library ./annals.db log
annals --library ./annals.db diff 0 1
annals --library ./annals.db overview --at 1
annals --library ./annals.db revert 1
```

Concept, relationship, evidence, root, and search listings are paged locally.
Graph expansion is bounded by depth and node count and reports a frontier when
more of the graph exists beyond the returned neighborhood.

Every command supports `--json`. The library path resolves from `--library`,
then `ANNALS_LIBRARY`, then `./annals.db`.

See the [documentation index](docs/README.md) for the command, protocol,
architecture, storage, search, and runtime contracts.

The [experiment archive](experiments/README.md) documents the three-chat
baseline, its higher-grade rerun, the original scaled 20-chat comparison, and
the same 20-work comparison under the reconciliation-v2 contract.
