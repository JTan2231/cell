# Operate the installed Annals release

The user-owned macOS deployment installs Annals and Annals Usage together,
plus configuration, content-addressed releases, and the scheduled inbox
Clockwork binding `annals/inbox`. Nucleus remains a separately installed
execution and credential service; Clockwork remains a separately installed
activation and process-history service.

## Deploy or update

Build and test the candidate products first, then invoke the deployer only with
explicit installed-state authority:

```sh
cd /Users/joey/rust/cell
./annals/ci.sh
./annals/packaging/launchd/deploy-user.sh \
  --binary <ABSOLUTE_ANNALS_BINARY> \
  --usage-binary <ABSOLUTE_ANNALS_USAGE_BINARY> \
  --nucleus <ABSOLUTE_NUCLEUS_BINARY> \
  --nucleus-socket <ABSOLUTE_NUCLEUS_SOCKET> \
  --clockwork <ABSOLUTE_CLOCKWORK_BINARY>
```

The deployer stages a complete content-addressed release, checks candidate
programs and the Nucleus boundary, establishes Annals maintenance, quiesces
scheduled work, performs supported migration, switches the release and
exact Clockwork definition digest, and checks the installed commands. It first
registers the definition inactive. Before disabling or replacing a selected
binding, it verifies the complete current Annals release and compares every
stored executable-definition field with it; a same-key foreign definition is
left untouched. The first handoff similarly removes only an exactly owned
legacy LaunchAgent. It does not stop, replace, or take ownership of Nucleus or
Clockwork.

Definition inspection and binding mutation are not a compare-and-swap.
Concurrent same-user direct Clockwork mutation of `annals/inbox` during deploy
or migration is unsupported; reinspection detects attributable changes where
possible, fails the handoff closed, and may retain maintenance for recovery.

The immutable definition requests run-at-load and a 300-second interval,
skips overlap, has no activation timeout, and pins `/bin/sh` plus the
release-local Annals runner by SHA-256. The runner executes only its sibling
release payload as `annals --quiet inbox run` in the Annals state directory
with an explicit nonsecret environment and umask `077`. Clockwork records
process outcomes but does not inspect Annals domain state or ingest
Annals-owned log bodies.

The inbox storage gate is not a deployment lock. A closed gate does not by
itself reject an Annals deployment, globally stop the Nucleus service or
independent Nucleus jobs, or block another product's deployment. The deployer
still needs enough physical capacity to stage the release and write its backup,
migration, and rollback artifacts; any of those writes can fail
when the shared filesystem is actually full. A probe error is distinct from a
measured closed gate and can fail deployment status inspection with
`storage_probe_failed`. A deployment request authorizes only the deployer's
documented effects. It does not authorize a model or agent to clear user data
as separate storage remediation or to lower or disable the reserve. Either
action remains a user decision requiring explicit consent for the exact target
and scope.

An ordinary pre-commit failure restores the captured release, configuration,
library, and spool, then restores the exact prior Clockwork definition only if
its binding was enabled, or restores the legacy LaunchAgent, never both. A
previously absent or disabled binding stays disabled without transient
activation; its inactive selected digest may remain the candidate digest. If
that exclusive restoration cannot be proved, Annals leaves unproved scheduler
state untouched, keeps maintenance, removes its public selectors, and retains
private recovery material. Do not remove maintenance markers, edit
receipts, or swap database files manually after interruption; inspect the
deployer result and follow the installation guide's exact recovery procedure.

The attended migration from the former system LaunchDaemon uses a narrower
handoff. Its child fresh-state deploy keeps Annals maintenance in place and
renders the exact Clockwork definition, but does not register or select it.
The outer migration verifies that inert file and durably records its committed phase
before registration or binding selection, so the definition never points at a
state root that rollback can move away. RunAtLoad remains maintenance-gated
while the outer migration registers the definition, records its digest,
selects `annals/inbox`, and retires the system files. It clears maintenance
only after `system/org.annals.inbox` is proved absent and those steps complete.
A failed bootout or still-loaded service retains the legacy files,
transaction, and maintenance marker. A committed interruption retains the
transaction and handoff so a rerun can finish the same definition and binding
idempotently. Before commit the migration accepts only an absent Clockwork
binding or a disabled tombstone with no selected digest. Legacy plist removal
and restoration additionally require the complete file to match Annals'
rendered template, expected owner, and mode. A familiar label or executable is
not ownership, and an extra launchd key is treated as foreign.

## Provision the decisions library

The dedicated decisions account library is a separate, explicitly authorized
installation surface. After the primary installer has produced a verified
immutable content release, run as the current (non-root) user:

```sh
release="$HOME/Library/Application Support/Annals/install/releases/<64-hex-release-id>"
"$release/package/provision-decisions-user.sh" \
  --release-root "$release" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork /Users/joey/.local/bin/clockwork
```

The provisioner and both of its unrendered templates are separate members of
the format-four release identity. The invoked provisioner must match the
selected release's recorded hash; a source sibling, mutable selector, or
tampered package member is rejected before state or binding mutation.

This authorizes creation or supported migration only under
`$HOME/Library/Application Support/Annals/decisions` and registration or
switching only of `annals/decisions-inbox`. It does not deploy release bytes,
change Nucleus, or inspect or mutate the primary `annals/inbox` binding. It
shares Annals' product-wide `install/.update-lock`, so it cannot race the
primary deployer.

The provisioner validates the complete content release and complete selected
prior definition, creates and binds fresh state off-path, establishes
maintenance before a run-at-load definition can be selected, registers the
candidate inactive, drains an enabled owned prior, takes a consistent backup
before migration, proves inbox and feed readiness, then switches the exact
definition. Fresh state is initialized with the immutable `decisions` role;
an existing or migrated `general` database fails readiness even if its
persistent ID matches the config. Foreign or concurrently changed state fails
closed. A pre-commit
failure restores captured state and the exact enabled or disabled-selected
prior without activating a prior-disabled schedule. If an exact restoration
cannot be proved, the dedicated library remains maintenance-gated and only an
attributable candidate is disabled; retained transaction material is reported
for recovery.

Before opening existing state, the provisioner requires its config, database
and SQLite sidecars, spool identity and control files, and maintenance files to
be operator-owned `0600` regular files with one hard link. It creates or
validates distinct private stdout and stderr log files before selection. A
symbolic link, extra hard link, foreign owner, or broader mode fails closed.

Success emits a single JSON envelope whose `data` contains contract version,
absolute config path, persistent library ID, Clockwork key and definition
digest, selected/enabled booleans, maintenance state, and release ID. With
`--keep-maintenance`, an Annals-owned receipt leaves the decisions gate engaged
for an outer Krisis/Semantics cutover. A later successful invocation without
that option clears only that matching owned hold. A pre-existing unreceipted
maintenance gate remains engaged.

## Fresh-state cutover

`--fresh-state` is a distinct destructive operation for a documented schema
boundary:

```sh
./annals/packaging/launchd/deploy-user.sh \
  --binary <ABSOLUTE_ANNALS_BINARY> \
  --usage-binary <ABSOLUTE_ANNALS_USAGE_BINARY> \
  --nucleus <ABSOLUTE_NUCLEUS_BINARY> \
  --nucleus-socket <ABSOLUTE_NUCLEUS_SOCKET> \
  --clockwork <ABSOLUTE_CLOCKWORK_BINARY> \
  --fresh-state
```

It replaces the active library and spool as one rollback generation, imports
the uncompleted backlog in preserved lane order, and resumes only after the new
generation passes its checks. Require explicit authority, retained prior state,
and a verified backlog/recovery plan. It must not be inferred from a routine
update request.

## Verification

After an authorized cutover, verify:

```sh
/Users/joey/.local/bin/annals --version
/Users/joey/.local/bin/annals-usage --version
/Users/joey/.local/bin/nucleus health
/Users/joey/.local/bin/annals stats
/Users/joey/.local/bin/annals inbox status
/Users/joey/.local/bin/annals-usage doctor
/Users/joey/.local/bin/clockwork --json binding show annals/inbox
/Users/joey/.local/bin/clockwork --json history annals/inbox --limit 20
```

Run a deliberate Annals integration canary when execution changed and inspect
the Annals reconciliation, not only the Nucleus job. Run usage report/budget
canaries when their projection or account path changed. Resume only the pause
or maintenance boundary established for deployment.

Installed libraries, spools, rollback generations, logs, and Nucleus output
may contain complete private source and model context. Preserve private
ownership and permissions. Deployment does not authorize
`annals/release.sh`, deletion of prior recovery material, or a fresh-state
replacement.

There is no supported raw path-only retirement sequence. A shared Clockwork
key, launchd label, command pathname, or provider pathname is not ownership;
leave it intact unless a product-owned operation has proved the exact current
definition, fully rendered legacy plist, and selector targets before mutation.
