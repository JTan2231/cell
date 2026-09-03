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

The registered Semantics repository `annals` defines the shared terms used by
contributors and in conversations about Annals. Discover its read contract
through `semantics.repository.explore`, then query it with `semantics
repository show annals`. Code, tests, and these component documents remain
authoritative for actual behavior. Semantics repository output is not content
supplied to the liaison or part of the runtime contract.

## Requirements

- macOS or Linux and Rust 1.97.1 to build the Cell workspace;
- an installed, running, and authenticated Nucleus service for `annals
  integrate` or `annals inbox run`.

Annals is an independent product within the Cell monorepo and root Cargo
workspace. From the Cell root its packages live under `annals/crates/annals`
and `annals/crates/annals-usage`; this README and the product scripts are under
`annals/`. Nucleus is another independent product in the same workspace, and
the Annals packages use its local workspace crates directly. The shared lockfile
is `../Cargo.lock`, and Annals release builds produce `../target/release/annals`
and `../target/release/annals-usage` when commands are run from this directory.

The liaison defaults to high quality: `gpt-5.6-sol` with max reasoning.
`--quality low` selects `gpt-5.6-luna` with medium reasoning, and `--quality
medium` selects `gpt-5.6-terra` with medium reasoning. `--model` provides an
exact model override. Annals gives the liaison a short pointer prompt and
exactly nine session-scoped tools through a Nucleus-owned isolated Codex app-server; no
shell, web, planning, user-input, or multi-agent tools are available. The
complete work is not placed in the prompt. The liaison starts a reconciliation
draft, corrects only operations Annals identifies when needed, and may inspect
or discard that draft. A recorded reconciliation side effect, rather than the
model's final response, is the deliverable.

## Build and use

```sh
./ci.sh
cargo build --release --package annals --package annals-usage
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
reconciliation. Independently valid operations remain staged across correction
calls, while plain-language source hints identify only the operations needing
attention. Annals records the complete server-assembled request automatically
when every operation works together. With `--apply`, a projected state
transition is committed; if the projected corpus state is mechanically equal
to the base, the reconciliation is stored with status `recorded` and the
revision stays where it is.
Optional free-form annotations are retained with the reconciliation. Their
shape and text are contract-validated, but they have no corpus-validation or
application semantics.

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

A fresh inbox delivery whose exact bytes already select a retained work is
recorded with `duplicate` retention and result `retained`, then archived under
`duplicates/` without another examination, reconciliation, or commit. Explicit
manual `integrate` commands keep their normal integration behavior for an
already retained work.

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
`annals search`, `annals inbox status`, and the inbox control commands operate
on the same user library and spool as the scheduled inbox worker. An explicit
option or environment variable still selects a different target.

The companion CLI reports the token consumption attributed to source
deliveries and reads the current account-wide Codex allowance:

```sh
annals-usage report
annals-usage budget
annals-usage doctor
```

The report is calculated live from Nucleus model output and Annals attribution;
it stores no reporting database. It distinguishes exact measurements,
cumulative fallbacks, deliveries that invoked no model, and observations with
gaps. Token categories overlap, and the Codex subscription percentage has no
exposed token denominator, so it cannot be divided into an exact per-delivery
subscription share. See
[Consumption telemetry](docs/telemetry.md) for the accounting contract.
The independently versioned
[Annals Usage Chancery provider](chancery/annals-usage/provider.json) publishes
the installed reporting promise and its uncontracted upstream boundaries;
`chancery resolve annals-usage.consumption.inspect` assembles those claims
without treating broad documentation dependencies as data contracts. The
[Annals provider](chancery/annals/provider.json) remains a separate release and
authority.

## Release

The two Annals packages are versioned independently in
`crates/annals/Cargo.toml` and `crates/annals-usage/Cargo.toml`. Annals releases
use annotated tags named `annals-vMAJOR.MINOR.PATCH`; `annals-usage` releases use
`annals-usage-vMAJOR.MINOR.PATCH`. From a clean `main` branch that exactly
matches `origin/main`, release Annals with one of:

```sh
./release.sh --patch
./release.sh --minor
./release.sh --major
```

The explicit `annals` package name is equivalent. Release the telemetry package
separately with:

```sh
./release.sh annals-usage --patch
./release.sh annals-usage --minor
./release.sh annals-usage --major
```

An empty `origin` is also accepted for the first publication. The script bumps
only the selected package version, refreshes the Cell root `Cargo.lock`, runs
the complete Annals `ci.sh` suite on the bumped tree, creates a
release commit, tags that commit, and atomically pushes `main` and only that
tag. Both package versions are independent of library schema versions, corpus
revisions, and the exact content-addressed release IDs used by the macOS
deployer.

## Scheduled macOS installation

The macOS installation is deliberately user-owned. Deploy Nucleus first,
import or establish authentication through Nucleus, and verify that its daemon
is healthy. Then deploy Annals with the Nucleus executable and socket:

```sh
./ci.sh
./packaging/launchd/deploy-user.sh \
  --binary "$PWD/../target/release/annals" \
  --usage-binary "$PWD/../target/release/annals-usage" \
  --nucleus "$HOME/.local/bin/nucleus" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock"
```

The deployer installs `~/.local/bin/annals` and
`~/.local/bin/annals-usage`, versioned complete releases under
`~/Library/Application Support/Annals/install`, and a user LaunchAgent under
`~/Library/LaunchAgents`. It selects the Nucleus socket in Annals'
configuration and keeps the companion `usage.toml` beside the Annals library.
Running the same command again is the unattended update process. It drains the
worker between jobs, takes a consistent Annals library backup, switches both
binaries through one release selector, updates both configurations within a
rollback-protected transaction, verifies the result, and automatically
restores the previous release if launchd cutover fails. Configuration,
library data, logs, the operator pause state, and
queued or archived sources are retained.
For a later operator-requested rollback, `install/last-update.json` names a
durable snapshot containing the prior configs and LaunchAgent; restore that
snapshot together with the `previous` release selector. Credentials are not
included, and Nucleus state remains outside the Annals rollback boundary.

If Nucleus authentication expires, queued jobs remain unattempted behind the
authenticated dispatch preflight. Pause Annals, run `annals-usage login
--device-auth` (which delegates to `nucleus auth login --device-auth`), verify
with `annals-usage doctor`, canary, and resume.

The current schema is version 4. Normal deployment additively migrates a
version-3 library to add bounded retry-event provenance while retaining its
contents and spool. Version 3 remains the intentional fresh-state boundary;
the one-time cutover from an older schema adds `--fresh-state` to the command
above. That mode archives the old library, its sidecars, and the spool
as one rollback generation, verifies the empty replacement, imports the
uncompleted backlog while preserving priority choices and sequence order
within each lane, explicitly resumes it, and only then wakes launchd. See the
[system installation guide](docs/system-installation.md) for the guarded
sequence and recovery receipt. Any obsolete `usage.db` and sidecars are
discarded after a successful cutover rather than entering that generation.

After installation, the user—and Codex running as that user—uses the default
library without `sudo`:

```sh
annals stats
annals inbox status
annals inbox pause
annals inbox register
annals inbox enqueue --priority ./report.md
annals inbox prioritize JOB_ID
annals inbox deprioritize JOB_ID
annals inbox retry preview --from FIRST_FAILED_JOB --through LAST_FAILED_JOB
annals inbox retry status
annals inbox resume
```

Drop complete UTF-8 files into
`$HOME/Library/Application Support/Annals/spool/incoming`. The one-shot worker
runs at login and then receives another wake-up every five minutes while idle.
Registration moves each settled arrival into a durable normal-lane `queued/`
job with an immutable sequence; `annals inbox register` exposes that admission
step without starting a delivery. `annals inbox enqueue` copies explicitly
selected files directly into queued jobs and `--priority` selects the priority
lane. Named queued jobs can move between lanes with `prioritize` and
`deprioritize` without changing their sequences. An activation performs the
same registration, dispatches priority jobs before normal jobs in sequence
order within each lane, and drains the queue until it is empty; it has no
item-count or lifetime cap. Before each new claim, Annals requires the
configured available-space reserve on both its library and spool filesystems.
If either is low, the job remains queued and a later wake-up checks again
without requiring an operator pause or `resume`. New arrivals are rescanned
between jobs. New works
enter the liaison flow, while fresh exact-byte duplicates complete at
retention and move to `duplicates/`; each individual liaison is limited to 60
minutes.

`annals inbox pause` lets the current delivery finish and prevents the next
queued job from starting. Timer activations continue registering arrivals
while paused, and direct enqueue and queued-job priority changes remain
available. While paused and quiescent, two inclusive failed-job anchors can be
previewed and started as one durable retry event. The event freezes its exact
membership, preserves every original failure, and records the outcome of each
fresh linked child; there is no retry-all command. `annals inbox resume` refuses
an unfinished retry event, and otherwise reopens dispatch without starting a
worker. Use `annals inbox run` for immediate processing or wait for the next
LaunchAgent wake-up. The operator pause is independent of deployer maintenance
and survives updates. The LaunchAgent runs only while that macOS user is logged
in and wakes again at the next login. For retry, migration, status, removal,
and Linux systemd instructions, see the [system installation
guide](docs/system-installation.md).

See the [documentation index](docs/README.md) for the command, protocol,
architecture, storage, telemetry, search, and runtime contracts.

The [experiment archive](experiments/README.md) documents the three-chat
baseline, its higher-grade rerun, the original scaled 20-chat comparison, and
the same 20-work comparison under the reconciliation-v2 contract.
