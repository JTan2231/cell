# Data model

SQLite schema version 1 has two domain tables. There are no JSON or JSONB
columns.

## `todos`

```text
id             INTEGER primary key; public spelling is tN
title          nonblank single line
note           nonblank researched todo body
pointer        nonblank caller direction
source_path    nonblank originating file path
status         open | done
created_at     UTC creation timestamp
completed_at   UTC completion timestamp, null while open
```

Title, note, pointer, source path, and creation time are immutable. A todo
begins open. `done` records a completion time and `reopen` clears it. The
database enforces the relationship between status and completion time.

The path says, “this exists because it was thought about at this place.” The
source is not a separate entity: no source row, file contents, digest, format,
or retrieval feature is retained.

## `todo_notes`

```text
id          INTEGER primary key
todo_id     required parent todo
text        nonblank working note
created_at  UTC creation timestamp
```

Working notes are append-only. They cannot be updated or deleted. Reads order
them by `created_at ASC, id ASC`; the ID deterministically breaks timestamp
ties. The text and parent relationship are the only domain content.

The initial researched todo note is immutable. Later findings belong in
working notes, so the record does not silently rewrite its own history.

Todos are not deleted in version 1. Foreign keys use restrictive deletion
semantics.

## Email projection

Email configuration, API credentials, rendered digests, Resend identifiers,
send attempts, and delivery status are not stored in SQLite. A digest is a
current read projection over the open rows. The installed configuration owns
the sender and recipient, the process environment owns `RESEND_API_KEY`, and
Resend owns its external delivery and short-lived idempotency records.
