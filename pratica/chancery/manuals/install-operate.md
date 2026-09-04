# Install and operate Pratica

Pratica is a user-owned short-lived CLI. It installs no service or schedule.
Nucleus is a separately installed runtime used only by explicit steward,
composition, or conformance commands.

## Build without changing installed state

```sh
cd /Users/joey/rust/cell
./pratica/ci.sh
cargo build --release --locked --package pratica
```

Product CI uses synthetic sources and fake Nucleus transport. It does not read
real negotiations, invoke the live service, deploy, initialize state, or
publish a release.

## Deploy only with explicit authority

```sh
pratica/packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/pratica
```

The deployer validates the exact Pratica Chancery tree, candidate/provider
version match, content-addressed release identity, current and previous
selectors, foreign/tampered state refusal, and installed version/help. It owns
only `~/.local/bin/pratica`, the Pratica install tree, and
`providers/pratica`. It does not open or change `pratica.db`, stop or restart
Nucleus, or submit a job.

## Initialize, migrate, and diagnose separately

```sh
/Users/joey/.local/bin/pratica init
/Users/joey/.local/bin/pratica doctor
/Users/joey/.local/bin/chancery show pratica.integration.negotiate
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/nucleus health
```

Initialization exclusively creates schema two. Doctor proves schema and private
permissions plus strict protocol-v1 Nucleus readiness and the immutable
registrations `pratica/steward-response/1`,
`pratica/composition-review/1`, and `pratica/conformance-review/1`. It does not
negotiate, review, or authorize a new attempt.

To upgrade an existing schema-one database, first stop every other Pratica
process, confirm no attempt is active, and select an absent backup path inside
an existing private directory:

```sh
/Users/joey/.local/bin/pratica migrate \
  --backup /absolute/private/path/pratica-schema-1.db
/Users/joey/.local/bin/pratica doctor
```

Migration supports only schema one to schema two. Under one immediate writer
lock, it rechecks that no attempt is active, creates the SQLite backup mode
0600, and then applies and commits the schema update. A schema-two invocation
is a true no-op: it does not inspect or create the supplied backup path.
Ordinary commands never migrate implicitly.

Default state is
`~/Library/Application Support/Pratica/pratica.db`; use an absolute
`--database` path or `PRATICA_DATABASE` for an intentional alternate database.

## Roll back the program and database together when required

After a committed cutover, the prior release is retained under
`install/previous`. Redeploy its exact binary with its packaged deployer only
after proving the retained schema is compatible. The ordinary deployer rollback
changes binary and provider selection and never rewrites negotiation state.

If schema one was migrated, a rollback requires restoring the retained
schema-one backup and selecting the matching old binary together. Do not run an
old binary against schema two or treat program-only rollback as database
recovery. Preserve the newer database until the decision and recovery evidence
are complete.

Version 0.1 has no uninstaller or automatic release pruning. Deleting terms,
reviews, database bytes, Nucleus records, or releases requires separate explicit
authority.

## Canary

The deliberate requester canary brokers CRM contracts derived from the recent
“Review CRM data model concerns” task in an isolated database. It registers
explicit source snapshots, opens one track per actual steward concern, obtains
real steward responses, reaches exact bilateral assent, runs an independent
composition review, and exports the sealed Markdown set. Verify Pratica domain
records and correlated Nucleus jobs.

The canary must create no CRM implementation, database, API, migration, UI,
deployment, or release. A terminal model response, unsealed offer, or composition
review without agreement seals is not success. Do not store the real transcript
or terms in source fixtures, release bytes, logs, or Chancery.
