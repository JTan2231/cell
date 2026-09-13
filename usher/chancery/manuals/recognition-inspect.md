# Report declared Cell membership

Usher reads a product's declared identity, owned root, Semantics participation,
and Chancery presence to report its Cell membership. It is a Rust CLI with no
database, daemon, network or model calls, service invocation, or retained state.

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

These **introduction projections** select provider identity, indexed entry
identity and version, and indexed manual presence. Chancery validates bundle
declarations and dependencies; product packaging checks release alignment.
Usher applies the membership, path, read-limit, and collision rules described
here.

Iatreion uses a separate Rust API projection of the optional `STATUS_SCHEMA`,
`STATUS_COMMAND`, and `STATUS_UNITS` descriptor fields. Each unit line contains
`unit ID|intent|Clockwork key|inspection capability`. The projection validates
identities, selector basenames, supported schema one, and global unit
collisions. It never invokes a command. These fields do not change membership
CLI output or completeness.

A marker records the declared Semantics project ID. A source bundle records
the declared Chancery provider and indexed introductions. Recognition reads
these repository files directly, using the rules above.

## Results and failures

Each product reports `identity`, `semantics`, and `chancery` findings with
identities, relative evidence paths, and explicit issues:

- `declared`: the required introduction evidence is structurally recognizable.
- `missing`: an expected declaration or referenced file is absent.
- `invalid`: malformed or conflicting evidence, an unsafe path, or an invalid identity.
- `unassessed`: unreadable evidence, an unsupported format, or a read limit.

Combined findings retain every issue. The displayed status prioritizes
`unassessed`, `invalid`, `missing`, then `declared`. All three findings must be
`declared` for a complete product. Selection by `--product` occurs after global
collision checks. Unknown or ambiguous selections are errors.

`report` exits 0 when it can produce the report, including incomplete products.
`check` emits counts and all incomplete boundary findings, omits successful evidence,
and exits 1 for incomplete selected products.
Both exit 2 for command or inventory errors; an absent or empty inventory cannot
produce a successful empty report. Report JSON has `schema_version: 1`; check JSON has `schema_version: 2`. Both have
`scope: "repository_declarations"`; fatal errors have a schema-versioned
`error` field. The operational projection is available through
`usher::api::inspect_operations`; it is not a fourth membership finding.
Findings contain no document bodies.

Descriptors are parsed as data: uppercase/underscore assignment names, plain
alphanumeric/path literals, and whole single-quoted values, including multiline
values. Blank lines and comments are allowed; duplicate assignments are invalid.
Shell expressions, double quotes, and other shell syntax are unassessed and
never executed. All evidence paths must be relative and remain under their
owning roots without symlinks. Files must be regular UTF-8, at most 1 MiB;
inventories are limited to 256 descriptors and each provider to 512 entries.

The same unchanged checkout and Usher version produce the same report. Keep the
checkout stable during reads; they are not an atomic filesystem snapshot. Root
CI additionally rejects results if its source candidate changes during the run.
Fix the source declaration and rerun. Usher has no reset or repair command.

## Output selection

`check` uses schema 2. It returns counts and each incomplete identity, Semantics,
or Chancery finding, and omits successful evidence. `report` returns the full
product report in schema 1. An incomplete check exits 1. Fatal command or
inventory errors exit 2. Both outputs describe repository declarations; they do
not test runtime readiness or registration.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
