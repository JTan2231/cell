# Rust API

Import `bazaar::api::{Reader, Writer, Record, Error}`. These types use SQLite
in the caller's process. They do not invoke the CLI, a daemon, or a model.

```rust
use bazaar::api::{Reader, Writer};

fn example(database: &std::path::Path) -> bazaar::api::Result<()> {
    let mut writer = Writer::initialize(database)?;
    let saved = writer.update("example.prompt", "Read {{input}} carefully.")?;

    let reader = Reader::open(database)?;
    let latest = reader.get("example.prompt", None)?;
    let pinned = reader.get("example.prompt", Some(saved.version))?;
    let versions = reader.history("example.prompt")?;
    assert_eq!(pinned, saved);
    assert!(versions.contains(&latest.version));
    Ok(())
}
```

Use an absolute database path with a private regular parent directory. The
default path is returned by `bazaar::database_path(home)`. Initialization creates
missing directories with mode 0700 and the database with mode 0600. Readers also
work with mode 0500 directories and mode 0400 database files. Group and other
permissions and symlinks at the parent or database path are refused.

`Reader::open` and `Writer::open` require initialized state and create no files.
Only `Writer::initialize` creates state. It preserves compatible existing
versions and refuses foreign or unsupported databases. A reader has no write
methods. Each read observes committed state at its own query boundary.

`Record` has `id: String`, `version: i64`, and `content: String`. Version numbers
are positive and increase independently for each ID. `get(id, None)` reads the
latest version. `get(id, Some(version))` reads an exact version. `history(id)`
returns every version number in descending order without loading content.
Unknown IDs or versions return `Error::NotFound`. Empty IDs and nonpositive
versions are errors. IDs are otherwise opaque and case-sensitive.

`Writer::update` appends a complete string and returns its committed record.
It preserves empty strings, whitespace, Unicode, and NUL characters. It does
not parse JSON or expand templates. Identical content appends another version.
Concurrent writers allocate versions in one immediate SQLite transaction.
Each ID versions independently; separate updates have no shared transaction.

Updates have no idempotency key. If completion is uncertain, inspect history
and content before deciding whether to append again. Old rows cannot be updated
or deleted through the API. To revert, append an earlier version's content.

SQLite waits up to five seconds for lock contention. Operations return typed
storage errors after failure. A connection is not shared concurrently between
threads; callers can open independent handles to the same database. Content and
complete version lists use memory proportional to their size. No capacity,
throughput, or hard latency guarantee is supplied.

Callers own ID selection, template rendering, content parsing, and use of the
result. Direct API calls do not record CLI usage. No caller migration or agent
execution is part of this API.
