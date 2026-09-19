# Append a Bazaar string version

Append a complete string when the user or application authorizes that content
update. Bazaar owns storage and version identity. The caller owns the ID's
meaning and the content format.

```sh
bazaar update example.prompt 'Read {{input}} carefully.'
bazaar update example.prompt --file /absolute/prompt.txt
bazaar update example.prompt --stdin < /absolute/prompt.txt
bazaar --database /absolute/private/bazaar.sqlite3 update example.prompt ''
```

Supply exactly one content source: a positional string, `--file`, or `--stdin`.
Files and standard input must contain valid UTF-8. Bytes remain unchanged,
including trailing newlines, whitespace, and NUL characters. Empty strings are
valid. Prefer a file or standard input for private or multiline content.

The database must already be initialized. The default is
`~/.local/share/bazaar/bazaar.sqlite3`; `--database` selects another absolute
private path. IDs are nonempty exact strings. Bazaar validates no prompt,
configuration, or template syntax.

The first update under an ID creates version 1. Each later update creates the
next version, including identical content. The receipt returns committed `id`,
`version`, and `content` in schema-one JSON with `ok:true`. Another writer can
append after that commit, so the receipt does not promise to remain latest.

Rust programs use `bazaar::api::Writer::open(database)` and
`update(id, content)`. The in-process method returns a `Record` with a positive
`i64` version or a typed error. It does not invoke the CLI.

Version allocation and insertion use one immediate SQLite transaction with
full synchronous commits. Concurrent writers serialize. Constraints and triggers
reject updates, deletes, replacements, and gaps. Each ID versions independently.
A caller can store several related values in one string when they must change
together. Bazaar does not interpret that string.

There is no idempotency key or automatic retry. If the receipt is lost, the
update may have committed. Read history and relevant content before deciding
to append again. Repeating a successful update creates another version.
To revert, read an older version and append its content. No supported operation
overwrites or removes history.

Unknown database formats, empty IDs, exhausted version numbers, invalid UTF-8,
storage failures, and lock contention are errors. SQLite waits up to five
seconds for contention. No content-size, throughput, or completion-time guarantee
is supplied. Content is held in memory as one complete string.

Keep private content and output outside source, releases, and unrelated messages.
An append starts no model and changes no caller configuration by itself. It
authorizes no migration, interpretation, external send, or history deletion.
CLI dispatch records only best-effort command metadata in Chancery; direct API
appends do not.
