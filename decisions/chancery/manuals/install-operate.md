# Install and operate Krisis

The public binary and provider are `krisis`. The compatibility provider is
`decisions`, the active Clockwork key is `krisis/observer`, and the schema is 4.
Existing Decisions application-support and log paths are retained for
compatibility with persistent history.

`krisis-install install` requires absolute paths for Krisis, Clockwork, Codex,
Annals, and the dedicated Annals config. The decisions-library ID must contain
exactly 32 lowercase hexadecimal characters. The selected Codex path is recorded
in the immutable Clockwork definition
and used unchanged for final-cutover doctor and scheduled Conversations reads;
the observer does not discover another Codex installation at runtime.
Interactive source-reading commands must receive that same path through
`CONVERSATIONS_CODEX` because the installed command does not inherit
Clockwork's observer environment; doctor and process also require the complete
explicit Annals configuration. The default operation prepares and verifies a
content-addressed release and Clockwork definition while retaining the
maintenance gate; it does not select or activate them. After the outer cutover
has separately proved its Annals and semantic prerequisites, `--final-cutover`
performs writer shutdown, quiescent backup, migration, doctor, baseline
activation, selector/hook publication, and schedule handoff. Clockwork process state
is not cross-system proof.

Every selected definition and legacy plist is inspected and attributed before
mutation. Disabled or foreign legacy bindings are untouched. A provable failure
restores exact prior selectors and enabled state; a prior null Clockwork
selection that cannot be restored leaves the owned candidate disabled with the
maintenance gate and transaction evidence retained.

After a separately authorized final cutover, verify the baseline, doctor, exact
hook trust, private observer ownership receipt, `krisis/observer` history,
retired binding state, and body-free logs. Uninstall disables only the
exact owned active binding and retains the database, receipt ledger, releases,
logs, scheduler history, and legacy history.

## Run-owned deployment admission

```text
krisis --database DATABASE --json maintenance status
krisis --database DATABASE --json maintenance hold RUN_ID
krisis --database DATABASE --json maintenance release RUN_ID
```

The private sibling `<database>.cell-maintenance` is separate from the
installer's `.clockwork-maintenance` marker and receipt. These commands never
open, initialize, or migrate SQLite; status leaves an absent gate absent.
Their JSON has `protocol_version: 1`, `contract_version: 1`, `holds`, and
`drained`. Drain describes participating live commands. The product must
separately verify that durable observations and dependency jobs have stopped.

Any hold fences every other public CLI and typed client command before
database access, including status and doctor, since opening state can migrate
it. Existing commands may settle. Holds survive process exit; repeated hold
and release are idempotent. Release removes only its named owner and preserves
the observer baseline. IDs contain 1–128 ASCII letters, digits, hyphens,
underscores, or periods and cannot begin with a period.

Controlled commands set `CELL_DEPLOYMENT_RUN_ID` for the exact sole hold and
exclusive drained activity. With no hold they use ordinary admission. Only
doctor can use this identity to prove deliberately held Nucleus readiness;
runtime drain, authentication, harness, product capability, and protocol
checks still apply. Observation processing requires normal Nucleus admission.

The sealed Rust `krisis-install adapter OP` boundary composes preparation and explicit final cutover while
preserving captured schedule enabled booleans and baseline identity. It does
not infer a legacy Semantics activation watermark. An ordinary Annals binary
or config pin update proves the prior definition against its release and old
receipt target, requires the same persistent decisions-library ID, then
validates the new target with candidate doctor. A foreign receipt or changed
library ID stops the transition.

Coordinated inspection requires maintenance support from installed public
executables before effects. Unsupported old binaries need their compatibility
release through the documented deployer and writer-quiescence procedure; a
candidate gate cannot fence them. Recovery stops on retained installer
maintenance or an unfinished product transaction and leaves the outer hold
for the existing recovery procedure. It never deletes those markers or
another owner's hold to force progress.

The deployment adapter verifies the installed dependency configuration with
doctor. Verification does not create observations or submit Nucleus jobs.
When Krisis is selected for upgrade, verification requires the exact admitted
candidate and candidate dependency pins. When it participates only in
maintenance, verification requires the unchanged captured installation and
checks readiness with its retained dependency pins.

The package builds both `krisis` and `krisis-install`. New releases retain the
exact Rust helper at `bin/krisis-install` and `package/install`, with a complete
`cell-install-v2` artifact manifest. Static frontend and observer scripts remain
release data. The shared Rust library verifies artifacts and owns selector
transactions; Krisis owns hook, database, admission, and scheduler lifecycle.
`krisis-install verify-release ABSOLUTE_RELEASE` is read-only integrity proof.
`krisis-install inspect` checks the selected installation. Retained
`package/install install` uses its sibling package data and explicit exact
payload/dependency pins. Source invocation supplies `--source-root` for the
absolute Decisions product source directory. Legacy formats 2, 3, and 4 remain
validated migration inputs; archived shell deployers do not install the new
manifest. Uninstall uses `krisis-install uninstall --clockwork ABSOLUTE_PATH` and
retains current/previous, releases, private state, receipts, and maintenance.

For a source build, prepare with:

```text
krisis-install install --source-root ABSOLUTE_DECISIONS_SOURCE --binary ABSOLUTE_KRISIS --clockwork ABSOLUTE_CLOCKWORK --codex ABSOLUTE_CODEX --annals ABSOLUTE_ANNALS --annals-config ABSOLUTE_CONFIG --annals-library-id LOWERCASE_32_HEX
```

After the separate cutover prerequisites are proved, repeat the same inputs
with `--final-cutover`. Add `--keep-maintenance` only after a successful exact
preparation to retain its authenticated gate through external verification.
Repeat the same inputs with `--release-maintenance` to release that gate after
proving the exact current command, providers, hook, receipt, enabled observer,
and retired legacy schedules. `--home` selects an absolute operator home;
`--expected-current absent|releases/HASH` optionally refuses a changed selector.
The marker and receipt are distinct from the coordinator's named CLI hold.
