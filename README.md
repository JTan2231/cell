# Annals

Annals is a local CLI for maintaining an evidence-grounded conceptual corpus.
Source works are retained unchanged. Corpus concepts belong to the library, may
be supported by many works, and change only through atomic revision commits.

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
- an installed and authenticated `codex` executable for `annals integrate` or
  `annals inbox run`;
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
annals --library ./annals.db lately
```

`integrate` content-addresses the immutable work by its exact SHA-256 digest
before model examination. It records a provisional, best-current
reconciliation. With `--apply`, a projected state transition is committed; if
the projected corpus is mechanically equal to the base, the reconciliation is
stored with status `recorded` and the revision stays where it is.
Optional free-form annotations are retained with the reconciliation and have
no validation or application semantics.

Every delivered source receives a durable metadata receipt independently of
its content-addressed work. `lately` reports those deliveries over an exact UTC
window without reading or interpreting source text:

```sh
annals --library ./annals.db lately
annals --library ./annals.db lately --since 24h --by modified
annals --library ./annals.db lately \
  --since 2026-08-01 --until 2026-08-15 --channel inbox
```

The default is a rolling seven-day window based on ingestion time. Creation
and modification times are captured from filesystem metadata when available;
first-seen, ingestion, and completion times describe the Annals lifecycle.

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
annals --library ./annals.db lately --since 30d --by completed
annals --library ./annals.db shake
annals --library ./annals.db log
annals --library ./annals.db diff 0 1
annals --library ./annals.db overview --at 1
annals --library ./annals.db revert 1
```

Concept, relationship, evidence, root, and search listings are paged locally.
Graph expansion is bounded by depth and node count and reports a frontier when
more of the graph exists beyond the returned neighborhood.

`shake` reports HEAD's transitively implied parent edges and asks before
removing them in one revision. It preserves every ancestor-descendant pair;
`--yes` supplies noninteractive confirmation.

Every command supports `--json`. Select a library explicitly with `--library`
or `ANNALS_LIBRARY`, or select a TOML config with `--config` or
`ANNALS_CONFIG`. Annals never silently creates or opens `./annals.db`.

The installed macOS `annals` frontend selects
`$HOME/Library/Application Support/Annals/config.toml` when no explicit library
or config selection is present. Consequently, commands such as `annals stats`,
`annals search`, and `annals inbox status` operate on the same user library as
the scheduled inbox worker. An explicit option or environment variable still
selects a different target.

## Scheduled macOS installation

The macOS installation is deliberately user-owned, so Codex running as that
user can update the whole application without administrator credentials. After
state-local Codex authentication is prepared as described in the
[system installation guide](docs/system-installation.md), deploy with:

```sh
./ci.sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/target/release/annals" \
  --codex "$(command -v codex)"
```

The deployer installs `~/.local/bin/annals`, versioned complete releases under
`~/Library/Application Support/Annals/install`, and a user LaunchAgent under
`~/Library/LaunchAgents`. Running the same command again is the unattended
update process. It drains the worker between jobs, takes a consistent database
backup, switches releases atomically, validates the result, and automatically
restores the previous release if launchd cutover fails. Configuration,
credentials, corpus data, logs, and queued or archived sources are retained.

After installation, the user—and Codex running as that user—uses the default
library without `sudo`:

```sh
annals stats
annals validate
annals inbox status
```

Drop complete UTF-8 files into
`$HOME/Library/Application Support/Annals/spool/incoming`. The one-shot worker
runs at login and then receives another wake-up every five minutes while idle.
An activation registers settled arrivals and drains the durable FIFO queue
until it is empty; it has no item-count or lifetime cap. New arrivals are
rescanned between jobs, and each individual liaison is limited to 60 minutes.
The LaunchAgent runs only while that macOS user is logged in and resumes at the
next login. For migration, status, removal, and Linux systemd instructions, see
the [system installation guide](docs/system-installation.md).

See the [documentation index](docs/README.md) for the command, protocol,
architecture, storage, search, and runtime contracts.

The [experiment archive](experiments/README.md) documents the three-chat
baseline, its higher-grade rerun, the original scaled 20-chat comparison, and
the same 20-work comparison under the reconciliation-v2 contract.
