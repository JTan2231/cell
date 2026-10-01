# Change Nucleus

Use this operation for changes to Nucleus source, public invocation semantics,
persistent state, Codex adapter compatibility, authentication ownership,
service lifecycle, or deployment. It is distinct from operating the installed
runtime and from changing a requester-owned domain rule.

Always begin with:

```sh
/Users/joey/.local/bin/nucleus manual
```

The manual selects the procedure for the change: a routine patch, exact Codex
upgrade, protocol or client change, execution-capacity change, database-schema
change, requester schema/toolset/invocation change, or authentication or
service-ownership change.

Read `chancery resolve nucleus.develop.change` for this procedure and its
required feature contracts. The feature pages own detailed Nucleus behavior;
this manual owns the change sequence and verification obligations.

## Development sequence

1. Name the primary authority and all affected requesters.
2. Decide whether public meaning, store format, harness support, operational
   procedure, or recovery changes.
3. Modify the smallest owning component.
4. Update the owning feature contract and affected operation manuals in the
   same change. Update the ecosystem operator manual when shared operational
   facts or obligations change.
5. Commit the changes and submit them through the installed CI manager:

   ```sh
   cd /Users/joey/rust/cell
   ./ci.sh submit COMMIT
   ```

   The manager integrates, validates, attempts bounded repairs, deploys, and
   emails the outcome.
6. Verify the retained manager outcome and the selected coverage for affected
   requesters and contracts.
7. Treat remote release publication as a separate authorized action.

`nucleus/release.sh` is not a build command. It bumps the workspace release,
commits, tags, and pushes, and must not be run without explicit publication
intent.

## Compatibility-specific obligations

For an additive protocol change, deploy daemon support before any requester
emits the new form. For an incompatible change, retain both forms during
migration when possible or quiesce all affected requesters for a coordinated
cutover.

For a store migration, provide incremental migration from every supported
version, a representative old-state fixture, transactional proof, a backup and
rollback plan, and explicit handling of post-commit maintenance. Never restore
old binaries onto a database they cannot read.

For an exact Codex upgrade, stage the complete runtime with the candidate
installer's `stage-harness --codex /absolute/release/codex` command. Include its
matching `codex-code-mode-host`. Inspect the version, model catalog, app-server
schema, and every consumed semantic. Automated tests cover in-memory behavior;
they do not execute the runtime or create OS resource fixtures. Normal live
readiness verifies runtime file identities without model calls. Installation
does not perform that check. Update the adapter, applicable in-memory checks,
and installation instructions together before deployment.

For execution capacity, preserve one global ceiling of eight active attempts.
Verify the admission, timeout, and slot-retention rules in `nucleus.jobs`.
Capacity scheduling must not add workflow interpretation or automatic retry.

An output-decoder change must cover both supported authentication sequences
with in-memory records. Daemon restart and retained replay are outside automated
test coverage. Preserve
exact output atoms, authentication exclusions, active thread/turn correlation,
terminal freeze, and the absence of successful output on failed attempts. A
completed historical job may expose repaired derived output without changing
the operational record or executing another attempt; do not add outgoing
requests or stored result projections to repair missing correlation.

For authentication or service ownership, prevent new credential consumers and
let active users finish before attended login. Verify private modes, one
credential authority, concurrent account reads, serialized refresh, staged
writes, atomic promotion, and login exclusion against `nucleus.authentication`.
Preserve elected refresh and account reconciliation through requester
cancellation. Binary or database rollback must not replace a newer credential.

## Publish feature documentation

Keep one detailed explanation in the owning `nucleus.*` feature contract.
Keep action-critical conditions and verification in each operation. Use
`chancery show ID` to read one page and `chancery resolve ID` to read all required
contracts. Related feature references are navigation; required dependencies
declare compatible contracts and must remain acyclic.

Preserve `nucleus.execution.operate` contract 3 and the existing integration and
development contract versions when reorganizing prose without changing their
promises. New feature contracts have their own stable identities and versions.
Keep normalized claims aligned with their feature bodies. Preserve unsupported,
unspecified, and not-applicable boundaries instead of filling gaps from code.

Publish the overview, entries, and Markdown bodies together in the Nucleus
provider bundle. A provider schema-4 publication requires a compatible Chancery
reader before cutover. Documentation publication follows the product release;
never edit an installed immutable bundle in place.

## Deployment

When deployment is separately authorized, quiesce requesters if replacing the
daemon could lose active work. Preserve the recovery material required by the
selected playbook. Installation performs setup without artifact-integrity,
persistent-state-integrity, or operational-readiness checks. Ordinary health,
account, and requester diagnostics remain separate. No installation operation
submits a synthetic model job or creates a requester record.

Stop if a destructive migration or credential move lacks a recovery decision,
an affected requester cannot be quiesced, or the exact candidate harness has
not been proved. Development completion alone does not authorize release,
deployment, requester retries, or unrelated domain changes.

## Run an isolated foreground instance

Build from the Cell root and select isolated paths and the supported harness:

```sh
cargo build --release --package nucleus-cli --package nucleus-daemon
target/release/nucleusd serve \
  --socket /tmp/nucleus.sock \
  --database /tmp/nucleus.db \
  --codex /absolute/path/to/codex \
  --codex-home /tmp/nucleus-codex-home
```

These paths must not already belong to another instance. Building does not
install the user service. Use `nucleus.execution.operate` for service installation
and recovery procedures and `nucleus.service` for their guarantees.

## Sensitive material

Fixtures, backups, logs, and retained output can contain complete prompts,
source content, tool traffic, or authentication data. Keep them within their
documented private boundaries.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
