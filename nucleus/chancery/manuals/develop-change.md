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

## Development sequence

1. Name the primary authority and all affected requesters.
2. Decide whether public meaning, store format, harness support, operational
   procedure, or recovery changes.
3. Modify the smallest owning component.
4. Update the runtime contract and operator manual in the same change whenever
   shared operational facts or obligations change.
5. Run the Nucleus product quality gate:

   ```sh
   cd /Users/joey/rust/cell
   ./nucleus/ci.sh
   ```

   Treat it as Nucleus's complete product gate.
6. Run every affected requester quality gate and contract test.
7. Treat release and deployment as separate actions requiring their own
   authority.

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

For an exact Codex upgrade, inspect the candidate executable, version, model
catalog, app-server schema, and every consumed semantic. Update the adapter and
compatibility tests before deployment. Nucleus intentionally rejects an
unproved executable.

For execution capacity, preserve one global ceiling of eight active attempts.
An admitted job waits as `accepted` with a `pending` attempt until a slot is
available; its timeout starts after slot acquisition, and
`waiting_on_requester` continues to hold the slot through terminal cleanup.
Capacity scheduling must not add workflow interpretation or automatic retry.

An output-decoder change must exercise both supported authentication sequences
through the daemon job-read API and retained replay after restart. Preserve
exact output atoms, authentication exclusions, active thread/turn correlation,
terminal freeze, and the absence of successful output on failed attempts. A
completed historical job may expose repaired derived output without changing
the operational record or executing another attempt; do not add outgoing
requests or stored result projections to repair missing correlation.

For authentication or service ownership, prevent new credential consumers and
let active users finish before attended login. Keep private modes and one
authoritative managed credential. Allow account reads to overlap jobs, serialize
canonical refresh, and exclude attended login while job or account sessions
remain active.

Stage every Codex credential write away from the authoritative file. Atomically
promote the validated generation. Let elected refresh and account reconciliation
finish after requester cancellation. Credential recovery moves only forward;
binary or database rollback must not replace a newer credential.

## Deployment

When deployment is separately authorized, quiesce requesters if replacing the
daemon could lose active work. Preserve the recovery material required by the
selected playbook. After cutover, prove matching CLI and daemon versions,
strict health and its `maxActiveJobs`, `activeJobs`, and `availableSlots`
capacity, and the exact harness and account before resuming dispatch.
Deployment readiness checks do not submit model jobs or create synthetic
requester records.

Stop if a destructive migration or credential move lacks a recovery decision,
an affected requester cannot be quiesced, or the exact candidate harness has
not been proved. Development completion alone does not authorize release,
deployment, requester retries, or unrelated domain changes.

## Sensitive material

Fixtures, backups, logs, and retained output can contain complete prompts,
source content, tool traffic, or authentication data. Keep them within their
documented private boundaries.
