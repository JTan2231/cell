# Report declared Cell membership

Usher reports whether a product has declared the essentials for recognition
within Cell: its identity and owned root, its Semantics participation, and its
Chancery presence. It is a Rust CLI with no database, daemon, network, model,
service invocation, or retained completion state.

From a Cell checkout:

```sh
target/release/usher report .
target/release/usher check .
target/release/usher --json report . --product krisis
```

Build and test with `./ci.sh usher`. The root `./ci.sh` runs a candidate Usher
membership check for every invocation, including when selecting other product
gates. The compiler runs inside the existing CI broker's heavy lane.

## What is checked

The inventory is every `pipeline/products/*.sh` descriptor, in filename order.
Missing introductions do not remove products from the report. The Cell root,
individual Cargo crates, and extra release units are not separate products
unless they have their own descriptor. Annals therefore has one product with
two providers; Krisis retains the `decisions` descriptor ID and declared aliases.

1. **Identity:** a schema-one literal descriptor identifies the product, its
   display name, aliases, and an existing owned root. Its ID matches its
   filename. Product roots and ID/alias claims are unambiguous across the inventory.
2. **Semantics:** a regular root `AGENTS.md` contains exactly one exact
   `Semantics-Project: ID` line. The ID starts with a lowercase ASCII letter,
   contains only lowercase letters, digits, or hyphens, and is at most 64 bytes.
   Different products may not claim the same semantic project. The ID need not
   equal the directory, provider ID, or product ID.
3. **Chancery:** the descriptor declares at least one provider, and every
   declared bundle belongs to that product's root. Its readable `provider.json`
   identifies the declared provider, name, release, and a nonempty entry index.
   Indexed entries have unique provider-prefixed identities, positive contract
   versions, and readable nonempty indexed manuals. Provider IDs cannot have
   multiple claimants. Usher supports provider schemas 1 through 3.

These are **introduction projections**, not a replacement Chancery validator.
Usher does not validate the remaining contract fields, promise scope, dependency
graph, release alignment, or completeness of the published capabilities.
Chancery's existing gates retain full bundle validation. Usher checks no
Pratica, Nucleus, Clockwork, or other relationship. Those affairs belong to the
concerned systems.

A marker declares participation; it does not prove registration, an existing
vocabulary, or current vocabulary quality. A source bundle declares Chancery
presence; it does not prove installation or runtime readiness. No installed
service, database, environment-selected registry, or historical record is read.

## Results and failures

Each product reports `identity`, `semantics`, and `chancery` findings with
identities, relative evidence paths, and explicit issues:

- `declared`: the required introduction evidence is structurally recognizable.
- `missing`: an expected declaration or referenced file is absent.
- `invalid`: malformed or conflicting evidence, an unsafe path, or an invalid identity.
- `unassessed`: unreadable evidence, an unsupported format, or a read limit.

Combined findings retain every issue; the displayed status prioritizes
unassessed, invalid, missing, then declared. A product is complete only when all
three findings are declared. Selection by `--product` occurs after global
collision checks. Unknown or ambiguous selections are errors.

`report` exits 0 when it can produce the report, including incomplete products.
`check` emits the same report and exits 1 for incomplete selected products.
Both exit 2 for command or inventory errors; an absent or empty inventory cannot
produce a successful empty report. JSON output has `schema_version: 1` and
`scope: "repository_declarations"`; fatal errors have a schema-versioned
`error` field. Findings contain no document bodies.

Descriptors are parsed as data: uppercase/underscore assignment names, plain
alphanumeric/path literals, and whole single-quoted values, including multiline
values. Blank lines and comments are allowed; duplicate assignments are invalid.
Shell expressions, double quotes, and other shell syntax are unassessed and
never executed. All evidence paths must be relative and remain under their
owning roots without symlinks. Files must be regular UTF-8, at most 1 MiB;
inventories are limited to 256 descriptors and each provider to 512 entries.

The same unchanged checkout and Usher version produce the same report. Reads
are not an atomic filesystem snapshot; callers keep the checkout stable. Root
CI additionally rejects results if its source candidate changes during the run.
Fix the source declaration and rerun; Usher has no reset or repair command.
