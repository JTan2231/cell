# Operate the installed Annals release

The user-owned macOS deployment installs Annals and Annals Usage together,
plus configuration, content-addressed releases, and the scheduled inbox
LaunchAgent. Nucleus remains a separately installed execution and credential
service.

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
  --nucleus-socket <ABSOLUTE_NUCLEUS_SOCKET>
```

The deployer stages a complete content-addressed release, validates candidate
programs and the Nucleus boundary, establishes Annals maintenance, quiesces
scheduled work, performs supported migration, switches the release and
LaunchAgent configuration, and validates the installed commands. It does not
stop, replace, or take ownership of Nucleus.

The inbox storage gate is not a deployment lock. A closed gate does not by
itself reject an Annals deployment, globally stop the Nucleus service or
independent Nucleus jobs, or block another product's deployment. The deployer
still needs enough physical capacity to stage the release and write its backup,
migration, validation, and rollback artifacts; any of those writes can fail
when the shared filesystem is actually full. A probe error is distinct from a
measured closed gate and can fail deployment status validation with
`storage_probe_failed`. A deployment request authorizes only the deployer's
documented effects. It does not authorize a model or agent to clear user data
as separate storage remediation or to lower or disable the reserve. Either
action remains a user decision requiring explicit consent for the exact target
and scope.

An ordinary pre-commit failure restores the captured release, configuration,
library, spool, and service state. Do not remove maintenance markers, edit
receipts, or swap database files manually after interruption; inspect the
deployer result and follow the installation guide's exact recovery procedure.

## Fresh-state cutover

`--fresh-state` is a distinct destructive operation for a documented schema
boundary:

```sh
./annals/packaging/launchd/deploy-user.sh \
  --binary <ABSOLUTE_ANNALS_BINARY> \
  --usage-binary <ABSOLUTE_ANNALS_USAGE_BINARY> \
  --nucleus <ABSOLUTE_NUCLEUS_BINARY> \
  --nucleus-socket <ABSOLUTE_NUCLEUS_SOCKET> \
  --fresh-state
```

It replaces the active library and spool as one rollback generation, imports
the uncompleted backlog in preserved lane order, and resumes only after the new
generation validates. Require explicit authority, retained prior state, and a
verified backlog/recovery plan. It must not be inferred from a routine update
request.

## Validation

After an authorized cutover, verify:

```sh
/Users/joey/.local/bin/annals --version
/Users/joey/.local/bin/annals-usage --version
/Users/joey/.local/bin/nucleus health
/Users/joey/.local/bin/annals validate
/Users/joey/.local/bin/annals inbox status
/Users/joey/.local/bin/annals-usage doctor
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
