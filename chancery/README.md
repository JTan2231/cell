# Chancery

Chancery is the installed, read-only directory and exact-ID promise resolver
for local capabilities and adaptive operations. It lists complete semantic
catalog cards, presents versioned contracts, and assembles a selected entry's
provider scope, normalized boundary claims, dependency closure, exact basis,
and explicit gaps without executing the represented application, operation,
readiness check, or model.

Chancery has no daemon, database, network access, Nucleus integration, or
domain authority. Provider products own their capability truth and installed
provider selectors. Chancery owns bundle validation, catalog discovery,
deterministic dossier assembly, exact-basis identification, dependency
closure, facet and gap classification, and presentation. The interactive agent
uses ordinary language understanding to select plausible entries from the
catalog.

## Build and check

```sh
./ci.sh
```

## Deploy

After a release build:

```sh
./packaging/macos/deploy-user.sh \
  --binary "$PWD/../target/release/chancery"
```

See [the documentation index](docs/README.md) for the CLI, bundle contract,
architecture, and installation boundaries.
