# Bazaar installation and private state

This feature owns program publication, database initialization, integrity checks,
and compatible recovery. Use `chancery show bazaar.install.operate` for the
procedure. Use `chancery resolve bazaar.install.operate` to read that procedure
with this complete contract.

Bazaar is an on-demand CLI and in-process Rust library. It installs no daemon,
schedule, network listener, or model requester. The current user owns its
programs and private SQLite state. There is no application-level authentication.

## Program publication and verification

The immutable release contains `bazaar`, `bazaar-install`, its recovery
installer, and the matching Chancery provider. Releases live under
`~/Library/Application Support/Bazaar/install/releases/HASH`. The shared
`cell-install-v2` transaction selects public commands under `~/.local/bin`
and the Bazaar provider under Chancery. The provider follows program selection
and retained-release recovery. Its installed pages require no source checkout.

Direct installation accepts a sealed matching candidate:

```sh
bazaar-install install --binary /absolute/candidate/bazaar --bundle /absolute/bazaar/chancery
bazaar-install inspect
bazaar-install verify --binary /absolute/candidate/bazaar --bundle /absolute/bazaar/chancery
bazaar-install verify-release /absolute/owned/release
bazaar-install recover --release /absolute/owned/release
```

The installer accepts `--home ABSOLUTE_PATH` and
`--expected-current absent|releases/HASH`. Foreign selectors, changed release
bytes, and stale expected selections stop publication. Preserve unresolved
installation evidence after failure. Never edit a retained release in place or
take over another owner's selector.

Direct installation selects programs only. It does not initialize or replace
the database. The Cell coordinator owns deployment ordering. Bazaar accepts
no deployment settings or runtime service dependencies. Coordinated configure
initializes an empty default database or checks its existing schema. Verification
checks program identity and database integrity. Neither appends a string or
migrates a caller. Program installation remains useful without the Chancery
executable; the catalog is not a Bazaar runtime dependency.

The coordinator reliance applies only to coordinated deployment. Direct
installation and core state operations work independently. No dedicated installed
Chancery contract covers the shared coordinator; resolution exposes that gap.

## State identity and access

The default database is `~/.local/share/bazaar/bazaar.sqlite3`. Global
`--database ABSOLUTE_PATH` selects another database for ordinary CLI commands.
Rust programs use `bazaar::database_path(home)` to obtain the default path.
Deployment configures only the default database.

The path must be absolute. The immediate parent must be a private regular
directory, and the database must be a private regular file. Group and other
permissions and symlinks at those two paths are refused. Initialization creates
missing directories with mode 0700 and the database with mode 0600. Readers can
use mode 0500 directories and mode 0400 database files.

Only Bazaar database schema one and version-two installation packages are
supported. Database schema, CLI output schema, string version, entry contract
version, and product release are independent identities. Unsupported state
remains an error; no state migration or compatibility window is supplied.

The single application table contains `id`, `version`, and `content`. Its
compound primary key identifies each immutable version. SQLite stores the
application ID and schema version separately. No timestamp, configuration
schema, selected-version pointer, or content digest is stored. This description
does not make arbitrary direct SQL a supported interface.

## Initialization and integrity checks

```sh
bazaar init
bazaar doctor
bazaar --database /absolute/private/bazaar.sqlite3 init
bazaar --database /absolute/private/bazaar.sqlite3 doctor
```

Init creates schema in empty state and can finish an interrupted empty
initialization. It preserves compatible versions. Nonempty foreign databases
and unsupported identities fail without alteration. Initialization creates no
string versions and validates no stored content format.

Doctor opens state read-only and checks database identity and SQLite
`quick_check`. It returns no stored content. It establishes neither the meaning
of stored text nor future availability. Init and doctor return schema-one JSON;
operational errors use `ok:false` with `error.detail` and exit 1. Invalid command
syntax uses the Clap stderr diagnostic and exit 2. The optional `--json` flag
does not change output.

Rust programs import `bazaar::api::{Reader, Writer, Error}` and use
`bazaar::api::Result<T>` for typed results:

```rust
fn initialize(database: &std::path::Path) -> bazaar::api::Result<()> {
    let _writer = bazaar::api::Writer::initialize(database)?;
    bazaar::api::Reader::open(database)?.check()
}
```

Only `Writer::initialize` creates state. `Reader::open` and `Writer::open`
require initialized state and create no files. `Reader::check` returns `()` or
a typed error and reads no content into its result. These methods run in process
and invoke no CLI, daemon, or model. Direct API calls record no CLI usage.

Invalid paths, nonprivate files, unsupported or foreign state, integrity failure,
I/O failure, and SQLite failure are typed API errors. SQLite waits up to five
seconds for contention. No capacity, throughput, or hard latency guarantee is
supplied. Open separate handles when callers need concurrent connections;
a connection is not shared concurrently between threads.

## Recovery and retention

Retained-release recovery preserves the separate database. It never restores
or rewrites string versions. Compatible commands can finish across program
selection. A program rollback does not restore older data or make unsupported
state compatible.

Every committed string version remains retained. There is no automatic backup,
pruning, deletion, or restore command. For a filesystem backup, stop writers,
let current operations finish, and retain the database and any SQLite sidecars
together in private storage. Restore only a compatible complete backup under
exclusive access. Restoring older history can reuse later version numbers;
reconcile caller references before resuming.

Content and backups stay outside source and release files. Installation,
initialization, inspection, and usage registration authorize no content update,
caller migration, external send, or agent execution. `bazaar --register-usage`
separately registers command identities in Chancery and creates no strings.
CLI dispatch attempts metadata-only usage recording; errors preserve the Bazaar
result. No installed catalog is required for core state operations.

Read `bazaar.string.read` for exact content and history queries. Read
`bazaar.string.update` for append transactions and uncertain update recovery.
Those references are related reading, not required documentation dependencies.
