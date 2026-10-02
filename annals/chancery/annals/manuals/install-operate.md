# Install or recover Annals

The Cell manifest command places resources and performs setup without application
maintenance, work draining, schedule suspension, or readiness checks. Explicit
manual install and recovery retain the maintenance procedure below. It does not gate completion on persistent-state validation, artifact
integrity audits, or runtime readiness checks. The product's ordinary diagnostics
and runtime checks remain available separately.

Use this procedure for an authorized macOS installation, update, dedicated
decisions-library provision, retained-transaction recovery, older-installation
migration, or packaged Linux setup. Read `annals.installation` for the release,
maintenance, compatibility, and rollback guarantees; `annals.libraries` for
library configuration and schema; and `annals.inbox` for dispatch and recovery.

## Prepare the operation

1. Select the exact installation and effect. An ordinary update preserves the
   active library and spool. The installer has no data reset mode. Decisions
   provisioning owns only the separate
   decisions state and `annals/decisions-inbox`.
2. Select binaries and both provider bundles from a validated source candidate.
   Ordinary delivery uses `cell-ci submit COMMIT`; its manager integrates,
   validates, attempts bounded repairs, deploys, and emails the outcome.
   Use the commands below for separately authorized manual operations.
3. Check the reachable Nucleus binary and socket, compatible Clockwork, and the
   current user's graphical session before macOS activation. Keep Nucleus
   installation and authentication under Nucleus's own procedure.
4. Prepare the complete private Bazaar `cell.prompts.annals` selection. Import
   the reviewed migration seed before deploying these callers. Runtime reads
   do not initialize state or supply missing text. Read `annals.work.integrate`
   for the format and exact-version policy. For the original seed, review
   `prompting/seed.json`, then run this explicit import from the Cell checkout:

   ```sh
   cargo run --locked --offline --package cell-prompts -- \
     /absolute/private/bazaar.sqlite3 prompting/seed.json
   ```

   The importer initializes only that selected private path, preserves unrelated
   IDs, reuses identical latest text, and publishes each selection after its
   components exist. An interrupted import can leave unused versions; repeat
   the same import and inspect history after uncertain writes. Publication is
   per owner, without an atomic all-requester cutover. Verify exact referenced
   versions through Bazaar before deployment. Use the same database for all
   callers of the product; an interactive override does not configure a
   scheduled environment. Preserve selection version 1 and its components.
5. Check capacity for release, migration, and control writes. A closed
   inbox storage gate alone does not reject deployment. A probe error or failed
   write does. Do not clear user data or lower or disable the reserve without
   explicit consent for the exact target and scope.
6. Prove ownership of the complete selected Clockwork definition, rendered
   legacy plist, and command/provider selectors before mutation. Stop on
   foreign, changed, or uninspectable state. Do not mutate the same binding
   concurrently. Retain prior state and exact transaction identities.
7. Hold new affected work and drain admitted commands and durable dependency
   jobs before replacement. A candidate cannot fence an older installed
   command that lacks maintenance support. Use that installation's supported
   deployer and quiescence route for its compatibility update.

## Deploy or update on macOS

1. Run the candidate installer from the Cell root with absolute inputs:

```sh
cd /Users/joey/rust/cell
./target/release/annals-install install \
  --binary <ABSOLUTE_ANNALS_BINARY> \
  --usage-binary <ABSOLUTE_ANNALS_USAGE_BINARY> \
  --bundle "/Users/joey/rust/cell/annals/chancery/annals" \
  --usage-bundle "/Users/joey/rust/cell/annals/chancery/annals-usage" \
  --nucleus <ABSOLUTE_NUCLEUS_BINARY> \
  --nucleus-socket <ABSOLUTE_NUCLEUS_SOCKET> \
  --clockwork <ABSOLUTE_CLOCKWORK_BINARY>
```

2. Let the installer stage the release, establish
   maintenance, drain owned activation, migrate each selected library through
   supported schemas, switch exact selectors/binding, and publish commands. Preserve operator pauses and pre-existing disabled schedules.
   Stop if ownership, drain, migration, or restoration is unproved.
3. Use `ANNALS_UPDATE_WAIT_SECONDS` only for the documented inbox-lock wait
   when necessary; its default is 3,900 seconds. `--no-start` leaves scheduler
   state unchanged and does not complete a scheduled installation.
4. Select only the boolean `enabled` deployment setting when the coordinator
   needs an override. `{"annals":{"enabled":false}}` keeps both owned inbox
   bindings disabled after product setup. Omission preserves captured intent and
   a new schedule defaults enabled; recovery preserves prior intent and ignores
   the override. Preserve incident halts and operator pauses.
5. Release only the operation's maintenance and pause after setup completes. Do not clear Clockwork incident
   halts through deployment.

## Provision the dedicated decisions library

1. Complete the primary content release first.
2. Invoke its exact release-local installer as the current non-root user:

```sh
release="$HOME/Library/Application Support/Annals/install/releases/<64-hex-release-id>"
"$release/bin/annals-install" provision-decisions \
  --release-root "$release" \
  --nucleus-socket "$HOME/Library/Application Support/Nucleus/nucleus.sock" \
  --clockwork /Users/joey/.local/bin/clockwork
```

3. Check the returned `data`: `contract_version`, absolute config path,
   persistent library ID, Clockwork key and digest, selected/enabled booleans,
   maintenance state, and release ID. Check the decisions config, library,
   spool identity, and immutable `decisions` role agree.
4. Preserve private `0600`, operator-owned regular files with one hard link
   for existing config, database/sidecars, spool controls, and maintenance
   state. Require distinct private stdout/stderr logs. Stop on foreign owner,
   symlink, hard link, broader permissions, changed binding, or general role.
5. Use `--keep-maintenance` only for an outer authorized cutover. A successful
   later invocation without it releases only that receipt's hold; an unrelated
   maintenance marker stays engaged. `--home` selects an explicit alternate
   home. `--release-root` must be the content-addressed directory, not `current`.

This procedure creates or migrates only
`$HOME/Library/Application Support/Annals/decisions` and selects only
`annals/decisions-inbox`. It serializes with the primary update lock and changes
neither the primary inbox binding nor Nucleus. A failed pre-commit operation
restores captured state and the exact prior enabled intent. If that restoration
cannot be proved, keep maintenance and use the retained transaction.

## Operate run-owned admission

The Cell manifest runs `annals-install deploy` once with a schema-2 request on
stdin. Annals places the release, initializes or migrates the primary, decisions and
registered libraries, writes configuration, and selects
the two inbox schedules directly. It does not acquire or release application
holds, wait for admitted work, or suspend schedules. The executor
observes command completion and does not interpret Annals state or health.

The request supplies candidate paths, source paths, a run identity, selected
products and optional product settings. Annals accepts only the boolean
`enabled` setting. Omission preserves each existing binding's enabled state;
a new binding defaults enabled. Operator pauses and Clockwork incident halts
remain in force.

Use these explicit product commands to inspect or change Annals admission:

```text
annals --library DATABASE --json maintenance status
annals --library DATABASE --json maintenance hold RUN_ID
annals --library DATABASE --json maintenance release RUN_ID
```

A run ID contains 1–128 ASCII letters, digits, hyphens, underscores or periods,
with no leading period. Release removes only that owner's hold. An interrupted manifest command can leave completed setup effects in place.
Inspect its retained log and current selections before a further operation.
Retained manual transaction recovery remains an explicit operation.
The executor does not run it automatically.

## Recover an interrupted installation

1. Select the retained `install/transaction.primary.OWNER` or
   `install/transaction.decisions.OWNER` journal. Do not delete maintenance,
   rewrite receipts, exchange database files, or rely on a release selector
   as proof of database compatibility.
2. Run the exact retained candidate installer when public commands are
   suspended; otherwise use its verified installed selection:

```sh
"$HOME/.local/bin/annals-install" recover \
  "$HOME/Library/Application Support/Annals/install/transaction.primary.OWNER"
```

3. Let recovery restore compatible program, configuration and schedule state
   or finish the committed handoff. Current library and spool data remain in
   place. An incompatible prior program keeps maintenance and the transaction.
   Completed control journals are retained in `install/transactions/`.
   Legacy journal fields are accepted without restoring their database copies.
   An interrupted legacy fresh-state cutover requires its exact retained installer.
4. Check exclusive scheduler restoration and matching release/library/spool
   state. A previously disabled binding stays disabled; failed proof can leave
   public selectors removed and maintenance engaged. Stop until exact journal
   recovery succeeds. Coordinated recovery uses the same outer run owner.
5. Complete recovery before releasing this operation's boundary. Nucleus credentials remain forward-only.

## Unsupported libraries

Migration accepts only the schemas declared by `annals.libraries`. An unsupported
library stops deployment without resetting data. The installer has no replacement
or reset mode.

## Migrate a root-owned macOS installation

1. Select a validated candidate and verify the old system state and rollback
   plan. Log into the operator's graphical session.
2. Require `annals/inbox` to be absent or a disabled tombstone without a
   selected digest. Stop on any selected digest, enabled or disabled.
3. Run the attended migration from the Cell root:

```sh
sudo ./target/release/annals-install migrate-to-user \
  --binary <ABSOLUTE_ANNALS_BINARY> \
  --usage-binary <ABSOLUTE_ANNALS_USAGE_BINARY> \
  --bundle "/Users/joey/rust/cell/annals/chancery/annals" \
  --usage-bundle "/Users/joey/rust/cell/annals/chancery/annals-usage" \
  --nucleus <ABSOLUTE_NUCLEUS_BINARY> \
  --nucleus-socket <ABSOLUTE_NUCLEUS_SOCKET> \
  --clockwork <ABSOLUTE_CLOCKWORK_BINARY>
```

4. Let the migration disable and drain `system/org.annals.inbox`, move its whole
   state on one filesystem with WAL sidecars, rewrite the two legacy absolute
   paths, and migrate the moved library transactionally in place. No library
   copy or reset occurs. Unsupported old schemas stop the operation.
5. Check the inert child definition and retained outer committed phase before
   Clockwork registration/selection. Keep maintenance until the system job is
   proved absent and exact legacy-file retirement is complete. Never run both
   the system job and Clockwork binding intentionally together.
6. Stop if bootout or retirement fails. Keep files, transaction, and maintenance.
   A failure before child installation begins can restore system state without
   registering a definition. Phase `installing`, or `rewritten` with child evidence,
   retains moved state and outer and child recovery evidence. After outer commit,
   rerun the same migration to finish the retained handoff idempotently.
   Require the complete legacy plist, owner, and mode to match
   Annals' template; a matching label or executable is insufficient.
7. Run installed verification and preserve recovery material.

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

Resume only the pause or maintenance boundary established for deployment.


For a decisions installation, also read its explicit config's
`decision-feed watermark` and inspect `annals/decisions-inbox`. Check the
installed Chancery product overview, list, feature `show`, and operation
`resolve`; documentation presence is separate from runtime readiness.

Run `annals --register-usage` and `annals-usage --register-usage` after installation
or update. This registers command inventory without domain work.

Keep full sources, instructions, corpus evidence, spools, model context, and
logs private. This procedure does not authorize Git publication,
`annals/release.sh`, data cleanup, reserve reduction, prior-generation deletion,
or raw path-only retirement. Any retirement must prove exact product ownership
before mutation.

## Linux installation and maintenance

Use the packaged systemd route with a separately deployed and authenticated
Nucleus service. Select explicit library/spool/socket paths and the service
account. Read `annals.installation` for this platform's lifecycle and limitations.
Run the remaining commands from the Annals source directory. They use the
packaged paths; adjust configs and service if executables or the socket are
elsewhere. The packaged service sets
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`.

Build Annals, create a non-login service account, and install the files:

```sh
cargo build --release --package annals --package annals-usage

sudo groupadd --system annals
sudo useradd --system --gid annals --home-dir /var/lib/annals \
  --shell /usr/sbin/nologin annals
sudo install -m 0755 ../target/release/annals /usr/local/bin/annals
sudo install -m 0755 ../target/release/annals-usage \
  /usr/local/bin/annals-usage

sudo install -d -o root -g annals -m 0750 /etc/annals
sudo install -d -o annals -g annals -m 0700 /var/lib/annals
sudo install -d -o annals -g annals -m 0710 /var/spool/annals
sudo install -d -o annals -g annals -m 0770 \
  /var/spool/annals/incoming
sudo install -d -o annals -g annals -m 0700 \
  /var/spool/annals/queued \
  /var/spool/annals/processing \
  /var/spool/annals/done \
  /var/spool/annals/duplicates \
  /var/spool/annals/failed \
  /var/spool/annals/skipped

sudo install -o root -g annals -m 0640 \
  packaging/systemd/annals.toml /etc/annals/config.toml
sudo install -o root -g annals -m 0640 \
  packaging/systemd/usage.toml /etc/annals/usage.toml
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.service \
  /etc/systemd/system/annals-inbox.service
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.timer \
  /etc/systemd/system/annals-inbox.timer
```

Adjust the executable and socket paths in the configs and service if Nucleus
or Annals is installed elsewhere. Authenticate through the Nucleus service so
login, account reads, and model jobs remain under its single credential
authority:

```sh
sudo -u annals env HOME=/var/lib/annals \
  ANNALS_USAGE_CONFIG=/etc/annals/usage.toml \
  /usr/local/bin/annals-usage login --device-auth
```

The configured Nucleus service owns its private credential directory. Do not
give Annals a second Codex home or invoke Codex directly as a fallback.

Initialize the library and enable the timer:

```sh
sudo -u annals env HOME=/var/lib/annals \
  /usr/local/bin/annals --config /etc/annals/config.toml init

sudo -u annals env HOME=/var/lib/annals \
  /usr/local/bin/annals-usage doctor \
  --config /etc/annals/usage.toml

sudo systemctl daemon-reload
sudo systemctl enable --now annals-inbox.timer
```

Check both configs select the same reachable Nucleus socket. No companion
ledger is created.

Inspect or trigger it with:

```sh
sudo systemctl start annals-inbox.service
sudo systemctl status annals-inbox.timer annals-inbox.service
sudo journalctl -u annals-inbox.service
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox status
```

Read `annals.inbox.operate` for status fields, pause, priority,
interruption, retry, and delivery history. Run those commands as the `annals`
service account with the explicit config path shown above.

### Supply source files

For an explicit priority handoff that also keeps the source file in place, use:

```sh
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox enqueue --priority ./report.md
```

Do not also copy that file into `incoming/`; the enqueue command has already
created its queued job envelope.

A direct copy is supported by the settling interval:

```sh
sudo -u annals cp -n ./report.md /var/spool/annals/incoming/report.md
```

For an atomic handoff, use a private staging directory beside the inbox. The
final move is on the same filesystem and keeps the original basename:

```sh
sudo install -d -o annals -g annals -m 0700 /var/spool/annals/staging
sudo -u annals cp ./report.md /var/spool/annals/staging/report.md
sudo -u annals mv /var/spool/annals/staging/report.md \
  /var/spool/annals/incoming/report.md
```

Do not overwrite an existing inbox pathname. Reusing a basename with different
bytes may also conflict with the immutable work label derived from that name;
Annals records that job as failed rather than silently changing the label.


### Maintain the Linux service

Stop `annals-inbox.timer` before maintenance. Let the active service finish
before replacing executables, configuration, or library state. Restart the
timer after readiness checks. Use `inbox pause` for ordinary dispatch control.
