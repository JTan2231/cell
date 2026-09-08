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

An installed release upgrade does not retarget an existing immutable schedule.
Generate, register, and select a new definition when the scheduled runner should
use the new release. Preserve the previous selection for explicit recovery.
