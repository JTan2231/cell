# Conversations runtime and installation guarantees

Use this feature to select the local Codex reader, understand its diagnostic and
process boundary, interpret readiness, or rely on installation and recovery.
Conversations owns its short-lived adapter and installed release. The ChatGPT
app owns its bundled Codex executable and updates. App Server owns history
availability and storage compatibility. This contract grants no task mutation,
authentication, installation, transcript disclosure, or unrelated process authority.

Read `conversations.installation.operate` for installation and recovery steps.
Read `conversations.history.explore` for data and query semantics.

## Executable, host, and diagnostics

The CLI selects `--codex PATH`, then `CONVERSATIONS_CODEX`, then its platform
default. On macOS the default is the ChatGPT app's bundled executable at
`/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex`. Other
platforms use `codex` on `PATH`. `ClientConfig::default()` uses the same
environment and platform defaults; explicit library `codex_path` takes
precedence. A missing or unusable selected executable fails without trying
another executable.
For an app installed elsewhere, select its executable explicitly.
Conversations does not install or update Codex.

The CLI accepts `--host-id` or `CONVERSATIONS_HOST_ID` for an explicit stable
host override. On macOS the default is an opaque truncated SHA-256 digest of
the platform UUID. Renaming the Mac does not change that reference component.
The raw hardware UUID is not exposed or retained. An identity read failure
stops the operation. Other platforms use the hostname as a compatibility
fallback. Callers must preserve a chosen override when stable references matter.

`ClientConfig::stderr_policy` defaults to `StderrPolicy::Inherit`. The CLI
also defaults to `--app-server-stderr inherit`. This preserves Codex startup
and compatibility diagnostics, which can contain paths or operational context.
`StderrPolicy::Suppress` or `--app-server-stderr suppress` connects only the
spawned App Server's stderr to the null device before launch. JSON-RPC failures
remain observable through the returned error. Embedded or scheduled callers
whose logs must remain body-free must select suppression.

## Process lifecycle and access

Each invocation starts `codex app-server --stdio`, sends `initialize` with
`experimentalApi: true`, sends `initialized`, performs the bounded operation,
and terminates its launch. Requests use App Server's documented newline-delimited
JSON-RPC interface. Conversations never reads raw Codex JSONL or SQLite files.

On Unix, the selected command and inherited App Server descendants run in a
private process group. Conversations terminates that group on return or error;
those descendants do not outlive the operation. Unrelated Codex desktop and CLI
processes are outside this cleanup boundary.

Conversations retains no corpus, index, daemon state, or credentials. Results
return only to caller-selected process output or the local Rust caller.
Redirected output and inherited diagnostics become the caller's retention
responsibility. Conversations has no network service or model workflow of its own.

## Readiness and compatibility

```sh
conversations doctor [--json]
```

`doctor` checks that the selected executable starts and completes the App Server
handshake. It enumerates visible root-task metadata without repair or turn reads.
It reports the selected path and version, App Server user agent when available,
and differences from CLI versions recorded on visible tasks.

A recorded version difference is a compatibility observation. Runtime status
belongs to this newly launched App Server. A persisted task can appear as
`notLoaded` while another client owns a live process. Neither result proves
machine-wide task liveness.

A protocol, handshake, pagination, timeout, or identity failure stops the
operation. Inspect the selected path and version before changing installations.
Do not add an executable fallback, read private storage, change authentication,
or terminate unrelated processes to bypass an error. Provider installation
proves publication integrity; it does not prove live App Server readiness.

Provider release, entry contract version, selected Codex version, and App Server
protocol are distinct. No App Server availability, wall-clock latency,
throughput, history-retention, future source-kind, or protocol-support window is
promised. App Server is a substantive external reliance without a dedicated
installed contract for this data surface; complete resolution preserves that gap.

## Installed release and selectors

The macOS deployment has no service to start and no credential to source. It owns:

- `~/.local/bin/conversations` and `~/.local/bin/conversations-install` as stable selectors;
- `~/Library/Application Support/Conversations/install/releases/HASH` as the immutable release;
- `install/current` and `install/previous` selectors under that product directory; and
- `~/Library/Application Support/Chancery/providers/conversations`, selecting the current release's bundle.

The `cell-install-v2` release identity covers the binary, Rust installer, public
layout, and complete Chancery bundle. `manifest.json` records the exact inventory;
`package/install` retains the installer bytes. An identical deployment is a no-op.
Existing release bytes are verified before reuse. A selected current or previous
release must have an exact content-addressed selector, a self-consistent manifest,
and matching component hashes.

A PID-aware product lock serializes Conversations updates. A shared Chancery
catalog-writer lock serializes provider publication across the shared Rust
installers. The installer takes the product lock before the catalog lock and
recovers stale owners. It publishes `current` atomically. A failed version or
help check after switching restores the previous selectors. If restoration
cannot be verified, it detaches them. Foreign selectors, malformed or tampered
selected releases, and a provider selector without a current release are rejected.

Deployment snapshots `current` before waiting for the lock and rejects stale
cutover by default. A caller can make the guard explicit with
`--expected-current absent` or `--expected-current releases/HASH`.

Deployment does not run `doctor`, scan metadata, copy transcripts, or alter
Codex authentication. Recovery validates the complete retained release before
selecting it and never executes a retained installer to verify the release.
It supports both the previous shell-installed format and `cell-install-v2`.

Krisis and Paperboy embed the Conversations library. Rebuild and deploy each
consumer to apply a library change. Replacing the CLI does not update those
callers. Explicit consumer executable pins take precedence over library defaults.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
