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
or terminate unrelated processes to bypass an error. Provider installation selects program files; it does not prove live App Server readiness.

Provider release, entry contract version, selected Codex version, and App Server
protocol are distinct. No App Server availability, wall-clock latency,
throughput, history-retention, future source-kind, or protocol-support window is
promised. App Server is a substantive external reliance without a dedicated
installed contract for this data surface; complete resolution preserves that gap.

## Installed release and selectors

The installer copies the supplied programs and provider bundle into a retained
release and selects their owned public paths together. It creates required
installation directories and uses product and catalog locks with atomic selector
updates. `--expected-current absent|releases/HASH` guards the selected release.
Foreign public selectors are refused. File-operation or basic execution failures
restore the prior selectors when possible.

Release hashes name the staged files. Installation and recovery do not compare
artifact hashes, component versions, or retained file inventories. They do not
run database integrity checks, dependency probes, or readiness checks. Basic
`--help` and `--version` execution checks remain. Inspection reads recorded
installation metadata and selectors; it is not an integrity result.

The installation root is
`~/Library/Application Support/Conversations/install`. Public `conversations`,
`conversations-install`, and the Conversations provider follow `current`.
`previous` retains the prior selection. `manifest.json` describes the
`cell-install-v2` layout, and `package/install` retains the installer.
Recovery also reads the previous shell format.

Deployment does not run `doctor`, read history, or change Codex authentication.
It starts no service. The optional doctor command retains its normal App Server
handshake and metadata behavior; it is separate from installation.

Krisis and Paperboy embed the Conversations library. Rebuild and deploy each
consumer to apply library changes. Replacing the CLI does not update them.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
