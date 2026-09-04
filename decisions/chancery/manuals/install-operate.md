# Install and operate Krisis

The public binary/provider is `krisis`; the compatibility provider is
`decisions`; the active Clockwork key is `krisis/observer`; schema is 4.
Existing Decisions application-support and log paths are intentionally retained
for persistent-history compatibility.

Deployment requires explicit absolute Krisis, Clockwork, Annals, and dedicated
Annals-config paths plus the exact lowercase 32-hex decisions-library ID. The
default operation prepares and verifies a content-addressed release and
Clockwork definition while retaining the maintenance gate; it does not select
or activate them. After the outer cutover has separately proved its Annals and
semantic prerequisites, `--final-cutover` performs writer shutdown, quiescent
backup, migration, doctor, selector/hook publication, baseline activation, and
schedule handoff. Clockwork process state is not cross-system proof.

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
