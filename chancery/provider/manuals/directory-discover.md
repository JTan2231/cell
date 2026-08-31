# Discover installed capabilities and operations

Chancery answers two questions from installed, version-matched documentation:

1. Which capabilities and adaptive operations are installed, and what result
   does each one offer?
2. What exactly does a plausible capability claim to do, and where do its
   authority, side effects, recovery, privacy, and live-readiness boundaries
   end?

It does not answer those questions by scanning source trees, historical notes,
or arbitrary documentation. Every provider explicitly indexes its entries.
Chancery fixes each installed provider selector to one canonical bundle for the
duration of a command, validates the manifest and every indexed entry and
manual, and excludes malformed providers as units. Product packaging
separately checks the whole published tree before staging it.

## Routine discovery

For a request whose local-system route is not already established in the
current session, read the complete installed catalog:

```sh
/Users/joey/.local/bin/chancery list
```

The catalog groups entries into `use`, `operate`, and `develop` work and shows
every valid installed entry, including deprecated or dependency-unavailable
ones. Each card includes a discriminative title and summary plus its owner,
release, support, availability, compatibility, and readiness classification.
Registry issues remain visible.

Compare the user's intended outcome with those meanings and form a semantic
shortlist. Chancery does not receive the request, call a model, search the
manuals, or select an entry. If no entry is plausibly relevant, proceed
normally.

Read every plausible full contract before invoking anything:

```sh
/Users/joey/.local/bin/chancery show ENTRY_ID
```

Use the structured sections to decide whether the user's actual outcome fits.
In particular, observe `use_when`, `do_not_use_when`, dependencies, side
effects, privacy, and what the capability does not authorize. If several
contracts remain materially different but plausible, apply ordinary semantic
judgment or ask the user for the choice that matters. Then invoke the selected
interface separately if the request authorizes it. Chancery itself never
invokes it.

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
