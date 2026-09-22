# Install and verify Bazaar

Bazaar is an on-demand CLI and in-process Rust library backed by SQLite.
It installs no daemon, schedule, or model requester.

## Install programs

Use `cell-ci submit COMMIT` for ordinary CI delivery. The manager integrates,
validates, attempts bounded repairs, deploys, and emails the outcome. For a
separate authorized manual deployment, select a validated candidate on local
main, then preview and run the coordinator:

```sh
./deploy.sh plan bazaar
./deploy.sh bazaar
```

Bazaar accepts no deployment settings or runtime service dependencies.
Coordinated configure initializes an empty default database or checks its existing
schema. Verify checks program identity and database integrity. It creates no
string versions and performs no caller migration.

The immutable release contains `bazaar`, `bazaar-install`, its recovery
installer, and the matching provider. Releases live under
`~/Library/Application Support/Bazaar/install/releases/HASH`. The shared
`cell-install-v2` transaction selects public commands under `~/.local/bin`
and the Bazaar provider under Chancery. Foreign selectors, changed release
bytes, and stale expected selections stop publication.

Direct installation uses a sealed candidate:

```sh
bazaar-install install --binary /absolute/candidate/bazaar --bundle /absolute/bazaar/chancery
bazaar init
bazaar doctor
bazaar --register-usage
```

The installer accepts `--home ABSOLUTE_PATH` and
`--expected-current absent|releases/HASH`. Direct installation selects programs
only; initialize state separately. Usage registration adds command identities
to Chancery and creates no stored strings.

## Initialize and inspect

```sh
bazaar init
bazaar doctor
bazaar-install inspect
bazaar-install verify --binary /absolute/candidate/bazaar --bundle /absolute/bazaar/chancery
bazaar-install verify-release /absolute/owned/release
```

The default database is `~/.local/share/bazaar/bazaar.sqlite3`. Global
`--database ABSOLUTE_PATH` selects another database for ordinary commands.
Deployment configures only the default database. Initialization creates missing
directories with mode 0700 and the database with mode 0600. The immediate parent
and database must be regular and private. Group and other permissions and
symlinks at those two paths are refused. Readers can use read-only permissions.

Init creates the schema in empty state and can finish an interrupted empty
initialization. It preserves compatible versions. Nonempty foreign databases
and unsupported identities fail without alteration. Doctor opens state read-only
and checks database identity and SQLite integrity. These operations return
schema-one JSON with no stored content.

The single application table contains `id`, `version`, and `content`.
The compound primary key identifies each immutable version. SQLite stores an
application ID and schema version separately. No timestamp, configuration schema,
selected-version pointer, or content digest is stored.

## Recovery and privacy

Recover an exact retained program release:

```sh
bazaar-install recover --release /absolute/owned/release
```

Recovery preserves the separate database. It never restores or rewrites string
versions. Compatible commands can finish across program selection. Only
schema-one state and version-two installation packages are supported.
Unsupported state remains an error; no migration is supplied.

An uncertain content append may have committed. Inspect history before repeating
it. For a filesystem backup, stop writers, let current operations finish, and
retain the database and any SQLite sidecars together in private storage. Restore
only a compatible complete backup under exclusive access. Restoring older
history can reuse later version numbers, so reconcile caller references before
resuming. There is no automatic backup, pruning, deletion, or restore command.

Keep content and backups outside source and release files. Installation,
initialization, and inspection authorize no content update, agent execution,
or caller operation. Preserve unresolved installation evidence after failure.
