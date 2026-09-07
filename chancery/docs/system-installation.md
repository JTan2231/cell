# User-owned macOS installation

Chancery installs as a user-owned CLI with no service or scheduled process:

```text
~/.local/bin/chancery -> current Chancery release
~/.local/bin/chancery-install -> current Chancery installer
~/Library/Application Support/Chancery/
  providers/                 product-owned provider selectors
  install/
    releases/RELEASE_ID/
      bin/chancery
      bin/chancery-install
      package/install
      share/chancery/chancery/ Chancery-owned provider bundle
      manifest.json           cell-install-v2 exact inventory
    current -> releases/RELEASE_ID
    previous -> releases/RELEASE_ID
```

Chancery publishes only `providers/chancery`, which follows its current release.
It preserves other products' provider selectors. Each provider installer owns
its selector. A broken selector affects that provider; valid providers remain
available.

Deploy with:

```sh
<TESTED_CHANCERY_INSTALL> install --binary <TESTED_CHANCERY_BINARY> \
  --bundle /Users/joey/rust/cell/chancery/provider
```

The deployer stages a content-addressed release. It switches `current`,
`previous`, and the command, installer, and provider selectors with rollback
support. It then verifies the installed command. Nucleus health and
authentication are not required.

## Product-owned publication

Each product stages its unchanged bundle under
`share/chancery/PROVIDER_ID` inside its own content-addressed release and owns
exactly one provider selector. For example:

```text
~/Library/Application Support/Chancery/providers/todo
  -> ~/Library/Application Support/Todo/install/current/share/chancery/todo
```

Every new shared Rust installation uses the provider-ID child. Earlier Chancery
releases used `share/chancery` directly; the Rust installer verifies that legacy
layout when admitting an existing installation or recovering a retained release.
A combined release can carry independently versioned providers, as Annals does.

The selector may exist before the Chancery CLI is installed. Publishing it is
a packaging action only; the product runtime never invokes Chancery. A product
upgrade includes the bundle bytes in its release identity, advances `current`,
and leaves the selector following that current release. A failed upgrade
restores both product behavior and documentation coherently.

Provider schema 3 adds promise scope and normalized entry declarations. Before
publishing schema-3 bundles, deploy a Chancery reader that accepts them. This
reader also accepts schemas 1 and 2, so providers can migrate independently.
During migration, exact-ID dossiers report missing scope and facets as gaps.
Install the reader and required provider releases before global instructions
depend on `chancery resolve`.

Chancery owns `providers/chancery`. Its deployment refuses to take over an
existing selector with a foreign target and never removes other providers.
Uninstalling Chancery's binary should likewise preserve product selectors;
they become readable again when a compatible Chancery reader is installed.

To recover a retained program release, use a trusted tested `chancery-install
recover --release ABSOLUTE_RELEASE_DIRECTORY`, with the canonical owned directory
resolved from `install/previous`. The installer validates the retained release
before selecting it. Do not execute an unverified installer from that release.
Recovery changes Chancery's program and documentation only.

## Inspection and recovery

```sh
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/chancery list
/Users/joey/.local/bin/chancery show ENTRY_ID
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

`doctor` checks provider structure and cross-provider contract compatibility.
`resolve` assembles provider scope, normalized claims, dependency closure, and
exact source references. It reports gaps and leaves live readiness unchecked.
To repair an invalid provider, validate its source bundle, run its deployment
tests, and redeploy the product. Do not edit an installed content-addressed
release or point a selector at a source checkout.
