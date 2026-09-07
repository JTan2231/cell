# Weaver

Weaver is an independent Rust CLI that builds public narratives in five stages
through a separately installed Nucleus service. Weaver stores each submission
before the command returns. A detached Weaver child continues after the
submitting process exits. It reads each stage's inputs, includes them in an
immutable Nucleus request, and writes the returned Markdown into the repository.

The narrative repository remains the authority for its basis, active output
brief, source material, workflow prompts, and current generated outputs. Weaver
owns only the current operational request and its Nucleus correlations. Nucleus
owns execution, authentication, and raw agent-protocol history. Codex receives
no local-execution, web, or requester-owned tool capability. Weaver never
publishes, sends, uploads, or edits a public profile.

The product-owned [`chancery/`](chancery/) bundle publishes Weaver's supported
use, operation, and development capabilities for global discovery. It is
versioned with Weaver and installed with each release, but it is documentation:
the Weaver runtime never invokes Chancery. Use `chancery list`, then read each
applicable entry with `chancery show`. Select one entry and use
`chancery resolve <ENTRY_ID>` to read its contract, documentation dependencies,
basis, and gaps. Resolution does not check runtime readiness or authorize an
action. Unsupported, unspecified, and uncontracted results remain gaps.

## Requirements

- macOS or Linux and Rust/Cargo 1.97.1 to build;
- a healthy, authenticated per-user Nucleus service compatible with invocation
  protocol 1;
- on macOS, an interactive caller with access to the selected narrative
  repository; the detached Weaver worker preserves that caller's file-access
  context while the Nucleus-launched Codex process never enters the repository.

Weaver is a member of the Cargo workspace rooted at `/Users/joey/rust/cell`.
The workspace supplies its Nucleus client, contract, and third-party
dependencies while Weaver retains its independent release, domain authority,
state, and success rules. Runtime compatibility follows invocation protocol 1,
not a lockstep product version.

## Privacy and execution boundary

Weaver's repository access belongs to the interactive process lineage. A
launchd-owned process may lack access to protected repositories, and Nucleus
must not use the protected repository as its invocation working directory.

The interactive CLI's detached child performs all repository reads and writes.
For each stage, it snapshots the inputs selected by that stage's authored
contract. It includes their labeled contents in the durable job prompt.
The Nucleus request uses Weaver's private state root as a read-only working
directory. Local execution and web search are disabled. The request has no
launch context or dynamic toolset. Codex returns Markdown through the agent
protocol. It cannot inspect repository files or write output files.

This boundary moves private input bytes into operational state. While a stage
is active, `current.json` contains its exact persisted request. Nucleus retains
the complete request and raw protocol exchange in its private database; its
operational logs are sensitive too. Protect both state roots as sensitive data;
Weaver creates no second prompt log or completed-run archive.

## Repository contract

The selected repository must contain:

```text
narratives/NAME/
  basis.md
  brief.md
workflow/narrative/
  common.md
  voice.md
  stories.md
  themes.md
  compose.md
  review.md
  finalize.md
```

A successful build replaces the current five generated outputs in place:

```text
narratives/NAME/
  01-stories/output.md
  02-themes/output.md
  03-draft/output.md
  04-review/output.md
  05-final/output.md
```

The review verdict is `PASS`, `REVISE`, or `BLOCKED`. A blocked result is a
diagnostic and contains no publishable narrative. Generated files are the
current working artifacts for the five editorial stages.

## Build and use

```sh
cd /Users/joey/rust/cell/weaver
./ci.sh
cargo build --manifest-path ../Cargo.toml --package weaver --release

/Users/joey/rust/cell/target/release/weaver \
  --repo '/Users/joey/Documents/job finding' \
  submit how-i-work

/Users/joey/rust/cell/target/release/weaver wait RUN_ID
/Users/joey/rust/cell/target/release/weaver \
  --repo '/Users/joey/Documents/job finding' \
  check how-i-work
```

CI builds and uses the Chancery candidate from the Cell workspace to validate
Weaver's provider bundle. Weaver has no runtime dependency on Chancery.

`submit` atomically records the request, prints the run ID, starts the detached
worker, and exits. For a nonterminal run, `wait` periodically starts a worker
if necessary. Use it to recover after a logout or restart.
`status`, `wait`, and `cancel` accept an optional run ID. Supply it to prevent
the command from selecting a later replacement run. Without an ID, these
commands select the sole current run. A new submission can replace only a
terminal current run.

Use `--repo PATH` or `WEAVER_REPO` to select the narrative repository. The
default is the current directory. Use `--state-dir PATH` or
`WEAVER_STATE_DIR` to select operational state; the installed default is
`~/Library/Application Support/Weaver`.

See [the documentation index](docs/README.md) for the complete CLI,
architecture, recovery, and installation contracts.

## Release

Weaver releases use annotated tags named `weaver-vMAJOR.MINOR.PATCH`. From a
clean Cell `main` branch that exactly matches `origin/main`, run one of:

```sh
./release.sh --patch
./release.sh --minor
./release.sh --major
```

The script bumps `weaver/crates/weaver/Cargo.toml` and the Chancery provider
release together, refreshes the root `Cargo.lock`, runs Weaver's complete CI
suite, verifies the release binary version, commits and tags the release, and
atomically pushes `main` with its tag. It is a publication command, not a build
command.

## User-owned macOS deployment

After a release build, deploy without administrator privileges:

```sh
<TESTED_WEAVER_INSTALL> install \
  --binary <TESTED_WEAVER_BINARY> \
  --bundle /Users/joey/rust/cell/weaver/chancery
```

Use the `weaver-install` executable from the same sealed tested candidate as
`weaver`; its version must match the provider bundle. The Rust installer uses
the shared `cell-install` crate for exact artifacts, locks, selectors, and
compensation. It installs `~/.local/bin/weaver`, `~/.local/bin/weaver-install`,
private operational state, complete content-addressed releases, and Weaver's
global Chancery provider selector. It installs no LaunchAgent. Repository I/O
must stay in the interactive caller's process lineage.

An update starts Weaver maintenance and lets an active workflow finish. It
then atomically switches and validates the installed release and provider
documentation. It also removes the prototype `org.weaver.worker` service and
plist in the same transaction. A pre-commit migration failure restores them.
The update does not stop or replace Nucleus.

See [the macOS installation guide](docs/system-installation.md) for the exact
layout and maintenance behavior.

## Rust interface

`weaver::api` owns workflow status and receipt types, stage layout, editorial
verdicts, and the typed client for an explicitly selected Weaver executable.
The provider renders its existing CLI output from these same types; the client
owns decoding that output. Calls use Weaver's executable so detached workers
retain the existing process lineage. Private `current.json` records and
Nucleus requests are not exported. A terminal failed or blocked run remains a
typed domain outcome returned by `Client::wait`.
