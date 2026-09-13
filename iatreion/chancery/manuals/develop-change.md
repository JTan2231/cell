# Change Iatreion reporting

Query the applicable Semantics repository and read the affected installed
contracts before changing Iatreion or a product-owned probe.

Keep Iatreion stateless and read-only. Preserve product authority, explicit
unknown coverage, independent status dimensions, bounded fixed-argument probe
execution, and separate runtime and domain outcomes. A probe must not initialize,
migrate, repair, reconcile, refresh credentials, claim a notification, or run
product work.

Update code, tests, product declarations, documentation, packaging, and
Chancery claims together when their shared meaning changes. Version incompatible
status or report semantics explicitly.

Run focused tests, then the default root gate:

```sh
./ci.sh
```

Development does not authorize release, installation, deployment, schedule
changes, recovery, or Semantics state changes.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
