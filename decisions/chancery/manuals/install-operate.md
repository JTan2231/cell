# Install and operate Krisis

The public binary/provider is `krisis`; the compatibility provider is
`decisions`; the active Clockwork key is `krisis/observer`; schema is 4.
Existing Decisions application-support and log paths are intentionally retained
for persistent-history compatibility.

Deployment requires explicit absolute Krisis, Clockwork, Codex, Annals, and
dedicated Annals-config paths plus the exact lowercase 32-hex decisions-library
ID. The selected Codex path is recorded in the immutable Clockwork definition
and used unchanged for final-cutover doctor and scheduled Conversations reads;
the observer does not discover another Codex installation at runtime.
Interactive source-reading commands must receive that same path through
`CONVERSATIONS_CODEX` because the installed command does not inherit
Clockwork's observer environment; doctor and process also require the complete
explicit Annals configuration. The default operation prepares and verifies a
content-addressed release and Clockwork definition while retaining the
maintenance gate; it does not select or activate them. After the outer cutover
has separately proved its Annals and semantic prerequisites, `--final-cutover`
performs writer shutdown, quiescent backup, migration, doctor, selector/hook
publication, baseline activation, and schedule handoff. Clockwork process state
is not cross-system proof.

Every selected definition and legacy plist is inspected and attributed before
mutation. Disabled or foreign legacy bindings are untouched. A provable failure
restores exact prior selectors and enabled state; a prior null Clockwork
selection that cannot be restored leaves the owned candidate disabled with the
maintenance gate and transaction evidence retained.

After a separately authorized final cutover, verify the baseline, doctor, exact
hook trust, private observer ownership receipt, `krisis/observer` history,
retired binding state, and body-free logs
before any separately authorized synthetic canary. Uninstall disables only the
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
`drained`. Drain describes participating live commands; durable observations
and dependency jobs need separate product-owned quiescence proof.

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

The Cell adapter composes preparation and explicit final cutover while
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

The deployment adapter retains its private isolated canary state under the
run directory on success and failure; it does not remove that evidence. A
verified response identifies the canary directory. Annals exercises local
retention, Krisis durable baseline replay, and Semantics repository mutation
and replay, alongside each product's dependency doctor. These checks do not
claim a live model-backed domain integration.
The Cell deployment adapter additionally verifies one real Nucleus
classification using a retained private synthetic completed turn. Success
requires its durable classification receipt, one isolated account outbox item,
and a completed Nucleus job with structured final output. The installed Annals
binary/config/library pin remains a separate doctor check; the canary never
delivers a synthetic account to that library. Recovery reuses the fixed
canary directory and job identity and creates no successor attempt.
