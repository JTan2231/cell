# Install or recover Mantic

Use this procedure to install a matched Mantic program and provider bundle, or
select a retained compatible release. The user owns the decision to install or
recover. Mantic owns its program and provider selectors and config initialization.
Read `mantic.installation` for the detailed selection and state contract.

## Prerequisites and boundaries

Use absolute source paths and one matched release. Keep private definitions
outside the source and release trees. Installation has no daemon or schedule to
stop. Do not take over a foreign program or provider selector, edit an immutable
release, bypass unsupported state, or restore definitions through program recovery.

The Cell CI manager supplies a schema-two deployment request through
`mantic-install deploy`. It publishes programs and documentation, then initializes
the default database when empty. An initialization failure can occur after
program selection. Completed changes remain for inspection; no configs or items
are added. Use the retained manager outcome to distinguish delivery stages.

## Direct installation

1. Inspect the target selection with the trusted installer:

   ```sh
   /absolute/path/mantic-install inspect
   ```

2. Select the matched program and bundle. Guard the observed prior selection
   with `--expected-current absent` for a fresh installation or
   `--expected-current releases/ID` for an existing release:

   ```sh
   /absolute/path/mantic-install install \
     --binary /absolute/path/mantic \
     --bundle /absolute/path/chancery \
     --expected-current absent
   ```

3. Initialize the private config database when needed:

   ```sh
   /Users/joey/.local/bin/mantic init
   ```

4. Register the selected command inventory separately:

   ```sh
   /Users/joey/.local/bin/mantic --register-usage
   ```

5. Inspect program selection and installed documentation:

   ```sh
   /Users/joey/.local/bin/mantic-install inspect
   /Users/joey/.local/bin/chancery doctor
   /Users/joey/.local/bin/chancery product mantic
   /Users/joey/.local/bin/chancery list --provider mantic
   /Users/joey/.local/bin/chancery show mantic.config.manage
   /Users/joey/.local/bin/chancery resolve mantic.forecast.calculate
   ```

Successful selection reports the intended archive and publishes its bytes at
fixed runtime paths. `mantic config list` verifies
that the selected database can be read. Catalog checks verify publication only;
they do not prove a forecast or authorize adding definitions.

## Retained-release recovery

1. Inspect the current release and any completed changes from the failed
   instruction. Preserve the configuration database.
2. Select an exact retained release that supports the existing database:

   ```sh
   /Users/joey/.local/bin/mantic-install recover \
     --release '/Users/joey/Library/Application Support/Mantic/install/releases/RELEASE_ID' \
     --expected-current releases/CURRENT_ID
   ```

3. Inspect the selected release, register its usage inventory, and repeat the
   relevant catalog and config-read checks above.

Recovery selects programs and provider together. It does not restore deleted
configs, earlier item values, balances, or runs. Use `--home ABSOLUTE_PATH` on
all installer invocations for an isolated installation; use the runtime's
global `--database ABSOLUTE_PATH` separately for isolated definition checks.

Stop when source releases are mismatched, a selector has foreign ownership,
the expected selection differs, current state is unsupported, explicit usage
registration fails, or the retained operation has unknown effects. Inspect the
specific failure and preserve state before attempting another change.

Installation and recovery are local filesystem operations. They disclose no
definitions or results to a service. They do not authorize configuration edits,
data replacement, scheduling, or external output delivery.
