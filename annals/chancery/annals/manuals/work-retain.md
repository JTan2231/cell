# Retain an immutable source in Annals

Annals is a local, versioned knowledge library. Retention preserves nonblank
UTF-8 source bytes unchanged or recognizes bytes already present. It does not
invoke the AI reader or change the corpus.

## Add a work

Use a file, or standard input with an explicit label:

```sh
/Users/joey/.local/bin/annals work add <UTF8_INPUT> --name <LABEL>
/Users/joey/.local/bin/annals work add - --name <LABEL>
```

A file may omit `--name`, in which case its UTF-8 filename stem is the proposed
label. A standard-input source always requires a label. The selected library
comes from `--library`, `ANNALS_LIBRARY`, or configuration; Annals never
silently creates `./annals.db`. The selected database must have the immutable
`general` role; producer-accepted decisions libraries reject this source inlet
even when selected directly.

Exact retained bytes are addressed by SHA-256. Re-delivering the same bytes,
even with another requested label, selects the original retained work and its
canonical label. A label already attached to different bytes is a conflict.
Annals does not overwrite either work.

Inspect the result with:

```sh
/Users/joey/.local/bin/annals work list
/Users/joey/.local/bin/annals work show <LABEL>
```

`work show` returns the complete unchanged text, headings, and first-retained
time. Source heading paths describe the document and are not concept paths.

## Durable result and effects

The operation records one manual source delivery. New bytes become one
immutable work; duplicate bytes recognize the existing work. Retention does
not create an examination, reconciliation, commit, or corpus revision.

Use `annals.work.integrate` when the request is to compare the work with the
corpus. Do not use Annals retention for an actionable backlog, a casual note or
preference, a generic file save, or a summary.

The full source is retained in the selected local library. This capability
does not invoke Nucleus, Codex, or the network.

## Output selection

Retention returns its durable work/delivery receipt. `work list` returns a
schema-two `items`/`has_more` page with a default limit of 20. Increase positive
`--limit` to read more. `work show` returns the selected work's complete content
and structure.
