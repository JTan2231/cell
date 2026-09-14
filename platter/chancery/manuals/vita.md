# Vita career library

Vita is the Annals library named `vita`. It holds career accounts, preferences,
application guidance, and disclosure rules migrated from CRM. Annals owns its
storage and read interfaces. Vita has no separate executable or service.

## Read career material

Use the registered name. Annals resolves its managed storage; callers do not
need a database path.

```sh
~/.local/bin/annals library vita show
~/.local/bin/annals library vita work list --limit 1000
~/.local/bin/annals library vita work show LABEL
```

Copy an exact work label from the list into `LABEL`. Add `--json` for machine
output. Success uses the `ok`/`data` envelope. Work list returns schema-two
`items` and `has_more`; its default limit is 20. Increase `--limit` when
`has_more` is true. Work show returns the selected work's complete `text`.

A work is one immutable source document. Its label selects it; headings
describe its content. `size_bytes` counts its UTF-8 bytes, and
`first_retained_at` records when those bytes entered Annals. These fields do
not describe employment dates or when a career claim became true.

Reads invoke no model or network service. A missing library or failed read is
an error; reading never creates or repairs the library.

## Find supporting ideas

```sh
~/.local/bin/annals library vita search 'QUERY'
~/.local/bin/annals library vita concept evidence CONCEPT_ID
~/.local/bin/annals library vita instructions show
```

Search matches concept labels and ancestor context. It does not search all
source text. Evidence returns quotations from retained works. Read the full
work for qualifications and disclosure guidance. The library instructions
govern Annals' organization of concepts; they do not replace the source text.

## Use in Platter

Platter reads `vita` through `~/.local/bin/annals`. Both selections are fixed.
It lists up to 1000 works and reads each complete document. An empty library,
an incomplete list, or a failed read stops preparation through its normal
error handling.

Each work becomes one career entry: its label is the ID, its first heading is
the title (or its label when no heading exists), and its complete text is the
body. Platter retains this data with the preparation for its existing career
tools. It stores no additional Vita capture metadata. Work additions after the
list are available to a later preparation.

Platter reads all retained works, regardless of Annals corpus interpretation.
It does not query CRM, synchronize libraries, modify Vita, or run Annals
integration. Historical preparations continue to use their retained entries.

## Maintain the library

For an authorized addition, retain a new UTF-8 document:

```sh
~/.local/bin/annals library vita work add /absolute/career-note.md --name LABEL
```

Annals preserves its bytes. Identical bytes select the existing work; reusing
a label for different bytes fails. This is an addition, not replacement of an
earlier career entry. Platter reads both old and new works, so explain any
correction in the supplied text. Retention starts no model work.

Use the installed `annals.work.retain`, `annals.corpus.explore`, and
`annals.library.operate` Chancery contracts for Annals operations. Keep career
text, command output, and library backups private. Platter supplies the career
data to its existing Nucleus/model stages; authorized packet delivery uses its
normal email path.
