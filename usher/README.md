# Usher

Usher reads a product's declared identity, owned root, Semantics participation,
and Chancery presence to report its Cell membership. It is a Rust CLI with no
database, daemon, network or model calls, service invocation, or retained state.

From a Cell checkout:

```sh
target/release/usher report .
target/release/usher check .
target/release/usher --json report . --product krisis
```

Build and test with `./ci.sh usher`. The gate builds `usher` and the separate
`usher-install` executable. The root `./ci.sh` runs a candidate Usher
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

The Rust library exports these report and error types plus the read-only
`inspect` function through `usher::api`; the CLI uses the same definitions.
Usher decodes Chancery introductions through Chancery-owned partial views.
Those readers deliberately ignore unrelated contract fields; Usher still owns
its membership, path, read-limit, and collision checks.

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
`check` emits counts and every incomplete finding, omitting successful products
and successful evidence, and exits 1 for incomplete selected products. Its JSON
schema is 2; `report` retains full schema-one evidence. `CheckReport` is the
provider-owned compact type.
Both exit 2 for command or inventory errors; an absent or empty inventory cannot
produce a successful empty report. Full report JSON has `schema_version: 1` and
`scope: "repository_declarations"`; fatal errors have a schema-versioned
`error` field. Findings contain no document bodies.

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

## Installation

The product has an independent version. Its separate Rust `usher-install`
executable uses the shared `cell-install` library to install the recognition
binary and its version-matched Chancery documentation for the current macOS
user. Complete relevant development CI before release or deployment. Prepare
production executables and install with:

```sh
python3 /absolute/cell/deployment/build.py --source-root /absolute/cell \
  --product usher --output /absolute/cell-build
/absolute/cell-build/candidates/usher/bin/usher-install install \
  --binary /absolute/cell-build/candidates/usher/bin/usher \
  --bundle /absolute/cell/usher/chancery
```

The installer retains its own exact executable, both command binaries, and the
bundle in a content-addressed release with a `cell-install-v1` JSON manifest.
One `current` selector selects `.local/bin/usher`, `.local/bin/usher-install`,
and the owned Chancery provider together. `--expected-current
absent|releases/HASH` requires an exact prior selection; omission observes it
before waiting for the product lock. `--home ABSOLUTE_PATH` selects an
intentional alternate or isolated home.

`usher-install inspect` reports the owned installation, and `usher-install
verify --binary ABSOLUTE_PATH --bundle ABSOLUTE_PATH` checks it against an
exact candidate and the executing installer. `verify-release
ABSOLUTE_RELEASE_DIR` checks a retained release's integrity without changing
selectors. Deliberate recovery uses the retained Rust `package/install recover
--release ABSOLUTE_RELEASE_DIR` for either a new-format or supported legacy
release. Legacy recovery detaches the public installer selector; the retained
Rust executable can later reselect its release. See the
[installation contract](chancery/manuals/install-operate.md)
for ownership, failure and recovery requirements.

`./deploy.sh usher` prepares both executables through the shared release builder
and invokes the sealed installer's Rust adapter through the coordinator's
version-one JSON protocol. Usher has no Python deployment adapter or generated
shell installer. Other products retain their existing installation paths.
Release and deployment preparation build production binaries, reuse matching
sealed artifacts, and check versions and hashes. They do not rerun CI or require
a prior CI receipt; `./ci.sh usher` remains the full development gate.
Installation changes only Usher's release and command/provider selectors; the
recognition library and `usher` command remain read-only. No semantic project,
database, worker, schedule, or other product runtime is changed. See
[shared deployment](../deployment/README.md).
