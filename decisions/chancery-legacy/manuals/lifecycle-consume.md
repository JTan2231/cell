# Retired Decisions lifecycle compatibility

Existing consumers may continue to call `krisis events watermark` and
`krisis events read` with their durable opaque cursor. Persist each returned
legacy event and its item cursor atomically before advancing.

This surface is frozen: Krisis does not append new lifecycle events and new
decision accounts are available through Annals, not this stream. Do not parse
or manufacture cursors, read SQLite directly, or treat legacy state as proof of
Krisis delivery.

Rust compatibility consumers may use `krisis_api::lifecycle::Client` and its
exported watermark, page, item, and envelope structs. The client invokes the
same `krisis events` commands. This does not add new consumers, events, history
origins, acknowledgement, or direct database access.
