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

Commit the changes and submit them through the installed CI manager:

```sh
./ci.sh submit COMMIT
```

The manager integrates, validates, attempts bounded repairs, deploys, and
emails the outcome. Verify the retained job outcome. Remote publication,
schedule changes, recovery, and Semantics state changes remain separate
authorized operations.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
