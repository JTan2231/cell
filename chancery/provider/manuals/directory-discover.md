# Discover installed capabilities and operations

Chancery answers three questions from installed, version-matched documentation:

1. Which capabilities and adaptive operations are installed, and what result
   does each one offer?
2. What exactly does a plausible capability claim to do, and where do its
   authority, side effects, recovery, privacy, and live-readiness boundaries
   end?
3. Once one exact entry is selected, what complete outward promise, provider
   scope, dependency closure, exact basis, and unresolved gaps does it expose?

Each provider explicitly indexes its entries. For each command, Chancery
resolves every installed provider selector to one fixed canonical bundle.
It validates the manifest and indexed entries and manuals. Malformed providers
are excluded as units. Product packaging separately checks the complete tree
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
it self-contained. `resolve ID --summary` returns the same resolution outcome,
requirements, readiness and gaps without dossier bodies. Use full `resolve`
when the complete outward promise or a design reliance must be read.
Both text and JSON honor these content choices; JSON output schema is 3.
