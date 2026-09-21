# Cell prompt retrieval

`cell-prompts` is a caller library over Bazaar's supported read API. Bazaar
stores opaque strings. This library owns Cell's selection format and rendering.
Products still own permissions, models, schemas, tool implementations, input
data, and recovery. No prompt grants execution authority.

The import input `seed.json` preserves the original migration components and
adds separately named project-writing prompts. It includes instructions,
templates, tool and schema descriptions, Mentor's
problems and rubric, selected library documents, and compatibility prompts.
It is an explicit import artifact, never a runtime fallback.

## Read a selection

The default database is `~/.local/share/bazaar/bazaar.sqlite3`. An absolute
`CELL_BAZAAR_DATABASE` overrides that path for the caller. Reads require private,
initialized Bazaar state. Missing selections, missing components, unsupported
selection formats, and invalid templates stop request preparation.

Each product reads `cell.prompts.OWNER` once, then reads the exact component
versions named in that string. Owners are `annals`, `conatus`, `krisis`,
`semantics`, `paperboy`, `platter`, `weaver`, `mentor`, and `emt`. For example:

```json
{"schema_version":1,"entries":{"weaver.narrative.instructions":1,"weaver.tools.read_decisions.description":1,"weaver.tools.submit_document.description":1}}
```

Publish component versions before publishing the selection that names them.
This makes each product's selected set consistent without assuming that Bazaar
provides a transaction across reads. A selection change affects new requests.
Products retain exact requests or selection versions for interrupted work.

Templates use named `{value}` or positional `{}` placeholders and doubled
braces for literal braces. Callers supply already formatted values. Rendering
is one pass: source data cannot introduce another placeholder or prompt lookup.
Explicit `<bazaar:ID>` references are internal caller constants, resolved only
in trusted instruction and tool-description fields.

Annals library revisions and Mentor assignments remain immutable domain
snapshots. Bazaar supplies their initial authored documents. Changing a library
selection or refreshing Mentor's domain corpus remains an explicit product
operation. User directions, work documents, answers, and tool results remain
runtime input rather than shared prompt records.

## Import before deploying callers

1. Review `seed.json` and select the intended private Bazaar database.
2. Run the explicit importer from the Cell checkout:

   ```sh
   cargo run --locked --offline --package cell-prompts -- \
     /absolute/private/bazaar.sqlite3 prompting/seed.json
   ```

3. Read each `cell.prompts.OWNER` through Bazaar and verify its exact referenced
   versions before deploying a requester. Use the same database for all callers
   of one installed product. Scheduled environments normally use the default
   path; an interactive environment override does not configure Clockwork.

The importer initializes only the selected Bazaar path. It preserves unrelated
IDs, reuses identical latest content, and publishes selections after every
component exists. An interrupted import may leave unused component versions.
Repeat the same import to finish; inspect history after an uncertain write.
Publication is per product, not an atomic cutover of all nine products.

Selection version 1 is the immutable migration baseline for historical jobs.
Import the original seed before publishing edits. Keep that baseline and all
referenced versions. The importer does not change installed binaries, publish
Chancery bundles, start agents, send mail, or deploy services.

For a later edit, append text with Bazaar's `update` command, then append a
complete selection with the new returned version numbers. To roll back, append
the previous selection content. Do not overwrite history. New IDs in the seed
do not replace the original component text. The importer expects complete
per-owner content sets; a partial set would remove omitted keys from that
owner's next selection.

## Execution compatibility

New dynamic toolset versions encode the selection version above the product's
historical range: Annals adds 2; Platter adds 3; Krisis, Semantics, Paperboy, and
Weaver add 1. Annals also versions input schema IDs because its schema fields
contain editable descriptions. Old toolset identities keep the original text
from selection 1. Structural schema changes still require a code change and
the requester's compatibility procedure.

Annals records the selection in its examination prompt version and context
digest. Platter captures it with packet inputs. Other requesters preserve the
resolved request under their existing recovery and retention rules. Mentor
and EMT retain their existing limits on temporary answer and exchange content.

CI imports the seed into a private temporary Bazaar database for each admitted
test gate. It never uses the user's prompt database. Prompt-library edits select
all consumers in the default root CI; the shared `prompts` gate tests the
library and importer. Run `./ci.sh` from the Cell root.
