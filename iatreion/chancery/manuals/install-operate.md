# Install or recover Iatreion

Iatreion uses the shared stateless Cell installer. Installation selects one
immutable release containing the reporter, installer, and matching provider.
It creates no database, service, schedule, cache, or product state.

```sh
iatreion-install install --binary ABS --bundle ABS
iatreion-install inspect
iatreion-install verify --binary ABS --bundle ABS
iatreion-install verify-release ABS_RELEASE
iatreion-install recover --release ABS_RELEASE
```

Use `--expected-current absent|releases/HASH` to bind a mutation to the inspected
selection. Use `--home` only for an intentional alternate current-user boundary.

The installer rejects foreign, stale, tampered, or unproved content. A failed
publication restores the proved prior selectors when possible. Recovery selects
only an exact supported retained release. Installation does not deploy product
status probes, run Iatreion, register Semantics, or operate another product.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
