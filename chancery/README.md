# Chancery

Chancery is the installed directory for local capabilities and adaptive
operations. It lists catalog entries, reads versioned contracts, and resolves
one exact entry ID. Resolution gathers provider scope, boundary claims,
dependency contracts, source references, and gaps into one dossier.

Chancery reads documentation only. It has no daemon, database, network access,
Nucleus integration, or product-domain authority. It does not execute the
documented interface, test readiness, or call a model. Product owners publish
their contracts and own their installed provider selectors. The interactive
agent compares the user's request with catalog titles and summaries to select
plausible entries.

Chancery owns bundle validation, catalog discovery, deterministic resolution,
exact source identification, dependency closure, claim classification, and
presentation.

## Rust callers

Rust callers use `chancery::api::Client` for typed list, show, resolve, doctor,
and validate operations. The caller selects an executable and registry.
Reports distinguish unresolved or invalid domain results from transport errors.

The Rust library exposes provider-owned bundle documents and CLI output types
through `chancery::api`. `ProviderManifest::decode` and `EntryDocument::decode`
use the same codecs as the CLI, including legacy schema handling. Decoding a
document is separate from full bundle validation.

`ProviderIntroduction` and `EntryIntroduction` read the identity and
indexed-manual fields that Usher needs. They ignore other fields and do not
evaluate promises, dependencies, or full bundle validity. Usher owns membership
policy. `Output<T>` and the command result types define the JSON output that
the CLI serializes.

## Build and check

```sh
./ci.sh
```

## Deploy

After a release build:

```sh
<TESTED_CHANCERY_INSTALL> install \
  --binary <TESTED_CHANCERY_BINARY> \
  --bundle /Users/joey/rust/cell/chancery/provider
```

See [the documentation index](docs/README.md) for the CLI, bundle contract,
architecture, and installation boundaries.
