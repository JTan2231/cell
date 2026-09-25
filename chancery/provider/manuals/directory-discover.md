# Discover installed capabilities and operations

Chancery answers four questions from installed, version-matched documentation:

1. What does one installed product do, and how do its features fit together?
2. Which capabilities and adaptive operations are installed, and what result
   does each one offer?
3. What exactly does a plausible capability claim to do, and where do its
   authority, side effects, recovery, privacy, and live-readiness boundaries
   end?
4. Once one exact entry is selected, what complete outward promise, provider
   scope, dependency closure, exact basis, and unresolved gaps does it expose?

Each provider explicitly indexes its entries. For each command, Chancery
resolves every installed provider selector to one fixed canonical bundle.
It validates the manifest, any indexed overview, and indexed entries and
manuals. Malformed providers are excluded as units. Product packaging separately checks the complete tree
before staging. Chancery does not search source trees or historical notes.

## Routine discovery

For a request whose local-system route is not already established in the
current session, read the complete installed catalog:

```sh
/Users/joey/.local/bin/chancery list
```

The catalog groups entries into `use`, `operate`, and `develop` work and shows
every valid installed entry, including deprecated or dependency-unavailable
ones. Each card includes its ID, title and summary. Shared support, availability,
compatibility and readiness appear once; cards retain exceptions. `show`
provides owner, release and contract version.
Registry issues remain visible.

Compare the intended outcome with the entries and select plausible matches.
Chancery does not receive the request, call a model, search manuals, or select
an entry. If no entry fits, proceed normally.

Read every plausible operating contract before invoking anything:

```sh
/Users/joey/.local/bin/chancery show ENTRY_ID
```

Use each complete operating manual to determine whether the outcome fits.
Observe `use_when`, `do_not_use_when`, dependencies, effects, privacy, and
authorization limits. If several different contracts still fit, use judgment
or ask the user about the material choice. If the request authorizes use,
invoke the selected interface separately. Chancery never invokes it.

When the request concerns a complete system promise or a design reliance,
resolve the selected exact ID after discovery:

```sh
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

Resolution deterministically assembles provider scope, normalized claims, root
and transitive dependency contracts, exact source digests, and gaps. Preserve
the distinctions between `unsupported`, `unspecified`, `not_applicable`, and
`undeclared`. Do not turn a schema or implementation detail into a promise to
fill a gap.

## Product and feature reading

When the product is known, read its installed overview and inventory:

```sh
/Users/joey/.local/bin/chancery product PROVIDER_ID
/Users/joey/.local/bin/chancery list --provider PROVIDER_ID
```

`product` shows the provider identity, release, schema, promise scope when
published, authored overview, and all its entry cards. The inventory uses the
same status defaults and exceptions as `list`. The provider filter selects an
exact ID and combines with `--mode` and `--kind`. Both reads preserve registry
issues. An unknown or excluded provider returns `provider_not_found`.

Only schema 4 can index an optional overview. If the provider has none, the
product view reports `overview_status: not_published`; its inventory remains
readable. A published overview reports `overview_status: published`. These
states describe documentation, not runtime readiness. Chancery does not invent
an overview or follow repository links to obtain one.

Features use the existing `capability` kind. `show FEATURE_ID` reads the full
feature page. `show OPERATION_ID` reads the procedure and its complete operating
essentials. `resolve OPERATION_ID` also reads required feature contracts through
version-bounded `dependencies`. Related Chancery references in prose are
navigation only and do not enter this closure. There is no section-include
mechanism. The overview has no independent entry ID or contract version.

## State and failure boundaries

`support` comes from the owning provider. `availability=installed` means the
indexed bundle is structurally valid. `compatibility` describes declared
documentation-contract dependencies. `readiness` is never established by
Chancery: consult the represented product's own contract and live interface
when readiness matters.

Use `chancery doctor` to diagnose provider manifests, indexed files, duplicate
IDs, and cross-provider compatibility. One broken provider must not prevent a
valid provider from appearing in the catalog. Repair or redeploy the owning
product; do not edit an installed content-addressed bundle in place.

`show ID --full` also includes structured authoring fields and normalized
claims. Ordinary `show` renders the operating manual once; authors must keep
its operating essentials self-contained. Required feature contracts supply
the full behavioral explanation through `resolve`. `resolve ID --summary`
returns the same resolution outcome, requirements, readiness and gaps without
dossier bodies. Use full `resolve`
when the complete outward promise or a design reliance must be read.
Both text and JSON honor these content choices; JSON output schema is 3.
The reader accepts provider schemas 1 through 4. Earlier providers remain
readable without a product overview.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
