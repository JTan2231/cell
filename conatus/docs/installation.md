# Installation and background activation

Installation, initialization, and activation are separate operations. These
instructions describe the source release; they do not establish that it has
been built, validated, installed, or activated.

## Select a release

```sh
conatus-install install --binary ABS_BINARY --bundle ABS_BUNDLE --expected-current absent
conatus-install inspect
conatus-install verify --binary ABS_BINARY --bundle ABS_BUNDLE
conatus-install verify-release ABS_RELEASE
```

The installer stages a content-addressed release and its matching Chancery
bundle, then selects it through the product installation. The default install
root is `~/Library/Application Support/Conatus/install`; `--home ABS_HOME`
selects another home. For an upgrade, replace `absent` with the observed
`releases/HASH` selection. The operation installs no state, model job, or timer.

Recovery selects an exact retained release:

```sh
conatus-install recover --release ABS_RELEASE --expected-current releases/HASH
```

Release recovery does not roll back the Conatus database, Annals library, or
Clockwork binding. Read the retained records and active binding independently.

Initialize Conatus with the desired state directory and existing Annals
decisions configuration as described in [CLI and operation](cli.md). A selected
binary or valid provider bundle does not establish Annals or model readiness.

## Prepare and activate the runner

Generate a new definition from the currently selected installed release:

```sh
conatus-install schedule-definition --state-dir ABS_STATE --output ABS_DEFINITION
```

The state directory and output parent must already exist, and the output file
must be new. This command writes the definition and creates the private state
log directory. It does not register or activate a schedule. Its receipt reports
the key, release identity, definition path, and `registered:false` and
`activated:false`.

The definition pins the installed release and executable for `conatus/update`,
with a 300-second interval, run-at-load enabled, and overlap set to skip. It runs
`conatus --state-dir ABS_STATE update`. Clockwork opens Conatus-owned
`logs/update.out.log` and `logs/update.err.log`; Conatus owns their contents.
It uses an explicit home and minimal system search path, without credentials.

After the release and initialized state are ready, register the definition and
select its returned digest:

```sh
clockwork definition register ABS_DEFINITION
clockwork binding switch conatus/update DEFINITION_DIGEST
clockwork binding show conatus/update
clockwork history conatus/update --limit 20
```

Registration retains the definition without starting work. Binding selection
enables the schedule, including its run-at-load request. Inspect Conatus status
for domain outcomes; a Clockwork process result alone does not establish source
retention or interpretation. No maximum activation delay or completion time is
promised.

Use `clockwork binding disable conatus/update` to disable activation. Conatus
`pause` only gates product processing; it does not change the Clockwork binding.
Do not run a second Annals inbox schedule for the Conatus library alongside this
runner.

Direct installation does not retarget an existing immutable schedule. Cell
deployment captures and suspends the existing runner, rebinds Annals and
registers a disabled definition for the new release, then restores saved intent
after maintenance release. See the [complete operation](../chancery/manuals/update-operate.md#coordinated-deployment-setup)
for setup fields, fresh initialization and recovery.

## Scheduled failure policy

Conatus generates Clockwork definition schema 2 for `conatus/update`, with
`[failure] on_abend = "halt-until-approved"`. An update stops admitting work on
its first feed or handoff error. Annals dispatch uses `inbox run
--stop-on-failure`, so a failed source also stops its batch. The update retains
completed stage results before returning nonzero. A failed feed no longer starts
pending handoffs, and a failed handoff no longer starts inbox processing.

Clockwork owns the durable scheduling halt and one retained email notification
through `HOME/.local/bin/email`. Inspect `clockwork incident list conatus/update`
and `clockwork incident show INCIDENT_ID`. After explicit approval, use
`clockwork binding resume conatus/update INCIDENT_ID` to allow future scheduling.
A new definition, deployment, `conatus resume`, or Annals recovery cannot clear
that incident. Schema-one bindings acquire this behavior only after an explicit
schema-two definition switch; definition generation does not activate it.

Conatus' operator pause, Annals' operator pause and bounded retry-event halts,
source identities, handoff receipts, and explicit domain retry remain with their
products. Empty work and Annals low-storage dispatch readiness are successful
outcomes. A failed enqueue, including insufficient copy capacity, is an abend.
Continuation permits pending handoffs under their existing identity rules; it
does not give a failed Annals attempt another try.
