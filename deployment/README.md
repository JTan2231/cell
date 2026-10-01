# Cell deployment

Select systems from the Cell checkout:

```sh
./deploy.sh plan usher
./deploy.sh usher
./deploy.sh nucleus annals semantics krisis
./deploy.sh start nucleus annals semantics krisis
```

`plan` reads committed declarations without runtime changes. By default, a
run selects the exact **local `main` commit** at admission. Supply
`--source-commit FULL_COMMIT_ID` to select another exact commit. The value must
be a complete lowercase commit ID. The run ignores uncommitted edits,
other branches, and later commits. It never fetches, changes versions, commits,
tags, or pushes. Selecting Annals also selects Annals Usage. `decisions` is an
alias for `krisis`. Dependency declarations set the installation order. The plan
adds missing dependencies and dependencies outside the consumer's declared
`runtime_versions` interval. An unproved installed version selects the committed
dependency candidate. Incompatible committed candidates stop before maintenance.
Installed companions receive matching candidates, including consumers that embed
the provider's Rust libraries. The plan reports each addition's reason.

Retained dependencies receive a metadata inspection from their owning installer
before maintenance. Installation remnants, including broken selectors, are
inspected rather than treated as absence. Installation does not probe dependency
health or run product diagnostics.
Consumers can also require an explicitly indexed installed interface contract.
For example, EMT requires Email's account setup and discovery operation.
An older Email release with the same release number but without that operation
is selected for replacement. These explicit requirements do not turn Chancery's
documentation dependency graph into runtime or deployment edges.

Supply initial choices through a private JSON file keyed by canonical product
name. Subsequent runs reuse product configuration:

```sh
./deploy.sh semantics --settings /absolute/setup.json
```

For example, `{"semantics":{"enabled":false}}`
prepares Semantics without enabling its worker. Product installation manuals define
their accepted keys. Supply credential file references, never credential bytes.
Inspection gathers missing choices before maintenance. Email owns credential
installation and receiving-account discovery. A `plan` lists settings owners
without printing their values.

The command runs in the foreground until deployment ends. By default, it prints
one final JSON result to standard output. `--verbose` adds progress on standard
error. Otherwise, operations report activity at most once per minute after the
first minute. Failure and recovery excerpts share a 4 KiB output limit. Each
failure cause appears once in the result, with a 1 KiB limit. Adapter replies
and successful child logs are not printed. Deployment uses no model, Nucleus
job, or conversation continuation.

At startup, deployment pins its Python executable and complete source archive.
The host needs Python 3.11 or newer, Git, product build tools, and the current
macOS user session. Commit the coordinator in the selected source before use. There is no
background daemon or detached deployment API.

## Caller-correlated deployment

Use one stable request ID when a durable caller owns deployment:

```sh
./deploy.sh start usher --source-commit FULL_COMMIT_ID --request-id ci:JOB_ID:deployment:1
./deploy.sh status --request-id ci:JOB_ID:deployment:1
./deploy.sh reconcile --request-id ci:JOB_ID:deployment:1
```

`start --request-id` requires `--source-commit`. Admission records the logical
Git repository, exact commit, canonical requested product set, and captured
settings. Settings identity uses the JSON values, not the settings file path.
The admitted deployment plan records its expanded product set. Presentation
flags do not change request identity.

An exact replay returns the existing operation. Reuse of the same ID with a
different request stops. A terminal replay does not run installation again.
An active or interrupted replay does not start another worker. A different
active or unresolved operation blocks admission. Resolve that operation first.
Calls without a request ID retain their existing recovery-then-start behavior.

`status` reads one observation without runtime changes. `reconcile` operates
only on the named admitted operation. It waits for no surviving worker: if a
worker or descendant still owns the deployment lock, it reports active work.
After the lock becomes available, reconciliation uses the retained pinned
coordinator and product recovery procedure. It never creates a new deployment.
A missing request returns `not_found` and causes no deployment.

Results retain the ordinary deployment fields and add `request_id` and
`operation_state`. The operation state is `active`, `needs_reconciliation`,
`terminal`, `not_found`, or `blocked`. A blocked result can name its
`blocking_run_id`. Status observations have exit code zero unless they return
a terminal failed result. Active, interrupted, or blocked `start` replies and
active `reconcile` replies use exit code 75. Callers must inspect the structured
operation state and installation, maintenance, recovery, and cleanup outcomes.

Compact operation records remain under `deployments/operations`, outside the
temporary active workspace. The coordinator records the installation outcome
there before removing active evidence. A crash during cleanup does not authorize
another installation. Reconciliation completes or reports cleanup using the
recorded outcome. Receipts and request identities are retained without automatic
pruning. Keep them with deployment state backups. Their absence is not proof
that an operation never ran if storage was removed or restored incompletely.

CI owns its validation receipt and checks the exact source commit before this
handoff. Deployment does not determine CI coverage or turn a caller ID into
permission to deploy.

The coordinator and release builder use Python. Product installation and
deployment adapters are Rust executables backed by `cell-install`; retained
shell frontends are runtime assets for credential loading and scheduled jobs.

## Preparation and cutover

Each run creates a detached worktree at the selected commit. It calls the shared
release builder once for selected products, the maintenance closure, and retained
dependencies. Preparing an inspector does not select its product for upgrade.

Complete the relevant CI checks during development. Deployment does not run CI
or require a CI receipt. Preparation builds production binaries and packages the selected files. Tests, formatting, Clippy, documentation
builds, recognition gates, and generator CI remain development checks.

The builder uses one release-profile Cargo invocation for the selected packages
and binaries, then seals independent product candidates in parallel. It keeps
a persistent target and file lock per logical Git repository, separate from
the CI broker and target. Cargo defaults to the logical CPU count capped at
eight; `CELL_RELEASE_BUILD_JOBS` accepts a positive override. Completed build
bundles persist in a cache keyed by the clean Git commit, build inputs, and the
frozen macOS signing policy. Preparation verifies cached native signatures and
hashes before reuse.

The full Git commit ID identifies clean source. Each preparation of dirty source
gets a new identity and builds a fresh cache entry. A build from uncommitted
version edits is not reused after commit. Preparation does not hash source files.

A schema-one candidate records the source identity, selected commit, and
executable hashes and versions. Deployment retains a separate build receipt.
It deploys sealed executable copies without reading a later Cargo target.
Build records do not record CI success.

### macOS signing

Cell uses one current-user signing policy outside the repository. The shared
builder signs staged native runtime and installer executables before it computes
candidate hashes. Each executable keeps a permanent code identifier under the
configured namespace and its canonical product descriptor. Independent release
units do not change that product namespace. Compiler outputs remain unchanged.

Deployment captures the policy at admission and refuses policy changes before
publication or activation. Product publication verifies the expected certificate
and identifier and preserves signed bytes. Signature checks are new publication
requirements; they do not establish product readiness or grant macOS permissions.
Shell and Python assets remain release resources with their existing interpreters.

Read [Cell signing](../ci_manager/chancery/manuals/signing-operate.md) for explicit
identity setup, Keychain access, and full-inventory rotation.
Signing setup never occurs during an ordinary build, upgrade, or repair.

The same builder can prepare candidates without publication or installation:

```sh
python3 deployment/build.py --source-root /absolute/cell \
  --product usher --product nucleus --output /absolute/cell-build
```

The output contains `candidates/PRODUCT/bin`, each product's `candidate.json`,
and `result.json`. With a single product, optional `--unit UNIT` selects one
independently versioned release unit for release preparation. CI submission
uses `./ci.sh submit COMMIT`; the manager validates the candidate before it
requests exact-source deployment.

All candidates are prepared before maintenance begins. The coordinator then:

1. Inspects selected products and every additional product whose admission must
   be held. It retains each product-owned baseline, prerequisites and ordering.
2. Holds and drains each consumer before its providers, with Nucleus last, so
   admitted work can finish using its dependencies. An unselected requester may be held
   without changing its installed release.
3. Applies selected product adapters in their declared order, then configures
   affected products in dependency order. Products with atomic state-and-file
   transactions stage releases during apply and commit that transaction during
   configure. Nucleus starts its replacement service under its existing hold so
   requester configuration can use it.
4. Releases requester holds after configuration completes, then releases Nucleus
   last. Activation follows release of all holds.

Installers perform resource setup, program selection, configuration, and required
state initialization or migration. They verify configured macOS signatures on
native production code. They do not run persistent-state integrity or operational
readiness validation. Product diagnostics and
checks used during ordinary application work remain separate. CI does not assert
the removed installation validation behavior.

Replacing Nucleus holds its installed requesters, as declared by each product's
`requester_service`. Replacing one requester leaves Nucleus admission open.
When `~/.local/bin/cell-ci` is installed, the coordinator also captures that
command and holds its model admission before holding Nucleus. The CI manager
reports drained when no admitted or unresolved model attempt remains. Its
current CI job may still be waiting for deployment. The coordinator releases
only its own CI hold after product activation or coherent recovery. This shared
infrastructure hook does not install or update the CI manager. Explicit manager
upgrades must preserve its captured command during a deployment.

Selected products use their supplied candidate for maintenance. Affected-only
products use their installed command. These operations preserve owned holds and
drain admitted work without migration or early publication.

EMT captures worker intent during inspection, suspends its binding,
selects a disabled definition during configuration, and restores enabled state
after configuration and hold release. Existing pauses and incidents remain.
EMT resolves receiving configuration before maintenance. Conatus
preserves library identities and its cursor while rebinding Annals. Paperboy and
Platter reselect existing schedules with the installed program's exact pins.
Clockwork suspends every captured enabled binding. During activation, product
adapters restore their declared exact keys; Clockwork restores other captured
bindings through its current broker. Custom bindings and incident halts survive.

The coordinator owns the operation sequence and temporary execution state.
Product adapters own configuration discovery, admission, quiescence, migration,
service control, and recovery. The shared transaction library owns
immutable artifact manifests, public file selection, writer locks, attribution
checks, and file compensation.

Products own runtime state, admission holds, database backups, schedules,
services, and recovery decisions. Prove database recovery before restoring public
commands. Nucleus's guarded service installer owns copied executables and
authentication state.

## Temporary state and failure handling

Private active state is under
`deployments/active` inside the configured external workspace. While running,
it contains operation inputs/outcomes, logs, sealed source, executable candidates,
and the detached preparation worktree. These files may contain private paths
and baseline state; adapters must exclude credentials and domain document bodies.
Uncorrelated deployments have no retained completed history. Caller-correlated
operations retain compact receipts and support the status and reconciliation
commands above. There is no public wait or detached execution interface.

The foreground process acquires one host-wide deployment lock before creating
that workspace and passes its descriptor to the pinned worker. Every adapter
and deployer inherits the same lock. A surviving child therefore keeps another
deployment out even if its supervisors exit. Existing product locks and
admission gates remain necessary for coexistence with direct commands.

Success requires setup and configuration to complete and all run-owned holds to
be released. After a completed failure, the same invocation uses product recovery
operations. Recovery restores the recorded prior or candidate installation
before releasing holds. Nucleus releases last. Recovery never repeats an uncertain apply or clears another
owner's hold. Successful recovery does not change a failed deployment result.
If recovery cannot safely release a hold, it retains that hold and reports its
owner and error for product recovery.

If any release or activation was attempted, recovery first holds and drains the
affected products again. It preserves the original inspection baseline. Product
recovery then repairs the transaction and configuration before a new release and
activation attempt. A lost activation reply does not authorize configuration
changes while work is running.

After the worker exits, the parent reacquires the host lock. A surviving
descendant blocks cleanup. A resolved transaction is removed. An unresolved
transaction remains private with its source, candidates, baselines and logs.
The next ordinary deployment first invokes recovery using that retained
transaction. It starts the requested deployment only after recovery succeeds.
No caller-supplied run ID or separate recovery command is required. A failed
recovery retains evidence and holds. Original failure and recovery excerpts
share the final diagnostic budget.

Completed build bundles and
the release Cargo target live outside that workspace and survive cleanup. The
default cache is `releases/REPOSITORY_HASH` inside the external workspace,
shared by linked worktrees. `CELL_RELEASE_CACHE_DIR` can select another location
inside that workspace. Explicit build outputs must also remain there.
Compiler processes cannot write outside the external workspace. See
[external work storage](../ci_manager/STORAGE.md) for setup and failure rules.
The build cache has no automatic pruning.

The final result preserves `schema`, `run_id`, `state`, `products`,
`source_commit`, `detail` and `exit_code`. It additionally reports `recovery`
(`not_needed`, `succeeded`, `failed`, or an interrupted outcome), `maintenance`
(`not_started`, `released`, or `attention_required`) and `cleanup` dispositions
for installed releases and the temporary workspace. Outstanding maintenance
lists its owner and each affected product as `retained` or `uncertain`. A lost
hold or release reply is uncertain until a successful release is captured.
Successful recovery still returns deployment failure, with explicit released
maintenance. Cleanup failure preserves the completed installation outcome.

Product command failures retain bounded execution status. The shared adapter
does not relay arbitrary child messages, arguments, credentials, or domain
bodies. Unrecognized failures retain the executable and exit status.

After successful configuration and release, the coordinator removes unreferenced
installed Cell release history under the same global lock. It preserves current
releases and releases pinned by selected schedules (including disabled ones),
service definitions, current product receipts/configuration, and running
processes. Cleanup errors are reported separately: installed products stay in
place and no recovery or rollback begins. Direct product installers
retain their own previous releases; automatic pruning belongs to a successful
coordinated deployment.

The cleanup reader accepts complete legacy Clockwork binding arrays. For
version-two selections, it reads pages until the inventory is complete. An
unknown or incomplete inventory stops cleanup before deletion.

Nucleus retains its cutover journal. Its service-owned recovery procedure
restarts the recorded candidate when replacement was interrupted.

## Product adapter protocol, version 1

The product owns `PRODUCT_DIR/deployment/adapter.json` and its Rust installer.
Each declaration names its sealed installer executable in literal JSON:

```json
{"schema":1,"product":"usher","dependencies":[],"application":"Usher","adapter_binary":"usher-install","description":"Install Usher"}
```

`dependencies` declares installation prerequisites, separately from Chancery
documentation dependencies. `runtime_versions` maps each dependency to inclusive
`minimum` and exclusive `before` release versions. Maintain these bounds with the
product's supported interfaces. Optional `runtime_contracts` maps a dependency
to required installed entry IDs and inclusive `minimum`/exclusive `before`
contract versions. Only explicitly indexed, supported entries satisfy it.
`companions` selects installed consumers that need matching artifacts or pins.
`requester_service` names the service whose
replacement requires this installed requester to drain. `activation_bindings`
lists the exact Clockwork keys whose final intent the adapter owns. Optional
`pin_inventory` names read-only product CLI arguments that return its complete
configured executable references in an `ok: true` response with a `data.config`
object. Cleanup inspects that object, not status history or diagnostics. Use
commands supported by retained releases; Conatus provides them through `status`.
Build, deployment and cleanup read the same
literal product inventory. The adapter accepts one fixed operation argument:
`inspect`, `hold`, `drain`, `apply`, `configure`, `release`, `activate`,
or `recover`. It accepts
no caller-supplied command or workflow body. Standard input is one JSON object:

- `schema`, `product`, `run_id`, `run_dir` and immutable `source_root`;
- `selected_products`, `candidate_dir`, and the candidate manifest;
- `affected_products` and their exact `activation_bindings`;
- optional product `settings`, direct `dependency_settings`, and sealed
  `dependency_candidates` for read-only discovery before a first installation;
- `prior`, the opaque data captured by that product's inspection;
- `recovery`, the captured operation and hold/application/configuration progress
  during recovery within the active invocation.

`candidate_dir` and `candidate` identify a sealed candidate for selected and
affected products. Binaries reside at `candidate_dir/bin/COMMAND`; the manifest
records each path, SHA-256 and version plus exact packaging/provider source
hashes. The adapter executes `PRODUCT-install adapter OP` from the candidate,
reads its package metadata and refuses `apply` for an affected-only
product. A declared `maintenance_products` closure makes these executables
available before inspection and before any hold.

Standard output must be exactly one bounded JSON object, with diagnostics on
standard error:

```json
{"schema":1,"status":"ready","detail":"Owned installation inspected","data":{}}
```

The expected statuses, respectively, are `ready`, `held`, `drained`, `applied`,
`configured`, `released`, `activated` and `recovered`. A drain may
return `waiting`; the coordinator waits and retries that phase in the same run.
Waiting has no deployment-duration cutoff. Nonzero exits, `stopped`, invalid replies,
unknown statuses and outputs above 1 MiB stop the run. `inspect` data may declare
`maintenance_products` and `after` as lists of canonical system names.
Inspection must be read-only with respect to the installation.

Every hold is owned by `run_id`. Hold and release must be idempotent, preserve
pre-existing operator pauses and other runs' holds, and support an interrupted
response. Recovery releases every affected run-owned token idempotently after
proof, including when a hold's effect happened but its reply was lost. Each
mutation must retain enough product-owned evidence for recovery. The coordinator
records `any_apply_started` before permitting any product apply; an unchanged
prior installation may recover pre-cutover holds using its inspected baseline.
Recovery context also records configure, release and activation progress,
including attempts whose reply was lost. Each product determines its actual
cutover from its own transaction evidence.
Successful recovery additionally returns
`{"safe_to_release":true,"installed":"candidate"}` or `"installed":"prior"`
inside `data`. Missing proof retains maintenance. Recovering a product is never
permission to bypass its ownership, migration or forward-only authentication
boundary.

## Release publication policy

Deployment uses committed versions and content identities. Product `release.sh`
separately owns Git publication: its lock, version policy, build, commit, tag,
and atomic push. The coordinator does not treat an installed version as a Git
tag or impose a release cadence. Release and deployment share build artifacts.
They neither reuse nor enforce completed CI results.

## Shared Rust installation transactions

Every product builds a dedicated `PRODUCT-install` executable alongside its
runtime executables in the release builder's single Cargo invocation. Product
versions remain independent; Annals Usage remains a separate release unit.

Product installers use the shared `cell-install` library for file transactions.
Each installer owns its product policy and version-one coordinator adapter.
The library has no separate product identity or release publication.

Usher retains the `cell-install-v1` format. See
[Usher installation](../usher/chancery/manuals/install-operate.md) for its
commands and supported legacy recovery. Other products use
`cell-install-v2`. Its manifest records exact file digests and modes, independent
executable/provider versions, public entry mappings and a stable content identity.
The immutable tree retains `package/install` for supported recovery. Legacy
formats are read through the owning product's metadata reader.

Conversations, Chancery, Email, Cast, Clockwork and Platter use the common
program-selection entry point. Their product specifications supply the layout,
legacy metadata reader and runtime frontend where needed. Runtime schedule
operations retain their own release checks when pinning definitions.
Stateful products provide typed lifecycle code around the same file transaction.
See each installed product's installation contract for its exact arguments and
recovery limits.

Platter permits mutating installation and recovery only through coordinated
deployment. See [Platter installation](../platter/chancery/manuals/install-operate.md)
for its admission, migration, backup and activation rules.

Publication rechecks the captured selection under the product lock and holds
the Chancery writer lock through publication and compensation. Suspended public
commands stay absent during state rollback. Explicit interrupted-publication
recovery accepts only absent entries or entries attributable to the captured
prior release and exact candidate; foreign replacements are retained and stop
recovery. Directory locks preserve the legacy mkdir protocol and reclaim only a
recognized private owner marker whose process is proven dead. Empty legacy locks
and unknown or live owners require product/operator recovery.

After coordinated success, cleanup reads current selections and live references,
then removes unreferenced release directories only for the prepared products
selected for cleanup. Other products retain their history. It does not invoke installer
validation commands or audit release contents. Current releases and selected
schedule pins, including disabled bindings, remain protected. Active transaction
markers and unknown reference inventories stop deletion.

Platter participates in release-history cleanup under its PID-aware file lock.
Its private packet state and database backups remain outside the installation
tree and are retained.
