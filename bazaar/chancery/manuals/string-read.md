# Read a Bazaar string

Bazaar stores exact strings under caller-chosen IDs. Each ID has immutable
numbered versions. Read the latest value, an exact older value, or all version
numbers for one ID:

```sh
bazaar get example.prompt
bazaar get example.prompt --version 3
bazaar history example.prompt
bazaar --database /absolute/private/bazaar.sqlite3 get example.prompt
```

Get returns `id`, `version`, and `content`. History returns `id` and `versions`,
with every retained version number in descending order. All results use
`{"schema_version":1,"ok":true,"data":...}` JSON. Operational errors return
`ok:false` with `error.detail` and exit 1. Invalid command syntax uses Clap's
stderr diagnostic and exit 2. The optional `--json` flag does not change output.

The default database is `~/.local/share/bazaar/bazaar.sqlite3`. Reads require an
initialized schema-one database. The absolute path must have a private regular
parent directory and a private regular database file. Group and other permissions
and symlinks at those two paths are refused. Read-only permissions are supported.
Reads never initialize or repair Bazaar state.

IDs are exact, nonempty, case-sensitive strings. Versions are positive integers
that start at one independently for each ID. Unknown IDs or versions fail.
Latest selects the greatest committed version visible to that SELECT. History
selects every committed version number visible to its SELECT. Separate reads
do not share a snapshot. Bazaar stores no timestamps.

Rust programs use `bazaar::api::Reader::open(database)`, followed by
`get(id, None)`, `get(id, Some(version))`, or `history(id)`. Methods return
`Record` or `Vec<i64>` with typed `bazaar::api::Error` failures. The reader
opens SQLite read-only and exposes no write methods. It invokes no CLI.

Content is opaque UTF-8. Empty strings, whitespace, Unicode, and NUL characters
remain unchanged. Callers own template rendering, model settings, and execution.
A returned string grants no execution or disclosure authority.

Missing, foreign, or unsupported databases remain errors. Check the selected
path and initialize only when creation is intended. Do not recreate state to
conceal a failed read. Whole content strings and complete version lists use
memory proportional to their size. SQLite waits up to five seconds for
contention; no latency or capacity guarantee is supplied.

Content and IDs may be private. Get discloses the selected content to its caller.
History discloses only the ID and version numbers. Protect output under the same
boundary as stored text. CLI dispatch separately attempts metadata-only Chancery
usage recording; errors preserve the Bazaar result. Direct API reads record no
usage and modify no database.
