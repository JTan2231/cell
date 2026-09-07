# CRM

CRM is a private local case library for employment-oriented relationship work.
It also stores reusable career vignettes, statements, and other profile material
as directly editable Markdown entries in the same database. It keeps the raw information a caller supplies, an immutable revision history,
and exact execution correlations behind each AI-assisted update. Supported
reads expose the case lineage and CRM/Nucleus identities; version 0.3 does not
provide a raw-delivery or mailbox-receipt export command.

CRM stores case narratives, stages and supporting material. A `warranted`,
`connected`, or `helped` stage is part of the stored case history. Source
references remain attached to their deliveries. Any revision advisory is
shown prominently and never acts as a gate. CRM performs local library reads
and writes; the caller handles messages and other contact actions.

## Build and check

```sh
./crm/ci.sh
cargo build --release --locked --package crm
```

## Isolated smoke

```sh
temporary=$(mktemp -d)
target/release/crm --database "$temporary/crm.db" init
target/release/crm --database "$temporary/crm.db" \
  case new --title "Example opportunity"
target/release/crm --database "$temporary/crm.db" case list
```

The installed database defaults to
`~/Library/Application Support/CRM/crm.db`. Use `--database` or
`CRM_DATABASE` for an isolated library. CRM never falls back to a database in
the current directory.

`crm tell CASE_ID INPUT` stores the supplied UTF-8 text and a queued update,
launches the hidden worker, and returns without waiting for AI work. Inspect or
recover that work with `crm update list`, `show`, `wait`, `resume`, and
`retry`.

When no initial Markdown is supplied, CRM starts a small suggested outline:
`Current picture`, `People`, `Chronicle`, and `Open threads`. These headings
are editorial hints only. Caller-supplied Markdown and steward revisions remain
free-form; CRM never parses or requires the outline.

`crm profile new --title TITLE INPUT` retains one profile entry; `profile list`,
`show`, and `update` read or replace its current content. Profile operations run
locally without AI. Existing schema-one databases require an explicit
`crm migrate --backup PATH` before this release can use them.

Start with [the documentation index](docs/README.md). The
[CRM provider bundle](chancery/provider.json) is the release-matched index of
supported outward promises. After choosing an exact entry, use
`chancery resolve crm.library.explore` (or the chosen ID) for its normalized
boundary and explicit gaps. `release.sh` publishes a Git release and is not a
build or installation command.
