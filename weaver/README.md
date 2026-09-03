# Weaver

Weaver is an independent Rust CLI that runs the five-stage public-facing
narrative workflow through a separately installed Nucleus service. A submission
is durable before the command returns, and a detached Weaver child continues
the work after the submitting process exits. That child reads the exact inputs
for each stage, embeds their contents in an immutable Nucleus request, and alone
writes the returned Markdown into the repository.

The narrative repository remains the authority for its basis, active output
brief, source material, workflow prompts, and current generated outputs. Weaver
owns only the current operational request and its Nucleus correlations. Nucleus
owns execution, authentication, and raw agent-protocol history. Codex receives
no local-execution, web, or requester-owned tool capability. Weaver never
publishes, sends, uploads, or edits a public profile.

The product-owned [`chancery/`](chancery/) bundle publishes Weaver's supported
use, operation, and development capabilities for global discovery. It is
versioned with Weaver and installed with each release, but it is documentation:
the Weaver runtime never invokes Chancery. Use `chancery list`, then read every
plausible entry with `chancery show`. After selecting one exact entry, use
`chancery resolve <ENTRY_ID>` for its complete outward promise, documentation
dependency closure, exact basis, and explicit gaps. Resolution does not check
runtime readiness or authorize an effect, and an unsupported, unspecified, or
uncontracted result remains a gap.

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

Two live macOS canaries establish the process boundary. The prototype
LaunchAgent could not remove a generated output under `~/Documents`. Separately,
a Nucleus job whose invocation working directory was the protected repository
left the Codex app-server stuck in `getcwd` before its protocol handshake. The
second result occurred before the model could read a file or use a tool.

Weaver therefore performs all repository reads and writes in the detached child
of the interactive CLI. For each stage it snapshots only the inputs selected by
that stage's authored contract and includes their labeled contents directly in
the durable job prompt. The Nucleus request names Weaver's private state root as
its read-only working directory, with local execution and web search disabled,
no launch context, and no dynamic toolset. Codex returns Markdown through the
agent protocol and has no repository filesystem to inspect or output path to
mutate.

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
diagnostic and contains no publishable narrative. Generated files are current
working artifacts, not retained run history or factual authority.

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

`submit` prints the run ID after atomically recording the request, wakes the
detached worker, and exits. A nonterminal `wait` periodically ensures a worker
is active, so it is also the explicit recovery command after a logout or
restart. `status`, `wait`, and `cancel` accept an optional run ID; supplying it
prevents the command from accidentally observing or cancelling a later
replacement. Without an ID they select the sole current run. A new submission
may replace only a terminal current run.

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
./packaging/macos/deploy-user.sh \
  --binary "/Users/joey/rust/cell/target/release/weaver"
```

This installs `~/.local/bin/weaver`, private operational state, complete
content-addressed releases, and Weaver's one global Chancery provider selector.
It deliberately installs no LaunchAgent: repository I/O must stay in the
interactive caller's process lineage. An update establishes Weaver maintenance,
lets an active workflow finish, atomically switches the installed release and
provider documentation, and validates them. It also transactionally removes the
exact `org.weaver.worker` service and plist from that prototype, restoring them
if a pre-commit migration fails. It does not stop or replace Nucleus.

See [the macOS installation guide](docs/system-installation.md) for the exact
layout and maintenance behavior.
