# User-owned macOS installation

Chancery installs as a user-owned CLI with no service or scheduled process:

```text
~/.local/bin/chancery -> current Chancery release
~/Library/Application Support/Chancery/
  providers/                 product-owned provider selectors
  install/
    releases/RELEASE_ID/
      bin/chancery
      package/deploy-user.sh
      share/chancery/          Chancery-owned provider bundle
      manifest.txt
    current -> releases/RELEASE_ID
    previous -> releases/RELEASE_ID
```

Chancery deployment preserves provider selectors owned by other products. It
publishes only `providers/chancery`, which follows Chancery's own current
release. Each other provider installer owns exactly its selector. A broken
selector is a provider failure, not a reason to make valid providers
unavailable.

Deploy with:

```sh
./packaging/macos/deploy-user.sh --binary ABSOLUTE_PATH
```

The deployer stages a content-addressed release, switches `current`,
`previous`, and `~/.local/bin/chancery` atomically with rollback, then verifies
the installed command. No Nucleus health or authentication is required.

## Product-owned publication

Each product stages its unchanged bundle under
`share/chancery/PROVIDER_ID` inside its own content-addressed release and owns
exactly one provider selector. For example:

```text
~/Library/Application Support/Chancery/providers/todo
  -> ~/Library/Application Support/Todo/install/current/share/chancery/todo
```

Chancery's own single-provider release uses `share/chancery` as its bundle
root. Other products use the provider-ID child so one combined release can
carry more than one independently versioned provider, as Annals does.

The selector may exist before the Chancery CLI is installed. Publishing it is
a packaging action only; the product runtime never invokes Chancery. A product
upgrade includes the bundle bytes in its release identity, advances `current`,
and leaves the selector following that current release. A failed upgrade
restores both product behavior and documentation coherently.

Provider schema 3 adds provider promise scope and normalized entry
declarations. Deploy a Chancery reader that accepts schema 3 before publishing
the first schema-3 product bundle. That reader continues to accept schemas 1
and 2, so providers can migrate independently; their exact-ID dossiers show
missing scope and normalized facets as explicit gaps during the mixed-schema
period. Only after the reader and required provider releases are installed may
global instructions depend on `chancery resolve`.

Chancery owns `providers/chancery`. Its deployment refuses to take over an
existing selector with a foreign target and never removes other providers.
Uninstalling Chancery's binary should likewise preserve product selectors;
they become readable again when a compatible Chancery reader is installed.

## Inspection and recovery

```sh
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/chancery list
/Users/joey/.local/bin/chancery show ENTRY_ID
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

`doctor` checks provider structure and cross-provider contract compatibility,
not live product readiness. `resolve` assembles the installed provider scope,
normalized claims, dependency closure, and exact basis—or reports their
explicit gaps—while continuing to report readiness separately as unchecked.
Repair an invalid provider by
validating its source bundle, running that product's deployment tests, and
redeploying the owning product. Do not edit a content-addressed installed
release or repoint a selector to a source checkout.
